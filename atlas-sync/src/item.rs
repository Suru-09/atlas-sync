pub mod item {
    use serde::{Deserialize, Serialize};
    use uuid::Uuid;

    #[derive(Debug, Serialize, Deserialize)]
    pub enum ItemType {
        File(FileType),
        Directory,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub enum FileType {
        Binary,
        Image,
        Text,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub enum ItemUpdateType {
        Created,
        Updated,
        Deleted,
        Moved,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct ItemUpdate {
        pub update_type: ItemUpdateType,
        pub update_time: String, // TODO: Add a real time here..
    }

    #[derive(Debug)]
    pub struct Item {
        path: String,
        uid: Uuid,
        update: ItemUpdate,
    }
}
