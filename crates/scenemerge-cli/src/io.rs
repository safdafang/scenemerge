//! File loading helpers.

use std::path::Path;

use anyhow::{Context, Result};
use scene_model::SceneFile;

/// Whether `path` stands for "no file" in Git's external-diff protocol.
pub fn is_null_path(path: &Path) -> bool {
    let s = path.to_string_lossy();
    s == "/dev/null" || s.eq_ignore_ascii_case("nul")
}

/// Reads and models a scene/resource file. `display` is the name used in
/// error messages (Git passes temporary file names; the real path is nicer).
pub fn load(path: &Path, display: &Path) -> Result<SceneFile> {
    let bytes = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    SceneFile::parse_bytes(&bytes).map_err(|e| anyhow::Error::new(e.with_path(display)))
}

/// Like [`load`], but `/dev/null` yields `None`.
pub fn load_optional(path: &Path, display: &Path) -> Result<Option<SceneFile>> {
    if is_null_path(path) {
        Ok(None)
    } else {
        load(path, display).map(Some)
    }
}

/// An empty file of the same kind as `other`, used as the missing side when a
/// file is added or deleted.
pub fn empty_like(other: &SceneFile) -> SceneFile {
    let text = match other.kind {
        scene_model::FileKind::Scene => "[gd_scene format=3]\n".to_string(),
        scene_model::FileKind::Resource => format!(
            "[gd_resource type=\"{}\" format=3]\n",
            other
                .descriptor
                .resource_type
                .as_deref()
                .unwrap_or("Resource")
        ),
    };
    // The literal above is always valid.
    SceneFile::parse(&text).unwrap_or_else(|_| other.clone())
}

/// Writes `bytes` to `path` atomically when possible (temporary file + rename).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty());
    let tmp_name = format!(
        ".{}.scenemerge-tmp",
        path.file_name()
            .map_or("out".into(), |n| n.to_string_lossy())
    );
    let tmp = match dir {
        Some(d) => d.join(tmp_name),
        None => Path::new(&tmp_name).to_path_buf(),
    };
    std::fs::write(&tmp, bytes).with_context(|| format!("cannot write {}", tmp.display()))?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        // Fall back to a direct write (e.g. cross-device or locked target on Windows).
        let _ = std::fs::remove_file(&tmp);
        std::fs::write(path, bytes)
            .with_context(|| format!("cannot write {} ({e})", path.display()))?;
    }
    Ok(())
}
