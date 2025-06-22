pub mod watcher {
    use crate::crdt::crdt::{JsonNode, Mutation};
    use crate::crdt_index::crdt_index::IndexCmd;
    use crate::fswrapper::fswrapper::{
        compute_file_absolute_path, compute_file_relative_path, last_name, path_to_vec, EntryMeta,
    };
    use log::{debug, error, info};
    use notify::event::{CreateKind, MetadataKind, ModifyKind, RemoveKind, RenameMode};
    use notify::{
        Event, EventKind, RecommendedWatcher, RecursiveMode, Result as NotifyResult, Watcher,
    };
    use once_cell::sync::Lazy;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::channel;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::sync::mpsc::UnboundedSender;

    pub static RECENTLY_WRITTEN: Lazy<Arc<Mutex<Vec<String>>>> =
        Lazy::new(|| Arc::new(Mutex::new(Vec::new())));

    pub fn watch_path(path: &Path, index_tx: UnboundedSender<IndexCmd>) -> NotifyResult<()> {
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
                        if event.paths.iter().any(|p| {
                            p.file_name().map_or(false, |name| {
                                let name_str = name.to_str().unwrap_or("");
                                let mut vec = RECENTLY_WRITTEN.lock().unwrap();
                                info!(
                                    "Should this be ignored?: {}, rec WRITTEN: {:?}",
                                    name_str, vec
                                );

                                let mut set_contains = false;
                                if let Some(pos) = vec
                                    .iter()
                                    .position(|val| last_name(Path::new(val)).unwrap() == name_str)
                                {
                                    vec.remove(pos);
                                    set_contains = true;
                                }

                                name == "index.json"
                                    || name_str.contains(".goutput")
                                    || set_contains
                            })
                        }) {
                            debug!("Skiping files from event paths: {:?}", event.paths);
                            continue;
                        }

                        match event.kind {
                            EventKind::Access(_) => {
                                // interesting only for initial connections, generally ignored.
                            }
                            EventKind::Create(create_kind) => {
                                if let Some(new_cmd) = extract_new_cmd(&event.paths, &create_kind) {
                                    info!("Sending new cmd: {:?}", new_cmd);
                                    let _ = index_tx.send(new_cmd);
                                }
                            }
                            EventKind::Modify(modify_kind) => {
                                for cmd in extract_update_cmd(&event.paths, &modify_kind) {
                                    match cmd {
                                        Some(command) => {
                                            if let Err(e) = index_tx.send(command) {
                                                error!(
                                                    "Failed sending update command due to err: {}",
                                                    e
                                                );
                                            }
                                        }
                                        _ => {
                                            error!("Extract update command failed miserably!");
                                        }
                                    }
                                }
                            }
                            EventKind::Remove(remove_kind) => {
                                if let Some(delete_cmd) =
                                    extract_remove_op(&event.paths, &remove_kind)
                                {
                                    info!("Sending DELETE cmd: {:?}", delete_cmd);
                                    let _ = index_tx.send(delete_cmd);
                                }
                            }
                            EventKind::Other | EventKind::Any => {
                                error!("Other or any event type: {:?}", event);
                            }
                        }
                    }
                    Err(e) => error!("watch error: {:?}", e),
                }
            }
        });

        Ok(())
    }

    fn extract_new_cmd(paths: &Vec<PathBuf>, create_kind: &CreateKind) -> Option<IndexCmd> {
        assert!(paths.len() == 1); // why would I have multiple paths on a create operation?
        let path = compute_file_relative_path(paths.first().unwrap());
        let abs_path = compute_file_absolute_path(&path);

        match create_kind {
            CreateKind::Any | CreateKind::Other => {
                error!("Why am I receiving Other/Any on create operation? create_kind: {:?} with path: {:?}", create_kind, path);
                None
            }
            CreateKind::File => {
                let file_metadata = EntryMeta::from_path(&abs_path).unwrap();
                Some(IndexCmd::LocalOp {
                    cur: path_to_vec(&path),
                    mutation: Mutation::New {
                        key: path.to_string_lossy().into_owned(),
                        value: JsonNode::Entry(file_metadata),
                    },
                })
            }
            CreateKind::Folder => {
                let file_metadata = EntryMeta::from_path(&abs_path).unwrap();
                Some(IndexCmd::LocalOp {
                    cur: path_to_vec(&path),
                    mutation: Mutation::New {
                        key: path.to_string_lossy().into_owned(),
                        value: JsonNode::Entry(file_metadata),
                    },
                })
            }
        }
    }

    fn extract_remove_op(paths: &Vec<PathBuf>, remove_kind: &RemoveKind) -> Option<IndexCmd> {
        assert!(paths.len() == 1); // why would I have multiple paths on a create operation?
        let path = compute_file_relative_path(paths.first().unwrap());

        match remove_kind {
            RemoveKind::Any | RemoveKind::Other => {
                error!(
                    "Neither folder nor file? {:?} and path: {:?}",
                    remove_kind, path
                );
                None
            }
            RemoveKind::File => Some(IndexCmd::LocalOp {
                cur: path_to_vec(&path),
                mutation: Mutation::Delete {
                    key: path.to_string_lossy().into_owned(),
                },
            }),
            RemoveKind::Folder => Some(IndexCmd::LocalOp {
                cur: path_to_vec(&path),
                mutation: Mutation::Delete {
                    key: path.to_string_lossy().into_owned(),
                },
            }),
        }
    }

    fn extract_update_cmd(paths: &Vec<PathBuf>, modify_kind: &ModifyKind) -> Vec<Option<IndexCmd>> {
        debug!(
            "[extract_update_cmd] Update event: {:?} with paths: {:?}",
            modify_kind, paths
        );
        if paths.len() >= 3 || paths.len() < 1 {
            panic!("Should be some logical value...");
        }

        let mut file_metadata = EntryMeta {
            name: String::from("placeholder"),
            path: String::from("placeholder"),
            is_directory: false,
            accessed: None,
            modified: None,
            created: None,
            permissions: None,
            size: None,
            content_hash: None,
            owner: None,
        };
        let path;

        match modify_kind {
            ModifyKind::Any | ModifyKind::Other => {
                assert!(paths.len() == 1);
                path = compute_file_relative_path(paths.first().unwrap());
                error!("Why am I receiving Other/Any on update operation? update_kind: {:?} with path: {:?}", modify_kind, path);
                vec![]
            }
            ModifyKind::Data(_) => {
                assert!(paths.len() == 1);
                path = compute_file_relative_path(paths.first().unwrap());
                let abs_path = compute_file_absolute_path(&path);

                file_metadata = EntryMeta::from_path(&abs_path).unwrap();
                vec![Some(IndexCmd::LocalOp {
                    cur: path_to_vec(&path),
                    mutation: Mutation::Edit {
                        key: path.to_string_lossy().into_owned(),
                        value: JsonNode::Entry(file_metadata),
                    },
                })]
            }
            ModifyKind::Metadata(metadata_kind) => {
                assert!(paths.len() == 1);
                path = compute_file_relative_path(paths.first().unwrap());
                let abs_path = compute_file_absolute_path(&path);

                file_metadata = EntryMeta::from_path(&abs_path).unwrap();
                match metadata_kind {
                    MetadataKind::Ownership => {
                        file_metadata.owner = Some(String::from("Suru"));
                    }
                    MetadataKind::Permissions => {
                        file_metadata.permissions = Some(777);
                    }
                    MetadataKind::WriteTime => {
                        file_metadata.modified = Some(
                            SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap()
                                .as_secs(),
                        )
                    }
                    _ => {}
                }
                vec![Some(IndexCmd::LocalOp {
                    cur: path_to_vec(&path),
                    mutation: Mutation::Edit {
                        key: path.to_string_lossy().into_owned(),
                        value: JsonNode::Entry(file_metadata),
                    },
                })]
            }
            ModifyKind::Name(name) => match name {
                RenameMode::Both => {
                    let path = compute_file_relative_path(paths.first().unwrap());
                    let abs_path = compute_file_absolute_path(&path);
                    file_metadata = EntryMeta::from_path(&abs_path).unwrap_or(file_metadata);

                    let delete_op = IndexCmd::LocalOp {
                        cur: path_to_vec(&path),
                        mutation: Mutation::Delete {
                            key: path.to_string_lossy().into_owned(),
                        },
                    };

                    let renamed_path = compute_file_relative_path(paths.get(1).unwrap());
                    file_metadata.name =
                        last_name(&renamed_path).unwrap_or_else(|| String::from("empty_name??"));
                    file_metadata.path = renamed_path.to_string_lossy().into_owned();

                    let new_op = IndexCmd::LocalOp {
                        cur: path_to_vec(&renamed_path),
                        mutation: Mutation::New {
                            key: renamed_path.to_string_lossy().into_owned(),
                            value: JsonNode::Entry(file_metadata),
                        },
                    };

                    vec![Some(delete_op), Some(new_op)]
                }
                // for some reason this one is editing a file...
                RenameMode::To => {
                    let path = compute_file_relative_path(paths.first().unwrap());
                    let abs_path = compute_file_absolute_path(&path);
                    file_metadata = EntryMeta::from_path(&abs_path).unwrap_or(file_metadata);

                    let update_op = IndexCmd::LocalOp {
                        cur: path_to_vec(&path),
                        mutation: Mutation::Edit {
                            key: path.to_string_lossy().into_owned(),
                            value: JsonNode::Entry(file_metadata),
                        },
                    };

                    vec![Some(update_op)]
                }
                // for some reason this one is deleting a file...
                RenameMode::From => {
                    let path = compute_file_relative_path(paths.first().unwrap());
                    let update_op = IndexCmd::LocalOp {
                        cur: path_to_vec(&path),
                        mutation: Mutation::Delete {
                            key: path.to_string_lossy().into_owned(),
                        },
                    };

                    vec![Some(update_op)]
                }
                _ => vec![],
            },
        }
    }
}
