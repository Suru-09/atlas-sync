pub mod fswrapper {
    use log::error;
    use serde::{Deserialize, Serialize};
    use sha2::{Digest, Sha256};
    use std::io::Write;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::path::Path;
    use std::time::UNIX_EPOCH;
    use std::{fs, io};

    #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
    pub struct LogicalTimestamp(pub u64);

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    pub struct FileMeta {
        pub name: String,
        pub path: String,
        pub is_directory: bool,
        pub accessed: Option<u64>,
        pub modified: Option<u64>,
        pub created: Option<u64>,
        pub permissions: Option<u32>,
        pub size: Option<u64>,
        pub owner: Option<String>,
        pub content_hash: Option<String>,
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

        pub fn from_path(path: &Path) -> std::io::Result<Self> {
            let name = path.to_string_lossy().into_owned();
            let content = fs::read(&path)?;
            let mut hasher = Sha256::new();
            hasher.update(&content);
            let checksum = format!("{:x}", hasher.finalize());
            let size = fs::metadata(&path)?.len();
            Ok(FileBlob {
                name,
                checksum,
                size,
                content,
            })
        }
    }

    impl FileMeta {
        pub fn from_path(path: &Path) -> std::io::Result<Self> {
            if !path.exists() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Path not found",
                ));
            }

            let name = path.to_string_lossy().into_owned();
            let metadata = fs::metadata(&path)?;
            let last_accesed = if let Ok(access_time) = metadata.accessed() {
                Some(access_time.duration_since(UNIX_EPOCH).unwrap().as_secs())
            } else {
                None
            };

            let last_modified = if let Ok(edit_time) = metadata.modified() {
                Some(edit_time.duration_since(UNIX_EPOCH).unwrap().as_secs())
            } else {
                None
            };

            let created = if let Ok(create_time) = metadata.created() {
                Some(create_time.duration_since(UNIX_EPOCH).unwrap().as_secs())
            } else {
                None
            };

            if path.is_dir() {
                return Ok(FileMeta {
                    name,
                    path: path.to_str().unwrap().to_string(),
                    is_directory: true,
                    accessed: last_accesed,
                    modified: last_modified,
                    created: created,
                    size: Some(metadata.size()),
                    permissions: Some(metadata.permissions().mode()),
                    owner: None,
                    content_hash: None,
                });
            } else if path.is_file() {
                let content = fs::read(&path)?;
                let mut hasher = Sha256::new();
                hasher.update(&content);
                let checksum = format!("{:x}", hasher.finalize());

                return Ok(FileMeta {
                    name,
                    path: path.to_str().unwrap().to_string(),
                    is_directory: true,
                    accessed: last_accesed,
                    modified: last_modified,
                    created: created,
                    size: Some(metadata.size()),
                    permissions: Some(metadata.permissions().mode()),
                    owner: None,
                    content_hash: Some(checksum),
                });
            }

            Err(std::io::Error::new(std::io::ErrorKind::Other, "HMM.."))
        }
    }
}
