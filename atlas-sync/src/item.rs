pub mod item {
    use uuid::Uuid;

    #[derive(Debug)]
    pub enum ItemType {
        File(FileType),
        Directory,
    }

    #[derive(Debug)]
    pub enum FileType {
        Binary,
        Image,
        Text,
    }

    #[derive(Debug)]
    pub enum ItemUpdateType {
        Created,
        Updated,
        Deleted,
        Moved,
    }

    #[derive(Debug)]
    pub struct ItemUpdate {
        pub update_type: ItemUpdateType,
        pub update_time: String, // TODO: Add a real time here..
    }

    pub struct Item {
        path: String,
        uid: Uuid,
        update: ItemUpdate,
    }
}
