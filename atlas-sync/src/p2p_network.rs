pub mod p2p_network {
    use crate::item::item::ItemUpdate;
    use libp2p::{
        floodsub::{Floodsub, FloodsubEvent, Topic},
        identity,
        mdns::{Mdns, MdnsEvent},
        swarm::NetworkBehaviourEventProcess,
        NetworkBehaviour, PeerId,
    };
    use log::info;
    use once_cell::sync::Lazy;
    use serde::{Deserialize, Serialize};
    use tokio::sync::mpsc;

    pub static KEYS: Lazy<identity::Keypair> = Lazy::new(|| identity::Keypair::generate_ed25519());
    pub static PEER_ID: Lazy<PeerId> = Lazy::new(|| PeerId::from(KEYS.public()));
    pub static TOPIC: Lazy<Topic> = Lazy::new(|| Topic::new("FILE_SHARING"));

    #[derive(Debug, Serialize, Deserialize)]
    pub enum FileEventType {
        Created(ItemUpdate),
        Updated(ItemUpdate),
        Deleted(ItemUpdate),
        Moved(ItemUpdate),
    }

    #[derive(NetworkBehaviour)]
    pub struct MyFileBehavior {
        pub floodsub: Floodsub,
        pub mdns: Mdns,
        #[behaviour(ignore)]
        pub response_sender: mpsc::UnboundedSender<FileEventType>,
    }

    impl NetworkBehaviourEventProcess<FloodsubEvent> for MyFileBehavior {
        fn inject_event(&mut self, event: FloodsubEvent) {
            match event {
                FloodsubEvent::Message(msg) => {
                    if let Ok(resp) = serde_json::from_slice::<FileEventType>(&msg.data) {
                        info!("Response from {}:", msg.source);
                        info!("Response data: {:?}", resp);
                    }
                }
                _ => (),
            }
        }
    }

    impl NetworkBehaviourEventProcess<MdnsEvent> for MyFileBehavior {
        fn inject_event(&mut self, event: MdnsEvent) {
            match event {
                MdnsEvent::Discovered(discovered_list) => {
                    for (peer, _addr) in discovered_list {
                        self.floodsub.add_node_to_partial_view(peer);
                    }
                }
                MdnsEvent::Expired(expired_list) => {
                    for (peer, _addr) in expired_list {
                        if !self.mdns.has_node(&peer) {
                            self.floodsub.remove_node_from_partial_view(&peer);
                        }
                    }
                }
            }
        }
    }
}
