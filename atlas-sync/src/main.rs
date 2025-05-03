mod args_parser;
mod coordinator;
mod crdt;
mod file;
mod ignore_list;
mod index;
mod p2p_network;
mod uuid_wrapper;
mod watcher;

use clap::Parser;
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
use log::info;
use std::path::Path;
use tokio::sync::mpsc;

use args_parser::args_parser::Args;
use file::file::FileEventType;
use p2p_network::p2p_network::*;
use watcher::watcher::watch_path;

static WATCHED_FILE_PATH: &str = "src/resources/test_watcher";

#[tokio::main]
async fn main() {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info")
    }
    pretty_env_logger::init();

    let args = Args::parse();

    let watched_file_path = match args.watch_path.is_empty() {
        true => WATCHED_FILE_PATH,
        false => &args.watch_path,
    };

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

    watch_path(Path::new(watched_file_path), response_sender.clone())
        .expect("Failed to start file watcher");

    let mut first_time = true;
    loop {
        let evt = {
            tokio::select! {
                _ = swarm.next() => {
                    None
                },
                response = response_rcv.recv() => Some(response.expect("Has some data")),
            }
        };

        if let Some(event) = evt {
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

            // handle initial sync.
            if !args.peer_id.is_empty() && first_time {
                let json_bytes = serde_json::to_vec(&PeerConnectionEvent::InitialConnection(
                    args.peer_id.clone(),
                ))
                .expect("Should be serializable");
                swarm
                    .behaviour_mut()
                    .floodsub
                    .publish(TOPIC.clone(), json_bytes);
                first_time = false;
            }

            swarm
                .behaviour_mut()
                .floodsub
                .publish(TOPIC.clone(), json_bytes);
        }
    }
}
