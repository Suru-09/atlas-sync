pub mod watcher {
    use crate::{FilesResponse, MyFile};
    use notify::{recommended_watcher, Event, RecursiveMode, Watcher};
    use std::path::Path;
    use std::sync::mpsc;
    use tokio::sync::mpsc::UnboundedSender;

    pub fn watch_path(path: &Path, file_tx: &UnboundedSender<FilesResponse>) -> notify::Result<()> {
        let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
        let mut watcher = recommended_watcher(tx)?;

        watcher.watch(path, RecursiveMode::Recursive)?;
        for res in rx {
            match res {
                Ok(event) => {
                    let _ = file_tx.send(FilesResponse {
                        data: vec![
                            MyFile {
                                name: String::from("file1"),
                            },
                            MyFile {
                                name: String::from("file2"),
                            },
                        ],
                        receiver: String::from("The watchman"),
                    });
                    println!("event: {:?}", event.kind);
                }
                Err(e) => println!("watch error: {:?}", e),
            }
        }
        Ok(())
    }
}
