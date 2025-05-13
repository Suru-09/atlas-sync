pub mod coordinator {
    use crate::args_parser::args_parser::Args;
    use crate::file::file::FileEventType;
    use crate::p2p_network::p2p_network::*;
    use crate::watcher::watcher::watch_path;
    use libp2p::{
        core::upgrade,
        floodsub::Floodsub,
        futures::StreamExt,
        mdns::Mdns,
        mplex,
        noise::{Keypair, NoiseConfig, X25519Spec},
        swarm::{self, Swarm, SwarmBuilder},
        tcp::TokioTcpConfig,
        Transport,
    };
    use log::info;
    use std::path::Path;
    use tokio::sync::mpsc;

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

        let mut behaviour = AtlasSyncBehavior {
            floodsub: Floodsub::new(PEER_ID.clone()),
            mdns: Mdns::new(Default::default())
                .await
                .expect("can create mdns"),
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

        watch_path(
            Path::new(WATCHED_PATH.get().unwrap()),
            response_sender.clone(),
        )
        .expect("Failed to start file watcher");

        let mut first_time = true;
        loop {
            let evt = {
                tokio::select! {
                    _ = swarm.next() => {
                        None
                    },
                    response = response_rcv.recv() => {
                        handle_peer_connection(&mut first_time, &args.peer_id, &PEER_ID.to_string(), &mut swarm);
                        Some(response.expect("Has some data"))
                    },
                }
            };

            if let Some(event) = evt {
                handle_peer_connection(
                    &mut first_time,
                    &args.peer_id,
                    &PEER_ID.to_string(),
                    &mut swarm,
                );

                let json_bytes = match event {
                    FileEventType::Created(create_op) => {
                        info!("File created: {:?}.", create_op);
                        serde_json::to_vec(&FileEventType::Created(create_op))
                            .expect("Should be serializable")
                    }
                    FileEventType::Updated(update_op) => {
                        info!("File updated : {:?}!", update_op);
                        serde_json::to_vec(&FileEventType::Updated(update_op))
                            .expect("Should be serializable")
                    }
                    FileEventType::Deleted(delete_op) => {
                        info!("File deleted: {:?}.", delete_op);
                        serde_json::to_vec(&FileEventType::Deleted(delete_op))
                            .expect("Should be serializable")
                    }
                };

                swarm
                    .behaviour_mut()
                    .floodsub
                    .publish(TOPIC.clone(), json_bytes);
            }
        }
    }

    fn handle_peer_connection(
        first_time: &mut bool,
        peer_id: &str,
        local_peer_id: &str,
        swarm: &mut Swarm<AtlasSyncBehavior>,
    ) {
        if !peer_id.is_empty() && *first_time {
            let json_bytes = serde_json::to_vec(&PeerConnectionEvent::InitialConnection((
                peer_id.to_string(),
                local_peer_id.to_string(),
            )))
            .expect("Should be serializable");

            info!("Hahah happening rn!!");

            swarm
                .behaviour_mut()
                .floodsub
                .publish(TOPIC.clone(), json_bytes);
            *first_time = false;
        }
    }
}
