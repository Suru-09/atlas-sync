pub mod p2p_network {
    use crate::crdt::crdt::{Mutation, Operation};
    use crate::crdt_index::crdt_index::IndexCmd;
    use crate::fswrapper;
    use crate::fswrapper::fswrapper::FileBlob;
    use crate::fswrapper::fswrapper::{INDEX_NAME, WATCHED_PATH};
    use libp2p::{
        floodsub::{Floodsub, FloodsubEvent, Topic},
        identity,
        mdns::{Mdns, MdnsEvent},
        request_response::{ProtocolName, RequestResponseCodec, RequestResponseMessage},
        swarm::NetworkBehaviourEventProcess,
        NetworkBehaviour, PeerId,
    };
    use log::{debug, error, info};
    use once_cell::sync::Lazy;
    use serde::{Deserialize, Serialize};
    use std::env;
    use std::path::Path;
    use std::str::FromStr;
    use std::{io, iter};
    use tokio::sync::mpsc::UnboundedSender;

    use futures::prelude::*;
    use tracing_subscriber::EnvFilter;

    pub static KEYS: Lazy<identity::Keypair> = Lazy::new(|| identity::Keypair::generate_ed25519());
    pub static PEER_ID: Lazy<PeerId> = Lazy::new(|| PeerId::from(KEYS.public()));
    pub static TOPIC: Lazy<Topic> = Lazy::new(|| Topic::new("FILE_SHARING"));

    #[derive(Debug, Serialize, Deserialize, Clone)]
    pub struct FileRequest {
        name: String,
    }

    #[derive(NetworkBehaviour)]
    pub struct AtlasSyncBehavior {
        pub floodsub: Floodsub,
        pub mdns: Mdns,
        pub req_resp: RequestResponse<FileCodec>,
        #[behaviour(ignore)]
        pub index_tx: UnboundedSender<IndexCmd>,
        #[behaviour(ignore)]
        pub peer_tx: UnboundedSender<PeerConnectionEvent>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub enum PeerConnectionEvent {
        InitialConnection((String, String)),
        SyncFile((String, FileBlob)),
        InitialConnCompleted(String),
    }

    impl NetworkBehaviourEventProcess<FloodsubEvent> for AtlasSyncBehavior {
        fn inject_event(&mut self, event: FloodsubEvent) {
            match event {
                FloodsubEvent::Message(msg) => {
                    if let Ok(parsed) = serde_json::from_slice::<Operation>(&msg.data) {
                        match parsed.mutation {
                            Mutation::New { key, value } => {
                                info!(
                                    "[REMOTE_EVENT] New mutation with key: {:?} and value: {:?}",
                                    key, value
                                );
                                let cmd = IndexCmd::RemoteOp {
                                    mutation: Mutation::New {
                                        key: key.clone(),
                                        value: value,
                                    },
                                    cur: vec![key.clone()],
                                };
                                let _ = self.index_tx.send(cmd);
                                let _ = self.req_resp.send_request(
                                    &PeerId::from_str(parsed.id.replica_id.as_str())
                                        .expect("Valid peer id"),
                                    FileRequest { name: key },
                                );
                            }
                            Mutation::Edit { key, value } => {
                                info!(
                                    "[REMOTE_EVENT] EDIT mutation with key: {:?} and value: {:?}",
                                    key, value
                                );
                                let cmd = IndexCmd::RemoteOp {
                                    mutation: Mutation::Edit {
                                        key: key.clone(),
                                        value: value,
                                    },
                                    cur: vec![key.clone()],
                                };
                                let _ = self.index_tx.send(cmd);
                                let _ = self.req_resp.send_request(
                                    &PeerId::from_str(parsed.id.replica_id.as_str())
                                        .expect("Valid peer id"),
                                    FileRequest { name: key },
                                );
                            }
                            Mutation::Delete { key } => {
                                info!("[REMOTE_EVENT] DELETE mutation with key: {:?}.", key);
                                let cmd = IndexCmd::RemoteOp {
                                    mutation: Mutation::Delete { key: key.clone() },
                                    cur: vec![key],
                                };
                                let _ = self.index_tx.send(cmd);
                            }
                        }
                    } else if let Ok(parsed) =
                        serde_json::from_slice::<PeerConnectionEvent>(&msg.data)
                    {
                        let base_path = Path::new(WATCHED_PATH.get().unwrap());
                        match parsed {
                            PeerConnectionEvent::InitialConnection((target_peer, source_peer)) => {
                                //info!("Target peer: {}, Source peer: {}", target_peer, source_peer);
                                if PEER_ID.to_string() == target_peer {
                                    // go through each file and do stuff.
                                    let blob_files =
                                        FileBlob::collect_files_to_be_synced(base_path).unwrap();
                                    for file_blob in blob_files.iter() {
                                        let json_bytes =
                                            serde_json::to_vec(&PeerConnectionEvent::SyncFile((
                                                source_peer.clone(),
                                                file_blob.clone(),
                                            )))
                                            .expect("File Blob is serializable");
                                        self.floodsub.publish(TOPIC.clone(), json_bytes);
                                    }

                                    // signal the end of the initial connection.
                                    let json_bytes = serde_json::to_vec(
                                        &PeerConnectionEvent::InitialConnCompleted(
                                            target_peer.clone(),
                                        ),
                                    )
                                    .expect("File Blob is serializable");
                                    self.floodsub.publish(TOPIC.clone(), json_bytes);
                                }
                            }
                            PeerConnectionEvent::SyncFile((target_peer, file_blob)) => {
                                //info!("Sync file event!");
                                if PEER_ID.to_string() == target_peer {
                                    let _ = file_blob.write_to_disk(&base_path);
                                }
                            }
                            PeerConnectionEvent::InitialConnCompleted(target_peer) => {
                                if PEER_ID.to_string() == target_peer {
                                    let _ = self.peer_tx.send(
                                        PeerConnectionEvent::InitialConnCompleted(target_peer),
                                    );
                                }
                            }
                        }
                    } else {
                        error!("Failed to parse!");
                    }
                }
                FloodsubEvent::Subscribed { peer_id, topic } => {
                    debug!(
                        "Subscriber with peer_id: {} connected to the topic: {:?}",
                        peer_id, topic
                    );
                }
                FloodsubEvent::Unsubscribed { peer_id, topic } => {
                    debug!(
                        "Subscriber with peer_id: {} disconnected from the topic: {:?}",
                        peer_id, topic
                    );
                }
            }
        }
    }

    impl NetworkBehaviourEventProcess<MdnsEvent> for AtlasSyncBehavior {
        fn inject_event(&mut self, event: MdnsEvent) {
            match event {
                MdnsEvent::Discovered(discovered_list) => {
                    for (peer, _addr) in discovered_list {
                        self.floodsub.add_node_to_partial_view(peer);
                        debug!("Peer: {} has been discovered!", peer);
                    }
                }
                MdnsEvent::Expired(expired_list) => {
                    for (peer, _addr) in expired_list {
                        if !self.mdns.has_node(&peer) {
                            debug!("Peer: {} has expired!", peer);
                            self.floodsub.remove_node_from_partial_view(&peer);
                        }
                    }
                }
            }
        }
    }

    impl NetworkBehaviourEventProcess<RequestResponseEvent<FileRequest, FileBlob>>
        for AtlasSyncBehavior
    {
        fn inject_event(&mut self, event: RequestResponseEvent<FileRequest, FileBlob>) {
            match event {
                RequestResponseEvent::Message { peer, message } => {
                    info!("Request Message for peer: {} with msg: {:?}", peer, message);
                    match message {
                        RequestResponseMessage::Request {
                            request_id,
                            request,
                            channel,
                        } => {
                            let path = fswrapper::fswrapper::compute_file_absolute_path(Path::new(
                                &request.name,
                            ));
                            error!("request path: {:?}", path);
                            let mut file_blob: FileBlob = match FileBlob::from_path(&path) {
                                Ok(blob) => blob,
                                Err(e) => {
                                    error!(
                                            "Could not extract file blob from request: {:?} with request_id: {} due to error: {:?}",
                                            request, request_id, e
                                        );
                                    FileBlob::default()
                                }
                            };

                            // really important to use the relative path and not absolute!!
                            file_blob.name = request.name;
                            let _ = self.req_resp.send_response(channel, file_blob);
                        }
                        RequestResponseMessage::Response {
                            request_id,
                            response,
                        } => {
                            error!("received path: {:?}", response.name);
                            let base_path = fswrapper::fswrapper::compute_file_absolute_path(
                                Path::new(&response.name),
                            );
                            error!("base path: {:?}", base_path);
                            match response.write_to_disk(&base_path) {
                                Ok(_) => {}
                                Err(e) => {
                                    error!(
                                        "Could not write blob from request_id: {} to disk: {:?}",
                                        request_id, e
                                    );
                                }
                            }
                        }
                    }
                }
                RequestResponseEvent::ResponseSent { peer, request_id } => {
                    info!(
                        "Response Sent for peer: {} with req_id: {:?}",
                        peer, request_id
                    );
                }
                RequestResponseEvent::OutboundFailure {
                    peer,
                    request_id,
                    error,
                } => {
                    error!("[OUTBOUND FAILURE] Peer: {peer:?}, RequestId: {request_id:?}, Error: {error:?}");
                }
                RequestResponseEvent::InboundFailure {
                    peer,
                    request_id,
                    error,
                } => {
                    error!("[INBOUND FAILURE] Peer: {peer:?}, RequestId: {request_id:?}, Error: {error:?}");
                }
            }
        }
    }

    use async_trait::async_trait;
    use libp2p::request_response::{RequestResponse, RequestResponseEvent};

    #[derive(Debug, Clone)]
    pub struct FileProtocol();

    #[derive(Clone)]
    pub struct FileCodec();

    impl ProtocolName for FileProtocol {
        fn protocol_name(&self) -> &[u8] {
            b"/my/protocol/1.0.0"
        }
    }

    #[async_trait]
    impl RequestResponseCodec for FileCodec {
        type Protocol = FileProtocol;
        type Request = FileRequest;
        type Response = FileBlob;

        async fn read_request<T>(
            &mut self,
            _: &FileProtocol,
            io: &mut T,
        ) -> io::Result<Self::Request>
        where
            T: AsyncRead + Unpin + Send,
        {
            let mut len_buf = [0u8; 4];
            io.read_exact(&mut len_buf).await?;
            let len = u32::from_be_bytes(len_buf) as usize;
            let mut buf = vec![0u8; len];
            io.read_exact(&mut buf).await?;
            serde_json::from_slice(&buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        }

        async fn read_response<T>(
            &mut self,
            _: &FileProtocol,
            io: &mut T,
        ) -> io::Result<Self::Response>
        where
            T: AsyncRead + Unpin + Send,
        {
            let mut len_buf = [0u8; 4];
            io.read_exact(&mut len_buf).await?;
            let len = u32::from_be_bytes(len_buf) as usize;
            let mut buf = vec![0u8; len];
            io.read_exact(&mut buf).await?;
            serde_json::from_slice(&buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        }

        async fn write_request<T>(
            &mut self,
            _: &FileProtocol,
            io: &mut T,
            req: FileRequest,
        ) -> io::Result<()>
        where
            T: AsyncWrite + Unpin + Send,
        {
            let bytes = serde_json::to_vec(&req).unwrap();
            let len = (bytes.len() as u32).to_be_bytes();
            io.write_all(&len).await?;
            io.write_all(&bytes).await?;
            io.flush().await
        }

        async fn write_response<T>(
            &mut self,
            _: &FileProtocol,
            io: &mut T,
            resp: FileBlob,
        ) -> io::Result<()>
        where
            T: AsyncWrite + Unpin + Send,
        {
            let bytes = serde_json::to_vec(&resp).unwrap();
            let len = (bytes.len() as u32).to_be_bytes();
            io.write_all(&len).await?;
            io.write_all(&bytes).await?;
            io.flush().await
        }
    }
}
