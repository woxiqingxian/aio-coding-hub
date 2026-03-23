use serde::{Deserialize, Serialize};
use std::path::Path;

const MANAGED_MARKER_FILE: &str = ".aio-coding-hub.managed";
const SOURCE_MARKER_FILE: &str = ".aio-coding-hub.source.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SkillSourceMetadata {
    pub source_git_url: String,
    pub source_branch: String,
    pub source_subdir: String,
}

pub(super) fn copy_dir_recursive(src: &Path, dst: &Path) -> crate::shared::error::AppResult<()> {
    let src_meta = std::fs::symlink_metadata(src)
        .map_err(|e| format!("failed to read metadata {}: {e}", src.display()))?;
    if src_meta.file_type().is_symlink() {
        return Err(format!("SKILL_COPY_BLOCKED_SYMLINK: {}", src.display()).into());
    }
    if !src_meta.is_dir() {
        return Err(format!(
            "SEC_INVALID_INPUT: copy source is not a directory: {}",
            src.display()
        )
        .into());
    }

    std::fs::create_dir_all(dst).map_err(|e| format!("failed to create {}: {e}", dst.display()))?;
    let entries =
        std::fs::read_dir(src).map_err(|e| format!("failed to read dir {}: {e}", src.display()))?;
    for entry in entries {
        let entry =
            entry.map_err(|e| format!("failed to read dir entry {}: {e}", src.display()))?;
        let path = entry.path();
        let file_name = entry.file_name();
        let dst_path = dst.join(&file_name);

        let file_type = entry
            .file_type()
            .map_err(|e| format!("failed to read file type {}: {e}", path.display()))?;
        if file_type.is_symlink() {
            return Err(format!("SKILL_COPY_BLOCKED_SYMLINK: {}", path.display()).into());
        }
        if file_type.is_dir() {
            copy_dir_recursive(&path, &dst_path)?;
            continue;
        }
        if !file_type.is_file() {
            return Err(format!("SKILL_COPY_BLOCKED_SPECIAL_FILE: {}", path.display()).into());
        }
        std::fs::copy(&path, &dst_path).map_err(|e| {
            format!(
                "failed to copy {} -> {}: {e}",
                path.display(),
                dst_path.display()
            )
        })?;
    }
    Ok(())
}

pub(super) fn write_marker(dir: &Path) -> crate::shared::error::AppResult<()> {
    let path = dir.join(MANAGED_MARKER_FILE);
    std::fs::write(&path, "aio-coding-hub\n")
        .map_err(|e| format!("failed to write marker {}: {e}", path.display()).into())
}

pub(super) fn remove_marker(dir: &Path) {
    let path = dir.join(MANAGED_MARKER_FILE);
    let _ = std::fs::remove_file(path);
}

pub(super) fn write_source_metadata(
    dir: &Path,
    metadata: &SkillSourceMetadata,
) -> crate::shared::error::AppResult<()> {
    let path = dir.join(SOURCE_MARKER_FILE);
    let content = serde_json::to_vec_pretty(metadata).map_err(|e| {
        format!(
            "failed to serialize source metadata {}: {e}",
            path.display()
        )
    })?;
    std::fs::write(&path, content)
        .map_err(|e| format!("failed to write source metadata {}: {e}", path.display()).into())
}

pub(super) fn read_source_metadata(
    dir: &Path,
) -> crate::shared::error::AppResult<Option<SkillSourceMetadata>> {
    let path = dir.join(SOURCE_MARKER_FILE);
    if !path.exists() {
        return Ok(None);
    }

    let bytes = std::fs::read(&path)
        .map_err(|e| format!("failed to read source metadata {}: {e}", path.display()))?;
    let metadata = serde_json::from_slice::<SkillSourceMetadata>(&bytes)
        .map_err(|e| format!("failed to parse source metadata {}: {e}", path.display()))?;
    Ok(Some(metadata))
}

pub(super) fn is_managed_dir(dir: &Path) -> bool {
    dir.join(MANAGED_MARKER_FILE).exists()
}

pub(super) use crate::shared::fs::is_symlink;

pub(super) fn has_skill_md(path: &Path) -> bool {
    path.join("SKILL.md").exists()
}

pub(super) fn remove_managed_dir(dir: &Path) -> crate::shared::error::AppResult<()> {
    if !dir.exists() {
        return Ok(());
    }
    if !is_managed_dir(dir) {
        return Err(format!(
            "SKILL_REMOVE_BLOCKED_UNMANAGED: target exists but is not managed: {}",
            dir.display()
        )
        .into());
    }
    std::fs::remove_dir_all(dir).map_err(|e| format!("failed to remove {}: {e}", dir.display()))?;
    Ok(())
}
