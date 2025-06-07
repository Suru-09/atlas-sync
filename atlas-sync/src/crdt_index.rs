pub mod crdt_index {
    use crate::crdt::crdt::{JsonNode, LamportTimestamp, Mutation, Operation, VersionVector};
    use crate::fswrapper::fswrapper::{compute_file_relative_path, FileBlob, EntryMeta};
    use crate::fswrapper::fswrapper::{INDEX_NAME, WATCHED_PATH};
    use serde::{Deserialize, Serialize};
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};
    use uuid::Uuid;
    use walkdir::WalkDir;
    use log::{trace, info, error};

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct CRDTIndex {
        pub replica_id: Uuid,
        root: JsonNode,
        root_path: String,
        clock: u64,
        vv: VersionVector,
        applied: HashSet<LamportTimestamp>,
        pub op_log: Vec<Operation>,
    }

    impl CRDTIndex {
        pub fn new(replica_id: Uuid, root_path: String) -> Self {
            Self {
                replica_id,
                root: JsonNode::new_map(),
                root_path,
                clock: 0,
                vv: VersionVector::default(),
                applied: HashSet::new(),
                op_log: Vec::new(),
            }
        }

        pub fn next_ts(&mut self) -> LamportTimestamp {
            self.clock += 1;
            LamportTimestamp {
                counter: self.clock,
                replica_id: self.replica_id,
            }
        }

        pub fn record_apply(&mut self, op: Operation) -> Operation {
            let _ = self.root.apply(&op, &mut self.applied);
            self.vv.record(&op.id);
            self.op_log.push(op.clone());
            op
        }

        fn current_deps(&self) -> HashSet<LamportTimestamp> {
            self.vv
                .0
                .iter()
                .map(|(rid, c)| LamportTimestamp {
                    counter: *c,
                    replica_id: *rid,
                })
                .collect()
        }

        pub fn apply_local_op(&mut self, cursor: &[String], mutation: Mutation) -> Operation {
            match mutation.clone() {
                Mutation::New { key, value } => {
                    self.insert(cursor, key, value);
                }
                Mutation::Edit { key, value } => {
                    self.edit(cursor, key, value);
                }
                Mutation::Delete { key } => {
                    self.delete(cursor, key);
                }
            }

            let op = self.make_op(cursor.to_vec(), mutation);
            self.record_apply(op.clone());
            op
        }

        pub fn insert(&mut self, cursor: &[String], key: String, value: JsonNode) -> Operation {
            let id = self.next_ts();
            let deps = self.current_deps();
            let cur: Vec<_> = cursor.iter().cloned().collect();
            let op = Operation {
                id,
                deps,
                cursor: cur,
                mutation: Mutation::New { key, value },
            };
            self.record_apply(op)
        }

        pub fn edit(&mut self, cursor: &[String], key: String, value: JsonNode) -> Operation {
            let id = self.next_ts();
            let deps = self.current_deps();
            let cur: Vec<_> = cursor.iter().cloned().collect();
            let op = Operation {
                id,
                deps,
                cursor: cur,
                mutation: Mutation::Edit { key, value },
            };
            self.record_apply(op)
        }

        pub fn delete(&mut self, cursor: &[String], key: String) -> Operation {
            let id = self.next_ts();
            let deps = self.current_deps();
            let cur: Vec<_> = cursor.iter().cloned().collect();
            let op = Operation {
                id,
                deps,
                cursor: cur,
                mutation: Mutation::Delete { key },
            };
            self.record_apply(op)
        }

        pub fn apply_remote(&mut self, op: &Operation) -> bool {
            if self.applied.contains(&op.id) || !op.deps.iter().all(|d| self.applied.contains(d)) {
                return false; // duplicate or out‑of‑causal‑order
            }
            let ok = self.root.apply(op, &mut self.applied);
            if ok {
                self.vv.record(&op.id);
                self.op_log.push(op.clone());
            }
            ok
        }

        pub fn summary(&self) -> &VersionVector {
            &self.vv
        }

        pub fn compact(&mut self, retain_after: &VersionVector) {
            self.root.compress();
            self.op_log.retain(|op| !retain_after.dominates(&op.id));
        }

        pub fn make_op(&mut self, cursor: Vec<String>, mutation: Mutation) -> Operation {
            let op = Operation {
                id: self.next_ts(),
                cursor: cursor,
                deps: self.current_deps(),
                mutation,
            };

            op
        }

        pub fn load_or_init(replica_id: Uuid, root_path: String) -> std::io::Result<Self> {
            let path = Path::new(&root_path);
            if path.exists() {
                let bytes = std::fs::read(path)?;
                let mut idx: CRDTIndex = serde_json::from_slice(&bytes)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

                idx.applied = idx.op_log.iter().map(|op| op.id.clone()).collect();
                for id in &idx.applied {
                    idx.vv.record(id);
                }
                return Ok(idx);
            }

            /* ---------- Cold start: build from filesystem ------------------ */
            let mut idx = CRDTIndex::new(replica_id, root_path.clone());
            let mut components = path.components();
            components.next_back(); // remove the last component
            let watched_path = components.as_path();

            println!("Path: {:?}", path);
            println!("Watched path: {:?}", watched_path);

            for entry in WalkDir::new(watched_path)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|e| e.file_type().is_file() || e.file_type().is_dir())
            {
                let rel = compute_file_relative_path(entry.path());
                let cursor: Vec<String> = rel
                    .components()
                    .map(|c| String::from(c.as_os_str().to_str().unwrap()))
                    .collect();

                // error!("Entry path: {:?}, prefix remove: {:?}",
                //   entry.path(), rel);
                // info!("Cursor: {:?}", cursor);

                let key = entry.file_name().to_string_lossy().to_string();
                let meta = EntryMeta::from_path(entry.path())?;
                let mutation = Mutation::New {
                    key: key.clone(),
                    value: JsonNode::Entry(
                      meta
                    ),
                };

                let op = idx.make_op(cursor, mutation);
                idx.record_apply(op.clone());
            }

            let _ = idx.save_to_disk();
            Ok(idx)
        }

        pub fn save_to_disk(&self) -> std::io::Result<()> {
            let path = Path::new(&self.root_path);
            error!("Index: {:?}", self.root);
            let json = serde_json::to_vec_pretty(&self)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            error!("Writing to disk to path: {:?}", path);
            std::fs::write(path, json)
        }
    }

    #[derive(Debug)]
    pub enum IndexCmd {
        LocalOp {
            mutation: Mutation,
            cur: Vec<String>,
        },
        RemoteOp {
            mutation: Mutation,
            cur: Vec<String>,
        },
    }
}
