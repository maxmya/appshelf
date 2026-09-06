use crate::manager::home;
use anyhow::Result;
use std::{fs, path::Path, path::PathBuf, process::Command};

pub fn service_dir() -> PathBuf {
    home().join(".config/systemd/user")
}

pub fn autostart_dir() -> PathBuf {
    home().join(".config/autostart")
}

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

    println!("AppShelf background service and tray enabled (systemd user service + autostart)");
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

    println!("AppShelf background service and tray disabled");
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
