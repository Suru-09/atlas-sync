pub mod fswrapper {
    use log::error;
    use once_cell::sync::{Lazy, OnceCell};
    use serde::{Deserialize, Serialize};
    use sha2::{Digest, Sha256};
    use std::io::Write;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::path::{Component, Path, PathBuf};
    use std::time::UNIX_EPOCH;
    use std::{fs, io};

    pub static INDEX_NAME: Lazy<String> = Lazy::new(|| String::from("/index.json"));
    pub static WATCHED_PATH: OnceCell<String> = OnceCell::new();

    #[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
    pub struct LogicalTimestamp(pub u64);

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
    pub struct EntryMeta {
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

    #[derive(Debug, Serialize, Deserialize, Clone, Default)]
    pub struct FileBlob {
        pub name: String,
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
                    let name = compute_file_relative_path(&path)
                        .to_string_lossy()
                        .into_owned();
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
            let full_path = smart_join(base_path, &Path::new(&self.name));

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

    impl EntryMeta {
        pub fn from_path(path: &Path) -> std::io::Result<Self> {
            if !path.exists() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Path not found",
                ));
            }

            let name = last_name(path).unwrap_or(String::from("empty_name"));
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
                return Ok(EntryMeta {
                    name,
                    path: compute_file_relative_path(path)
                        .to_str()
                        .unwrap()
                        .to_string(),
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

                return Ok(EntryMeta {
                    name,
                    path: compute_file_relative_path(path)
                        .to_str()
                        .unwrap()
                        .to_string(),
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

    ///
    /// # Arguments
    ///
    /// * 'full_path' -
    /// * 'sub_path' -
    ///
    /// # Examples
    pub fn relative_intersection(full_path: &Path, sub_path: &Path) -> Option<PathBuf> {
        let full_components: Vec<_> = full_path.components().collect();
        let sub_components: Vec<_> = sub_path.components().collect();

        full_components
            .windows(sub_components.len())
            .position(|window| window == sub_components.as_slice())
            .map(|index| full_components[index..].iter().collect())
    }

    pub fn path_to_vec(path: &Path) -> Vec<String> {
        path.components()
            .filter_map(|c| match c {
                Component::RootDir => None,
                Component::Prefix(p) => Some(p.as_os_str().to_string_lossy().into_owned()),
                Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                Component::CurDir | Component::ParentDir => None,
            })
            .collect()
    }

    pub fn last_name(path: &Path) -> Option<String> {
        if let Some(os) = path.file_name() {
            return Some(os.to_string_lossy().into_owned());
        }

        path.components().rev().find_map(|c| match c {
            Component::Normal(os) => Some(os.to_string_lossy().into_owned()),
            _ => None,
        })
    }

    pub fn components_to_path_string(components: &[Component<'_>]) -> String {
        let mut pb = PathBuf::new();
        for c in components {
            pb.push(c.as_os_str());
        }
        pb.to_string_lossy().to_string()
    }

    pub fn compute_file_relative_path(abs_path: &Path) -> PathBuf {
        let last_name_watched = last_name(&Path::new(WATCHED_PATH.get().unwrap())).unwrap();
        relative_intersection(abs_path, &Path::new(&last_name_watched)).unwrap()
    }

    pub fn smart_join(a: &Path, b: &Path) -> PathBuf {
        let a_components: Vec<_> = a.components().map(|c| c.as_os_str()).collect();
        let b_components: Vec<_> = b.components().map(|c| c.as_os_str()).collect();

        let mut overlap = 0;
        let max_overlap = std::cmp::min(a_components.len(), b_components.len());
        for i in 1..=max_overlap {
            if a_components[a_components.len() - i..] == b_components[..i] {
                overlap = i;
            }
        }

        let mut result = a_components.clone();
        result.extend_from_slice(&b_components[overlap..]);
        result.iter().collect()
    }

    pub fn compute_file_absolute_path(relative_path: &Path) -> PathBuf {
        let watched_path = &Path::new(WATCHED_PATH.get().unwrap());
        smart_join(watched_path, relative_path)
    }

    pub fn delete_path<P: AsRef<Path>>(path: P) -> io::Result<()> {
        let path = path.as_ref();
        if path.is_dir() {
            fs::remove_dir_all(path)
        } else {
            fs::remove_file(path)
        }
    }
}
