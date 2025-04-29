mod coordinator;
mod crdt;
mod ignore_list;
mod index;
mod item;
mod uuid_wrapper;
mod watcher;

use crdt::crdt::*;
use std::path::Path;
use watcher::watcher::watch_path;

fn main() -> notify::Result<()> {
    watch_path(Path::new("src/resources/"))?;
    Ok(())
}
