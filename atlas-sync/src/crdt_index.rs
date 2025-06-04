pub mod crdt_index {
    use crate::crdt::crdt::{JsonNode, LamportTimestamp, Mutation, Operation, VersionVector};
    use std::collections::HashSet;
    use std::path::Path;
    use tokio::sync::{mpsc, oneshot};
    use uuid::Uuid;

    #[derive(Clone, Debug)]
    pub struct CRDTIndex {
        pub replica_id: Uuid,
        root: JsonNode,
        clock: u64,
        vv: VersionVector,
        applied: HashSet<LamportTimestamp>,
        pub op_log: Vec<Operation>,
    }

    impl CRDTIndex {
        pub fn new(replica_id: Uuid) -> Self {
            Self {
                replica_id,
                root: JsonNode::new_map(),
                clock: 0,
                vv: VersionVector::default(),
                applied: HashSet::new(),
                op_log: Vec::new(),
            }
        }

        /// Lamport *tick* → allocate fresh id
        pub fn next_ts(&mut self) -> LamportTimestamp {
            self.clock += 1;
            LamportTimestamp {
                counter: self.clock,
                replica_id: self.replica_id,
            }
        }

        pub fn record_apply(&mut self, op: Operation) -> Operation {
            let _ = self.root.apply(&op, &mut self.applied); // always true (deps satisfied)
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

        /// User‑level mutation APIs ------------------------------------------------
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

        /// Apply op from *remote* peer – returns true if integrated, false otherwise
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

        /// Produce the doc’s current *version vector* (cheap) – send in handshake.
        pub fn summary(&self) -> &VersionVector {
            &self.vv
        }

        /// Compress both state (tombstones) and op‑log older than `retain_after`.
        pub fn compact(&mut self, retain_after: &VersionVector) {
            self.root.compress();
            self.op_log.retain(|op| !retain_after.dominates(&op.id));
        }

        pub fn make_op(&mut self, mutation: Mutation) -> Operation {
            let id =

            let op = Operation {
                id: self.next_ts(),
                deps: self.current_deps(),
                mutation,
            };

            self.record_apply(op.clone());

            op
        }
    }

    #[derive(Debug)]
    pub enum IndexCmd {
        LocalOp(Mutation),
        RemoteOp(Mutation),
    }
}
