pub mod file {
    use serde::{Deserialize, Serialize};
    use std::path::PathBuf;

    #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
    pub struct LogicalTimestamp(pub u64);

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct FileMetadata {
        pub logical_time: LogicalTimestamp,
        pub is_directory: bool,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct CreateOp {
        pub metadata: FileMetadata,
        pub path: PathBuf,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct UpdateOp {
        pub metadata: FileMetadata,
        pub path: PathBuf,
        pub changes: Vec<FileChange>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct DeleteOp {
        pub metadata: FileMetadata,
        pub path: PathBuf,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum FileChange {
        ContentHash(String),
        Permissions(u32),
        TimestampModified(u64),
        Owner(String),
        Renamed(String),
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum FileEventType {
        Created(CreateOp),
        Updated(UpdateOp),
        Deleted(DeleteOp),
    }
}
