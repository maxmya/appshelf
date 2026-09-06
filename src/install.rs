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
const MIME: &str = "application/vnd.appimage";

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
pub fn install(resources: &Path, integrate: bool) -> Result<()> {
    runtime::runtime_path(resources)?;
    let commons = env::var_os("OMARCHY_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/usr/share/omarchy"))
        .join("shell/Commons");
    ensure!(
        commons.is_dir(),
        "Omarchy Quickshell Commons components are required"
    );
    let data = data_home();
    let destination = data.join("appshelf/program");
    let binary = home().join(".local/bin/appshelf");
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
    let icon = data.join(format!("icons/hicolor/scalable/apps/{APP_ID}.svg"));
    fs::create_dir_all(icon.parent().unwrap())?;
    fs::copy(resources.join(format!("packaging/{APP_ID}.svg")), icon)?;
    // Themes that skip scalable icons still find the rendered tile.
    let raster = data.join(format!("icons/hicolor/512x512/apps/{APP_ID}.png"));
    fs::create_dir_all(raster.parent().unwrap())?;
    let _ = fs::copy(resources.join(format!("packaging/{APP_ID}.png")), raster);
    if integrate {
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
                .args(["default", &format!("{APP_ID}.desktop"), MIME])
                .status()?
                .success(),
            "Could not register AppImage association"
        );
    }
    let _ = Command::new("update-desktop-database")
        .arg(desktop.parent().unwrap())
        .status();
    let _ = crate::service::install_service(&binary);
    println!(
        "Installed {}{}",
        binary.display(),
        if integrate {
            " with Flea/AppImage integration"
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
    let output = Command::new("xdg-mime")
        .args(["query", "default", MIME])
        .output()?;
    if String::from_utf8_lossy(&output.stdout).trim() == format!("{APP_ID}.desktop") {
        if !previous.is_empty() {
            ensure!(
                Command::new("xdg-mime")
                    .args(["default", previous, MIME])
                    .status()?
                    .success(),
                "Failed to restore association"
            );
        } else {
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
                        if let Some(value) = line.strip_prefix(&format!("{MIME}=")) {
                            let values: Vec<_> = value
                                .split(';')
                                .filter(|v| !v.is_empty() && *v != format!("{APP_ID}.desktop"))
                                .collect();
                            if !values.is_empty() {
                                out.push_str(&format!("{MIME}={};\n", values.join(";")));
                            }
                        } else {
                            out.push_str(line);
                            out.push('\n');
                        }
                    }
                    fs::write(path, out)?;
                }
            }
        }
    }
    fs::remove_file(backup)?;
    println!("Previous AppImage file association restored. Apps and launchers kept.");
    Ok(())
}
