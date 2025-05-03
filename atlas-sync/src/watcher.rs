pub mod watcher {
    use crate::item::item::{ItemUpdate, ItemUpdateType};
    use crate::FileEventType;
    use log::{error, info};
    use notify::event::AccessKind;
    use notify::{
        Event, EventKind, RecommendedWatcher, RecursiveMode, Result as NotifyResult, Watcher,
    };
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
                    Ok(event) => match event.kind {
                        EventKind::Access(_) => {
                            // not really interesting in a file sharing app.
                        }
                        EventKind::Create(create_kind) => {
                            info!("{:?}", create_kind);
                        }
                        EventKind::Modify(modify_kind) => {
                            info!("{:?}", modify_kind);
                        }
                        EventKind::Remove(remove_kind) => {
                            info!("{:?}", remove_kind);
                        }
                        EventKind::Other => {
                            info!("Other Event type");
                        }
                        _ => {
                            panic!("Unrecognized event!")
                        }
                    },
                    Err(e) => error!("watch error: {:?}", e),
                }
            }
        });

        Ok(())
    }
}
