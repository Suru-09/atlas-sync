pub mod p2p_network {
    use crate::crdt::crdt::{Mutation, Operation};
    use crate::fswrapper::fswrapper::FileBlob;
    use libp2p::{
        floodsub::{Floodsub, FloodsubEvent, Topic},
        identity,
        mdns::{Mdns, MdnsEvent},
        swarm::NetworkBehaviourEventProcess,
        NetworkBehaviour, PeerId,
    };
    use log::{debug, error, info};
    use once_cell::sync::{Lazy, OnceCell};
    use serde::{Deserialize, Serialize};
    use std::path::Path;

    pub static KEYS: Lazy<identity::Keypair> = Lazy::new(|| identity::Keypair::generate_ed25519());
    pub static PEER_ID: Lazy<PeerId> = Lazy::new(|| PeerId::from(KEYS.public()));
    pub static TOPIC: Lazy<Topic> = Lazy::new(|| Topic::new("FILE_SHARING"));
    pub static WATCHED_PATH: OnceCell<String> = OnceCell::new();

    #[derive(NetworkBehaviour)]
    pub struct AtlasSyncBehavior {
        pub floodsub: Floodsub,
        pub mdns: Mdns,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub enum PeerConnectionEvent {
        InitialConnection((String, String)),
        SyncFile((String, FileBlob)),
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
                            }
                            Mutation::Edit { key, value } => {
                                info!(
                                    "[REMOTE_EVENT] EDIT mutation with key: {:?} and value: {:?}",
                                    key, value
                                );
                            }
                            Mutation::Delete { key } => {
                                info!("[REMOTE_EVENT] DELETE mutation with key: {:?}.", key);
                            }
                        }
                    } else if let Ok(parsed) =
                        serde_json::from_slice::<PeerConnectionEvent>(&msg.data)
                    {
                        let base_path = Path::new(WATCHED_PATH.get().unwrap());
                        match parsed {
                            PeerConnectionEvent::InitialConnection((target_peer, source_peer)) => {
                                info!("Target peer: {}, Source peer: {}", target_peer, source_peer);
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
                                }
                            }
                            PeerConnectionEvent::SyncFile((target_peer, file_blob)) => {
                                if PEER_ID.to_string() == target_peer {
                                    let _ = file_blob.write_to_disk(&base_path);
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
                        info!("Peer: {} has been discovered!", peer);
                    }
                }
                MdnsEvent::Expired(expired_list) => {
                    for (peer, _addr) in expired_list {
                        if !self.mdns.has_node(&peer) {
                            info!("Peer: {} has expired!", peer);
                            self.floodsub.remove_node_from_partial_view(&peer);
                        }
                    }
                }
            }
        }
    }
}
