pub mod watcher {
    use notify::{recommended_watcher, Event, RecursiveMode, Watcher};
    use std::path::Path;
    use std::sync::mpsc;
    use tokio::sync::mpsc::Sender;

    pub fn watch_path(path: &Path, file_tx: Sender<String>) -> notify::Result<()> {
        let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
        let mut watcher = recommended_watcher(tx)?;

        watcher.watch(path, RecursiveMode::Recursive)?;
        for res in rx {
            match res {
                Ok(event) => {
                    let _ = file_tx.send(format!("{event:?}"));
                    println!("event: {:?}", event.kind);
                }
                Err(e) => println!("watch error: {:?}", e),
            }
        }
        Ok(())
    }
}
