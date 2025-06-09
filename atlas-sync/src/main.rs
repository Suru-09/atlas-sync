mod args_parser;
mod coordinator;
mod crdt;
mod crdt_index;
mod fswrapper;
mod ignore_list;
mod p2p_network;
mod uuid_wrapper;
mod watcher;

use args_parser::args_parser::Args;
use clap::Parser;
use coordinator::coordinator::start_coordination;

#[tokio::main]
async fn main() {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "debug")
    }
    pretty_env_logger::init();

    let args = Args::parse();
    start_coordination(args).await;
}
