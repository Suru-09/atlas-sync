pub mod crdt {
    use crate::fswrapper::fswrapper::EntryMeta;
    use log::{debug, error, info};
    use serde::{Deserialize, Serialize};
    use std::collections::{BTreeMap, HashMap, HashSet};

    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
    pub struct LamportTimestamp {
        pub counter: u64,
        pub replica_id: String,
    }

    impl LamportTimestamp {
        pub fn increment(&mut self) {
            self.counter += 1
        }

        pub fn swap(&mut self, other: &LamportTimestamp) {
            self.counter = other.counter;
        }

        pub fn merge(&mut self, other: &Self) {
            self.counter = self.counter.max(other.counter);
        }
    }

    #[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    pub struct VersionVector(pub HashMap<String, u64>);

    impl VersionVector {
        pub fn record(&mut self, ts: &LamportTimestamp) {
            self.0
                .entry(ts.replica_id.clone())
                .and_modify(|c| *c = (*c).max(ts.counter))
                .or_insert(ts.counter);
        }

        pub fn dominates(&self, ts: &LamportTimestamp) -> bool {
            self.0
                .get(&ts.replica_id)
                .map_or(false, |c| *c >= ts.counter)
        }

        pub fn merge(&mut self, other: &Self) {
            for (id, c) in &other.0 {
                self.0
                    .entry(id.clone())
                    .and_modify(|cc| *cc = (*cc).max(*c))
                    .or_insert(*c);
            }
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Operation {
        pub id: LamportTimestamp,
        pub deps: HashSet<LamportTimestamp>,
        pub cursor: Vec<String>,
        pub mutation: Mutation,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum Mutation {
        New { key: String, value: JsonNode },
        Edit { key: String, value: JsonNode },
        Delete { key: String },
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    pub enum JsonNode {
        Tombstone,
        #[serde(untagged)] // do not serialize enum name
        Map(BTreeMap<String, JsonNode>),
        #[serde(untagged)] // do not serialize enum name
        Entry(EntryMeta),
    }

    impl JsonNode {
        pub fn new_map() -> Self {
            JsonNode::Map(BTreeMap::new())
        }

        pub fn apply(
            &mut self,
            op: &Operation,
            applied_ops: &mut HashSet<LamportTimestamp>,
        ) -> bool {
            // if !op.deps.is_subset(applied_ops) {
            //      return false;
            // }
            //

            let mut target = self;
            for segment in &op.cursor {
                match target {
                    JsonNode::Map(map) => {
                        target = map.entry(segment.clone()).or_insert(JsonNode::new_map());
                    }
                    _ => return false,
                }
            }

            debug!("APPLY OP: {:?} on target: {:?}", op, target);

            match &op.mutation {
                Mutation::New { key, value } => match target {
                    JsonNode::Map(map) => {
                        if let JsonNode::Entry(_) = value {
                            map.insert(String::from("metadata"), value.clone());
                        } else {
                            map.insert(key.clone(), value.clone());
                        }
                    }
                    _ => return false,
                },
                Mutation::Delete { .. } => {
                    *target = JsonNode::Tombstone;
                }
                Mutation::Edit { key: _, value } => match target {
                    JsonNode::Map(map) => {
                        if let JsonNode::Entry(e) = value {
                            if let Some(entry) = map.get_mut("metadata") {
                                *entry = JsonNode::Entry(e.clone());
                            } else {
                                return false;
                            }
                        }
                    }
                    _ => return false,
                },
            }

            applied_ops.insert(op.id.clone());
            true
        }

        pub fn compress(&mut self) {
            match self {
                JsonNode::Map(map) => {
                    map.retain(|_, v| !matches!(v, JsonNode::Tombstone));
                    for node in map.values_mut() {
                        node.compress();
                    }
                }
                _ => {}
            }
        }

        pub fn get_entry_meta(&self, cursor: &[String]) -> Option<EntryMeta> {
            error!("[get_entry_meta] Cursor: {:?}", cursor);
            let mut target = self;
            for segment in cursor.iter() {
                match target {
                    JsonNode::Map(map) => {
                        let key = segment.clone();
                        if map.contains_key(&key) {
                            target = map.get(&key).unwrap();
                        }
                    }
                    _ => {}
                }
            }

            if let JsonNode::Map(map) = target {
                if map.contains_key("metadata") {
                    if let JsonNode::Entry(e) = map.get("metadata").unwrap() {
                        return Some(e.clone());
                    }
                }
            }

            None
        }
    }

    #[cfg(test)]
    mod tests {}
}
