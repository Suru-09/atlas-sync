pub mod crdt {
    use crate::fswrapper::fswrapper::EntryMeta;
    use serde::{Deserialize, Serialize};
    use std::collections::{BTreeMap, HashMap, HashSet};
    use uuid::Uuid;
    use log::{error, info};

    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
    pub struct LamportTimestamp {
        pub counter: u64,
        pub replica_id: Uuid,
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
    pub struct VersionVector(pub HashMap<Uuid, u64>);

    impl VersionVector {
        pub fn record(&mut self, ts: &LamportTimestamp) {
            self.0
                .entry(ts.replica_id)
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
                    .entry(*id)
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
    #[serde(untagged)] // do not introduce Map, List, etc. in serialization
    pub enum JsonNode {
        Map(BTreeMap<String, JsonNode>),
        Entry(EntryMeta),
        Tombstone,
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

            if !op.deps.is_subset(applied_ops) {
                 return false;
            }

            let mut target = self;
                for segment in &op.cursor {
                    match target {
                        JsonNode::Map(map) => {
                            target = map.entry(segment.clone()).or_insert(JsonNode::new_map());
                        }
                        _ => return false,
                    }
                }

            //error!("NEWLY_CREATED: {} and target: {:?}", newly_created, target);

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
                   Mutation::Delete { key } => match target {
                       JsonNode::Map(map) => {
                           if let Some(entry) = map.get_mut(key) {
                               *entry = JsonNode::Tombstone;
                           }
                       }
                       _ => return false,
                   },
                   Mutation::Edit { key, value } => match target {
                       JsonNode::Map(map) => {
                           if let Some(entry) = map.get_mut(key) {
                               if !matches!(entry, JsonNode::Tombstone) {
                                   *entry = value.clone();
                               } else {
                                   return false;
                               }
                           } else {
                               return false;
                           }
                       }
                       _ => return false,
                   },
               }

            applied_ops.insert(op.id.clone());
            true
        }

        pub fn compress(&mut self) {
            // match self {
            //     JsonNode::Map(map) => {
            //         map.retain(|_, v| !matches!(v, JsonNode::Tombstone));
            //         for node in map.values_mut() {
            //             node.compress();
            //         }
            //     }
            //     _ => {}
            // }
        }
    }

    #[cfg(test)]
    mod tests {
    }
}
