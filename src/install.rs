use crate::{
    manager::{data_home, desktop_exec, home, APP_ID},
    runtime,
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    env, fs,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    process::Command,
};
/// Everything AppShelf can open. `application/x-alpm-package` is AppShelf's
/// own contribution to the shared MIME database — see
/// `packaging/org.omarchy.appshelf.mime.xml` — because an Arch package is a
/// plain compressed tarball that the desktop would otherwise class with every
/// other one.
const MIMES: &[&str] = &[
    "application/vnd.appimage",
    "application/x-alpm-package",
    "application/vnd.debian.binary-package",
    "application/x-rpm",
];
/// The one whose previous owner is saved and given back. AppImages are what
/// AppShelf displaces from an established handler; nothing else on an Omarchy
/// box claims a `.deb`.
const MIME: &str = "application/vnd.appimage";

/// What an install should do about the `application/vnd.appimage` handler.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Association {
    /// Leave whatever opener is configured alone.
    Keep,
    /// Make AppShelf the default opener, saving the previous one.
    Claim,
    /// Hand the association back to the previously saved opener.
    Release,
}

fn commons_dir() -> PathBuf {
    env::var_os("OMARCHY_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/usr/share/omarchy"))
        .join("shell/Commons")
}
/// Where `--install` puts the self-contained copy, and the symlink onto $PATH.
pub fn program_dir() -> PathBuf {
    data_home().join("appshelf/program")
}
pub fn binary_link() -> PathBuf {
    home().join(".local/bin/appshelf")
}
fn is_default_opener() -> bool {
    Command::new("xdg-mime")
        .args(["query", "default", MIME])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == format!("{APP_ID}.desktop"))
        .unwrap_or(false)
}
/// Publish the Arch package MIME type into the user's share of the shared
/// database. Without it `.pkg.tar.zst` is just another compressed tarball and
/// no association could name it.
fn install_mime_type(resources: &Path, data: &Path) -> Result<()> {
    let source = resources.join(format!("packaging/{APP_ID}.mime.xml"));
    if !source.is_file() {
        return Ok(());
    }
    let packages = data.join("mime/packages");
    fs::create_dir_all(&packages)?;
    fs::copy(source, packages.join(format!("{APP_ID}.xml")))?;
    let _ = Command::new("update-mime-database")
        .arg(data.join("mime"))
        .status();
    Ok(())
}
/// Everything the setup window needs to describe this machine: whether AppShelf
/// is already installed, at which version, and who currently opens AppImages.
pub fn state(resources: &Path) -> Result<Value> {
    let program = program_dir().join("appshelf");
    let link = binary_link();
    let installed = program.is_file();
    let installed_version = installed
        .then(|| Command::new(&program).arg("--version").output().ok())
        .flatten()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .trim_start_matches("appshelf ")
                .to_string()
        });
    // `install` refuses to clobber a symlink it did not make; say so up front
    // rather than after the button is pressed.
    let blocked = (link.exists() || link.is_symlink())
        && !(link.is_symlink() && fs::read_link(&link).map(|t| t == program).unwrap_or(false));
    let icon = resources.join(format!("packaging/{APP_ID}.png"));
    Ok(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "appimage": crate::selfupdate::appimage_path().map(|p| p.to_string_lossy().into_owned()),
        "installed": installed,
        "installed_version": installed_version,
        "program": program.to_string_lossy(),
        "binary": link.to_string_lossy(),
        "blocked": blocked,
        "integrated": is_default_opener(),
        "icon": icon.is_file().then(|| icon.to_string_lossy().into_owned()),
    }))
}

fn copy_tree(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == "Commons" || name == "__pycache__" {
            continue;
        }
        let dest = target.join(name);
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &dest)?;
        } else if entry.file_type()?.is_file() {
            fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}
pub fn install(resources: &Path, association: Association) -> Result<()> {
    runtime::runtime_path(resources)?;
    let commons = commons_dir();
    ensure!(
        commons.is_dir(),
        "Omarchy Quickshell Commons components are required"
    );
    let data = data_home();
    let destination = program_dir();
    let binary = binary_link();
    if binary.exists() || binary.is_symlink() {
        ensure!(
            binary.is_symlink() && fs::read_link(&binary)? == destination.join("appshelf"),
            "Refusing to replace unrelated {}",
            binary.display()
        );
    }
    fs::create_dir_all(&destination)?;
    for folder in ["ui", "vendor", "packaging"] {
        copy_tree(&resources.join(folder), &destination.join(folder))?;
    }
    // Atomic replacement also permits upgrading while the previous binary is running.
    let staged = destination.join(".appshelf-new");
    fs::copy(env::current_exe()?, &staged)?;
    fs::rename(staged, destination.join("appshelf"))?;
    let link = destination.join("ui/Commons");
    if !link.exists() {
        symlink(commons, &link)?;
    }
    fs::create_dir_all(binary.parent().unwrap())?;
    if !binary.is_symlink() {
        symlink(destination.join("appshelf"), &binary)?;
    }
    let desktop = data.join(format!("applications/{APP_ID}.desktop"));
    fs::create_dir_all(desktop.parent().unwrap())?;
    let body = fs::read_to_string(resources.join(format!("packaging/{APP_ID}.desktop")))?;
    fs::write(
        &desktop,
        body.replace(
            "Exec=appshelf %f",
            &format!("Exec={} %f", desktop_exec(&destination.join("appshelf"))),
        ),
    )?;
    install_mime_type(resources, &data)?;
    let icon = data.join(format!("icons/hicolor/scalable/apps/{APP_ID}.svg"));
    fs::create_dir_all(icon.parent().unwrap())?;
    fs::copy(resources.join(format!("packaging/{APP_ID}.svg")), icon)?;
    // Themes that skip scalable icons still find the rendered tile.
    let raster = data.join(format!("icons/hicolor/512x512/apps/{APP_ID}.png"));
    fs::create_dir_all(raster.parent().unwrap())?;
    let _ = fs::copy(resources.join(format!("packaging/{APP_ID}.png")), raster);
    if association == Association::Claim {
        let backup = data.join("appshelf/integration.json");
        if !backup.exists() {
            let output = Command::new("xdg-mime")
                .args(["query", "default", MIME])
                .output()?;
            ensure!(
                output.status.success(),
                "Could not query AppImage association"
            );
            fs::write(
                backup,
                serde_json::to_vec(
                    &json!({"previous":String::from_utf8_lossy(&output.stdout).trim()}),
                )?,
            )?;
        }
        ensure!(
            Command::new("xdg-mime")
                .args(["default", &format!("{APP_ID}.desktop")])
                .args(MIMES)
                .status()?
                .success(),
            "Could not register the AppShelf file associations"
        );
    }
    // Giving the association back is only meaningful if we hold it and saved
    // what came before; otherwise there is nothing to undo.
    if association == Association::Release
        && is_default_opener()
        && data.join("appshelf/integration.json").is_file()
    {
        restore_association()?;
    }
    let _ = Command::new("update-desktop-database")
        .arg(desktop.parent().unwrap())
        .status();
    let _ = crate::service::install_service(&binary);
    println!(
        "Installed {}{}",
        binary.display(),
        if association == Association::Claim {
            " as the opener for AppImages and packages"
        } else {
            ""
        }
    );
    Ok(())
}
pub fn restore_association() -> Result<()> {
    let backup = data_home().join("appshelf/integration.json");
    let saved: Value =
        serde_json::from_slice(&fs::read(&backup).context("No saved file association")?)?;
    let previous = saved["previous"].as_str().unwrap_or_default();
    // Only the AppImage association had an owner worth remembering, so only it
    // is handed back to a named application. The package types are simply
    // released: nothing held them before AppShelf did.
    let mine = format!("{APP_ID}.desktop");
    let held: Vec<&str> = MIMES
        .iter()
        .copied()
        .filter(|mime| {
            Command::new("xdg-mime")
                .args(["query", "default", mime])
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim() == mine)
                .unwrap_or(false)
        })
        .collect();
    if held.contains(&MIME) && !previous.is_empty() {
        ensure!(
            Command::new("xdg-mime")
                .args(["default", previous, MIME])
                .status()?
                .success(),
            "Failed to restore association"
        );
    }
    // Whatever is left is dropped by name from the user's own mimeapps lists;
    // there is no xdg-mime verb for "no default", so the entry has to go.
    let release: Vec<&str> = held
        .into_iter()
        .filter(|mime| !(*mime == MIME && !previous.is_empty()))
        .collect();
    if !release.is_empty() {
        let config = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".config"));
        for path in [
            config.join("mimeapps.list"),
            data_home().join("applications/mimeapps.list"),
        ] {
            if let Ok(body) = fs::read_to_string(&path) {
                let mut out = String::new();
                for line in body.lines() {
                    let released = release.iter().find_map(|mime| {
                        line.strip_prefix(&format!("{mime}="))
                            .map(|value| (*mime, value))
                    });
                    match released {
                        Some((mime, value)) => {
                            let values: Vec<_> = value
                                .split(';')
                                .filter(|v| !v.is_empty() && *v != mine)
                                .collect();
                            if !values.is_empty() {
                                out.push_str(&format!("{mime}={};\n", values.join(";")));
                            }
                        }
                        None => {
                            out.push_str(line);
                            out.push('\n');
                        }
                    }
                }
                fs::write(path, out)?;
            }
        }
    }
    fs::remove_file(backup)?;
    println!("Previous file associations restored. Apps and launchers kept.");
    Ok(())
}
