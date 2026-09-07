use crate::manager::home;
use anyhow::{ensure, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub fn service_dir() -> PathBuf {
    home().join(".config/systemd/user")
}

pub fn autostart_dir() -> PathBuf {
    home().join(".config/autostart")
}

/// Silent on stdout: the backend protocol shares it, so a stray status line
/// would arrive at the window as an unparseable message.
pub fn install_service(binary: &Path) -> Result<()> {
    let s_dir = service_dir();
    fs::create_dir_all(&s_dir)?;
    let service_file = s_dir.join("appshelf-tray.service");
    let content = format!(
        "[Unit]\nDescription=AppShelf System Tray & Update Notifier\nPartOf=graphical-session.target\nAfter=graphical-session.target\n\n[Service]\nType=simple\nExecStart={} --tray\nRestart=on-failure\nRestartSec=5s\n\n[Install]\nWantedBy=graphical-session.target\n",
        binary.display()
    );
    fs::write(&service_file, content)?;

    let a_dir = autostart_dir();
    fs::create_dir_all(&a_dir)?;
    let autostart_file = a_dir.join("org.omarchy.appshelf.tray.desktop");
    let autostart_content = format!(
        "[Desktop Entry]\nType=Application\nName=AppShelf Tray\nExec={} --tray\nIcon=org.omarchy.appshelf\nTerminal=false\nCategories=Utility;\nX-GNOME-Autostart-enabled=true\n",
        binary.display()
    );
    fs::write(&autostart_file, autostart_content)?;

    let _ = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();
    let _ = Command::new("systemctl")
        .args(["--user", "enable", "--now", "appshelf-tray.service"])
        .status();

    Ok(())
}

pub fn uninstall_service() -> Result<()> {
    let _ = Command::new("systemctl")
        .args(["--user", "disable", "--now", "appshelf-tray.service"])
        .status();

    let service_file = service_dir().join("appshelf-tray.service");
    if service_file.exists() {
        let _ = fs::remove_file(service_file);
    }
    let autostart_file = autostart_dir().join("org.omarchy.appshelf.tray.desktop");
    if autostart_file.exists() {
        let _ = fs::remove_file(autostart_file);
    }
    let _ = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();

    Ok(())
}

pub fn status_service() -> Result<()> {
    let service_file = service_dir().join("appshelf-tray.service");
    println!("Systemd user service installed: {}", service_file.is_file());
    let _ = Command::new("systemctl")
        .args(["--user", "status", "appshelf-tray.service"])
        .status();
    Ok(())
}

/// Whether the tray is set to come back on the next login.
pub fn service_installed() -> bool {
    service_dir().join("appshelf-tray.service").is_file()
}

/// Whether a tray process is up right now. Started either by the systemd user
/// unit or directly by the shelf window, so match on the command line rather
/// than asking systemd.
pub fn tray_running() -> bool {
    Command::new("pgrep")
        .args(["-f", "--", "appshelf --tray"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub fn start_tray(binary: &Path) -> Result<()> {
    if tray_running() {
        return Ok(());
    }
    if service_installed() {
        let started = Command::new("systemctl")
            .args(["--user", "start", "appshelf-tray.service"])
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if started {
            return Ok(());
        }
    }
    Command::new(binary)
        .arg("--tray")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    Ok(())
}

pub fn stop_tray() -> Result<()> {
    if service_installed() {
        let _ = Command::new("systemctl")
            .args(["--user", "stop", "appshelf-tray.service"])
            .status();
    }
    // A tray the window spawned directly is not the unit's child, so systemd
    // stopping the unit does not reach it.
    let _ = Command::new("pkill")
        .args(["-f", "--", "appshelf --tray"])
        .status();
    ensure!(!tray_running(), "The tray is still running");
    Ok(())
}
