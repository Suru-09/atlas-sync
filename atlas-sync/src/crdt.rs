pub mod crdt {
    use serde::{Deserialize, Serialize};
    use std::collections::HashMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct FileMetadata {
        pub path: String,
        pub hash: String,
        pub size: u64,
        pub modified: u64,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct FileIndexCRDT {
        pub entries: HashMap<String, FileMetadata>,
        pub timestamps: HashMap<String, u64>,
    }

    impl FileIndexCRDT {
        pub fn new() -> Self {
            Self {
                entries: HashMap::new(),
                timestamps: HashMap::new(),
            }
        }

        pub fn update(&mut self, metadata: FileMetadata) {
            let now = current_timestamp();
            let key = metadata.path.clone();
            let entry_time = self.timestamps.get(&key).copied().unwrap_or(0);

            if now >= entry_time {
                self.entries.insert(key.clone(), metadata);
                self.timestamps.insert(key, now);
            }
        }

        pub fn merge(&mut self, other: &FileIndexCRDT) {
            for (key, other_meta) in &other.entries {
                let self_time = self.timestamps.get(key).copied().unwrap_or(0);
                let other_time = other.timestamps.get(key).copied().unwrap_or(0);

                if other_time > self_time {
                    self.entries.insert(key.clone(), other_meta.clone());
                    self.timestamps.insert(key.clone(), other_time);
                }
            }
        }
    }

    fn current_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }
}

#[cfg(test)]
mod tests {
    use crate::crdt::crdt::{FileIndexCRDT, FileMetadata};

    fn make_metadata(path: &str, hash: &str, size: u64, modified: u64) -> FileMetadata {
        FileMetadata {
            path: path.to_string(),
            hash: hash.to_string(),
            size,
            modified,
        }
    }

    #[test]
    fn test_update_inserts_entry() {
        let mut crdt = FileIndexCRDT::new();
        let md = make_metadata("foo.txt", "h1", 100, 10);
        crdt.update(md.clone());

        let entry = crdt.entries.get("foo.txt").expect("entry missing");
        assert_eq!(entry.hash, "h1");
        assert_eq!(entry.size, 100);
        assert!(crdt.timestamps.get("foo.txt").is_some());
    }

    #[test]
    fn test_update_overrides_with_newer() {
        let mut crdt = FileIndexCRDT::new();

        let md1 = make_metadata("bar.txt", "h1", 200, 20);
        crdt.update(md1.clone());

        let md2 = make_metadata("bar.txt", "h2", 300, 30);
        crdt.update(md2.clone());

        let entry = crdt.entries.get("bar.txt").unwrap();
        assert_eq!(entry.hash, "h2");
        assert_eq!(entry.size, 300);
    }

    #[test]
    fn test_merge_prefers_higher_timestamp() {
        let mut a = FileIndexCRDT::new();
        let md1 = make_metadata("baz.txt", "ha", 50, 5);
        a.entries.insert("baz.txt".into(), md1.clone());
        a.timestamps.insert("baz.txt".into(), 1);

        let mut b = FileIndexCRDT::new();
        let md2 = make_metadata("baz.txt", "hb", 60, 6);
        b.entries.insert("baz.txt".into(), md2.clone());
        b.timestamps.insert("baz.txt".into(), 2);

        a.merge(&b);
        let entry = a.entries.get("baz.txt").unwrap();
        assert_eq!(entry.hash, "hb");

        b.merge(&a);
        let entry_b = b.entries.get("baz.txt").unwrap();
        assert_eq!(entry_b.hash, "hb");
    }

    #[test]
    fn test_merge_adds_missing_entries() {
        let mut a = FileIndexCRDT::new();
        let md1 = make_metadata("only_in_a.txt", "ha", 10, 1);
        a.entries.insert("only_in_a.txt".into(), md1.clone());
        a.timestamps.insert("only_in_a.txt".into(), 1);

        let mut b = FileIndexCRDT::new();
        let md2 = make_metadata("only_in_b.txt", "hb", 20, 2);
        b.entries.insert("only_in_b.txt".into(), md2.clone());
        b.timestamps.insert("only_in_b.txt".into(), 2);

        a.merge(&b);
        assert!(a.entries.contains_key("only_in_a.txt"));
        assert!(a.entries.contains_key("only_in_b.txt"));
    }

    #[test]
    fn test_json_roundtrip() {
        let mut original = FileIndexCRDT::new();
        let md = make_metadata("round.json", "rnd", 123, 456);
        original.update(md.clone());

        let json = serde_json::to_string_pretty(&original).expect("serialize failed");
        let deserialized: FileIndexCRDT = serde_json::from_str(&json).expect("deserialize failed");

        assert_eq!(original.entries.len(), deserialized.entries.len());
        assert_eq!(
            original.entries.get("round.json").unwrap().hash,
            deserialized.entries.get("round.json").unwrap().hash
        );
        assert_eq!(
            original.timestamps.get("round.json"),
            deserialized.timestamps.get("round.json")
        );
    }
}
