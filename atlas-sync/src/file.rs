pub mod file {
    use log::error;
    use serde::{Deserialize, Serialize};
    use sha2::{Digest, Sha256, Sha512};
    use std::collections::HashMap;
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::{fs, io};
    use walkdir::WalkDir;

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

    #[derive(Debug, Serialize, Deserialize, Clone)]
    pub struct FileBlob {
        name: String,
        checksum: String,
        size: u64,
        content: Vec<u8>,
    }

    impl FileBlob {
        pub fn collect_files_to_be_synced(dir: &Path) -> std::io::Result<Vec<FileBlob>> {
            let mut blobs = Vec::new();
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    blobs.extend(FileBlob::collect_files_to_be_synced(&path)?);
                } else if path.is_file() {
                    let name = path.to_string_lossy().into_owned();
                    let content = fs::read(&path)?;
                    let mut hasher = Sha256::new();
                    hasher.update(&content);
                    let checksum = format!("{:x}", hasher.finalize());
                    let size = fs::metadata(&path)?.len();
                    blobs.push(FileBlob {
                        name,
                        checksum,
                        size,
                        content,
                    });
                }
            }
            Ok(blobs)
        }

        pub fn write_to_disk(&self, base_path: &Path) -> io::Result<()> {
            let full_path = base_path.join(&self.name);

            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent)?;
            } else {
                error!("Parent path: {:?} does not exist!", full_path.parent());
            }

            let computed_checksum = {
                let mut hasher = Sha256::new();
                hasher.update(&self.content);
                format!("{:x}", hasher.finalize())
            };

            if computed_checksum != self.checksum {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Checksum mismatch",
                ));
            }

            if self.content.len() as u64 != self.size {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "Size mismatch"));
            }

            let mut file = fs::File::create(&full_path)?;
            file.write_all(&self.content)?;
            Ok(())
        }
    }
}
