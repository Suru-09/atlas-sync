pub mod crdt {
    use std::collections::{BTreeMap, HashSet};
    use uuid::Uuid;

    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
    }

    #[derive(Debug, Clone)]
    pub struct Operation {
        pub id: LamportTimestamp,
        pub deps: HashSet<LamportTimestamp>,
        pub cursor: Vec<String>,
        pub mutation: Mutation,
    }

    #[derive(Debug, Clone)]
    pub enum Mutation {
        Insert { key: String, value: JsonNode },
        Edit { key: String, value: JsonNode },
        Delete { key: String },
    }

    #[derive(Debug, Clone, PartialEq)]
    pub enum JsonNode {
        Map(BTreeMap<String, JsonNode>),
        List(Vec<JsonNode>),
        File(String),
        Tombstone,
    }

    impl JsonNode {
        pub fn new_map() -> Self {
            JsonNode::Map(BTreeMap::new())
        }

        pub fn new_list() -> Self {
            JsonNode::List(Vec::new())
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

            match &op.mutation {
                Mutation::Insert { key, value } => match target {
                    JsonNode::Map(map) => {
                        map.insert(key.clone(), value.clone());
                    }
                    _ => return false,
                },
                Mutation::Delete { key } => match target {
                    JsonNode::Map(map) => {
                        map.insert(key.clone(), JsonNode::Tombstone);
                    }
                    _ => return false,
                },
                Mutation::Edit { key, value } => match target {
                    JsonNode::Map(map) => {
                        if map.contains_key(key) && !matches!(map[key], JsonNode::Tombstone) {
                            map.insert(key.clone(), value.clone());
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
            match self {
                JsonNode::Map(map) => {
                    map.retain(|_, v| !matches!(v, JsonNode::Tombstone));
                    for node in map.values_mut() {
                        node.compress();
                    }
                }
                JsonNode::List(list) => {
                    list.retain(|v| !matches!(v, JsonNode::Tombstone));
                    for node in list {
                        node.compress();
                    }
                }
                _ => {}
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::uuid_wrapper::uuid_wrapper::create_new_uuid;
        use uuid::{uuid, Uuid};

        #[test]
        fn json_crdt_operations_and_consistency() {
            let replica_id = create_new_uuid();
            let mut applied_ops = HashSet::new();
            let mut root = JsonNode::new_map();

            let op1 = Operation {
                id: LamportTimestamp {
                    counter: 1,
                    replica_id,
                },
                deps: HashSet::new(),
                cursor: vec![],
                mutation: Mutation::Insert {
                    key: "dir1".into(),
                    value: JsonNode::new_map(),
                },
            };

            assert!(root.apply(&op1, &mut applied_ops));

            let op2 = Operation {
                id: LamportTimestamp {
                    counter: 2,
                    replica_id,
                },
                deps: [op1.id.clone()].iter().cloned().collect(),
                cursor: vec!["dir1".into()],
                mutation: Mutation::Insert {
                    key: "file.txt".into(),
                    value: JsonNode::File("content".into()),
                },
            };

            assert!(root.apply(&op2, &mut applied_ops));

            let op3 = Operation {
                id: LamportTimestamp {
                    counter: 3,
                    replica_id,
                },
                deps: [op2.id.clone()].iter().cloned().collect(),
                cursor: vec!["dir1".into()],
                mutation: Mutation::Edit {
                    key: "file.txt".into(),
                    value: JsonNode::File("updated content".into()),
                },
            };

            assert!(root.apply(&op3, &mut applied_ops));

            root.compress();

            if let JsonNode::Map(map) = &root {
                let dir1 = map.get("dir1").unwrap();
                if let JsonNode::Map(dir_map) = dir1 {
                    assert_eq!(
                        dir_map.get("file.txt"),
                        Some(&JsonNode::File("updated content".into()))
                    );
                }
            }
        }
    }
}
