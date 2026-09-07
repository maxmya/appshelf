//! AppShelf managing its own updates when it runs as an AppImage.
//!
//! The released AppImage embeds `gh-releases-zsync|…` in its `.upd_info` ELF
//! section, so the same machinery that updates managed applications also
//! resolves AppShelf's own releases — the manager is just another AppImage.
use crate::{
    install, runtime,
    update::{self, UpdateCheckResult},
};
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

/// Path of the running AppImage, as exported by the runtime.
pub fn appimage_path() -> Option<PathBuf> {
    let raw = env::var_os("APPIMAGE")?;
    let path = PathBuf::from(raw);
    path.is_file().then(|| path.canonicalize().unwrap_or(path))
}

pub const NOT_BUNDLED: &str = "AppShelf only manages its own updates when it runs as an AppImage; \
     this build was installed from source or a package, so update it the same way";

/// The same descriptor `scripts/build-appimage.sh` writes into the released
/// AppImage's `.upd_info`. Held here as well so the copy `--install` leaves in
/// `~/.local/share/appshelf/program` — a plain binary with no ELF section of
/// its own — can still find AppShelf's releases.
pub fn descriptor() -> String {
    format!(
        "gh-releases-zsync|maxmya|appshelf|latest|AppShelf-*-{}.AppImage.zsync",
        env::consts::ARCH
    )
}

/// True for any AppShelf AppImage, whatever its version. Discovery uses this to
/// keep AppShelf from offering to install itself as one of the applications it
/// manages, which would leave two AppShelf launchers behind.
pub fn is_appshelf(path: &Path) -> bool {
    runtime::update_info(path)
        .ok()
        .flatten()
        .map(|info| {
            info.trim()
                .starts_with("gh-releases-zsync|maxmya|appshelf|")
        })
        .unwrap_or(false)
}

/// Where an installed copy lives, if `--install` has been run.
fn installed_program() -> Option<PathBuf> {
    let program = install::program_dir().join("appshelf");
    program.is_file().then_some(program)
}

/// How this build can update itself: in place as an AppImage, by re-running a
/// freshly downloaded AppImage's `--install`, or not at all.
pub fn channel() -> &'static str {
    if appimage_path().is_some() {
        "appimage"
    } else if installed_program().is_some() {
        "installed"
    } else {
        "unmanaged"
    }
}

pub fn check() -> Result<UpdateCheckResult> {
    // From the AppImage the zsync checksum decides; from an installed copy
    // there is no file the release can be compared against, so the release tag
    // against this build's version is the only signal.
    let local = match channel() {
        "appimage" => Some(appimage_path().context(NOT_BUNDLED)?),
        "installed" => None,
        _ => bail!(NOT_BUNDLED),
    };
    let mut result = update::check_descriptor(&descriptor(), local.as_deref())?;
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
    let check = check()?;
    ensure!(check.supported, "{}", check.message);
    ensure!(check.has_update, "{}", check.message);
    let url = check
        .download_url
        .clone()
        .context("No AppImage asset resolved for the latest AppShelf release")?;
    match channel() {
        "appimage" => replace_appimage(&url, &check),
        "installed" => reinstall(&url, &check),
        _ => bail!(NOT_BUNDLED),
    }
}

/// Download the release and hand it to `--install`, so the copy under
/// `~/.local/share/appshelf/program` — binary, UI tree and packaging — is
/// replaced by the same code path that put it there.
fn reinstall(url: &str, check: &UpdateCheckResult) -> Result<Value> {
    let staging = tempfile::Builder::new()
        .prefix("appshelf-update-")
        .tempdir()
        .context("Cannot stage the update")?;
    let image = staging.path().join("AppShelf.AppImage");
    download(url, &image)?;
    runtime::filesystem(&image).context("Downloaded file is not a valid AppImage")?;
    fs::set_permissions(&image, fs::Permissions::from_mode(0o755))?;

    let output = Command::new(&image)
        .arg("--install")
        .output()
        .context("Could not run the downloaded AppShelf AppImage")?;
    ensure!(
        output.status.success(),
        "Install failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    // The tray is a long-lived process still running the previous binary.
    let _ = Command::new("systemctl")
        .args(["--user", "try-restart", "appshelf-tray.service"])
        .status();
    Ok(json!({
        "ok": true,
        "path": install::program_dir().join("appshelf").to_string_lossy(),
        "previous_version": env!("CARGO_PKG_VERSION"),
        "version": check.latest_version,
        "restart_required": true,
    }))
}

fn replace_appimage(url: &str, check: &UpdateCheckResult) -> Result<Value> {
    let target = appimage_path().context(NOT_BUNDLED)?;
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
        .arg(url)
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
        "restart_required": true,
    }))
}

fn download(url: &str, into: &Path) -> Result<()> {
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
        .arg(into)
        .arg(url)
        .status()
        .context("curl failed to execute download")?;
    ensure!(status.success(), "Update download failed");
    Ok(())
}
