pub mod args_parser {
    use clap::Parser;

    #[derive(Debug, Parser)]
    #[clap(author, version, about, long_about = None)]
    pub struct Args {
        // path to be watched
        #[clap(short, long, default_value_t = String::new())]
        pub watch_path: String,
        // peer ID of the host you're connecting to
        #[clap(short, long, default_value_t = String::new())]
        pub peer_id: String,
    }
}
