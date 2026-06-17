use crate::models::{AppError, AppResult};
use std::fs::{self, File};
use std::io;
use std::path::{Component, Path};
use walkdir::WalkDir;
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

pub const ARCHIVE_FILE: &str = "save_files.zip";

#[derive(Clone, Debug)]
pub struct ArchiveSummary {
    pub file_count: u64,
    pub total_size: u64,
    pub stored_size: u64,
}

pub fn create_zip_backup(save_path: &Path, archive_path: &Path) -> AppResult<ArchiveSummary> {
    if let Some(parent) = archive_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| AppError::io("ZIP backup parent create failed", parent, err))?;
    }

    let file = File::create(archive_path)
        .map_err(|err| AppError::io("ZIP backup file create failed", archive_path, err))?;
    let mut writer = ZipWriter::new(file);
    let options = FileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut file_count = 0u64;
    let mut total_size = 0u64;

    for entry in WalkDir::new(save_path).follow_links(false) {
        let entry = entry.map_err(|err| AppError::message(format!("ZIP walk failed: {err}")))?;
        let file_type = entry.file_type();
        let path = entry.path();
        if file_type.is_symlink() {
            return Err(AppError::message(format!(
                "ZIP backup does not support symlinks: {}",
                path.display()
            )));
        }
        if !file_type.is_file() {
            continue;
        }

        let relative_path = path
            .strip_prefix(save_path)
            .map_err(|_| AppError::message("ZIP relative path calculation failed"))?;
        validate_relative_path(relative_path)?;
        let name = relative_path.to_string_lossy().replace('\\', "/");
        writer
            .start_file(name, options)
            .map_err(|err| AppError::message(format!("ZIP entry create failed: {err}")))?;

        let mut source = File::open(path)
            .map_err(|err| AppError::io("ZIP source file open failed", path, err))?;
        let copied = io::copy(&mut source, &mut writer)
            .map_err(|err| AppError::io("ZIP file write failed", archive_path, err))?;
        file_count += 1;
        total_size += copied;
    }

    writer
        .finish()
        .map_err(|err| AppError::message(format!("ZIP finish failed: {err}")))?;
    let stored_size = fs::metadata(archive_path)
        .map_err(|err| AppError::io("ZIP metadata read failed", archive_path, err))?
        .len();

    Ok(ArchiveSummary {
        file_count,
        total_size,
        stored_size,
    })
}

pub fn extract_zip_backup(archive_path: &Path, target_dir: &Path) -> AppResult<()> {
    let file = File::open(archive_path)
        .map_err(|err| AppError::io("ZIP backup file open failed", archive_path, err))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|err| AppError::message(format!("ZIP open failed: {err}")))?;

    fs::create_dir_all(target_dir)
        .map_err(|err| AppError::io("ZIP extract target create failed", target_dir, err))?;
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|err| AppError::message(format!("ZIP entry read failed: {err}")))?;
        let relative_path = Path::new(file.name());
        validate_relative_path(relative_path)?;
        let output_path = target_dir.join(relative_path);

        if file.is_dir() {
            fs::create_dir_all(&output_path)
                .map_err(|err| AppError::io("ZIP directory extract failed", &output_path, err))?;
            continue;
        }

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| AppError::io("ZIP extract parent create failed", parent, err))?;
        }
        let mut output = File::create(&output_path)
            .map_err(|err| AppError::io("ZIP output file create failed", &output_path, err))?;
        io::copy(&mut file, &mut output)
            .map_err(|err| AppError::io("ZIP output file write failed", &output_path, err))?;
    }

    Ok(())
}

fn validate_relative_path(path: &Path) -> AppResult<()> {
    if path.as_os_str().is_empty() {
        return Err(AppError::message("ZIP contains an empty path"));
    }
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            _ => {
                return Err(AppError::message(format!(
                    "ZIP contains an unsafe path: {}",
                    path.display()
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn zip_backup_roundtrips_save_files() {
        let root = tempdir().unwrap();
        let save = root.path().join("save");
        fs::create_dir_all(save.join("nested")).unwrap();
        fs::write(save.join("slot.sav"), "save").unwrap();
        fs::write(save.join("nested").join("settings.json"), "{}").unwrap();
        let archive = root.path().join("backup.zip");

        let summary = create_zip_backup(&save, &archive).unwrap();
        assert_eq!(summary.file_count, 2);
        assert!(summary.stored_size > 0);

        let restored = root.path().join("restored");
        extract_zip_backup(&archive, &restored).unwrap();
        assert_eq!(
            fs::read_to_string(restored.join("slot.sav")).unwrap(),
            "save"
        );
        assert_eq!(
            fs::read_to_string(restored.join("nested").join("settings.json")).unwrap(),
            "{}"
        );
    }

    #[test]
    fn zip_extract_rejects_zip_slip_paths() {
        let root = tempdir().unwrap();
        let archive_path = root.path().join("evil.zip");
        let file = File::create(&archive_path).unwrap();
        let mut writer = ZipWriter::new(file);
        writer
            .start_file("../evil.txt", FileOptions::default())
            .unwrap();
        writer.write_all(b"evil").unwrap();
        writer.finish().unwrap();

        let err = extract_zip_backup(&archive_path, &root.path().join("out")).unwrap_err();
        assert!(err.user_message().contains("ZIP"));
    }
}
