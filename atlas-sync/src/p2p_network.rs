pub mod p2p_network {
    use crate::file::file::FileEventType;
    use libp2p::{
        floodsub::{Floodsub, FloodsubEvent, Topic},
        identity,
        mdns::{Mdns, MdnsEvent},
        swarm::NetworkBehaviourEventProcess,
        NetworkBehaviour, PeerId,
    };
    use log::{debug, error, info};
    use once_cell::sync::Lazy;
    use serde::{Deserialize, Serialize};

    pub static KEYS: Lazy<identity::Keypair> = Lazy::new(|| identity::Keypair::generate_ed25519());
    pub static PEER_ID: Lazy<PeerId> = Lazy::new(|| PeerId::from(KEYS.public()));
    pub static TOPIC: Lazy<Topic> = Lazy::new(|| Topic::new("FILE_SHARING"));

    #[derive(NetworkBehaviour)]
    pub struct AtlasSyncBehavior {
        pub floodsub: Floodsub,
        pub mdns: Mdns,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub enum PeerConnectionEvent {
        InitialConnection(String),
    }

    impl NetworkBehaviourEventProcess<FloodsubEvent> for AtlasSyncBehavior {
        fn inject_event(&mut self, event: FloodsubEvent) {
            match event {
                FloodsubEvent::Message(msg) => {
                    if let Ok(parsed) = serde_json::from_slice::<FileEventType>(&msg.data) {
                        info!("Got event: {:?}", parsed)
                    } else if let Ok(parsed) =
                        serde_json::from_slice::<PeerConnectionEvent>(&msg.data)
                    {
                        info!("Yeah I am in!!!: {:?}", parsed);
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
