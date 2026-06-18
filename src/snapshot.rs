use crate::fs_utils::ensure_dir;
use crate::models::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;
use walkdir::WalkDir;

pub const MANIFEST_FILE: &str = "manifest.json";
pub const OBJECT_STORE_DIR: &str = ".objects";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnapshotManifest {
    pub version: u32,
    pub files: Vec<SnapshotFile>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnapshotFile {
    pub relative_path: PathBuf,
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug)]
pub struct SnapshotSummary {
    pub file_count: u64,
    pub total_size: u64,
    pub stored_size: u64,
    pub snapshot_hash: String,
}

pub fn create_incremental_snapshot(
    save_path: &Path,
    game_backup_dir: &Path,
    backup_node_dir: &Path,
) -> AppResult<SnapshotSummary> {
    let object_store = game_backup_dir.join(OBJECT_STORE_DIR);
    ensure_dir(&object_store, "Incremental object store create failed")?;

    let mut files = Vec::new();
    let mut stored_size = 0u64;
    for entry in WalkDir::new(save_path).follow_links(false) {
        let entry = entry.map_err(|err| AppError::message(format!("Save walk failed: {err}")))?;
        let file_type = entry.file_type();
        let path = entry.path();

        if file_type.is_symlink() {
            return Err(AppError::message(format!(
                "Incremental backup does not support symlinks: {}",
                path.display()
            )));
        }
        if !file_type.is_file() {
            continue;
        }

        let relative_path = path
            .strip_prefix(save_path)
            .map_err(|_| AppError::message("Relative path calculation failed"))?
            .to_path_buf();
        validate_relative_path(&relative_path)?;

        let (sha256, size) = hash_file(path)?;
        let object_path = object_path(&object_store, &sha256);
        if !object_path.exists() {
            store_object(path, &object_path)?;
            stored_size += size;
        }

        files.push(SnapshotFile {
            relative_path,
            size,
            sha256,
        });
    }

    files.sort_by(|left, right| {
        path_sort_key(&left.relative_path).cmp(&path_sort_key(&right.relative_path))
    });
    let snapshot_hash = snapshot_hash(&files);
    let file_count = files.len() as u64;
    let total_size = files.iter().map(|file| file.size).sum();
    let manifest = SnapshotManifest { version: 1, files };
    write_manifest(&backup_node_dir.join(MANIFEST_FILE), &manifest)?;

    Ok(SnapshotSummary {
        file_count,
        total_size,
        stored_size,
        snapshot_hash,
    })
}

pub fn materialize_incremental_backup(backup_node_dir: &Path, target_dir: &Path) -> AppResult<()> {
    let game_backup_dir = backup_node_dir
        .parent()
        .ok_or_else(|| AppError::message("Incremental backup has no parent game folder"))?;
    let object_store = game_backup_dir.join(OBJECT_STORE_DIR);
    let manifest = read_manifest(&backup_node_dir.join(MANIFEST_FILE))?;

    ensure_dir(target_dir, "Incremental restore target create failed")?;
    for file in manifest.files {
        validate_relative_path(&file.relative_path)?;
        let object_path = object_path(&object_store, &file.sha256);
        if !object_path.is_file() {
            return Err(AppError::message(format!(
                "Incremental object is missing: {}",
                file.sha256
            )));
        }
        let (actual_sha256, actual_size) = hash_file(&object_path)?;
        if actual_sha256 != file.sha256 || actual_size != file.size {
            return Err(AppError::IncrementalObjectCorrupted {
                sha256: file.sha256,
                path: object_path,
            });
        }

        let dst = target_dir.join(&file.relative_path);
        if let Some(parent) = dst.parent() {
            ensure_dir(parent, "Incremental restore parent create failed")?;
        }
        fs::copy(&object_path, &dst).map_err(|err| {
            AppError::io(
                format!("Incremental restore copy failed for object {}", file.sha256),
                &dst,
                err,
            )
        })?;
    }
    Ok(())
}

pub fn calculate_directory_snapshot_hash(path: &Path) -> AppResult<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    if !path.is_dir() {
        return Err(AppError::message(format!(
            "Save path is not a folder: {}",
            path.display()
        )));
    }

    let mut files = Vec::new();
    for entry in WalkDir::new(path).follow_links(false) {
        let entry = entry.map_err(|err| AppError::message(format!("Save walk failed: {err}")))?;
        if entry.file_type().is_symlink() {
            return Err(AppError::message(format!(
                "Snapshot hash does not support symlinks: {}",
                entry.path().display()
            )));
        }
        if !entry.file_type().is_file() {
            continue;
        }
        let relative_path = entry
            .path()
            .strip_prefix(path)
            .map_err(|_| AppError::message("Relative path calculation failed"))?
            .to_path_buf();
        validate_relative_path(&relative_path)?;
        let (sha256, size) = hash_file(entry.path())?;
        files.push(SnapshotFile {
            relative_path,
            size,
            sha256,
        });
    }

    files.sort_by(|left, right| {
        path_sort_key(&left.relative_path).cmp(&path_sort_key(&right.relative_path))
    });
    Ok(Some(snapshot_hash(&files)))
}

pub fn read_manifest(path: &Path) -> AppResult<SnapshotManifest> {
    let raw = fs::read_to_string(path)
        .map_err(|err| AppError::io("Incremental manifest read failed", path, err))?;
    let manifest: SnapshotManifest = serde_json::from_str(&raw)?;
    for file in &manifest.files {
        validate_relative_path(&file.relative_path)?;
        validate_sha256_hex(&file.sha256)?;
    }
    Ok(manifest)
}

pub fn garbage_collect_objects(game_backup_dir: &Path) -> AppResult<usize> {
    let object_store = game_backup_dir.join(OBJECT_STORE_DIR);
    if !object_store.exists() {
        return Ok(0);
    }

    let mut referenced = HashSet::new();
    for entry in fs::read_dir(game_backup_dir)
        .map_err(|err| AppError::io("Game backup folder read failed", game_backup_dir, err))?
    {
        let entry = entry
            .map_err(|err| AppError::io("Game backup entry read failed", game_backup_dir, err))?;
        let path = entry.path();
        if !path.is_dir()
            || path.file_name().and_then(|name| name.to_str()) == Some(OBJECT_STORE_DIR)
        {
            continue;
        }
        let manifest_path = path.join(MANIFEST_FILE);
        if !manifest_path.exists() {
            continue;
        }
        let Ok(manifest) = read_manifest(&manifest_path) else {
            continue;
        };
        for file in manifest.files {
            referenced.insert(file.sha256);
        }
    }

    let mut deleted = 0usize;
    for entry in WalkDir::new(&object_store).min_depth(1).follow_links(false) {
        let entry =
            entry.map_err(|err| AppError::message(format!("Object store walk failed: {err}")))?;
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(name) = entry.path().file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !referenced.contains(name) {
            fs::remove_file(entry.path()).map_err(|err| {
                AppError::io("Unreferenced object delete failed", entry.path(), err)
            })?;
            deleted += 1;
        }
    }

    Ok(deleted)
}

fn write_manifest(path: &Path, manifest: &SnapshotManifest) -> AppResult<()> {
    let data = serde_json::to_string_pretty(manifest)?;
    fs::write(path, data)
        .map_err(|err| AppError::io("Incremental manifest write failed", path, err))
}

fn store_object(src: &Path, object_path: &Path) -> AppResult<()> {
    if let Some(parent) = object_path.parent() {
        ensure_dir(parent, "Incremental object parent create failed")?;
    }

    let temp_path = object_path.with_extension(format!("tmp-{}", Uuid::new_v4()));
    fs::copy(src, &temp_path)
        .map_err(|err| AppError::io("Incremental object write failed", &temp_path, err))?;
    if object_path.exists() {
        let _ = fs::remove_file(&temp_path);
        return Ok(());
    }
    fs::rename(&temp_path, object_path)
        .map_err(|err| AppError::io("Incremental object commit failed", object_path, err))
}

fn hash_file(path: &Path) -> AppResult<(String, u64)> {
    let mut file =
        File::open(path).map_err(|err| AppError::io("Hash file open failed", path, err))?;
    let mut hasher = Sha256::new();
    let mut size = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|err| AppError::io("Hash file read failed", path, err))?;
        if read == 0 {
            break;
        }
        size += read as u64;
        hasher.update(&buffer[..read]);
    }
    Ok((to_hex(&hasher.finalize()), size))
}

fn snapshot_hash(files: &[SnapshotFile]) -> String {
    let mut hasher = Sha256::new();
    for file in files {
        hasher.update(path_sort_key(&file.relative_path).as_bytes());
        hasher.update([0]);
        hasher.update(file.size.to_le_bytes());
        hasher.update([0]);
        hasher.update(file.sha256.as_bytes());
        hasher.update([0]);
    }
    to_hex(&hasher.finalize())
}

fn object_path(object_store: &Path, sha256: &str) -> PathBuf {
    let prefix = sha256.get(0..2).unwrap_or("00");
    object_store.join(prefix).join(sha256)
}

fn validate_sha256_hex(value: &str) -> AppResult<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AppError::message(format!(
            "Manifest contains an invalid SHA-256 value: {value}"
        )));
    }
    Ok(())
}

fn validate_relative_path(path: &Path) -> AppResult<()> {
    if path.as_os_str().is_empty() {
        return Err(AppError::message("Manifest contains an empty path"));
    }
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            _ => {
                return Err(AppError::message(format!(
                    "Manifest contains an unsafe path: {}",
                    path.display()
                )));
            }
        }
    }
    Ok(())
}

fn path_sort_key(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn incremental_snapshot_reuses_unchanged_objects_and_materializes_versions() {
        let root = tempdir().unwrap();
        let save = root.path().join("save");
        let game_backup = root.path().join("backups").join("Game");
        fs::create_dir_all(save.join("nested")).unwrap();
        fs::create_dir_all(&game_backup).unwrap();
        fs::write(save.join("slot.sav"), "one").unwrap();
        fs::write(save.join("nested").join("settings.json"), "{}").unwrap();

        let first = game_backup.join("first");
        fs::create_dir_all(&first).unwrap();
        let first_summary = create_incremental_snapshot(&save, &game_backup, &first).unwrap();

        fs::write(save.join("slot.sav"), "two").unwrap();
        fs::remove_file(save.join("nested").join("settings.json")).unwrap();
        let second = game_backup.join("second");
        fs::create_dir_all(&second).unwrap();
        let second_summary = create_incremental_snapshot(&save, &game_backup, &second).unwrap();

        assert_eq!(first_summary.file_count, 2);
        assert_eq!(second_summary.file_count, 1);
        assert!(second_summary.stored_size <= second_summary.total_size);

        let restore_first = root.path().join("restore_first");
        materialize_incremental_backup(&first, &restore_first).unwrap();
        assert_eq!(
            fs::read_to_string(restore_first.join("slot.sav")).unwrap(),
            "one"
        );
        assert!(restore_first.join("nested").join("settings.json").exists());

        let restore_second = root.path().join("restore_second");
        materialize_incremental_backup(&second, &restore_second).unwrap();
        assert_eq!(
            fs::read_to_string(restore_second.join("slot.sav")).unwrap(),
            "two"
        );
        assert!(!restore_second.join("nested").join("settings.json").exists());
    }

    #[test]
    fn garbage_collect_objects_removes_unreferenced_files() {
        let root = tempdir().unwrap();
        let save = root.path().join("save");
        let game_backup = root.path().join("backups").join("Game");
        fs::create_dir_all(&save).unwrap();
        fs::create_dir_all(&game_backup).unwrap();
        fs::write(save.join("slot.sav"), "one").unwrap();

        let backup = game_backup.join("backup");
        fs::create_dir_all(&backup).unwrap();
        create_incremental_snapshot(&save, &game_backup, &backup).unwrap();
        let object_count_before = WalkDir::new(game_backup.join(OBJECT_STORE_DIR))
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .count();
        assert_eq!(object_count_before, 1);

        fs::remove_dir_all(&backup).unwrap();
        let deleted = garbage_collect_objects(&game_backup).unwrap();
        assert_eq!(deleted, 1);
    }

    #[test]
    fn read_manifest_rejects_invalid_sha256_values() {
        let root = tempdir().unwrap();
        let path = root.path().join(MANIFEST_FILE);
        for value in [
            "".to_owned(),
            "abc".to_owned(),
            "../evil".to_owned(),
            "g".repeat(64),
        ] {
            let manifest = serde_json::json!({
                "version": 1,
                "files": [{
                    "relative_path": "slot.sav",
                    "size": 1,
                    "sha256": &value
                }]
            });
            fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
            assert!(read_manifest(&path).is_err(), "value should fail: {value}");
        }
    }

    #[test]
    fn read_manifest_accepts_valid_sha256() {
        let root = tempdir().unwrap();
        let path = root.path().join(MANIFEST_FILE);
        let manifest = SnapshotManifest {
            version: 1,
            files: vec![SnapshotFile {
                relative_path: PathBuf::from("slot.sav"),
                size: 1,
                sha256: "a".repeat(64),
            }],
        };
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();

        assert_eq!(read_manifest(&path).unwrap().files.len(), 1);
    }

    #[test]
    fn materialize_rejects_corrupted_object_and_identifies_it() {
        let root = tempdir().unwrap();
        let save = root.path().join("save");
        let game_backup = root.path().join("backups").join("Game");
        let backup = game_backup.join("backup");
        fs::create_dir_all(&save).unwrap();
        fs::create_dir_all(&backup).unwrap();
        fs::write(save.join("slot.sav"), "original").unwrap();
        create_incremental_snapshot(&save, &game_backup, &backup).unwrap();

        let manifest = read_manifest(&backup.join(MANIFEST_FILE)).unwrap();
        let object_hash = manifest.files[0].sha256.clone();
        let corrupted_path = object_path(&game_backup.join(OBJECT_STORE_DIR), &object_hash);
        fs::write(&corrupted_path, "corrupted").unwrap();

        let restore = root.path().join("restore");
        let err = materialize_incremental_backup(&backup, &restore).unwrap_err();
        match &err {
            AppError::IncrementalObjectCorrupted { sha256, path } => {
                assert_eq!(sha256, &object_hash);
                assert_eq!(path, &corrupted_path);
            }
            _ => panic!("unexpected error: {err}"),
        }
        let detail = err.to_string();
        assert!(detail.contains(&object_hash));
        assert!(err
            .user_message_for_language(crate::models::Language::EnUs)
            .contains(&corrupted_path.display().to_string()));
        assert!(!restore.join("slot.sav").exists());
    }
}
