pub mod watcher {
    use crate::file::file::{
        CreateOp, DeleteOp, FileChange, FileEventType, FileMetadata, LogicalTimestamp, UpdateOp,
    };
    use log::{error, info};
    use notify::event::{CreateKind, DataChange, MetadataKind, ModifyKind, RemoveKind, RenameMode};
    use notify::{
        Event, EventKind, RecommendedWatcher, RecursiveMode, Result as NotifyResult, Watcher,
    };
    use std::any::Any;
    use std::path::{Path, PathBuf};
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
                            let create_op = extract_create_op(&event.paths, &create_kind);
                            if let Some(c_op) = create_op {
                                let _ = file_tx.send(FileEventType::Created(c_op));
                            }
                        }
                        EventKind::Modify(modify_kind) => {
                            let updated_op = extract_update_op(&event.paths, &modify_kind);
                            if let Some(u_op) = updated_op {
                                if u_op.changes.len() > 0 {
                                    let _ = file_tx.send(FileEventType::Updated(u_op));
                                }
                            }
                        }
                        EventKind::Remove(remove_kind) => {
                            let delete_op = extract_remove_op(&event.paths, &remove_kind);
                            if let Some(del_op) = delete_op {
                                let _ = file_tx.send(FileEventType::Deleted(del_op));
                            }
                        }
                        EventKind::Other | EventKind::Any => {
                            error!("Other or any event type: {:?}", event);
                        }
                    },
                    Err(e) => error!("watch error: {:?}", e),
                }
            }
        });

        Ok(())
    }

    fn extract_create_op(paths: &Vec<PathBuf>, create_kind: &CreateKind) -> Option<CreateOp> {
        assert!(paths.len() == 1); // why would I have multiple paths on a create operation?
        let path = paths.first().unwrap().clone();

        match create_kind {
            CreateKind::Any | CreateKind::Other => {
                error!("Why am I receiving Other/Any on create operation? create_kind: {:?} with path: {:?}", create_kind, path);
                None
            }
            CreateKind::File => {
                let file_metadata = FileMetadata {
                    logical_time: LogicalTimestamp(1),
                    is_directory: false,
                };
                Some(CreateOp {
                    metadata: file_metadata,
                    path,
                })
            }
            CreateKind::Folder => {
                let file_metadata = FileMetadata {
                    logical_time: LogicalTimestamp(2),
                    is_directory: true,
                };
                Some(CreateOp {
                    metadata: file_metadata,
                    path,
                })
            }
        }
    }

    fn extract_remove_op(paths: &Vec<PathBuf>, remove_kind: &RemoveKind) -> Option<DeleteOp> {
        assert!(paths.len() == 1); // why would I have multiple paths on a create operation?
        let path = paths.first().unwrap().clone();

        match remove_kind {
            RemoveKind::Any | RemoveKind::Other => {
                error!(
                    "Neither folder nor file? {:?} and path: {:?}",
                    remove_kind, path
                );
                None
            }
            RemoveKind::File => {
                let file_metadata = FileMetadata {
                    logical_time: LogicalTimestamp(1),
                    is_directory: false,
                };
                Some(DeleteOp {
                    metadata: file_metadata,
                    path,
                })
            }
            RemoveKind::Folder => {
                let file_metadata = FileMetadata {
                    logical_time: LogicalTimestamp(2),
                    is_directory: true,
                };
                Some(DeleteOp {
                    metadata: file_metadata,
                    path,
                })
            }
        }
    }

    fn extract_update_op(paths: &Vec<PathBuf>, modify_kind: &ModifyKind) -> Option<UpdateOp> {
        if paths.len() >= 3 || paths.len() < 1 {
            panic!("Should be some logical value...");
        }

        let metadata = FileMetadata {
            logical_time: LogicalTimestamp(0),
            is_directory: false,
        };

        let path;
        let mut changes = vec![];

        match modify_kind {
            ModifyKind::Any | ModifyKind::Other => {
                assert!(paths.len() == 1);
                path = paths.first().unwrap().clone();
                error!("Why am I receiving Other/Any on update operation? update_kind: {:?} with path: {:?}", modify_kind, path);
                None
            }
            ModifyKind::Data(data_change) => {
                assert!(paths.len() == 1);
                path = paths.first().unwrap().clone();
                match data_change {
                    DataChange::Any | DataChange::Other => {
                        changes.push(FileChange::ContentHash(String::from(
                            "any_other_data_change",
                        )));
                    }
                    DataChange::Size => {
                        changes.push(FileChange::ContentHash(String::from("size_data_change")));
                    }
                    DataChange::Content => {
                        changes.push(FileChange::ContentHash(String::from("content_data_change")));
                    }
                }
                Some(UpdateOp {
                    metadata,
                    path,
                    changes,
                })
            }
            ModifyKind::Metadata(metadata_kind) => {
                assert!(paths.len() == 1);
                path = paths.first().unwrap().clone();
                match metadata_kind {
                    MetadataKind::Ownership => {
                        changes.push(FileChange::Owner(String::from("HihihiHahaha")));
                    }
                    MetadataKind::Permissions => {
                        changes.push(FileChange::Permissions(644));
                    }
                    MetadataKind::WriteTime => {
                        changes.push(FileChange::TimestampModified(21321321));
                    }
                    _ => {}
                }
                Some(UpdateOp {
                    metadata,
                    path,
                    changes,
                })
            }
            ModifyKind::Name(name) => {
                info!("Name: {:?}", name);
                path = match name {
                    RenameMode::Both => {
                        let tmp_path = paths.get(1).unwrap().clone();
                        let name = tmp_path
                            .file_name()
                            .map(|os_str| os_str.to_string_lossy().to_string());
                        if let Some(name) = name {
                            changes.push(FileChange::Renamed(String::from(name)));
                        }
                        tmp_path
                    }
                    _ => paths.first().unwrap().clone(),
                };
                Some(UpdateOp {
                    metadata,
                    path,
                    changes,
                })
            }
        }
    }
}
