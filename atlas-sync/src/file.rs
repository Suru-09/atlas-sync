pub mod file {
    use serde::{Deserialize, Serialize};
    use sha2::{Digest, Sha256, Sha512};
    use std::collections::HashMap;
    use std::io::Read;
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

    #[derive(Debug, Serialize, Deserialize)]
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
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct DirectoryTree {
        pub name: String,
        pub children: HashMap<String, DirectoryTree>,
    }

    impl DirectoryTree {
        pub fn new(root: &Path) -> Self {
            let root_name = root
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "".into());

            let mut root_dir = DirectoryTree {
                name: root_name,
                children: HashMap::new(),
            };

            for entry in WalkDir::new(root)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|e| e.path().is_dir() && e.path() != root)
            {
                let relative_path = entry.path().strip_prefix(root).unwrap();
                let components: Vec<String> = relative_path
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();

                root_dir.insert_path(&components);
            }

            root_dir
        }

        fn insert_path(&mut self, components: &[String]) {
            if components.is_empty() {
                return;
            }

            let head = &components[0];
            let child = self
                .children
                .entry(head.clone())
                .or_insert_with(|| DirectoryTree {
                    name: head.clone(),
                    children: HashMap::new(),
                });

            child.insert_path(&components[1..]);
        }

        pub fn write_to_disk(&self, base: &Path) -> std::io::Result<()> {
            let current_path = base.join(&self.name);
            std::fs::create_dir_all(&current_path)?;

            for child in self.children.values() {
                child.write_to_disk(&current_path)?;
            }

            Ok(())
        }
    }
}
