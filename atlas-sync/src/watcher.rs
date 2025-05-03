pub mod watcher {
    use crate::item::item::{ItemUpdate, ItemUpdateType};
    use crate::FileEventType;
    use notify::{Event, RecommendedWatcher, RecursiveMode, Result as NotifyResult, Watcher};
    use std::path::Path;
    use std::sync::mpsc::channel;
    use std::thread;
    use tokio::sync::mpsc::UnboundedSender;

    pub fn watch_path(path: &Path, file_tx: UnboundedSender<FileEventType>) -> NotifyResult<()> {
        let path = path.to_path_buf();
        thread::spawn(move || {
            let (tx, rx) = channel::<notify::Result<Event>>();
            let mut watcher: RecommendedWatcher =
                notify::recommended_watcher(tx).expect("watcher creation failed");

            watcher
                .watch(&path, RecursiveMode::Recursive)
                .expect("watch failed");

            for res in rx {
                match res {
                    Ok(event) => {
                        // You can inspect `event.kind` and send different FileEventType values.
                        let _ = file_tx.send(FileEventType::Created(ItemUpdate {
                            update_type: ItemUpdateType::Created,
                            update_time: "now".to_string(),
                        }));
                        println!("event: {:?}", event.kind);
                    }
                    Err(e) => println!("watch error: {:?}", e),
                }
            }
        });

        Ok(())
    }
}
