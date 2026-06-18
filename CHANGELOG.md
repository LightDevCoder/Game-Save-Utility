# Changelog

## [0.1.2] - 2026-06-18

### Summary

This is a maintenance release focused on backup safety, configuration recovery, localization consistency, and Windows release stability. It does not introduce major new user-facing features.

### Improved

- Hardened automatic backup cleanup so it only removes system-created automatic backups and does not delete manual backups or pre-restore backups.
- Improved automatic backup behavior so unchanged save folders no longer create duplicate automatic backups.
- Added stable backup label typing to avoid relying on localized label text for backup classification.
- Stored automatic and pre-restore backup labels in the active UI language while preserving stable metadata classification.
- Improved configuration recovery using `config.json.bak`.
- Renamed the internal application data identifier from `GameSaveBackupTool` to `GameSaveUtility`.
- Preserved compatibility with old `GameSaveBackupTool` data directories and location files.
- Improved Chinese and English localization for user-facing error messages.
- Added log rotation and clearer safety documentation around Windows unsafe operations.
- Added Windows-only CI checks for formatting, build, tests, Clippy, and release builds.

### Fixed

- Fixed a risk where automatic cleanup could remove important non-automatic backups.
- Fixed duplicate automatic backups being created when save files had not changed.
- Fixed a risk where corrupted incremental backup objects could be restored without SHA-256 verification.
- Fixed missing validation for SHA-256 values in incremental backup manifests.
- Fixed first-launch configuration handling so a clean first launch is not reported as a recovery warning.
- Fixed configuration backup handling to better preserve and restore `.bak` files.
- Fixed mixed-language error messages in Chinese and English UI modes.
- Fixed remaining mixed-language folder-opening errors in Chinese UI, including residual `(IO error)` text.
- Fixed inconsistent system backup labels stored in metadata and backup folder names.

### Compatibility

- Existing backups remain readable.
- Existing metadata without the new backup label kind field remains supported.
- Existing `GameSaveBackupTool` app data can be migrated or read for compatibility.
- The app remains Windows-only.

### Notes

- This release focuses on reliability and safety rather than new features.
- Background backup/restore task execution and progress UI remain future improvements.
