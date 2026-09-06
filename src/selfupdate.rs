//! AppShelf managing its own updates when it runs as an AppImage.
//!
//! The released AppImage embeds `gh-releases-zsync|…` in its `.upd_info` ELF
//! section, so the same machinery that updates managed applications also
//! resolves AppShelf's own releases — the manager is just another AppImage.
use crate::{
    runtime,
    update::{self, UpdateCheckResult},
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{env, fs, os::unix::fs::PermissionsExt, path::PathBuf, process::Command};

/// Path of the running AppImage, as exported by the runtime.
pub fn appimage_path() -> Option<PathBuf> {
    let raw = env::var_os("APPIMAGE")?;
    let path = PathBuf::from(raw);
    path.is_file().then(|| path.canonicalize().unwrap_or(path))
}

pub const NOT_BUNDLED: &str = "AppShelf only manages its own updates when it runs as an AppImage; \
     this build was installed from source or a package, so update it the same way";

pub fn check() -> Result<UpdateCheckResult> {
    let path = appimage_path().context(NOT_BUNDLED)?;
    let mut result = update::check_update(&path)?;
    result.current_version = Some(env!("CARGO_PKG_VERSION").into());
    // A release tag matching the running build means there is nothing to fetch,
    // even before the zsync checksum comparison gets a chance to say so.
    if let Some(latest) = &result.latest_version {
        if latest.trim_start_matches('v') == env!("CARGO_PKG_VERSION") {
            result.has_update = false;
            result.message = format!("AppShelf {} is up to date", env!("CARGO_PKG_VERSION"));
        }
    }
    Ok(result)
}

pub fn apply() -> Result<Value> {
    let target = appimage_path().context(NOT_BUNDLED)?;
    let check = check()?;
    ensure!(check.supported, "{}", check.message);
    ensure!(check.has_update, "{}", check.message);
    let url = check
        .download_url
        .context("No AppImage asset resolved for the latest AppShelf release")?;

    let parent = target.parent().context("AppImage has no directory")?;
    let staged = tempfile::Builder::new()
        .prefix(".appshelf-update-")
        .tempfile_in(parent)
        .context("Cannot stage the update beside the current AppImage")?;

    let status = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--proto",
            "=https",
            "--tlsv1.2",
            "--max-time",
            "600",
            "--output",
        ])
        .arg(staged.path())
        .arg(&url)
        .status()
        .context("curl failed to execute download")?;
    ensure!(status.success(), "Update download failed");

    // Refuse to install anything that is not a usable AppImage.
    runtime::filesystem(staged.path()).context("Downloaded file is not a valid AppImage")?;
    let mode = fs::metadata(&target)?.permissions().mode();
    staged
        .as_file()
        .set_permissions(fs::Permissions::from_mode(mode | 0o755))?;

    // Replacing the path is safe while this process runs: the kernel keeps the
    // open inode alive until the current AppImage exits.
    staged
        .persist(&target)
        .map_err(|e| e.error)
        .context("Failed to replace the running AppImage")?;

    Ok(json!({
        "ok": true,
        "path": target.to_string_lossy(),
        "previous_version": env!("CARGO_PKG_VERSION"),
        "version": check.latest_version,
    }))
}
