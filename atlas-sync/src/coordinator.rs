pub mod coordinator {
    use crate::args_parser::args_parser::Args;
    use crate::crdt::crdt::Operation;
    use crate::crdt_index::crdt_index::{CRDTIndex, IndexCmd};
    use crate::fswrapper::fswrapper::{INDEX_NAME, WATCHED_PATH};
    use crate::p2p_network::p2p_network::*;
    use crate::watcher::watcher::watch_path;
    use libp2p::request_response::{ProtocolSupport, RequestResponse, RequestResponseConfig};
    use libp2p::{
        core::upgrade,
        floodsub::Floodsub,
        futures::StreamExt,
        mdns::Mdns,
        mplex,
        noise::{Keypair, NoiseConfig, X25519Spec},
        swarm::{Swarm, SwarmBuilder},
        tcp::TokioTcpConfig,
        Transport,
    };
    use log::{info, trace};
    use std::path::Path;
    use tokio::sync::mpsc::UnboundedSender;
    use tokio::sync::mpsc::{self, UnboundedReceiver};

    pub async fn start_coordination(args: Args) {
        match args.watch_path.is_empty() {
            true => {
                WATCHED_PATH
                    .set(String::from("src/resources/test_watcher"))
                    .expect("WATCHED_PATH can only be set once");
            }
            false => {
                WATCHED_PATH
                    .set(args.watch_path.clone())
                    .expect("WATCHED_PATH can only be set once");
            }
        }

        info!("Peer Id: {}", PEER_ID.clone());
        let (response_sender, mut response_rcv) = mpsc::unbounded_channel();

        let auth_keys = Keypair::<X25519Spec>::new()
            .into_authentic(&KEYS)
            .expect("can create auth keys");

        let transp = TokioTcpConfig::new()
            .upgrade(upgrade::Version::V1)
            .authenticate(NoiseConfig::xx(auth_keys).into_authenticated())
            .multiplex(mplex::MplexConfig::new())
            .boxed();

        let index_tx = build_index(response_sender.clone());
        let (peer_ev_sender, mut peer_ev_rcv): (
            UnboundedSender<PeerConnectionEvent>,
            UnboundedReceiver<PeerConnectionEvent>,
        ) = mpsc::unbounded_channel();

        // request response protocol
        let protocols = std::iter::once((FileProtocol(), ProtocolSupport::Full));
        let mut cfg = RequestResponseConfig::default();
        cfg.set_connection_keep_alive(std::time::Duration::from_secs(10));
        let req_resp = RequestResponse::new(FileCodec(), protocols.clone(), cfg.clone());

        let mut behaviour = AtlasSyncBehavior {
            floodsub: Floodsub::new(PEER_ID.clone()),
            mdns: Mdns::new(Default::default())
                .await
                .expect("can create mdns"),
            req_resp: req_resp,
            index_tx: index_tx.clone(),
            peer_tx: peer_ev_sender.clone(),
        };

        behaviour.floodsub.subscribe(TOPIC.clone());

        let mut swarm = SwarmBuilder::new(transp, behaviour, PEER_ID.clone())
            .executor(Box::new(|fut| {
                tokio::spawn(fut);
            }))
            .build();

        Swarm::listen_on(
            &mut swarm,
            "/ip4/0.0.0.0/tcp/0"
                .parse()
                .expect("can get a local socket"),
        )
        .expect("swarm can be started");

        let mut first_time = true;
        let syncing = !args.peer_id.is_empty();
        while syncing {
            tokio::select! {
                _ = swarm.next() => {
                  trace!("Swarm event");
                },
                peer_rsp = peer_ev_rcv.recv() => {
                    match peer_rsp {
                      Some(PeerConnectionEvent::InitialConnection(_)) => {
                          if !args.peer_id.is_empty() {
                              handle_initial_peer_connection(&args.peer_id, &PEER_ID.to_string(), &mut swarm);
                          }
                      }
                      Some(PeerConnectionEvent::InitialConnCompleted(_)) => {
                          info!("Initial connection synchronization has been completed");
                          break;
                      }
                      _ => {
                        todo!("");
                      }
                    }
                },
                _ = tokio::time::sleep(tokio::time::Duration::from_millis(500)) => {
                    if first_time{
                        if !args.peer_id.is_empty() {
                            let _ = peer_ev_sender.send(PeerConnectionEvent::InitialConnection((
                                args.peer_id.to_string(),
                                PEER_ID.to_string(),
                            )));
                        }
                        first_time = false;
                    }
                }
            }
        }

        info!(
            "Starting to watch path: {:?}",
            Path::new(WATCHED_PATH.get().unwrap())
        );
        watch_path(Path::new(WATCHED_PATH.get().unwrap()), index_tx)
            .expect("Failed to start file watcher");

        loop {
            tokio::select! {
                _ = swarm.next() => {
                  trace!("Swarm event");
                },
                response = response_rcv.recv() => {
                  if let Some(event) = response {
                    let json_bytes = serde_json::to_vec(&event).unwrap();

                    swarm
                        .behaviour_mut()
                        .floodsub
                        .publish(TOPIC.clone(), json_bytes);
                  }
                },
            }
        }
    }

    fn handle_initial_peer_connection(
        peer_id: &str,
        local_peer_id: &str,
        swarm: &mut Swarm<AtlasSyncBehavior>,
    ) {
        if !peer_id.is_empty() {
            let json_bytes = serde_json::to_vec(&PeerConnectionEvent::InitialConnection((
                peer_id.to_string(),
                local_peer_id.to_string(),
            )))
            .expect("Should be serializable");

            info!(
                "Starting initial peer connection from peer: {} to target peer: {}.",
                peer_id, local_peer_id
            );

            swarm
                .behaviour_mut()
                .floodsub
                .publish(TOPIC.clone(), json_bytes);
        }
    }

    pub fn build_index(broadcast_tx: UnboundedSender<Operation>) -> UnboundedSender<IndexCmd> {
        let watched_path = WATCHED_PATH.get().unwrap().to_owned();
        let index_name = INDEX_NAME.as_str();
        let index_path_str = watched_path + index_name;
        let index_path = Path::new(&index_path_str);
        info!("CRDT Index path: {:?}", index_path);
        let index = CRDTIndex::load_or_init(PEER_ID.to_string(), index_path_str).unwrap();

        let (tx, mut rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut index = index;
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    IndexCmd::LocalOp { mutation, cur } => {
                        let op = index.apply_local_op(&cur, mutation);
                        let _ = index.save_to_disk();
                        info!("Local operation has been applied and is broadcasted to peers!");
                        let _ = broadcast_tx.send(op);
                    }
                    IndexCmd::RemoteOp { mutation, cur } => {
                        let op = index.make_op(cur, mutation);
                        let _ = index.apply_remote(&op);
                        let _ = index.save_to_disk();
                        info!("Remote operation has been applied!");
                    }
                }
            }
        });
        tx
    }
}
