use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub(super) fn resolve_local_path(root: &Path, local_path: &Path) -> Result<PathBuf> {
    let path = if local_path.is_absolute() {
        local_path.to_path_buf()
    } else {
        root.join(local_path)
    };

    path.canonicalize()
        .with_context(|| format!("local_path `{}` does not exist", local_path.display()))
}

pub(super) fn config_directory(config_path: &Path) -> &Path {
    config_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

pub(super) fn local_config_path(config_path: &Path) -> PathBuf {
    let file_stem = config_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("services");
    let local_name = format!("{file_stem}.local.toml");

    config_directory(config_path).join(local_name)
}
