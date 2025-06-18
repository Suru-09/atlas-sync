pub mod watcher {
    use crate::crdt::crdt::{JsonNode, Mutation};
    use crate::crdt_index::crdt_index::IndexCmd;
    use crate::fswrapper::fswrapper::{
        compute_file_relative_path, last_name, path_to_vec, relative_intersection, EntryMeta,
    };
    use crate::fswrapper::fswrapper::{INDEX_NAME, WATCHED_PATH};
    use log::{error, info};
    use notify::event::{CreateKind, DataChange, MetadataKind, ModifyKind, RemoveKind, RenameMode};
    use notify::{
        Event, EventKind, RecommendedWatcher, RecursiveMode, Result as NotifyResult, Watcher,
    };
    use once_cell::sync::Lazy;
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc::channel;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::sync::mpsc::UnboundedSender;

    pub static RECENTLY_WRITTEN: Lazy<Arc<Mutex<HashSet<String>>>> =
        Lazy::new(|| Arc::new(Mutex::new(HashSet::new())));

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
                                let mut set = RECENTLY_WRITTEN.lock().unwrap();
                                let set_contains = set
                                    .iter()
                                    .any(|val| last_name(Path::new(val)).unwrap() == name_str);
                                if set_contains {
                                    set.remove(name_str);
                                }
                                name == "index.json"
                                    || name_str.contains(".goutput")
                                    || set_contains
                            })
                        }) {
                            continue; // skip index.json changes
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
                                if let Some(update_cmd) =
                                    extract_update_cmd(&event.paths, &modify_kind)
                                {
                                    info!("Sending EDIT cmd: {:?}", update_cmd);
                                    let _ = index_tx.send(update_cmd);
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

        match create_kind {
            CreateKind::Any | CreateKind::Other => {
                error!("Why am I receiving Other/Any on create operation? create_kind: {:?} with path: {:?}", create_kind, path);
                None
            }
            CreateKind::File => {
                let file_metadata = EntryMeta {
                    name: path.to_str().unwrap().to_string(),
                    path: path.to_str().unwrap().to_string(),
                    is_directory: false,
                    accessed: None,
                    modified: None,
                    created: None,
                    permissions: None,
                    size: None,
                    content_hash: None,
                    owner: None,
                };
                Some(IndexCmd::LocalOp {
                    cur: path_to_vec(&path),
                    mutation: Mutation::New {
                        key: path.to_string_lossy().into_owned(),
                        value: JsonNode::Entry(file_metadata),
                    },
                })
            }
            CreateKind::Folder => {
                let file_metadata = EntryMeta {
                    name: path.to_str().unwrap().to_string(),
                    path: path.to_str().unwrap().to_string(),
                    is_directory: true,
                    accessed: None,
                    modified: None,
                    created: None,
                    permissions: None,
                    size: None,
                    content_hash: None,
                    owner: None,
                };
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

    fn extract_update_cmd(paths: &Vec<PathBuf>, modify_kind: &ModifyKind) -> Option<IndexCmd> {
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
                None
            }
            ModifyKind::Data(data_change) => {
                assert!(paths.len() == 1);
                path = compute_file_relative_path(paths.first().unwrap());

                file_metadata.name = last_name(&path).unwrap();
                file_metadata.path = path.to_str().unwrap().to_string();

                match data_change {
                    DataChange::Any | DataChange::Other => {
                        file_metadata.content_hash = Some(String::from("Data change"));
                    }
                    DataChange::Size => {
                        file_metadata.size = Some(156);
                    }
                    DataChange::Content => {
                        file_metadata.content_hash = Some(String::from("Content change"));
                    }
                }
                Some(IndexCmd::LocalOp {
                    cur: path_to_vec(&path),
                    mutation: Mutation::Edit {
                        key: path.to_string_lossy().into_owned(),
                        value: JsonNode::Entry(file_metadata),
                    },
                })
            }
            ModifyKind::Metadata(metadata_kind) => {
                assert!(paths.len() == 1);
                path = compute_file_relative_path(paths.first().unwrap());
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
                Some(IndexCmd::LocalOp {
                    cur: path_to_vec(&path),
                    mutation: Mutation::Edit {
                        key: path.to_string_lossy().into_owned(),
                        value: JsonNode::Entry(file_metadata),
                    },
                })
            }
            ModifyKind::Name(name) => {
                path = match name {
                    RenameMode::Both => {
                        let tmp_path = compute_file_relative_path(paths.first().unwrap());
                        let name = tmp_path
                            .file_name()
                            .map(|os_str| os_str.to_string_lossy().to_string());
                        if let Some(name) = name {
                            file_metadata.name = name;
                        }
                        tmp_path
                    }
                    _ => compute_file_relative_path(paths.first().unwrap()),
                };
                Some(IndexCmd::LocalOp {
                    cur: path_to_vec(&path),
                    mutation: Mutation::Edit {
                        key: path.to_string_lossy().into_owned(),
                        value: JsonNode::Entry(file_metadata),
                    },
                })
            }
        }
    }
}
