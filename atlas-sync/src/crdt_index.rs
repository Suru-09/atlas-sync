pub mod crdt_index {
    use crate::crdt::crdt::{JsonNode, LamportTimestamp, Mutation, Operation, VersionVector};
    use crate::fswrapper::fswrapper::{compute_file_relative_path, EntryMeta};
    use crate::p2p_network::p2p_network::PEER_ID;
    use log::{debug, error, info, warn};
    use serde::{Deserialize, Serialize};
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};
    use std::{fs, io};
    use walkdir::WalkDir;

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct CRDTIndex {
        pub replica_id: String,
        root: JsonNode,
        root_path: String,
        clock: u64,
        vv: VersionVector,
        applied: HashSet<LamportTimestamp>,
        pub op_log: Vec<Operation>,
    }

    impl CRDTIndex {
        pub fn new(replica_id: String, root_path: String) -> Self {
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
                replica_id: PEER_ID.to_string(),
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
                    replica_id: rid.clone(),
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
                debug!("I am deduplicating op: {:?}", op);
                return false; // duplicate or out‑of‑causal‑order
            }
            let ok = self.root.apply(op, &mut self.applied);
            if ok {
                self.vv.record(&op.id);
                self.op_log.push(op.clone());
            }
            ok
        }

        pub fn _summary(&self) -> &VersionVector {
            &self.vv
        }

        pub fn _compact(&mut self, retain_after: &VersionVector) {
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

        pub fn load_or_init(replica_id: String, root_path: String) -> std::io::Result<Self> {
            let path = Path::new(&root_path);
            if path.exists() {
                let bytes = std::fs::read(path)?;
                let mut idx: CRDTIndex = serde_json::from_slice(&bytes)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

                idx.applied = idx.op_log.iter().map(|op| op.id.clone()).collect();
                for id in &idx.applied {
                    idx.vv.record(id);
                }

                match idx.check_integrity() {
                    Ok(()) => return Ok(idx),
                    Err(e) => {
                        warn!(
                            "Check integrity failed due to: {}, fallback on cold start",
                            e
                        );
                    }
                }
            }

            /* ---------- Cold start: build from filesystem ------------------ */
            let mut idx = CRDTIndex::new(replica_id, root_path.clone());
            let mut components = path.components();
            components.next_back(); // remove the last component
            let watched_path = components.as_path();

            // println!("Path: {:?}", path);
            // println!("Watched path: {:?}", watched_path);

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
                    value: JsonNode::Entry(meta),
                };

                let op = idx.make_op(cursor, mutation);
                idx.record_apply(op.clone());
            }

            match idx.save_to_disk() {
                Ok(_) => info!("Writing index to disk..."),
                Err(e) => error!("Could not write index to disk due to: {:?}", e),
            }
            Ok(idx)
        }

        pub fn save_to_disk(&self) -> std::io::Result<()> {
            let path = Path::new(&self.root_path);
            let json = serde_json::to_vec_pretty(&self)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
            debug!("Writing to disk to path: {:?}", path);
            std::fs::write(path, json)
        }

        pub fn check_integrity(&self) -> io::Result<()> {
            fn collect_entries<'a>(
                node: &'a JsonNode,
                path: PathBuf,
                entries: &mut Vec<(PathBuf, &'a EntryMeta)>,
            ) {
                match node {
                    JsonNode::Entry(meta) => {
                        entries.push((path, meta));
                    }
                    JsonNode::Map(map) => {
                        for (name, child) in map {
                            let mut child_path = path.clone();
                            child_path.push(name);
                            collect_entries(child, child_path, entries);
                        }
                    }
                    _ => {}
                }
            }

            let fs_root = Path::new(&self.root_path);
            let mut entries = Vec::new();
            collect_entries(&self.root, PathBuf::new(), &mut entries);

            for (rel_path, meta) in &entries {
                let abs_path = fs_root.join(rel_path);

                if meta.is_directory {
                    if !abs_path.exists() {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("Directory: {:?} does not exist!", abs_path),
                        ));
                    }
                } else {
                    if !abs_path.exists() {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!("File: {:?} does not exist!", abs_path),
                        ));
                    } else {
                    }
                }
            }

            Ok(())
        }

        pub fn compute_missing_ops(&self, remote_vv: &VersionVector) -> Vec<Operation> {
            self.op_log
                .iter()
                .filter(|op| {
                    let remote_seen = remote_vv.0.get(&op.id.replica_id).copied().unwrap_or(0);
                    op.id.counter > remote_seen
                })
                .cloned()
                .collect()
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

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::crdt::crdt::{JsonNode, Mutation};
        use std::time::Instant;

        fn make_mutation(i: usize, variant: &str) -> Mutation {
            let key = format!("file_{}", i);
            let value = JsonNode::Entry(EntryMeta {
                name: format!("name_{}", i),
                path: format!("path/to/file_{}", i),
                is_directory: false,
                accessed: Some(0),
                modified: Some(0),
                created: Some(0),
                permissions: Some(0o755),
                size: Some(1234),
                owner: Some("owner".into()),
                content_hash: Some(
                    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
                ),
            });

            match variant {
                "new" => Mutation::New { key, value },
                "edit" => Mutation::Edit { key, value },
                "delete" => Mutation::Delete { key },
                _ => panic!("Unknown mutation variant"),
            }
        }

        #[test]
        fn apply_10_local_new() {
            timed_local_test("new", 10);
        }
        #[test]
        fn apply_100_local_new() {
            timed_local_test("new", 100);
        }
        #[test]
        fn apply_1000_local_new() {
            timed_local_test("new", 1000);
        }

        #[test]
        fn apply_10_local_edit() {
            timed_local_test("edit", 10);
        }
        #[test]
        fn apply_100_local_edit() {
            timed_local_test("edit", 100);
        }
        #[test]
        fn apply_1000_local_edit() {
            timed_local_test("edit", 1000);
        }

        #[test]
        fn apply_10_local_delete() {
            timed_local_test("delete", 10);
        }
        #[test]
        fn apply_100_local_delete() {
            timed_local_test("delete", 100);
        }
        #[test]
        fn apply_1000_local_delete() {
            timed_local_test("delete", 1000);
        }

        #[test]
        fn apply_10_remote_new() {
            timed_remote_test("new", 10);
        }
        #[test]
        fn apply_100_remote_new() {
            timed_remote_test("new", 100);
        }
        #[test]
        fn apply_1000_remote_new() {
            timed_remote_test("new", 1000);
        }

        #[test]
        fn apply_10_remote_edit() {
            timed_remote_test("edit", 10);
        }
        #[test]
        fn apply_100_remote_edit() {
            timed_remote_test("edit", 100);
        }
        #[test]
        fn apply_1000_remote_edit() {
            timed_remote_test("edit", 1000);
        }

        #[test]
        fn apply_10_remote_delete() {
            timed_remote_test("delete", 10);
        }
        #[test]
        fn apply_100_remote_delete() {
            timed_remote_test("delete", 100);
        }
        #[test]
        fn apply_1000_remote_delete() {
            timed_remote_test("delete", 1000);
        }

        fn timed_local_test(variant: &str, count: usize) {
            let mut index = CRDTIndex::new(PEER_ID.to_string(), "dummy_path.json".to_string());
            let start = Instant::now();
            for i in 0..count {
                let cursor = vec!["root".to_string()];
                let mutation = make_mutation(i, variant);
                index.apply_local_op(&cursor, mutation);
            }
            let elapsed = start.elapsed().as_micros();
            println!(
                "Applied {} local {} ops in {} µs",
                count,
                variant.to_uppercase(),
                elapsed
            );
            //assert!(false);
        }

        fn timed_remote_test(variant: &str, count: usize) {
            let mut index = CRDTIndex::new(PEER_ID.to_string(), "dummy_path.json".to_string());
            let mut ops = Vec::new();
            for i in 0..count {
                let cursor = vec!["root".to_string()];
                let mutation = make_mutation(i, variant);
                let op = index.make_op(cursor, mutation);
                ops.push(op);
            }

            let start = Instant::now();
            for op in ops {
                index.apply_remote(&op);
            }
            let elapsed = start.elapsed().as_micros();
            println!(
                "Applied {} remote {} ops in {} µs",
                count,
                variant.to_uppercase(),
                elapsed
            );
            //assert!(false);
        }
    }
}
