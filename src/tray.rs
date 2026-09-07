use crate::manager::{Manager, APP_ID};
use ksni::{
    menu::{MenuItem, StandardItem},
    Category, Status, ToolTip, Tray, TrayMethods,
};
use std::{process::Command, sync::Arc, time::Duration};

pub struct AppShelfTray {
    pub manager: Arc<Manager>,
    pub updates_count: usize,
    pub update_names: Vec<String>,
}

impl Tray for AppShelfTray {
    fn id(&self) -> String {
        APP_ID.into()
    }
    fn category(&self) -> Category {
        Category::ApplicationStatus
    }
    fn title(&self) -> String {
        if self.updates_count > 0 {
            format!("AppShelf ({} updates available)", self.updates_count)
        } else {
            "AppShelf".into()
        }
    }
    fn status(&self) -> Status {
        if self.updates_count > 0 {
            Status::NeedsAttention
        } else {
            Status::Active
        }
    }
    fn icon_name(&self) -> String {
        // Always AppShelf's own icon — Status::NeedsAttention already conveys
        // pending updates, and swapping in a stock glyph made the tray look
        // like a different application.
        APP_ID.into()
    }
    /// Hosts resolve `icon_name` through their own icon theme, and Qt-based
    /// ones (Quickshell, and so Omarchy's bar) cache a name that missed for the
    /// life of the process. A bar started before AppShelf was installed
    /// therefore renders the host's missing-icon placeholder forever. Handing
    /// over the directory the icon actually lives in makes the lookup a file
    /// read instead, so the tray is correct on first install without a
    /// re-login.
    fn icon_theme_path(&self) -> String {
        self.manager
            .resources
            .join("packaging")
            .to_string_lossy()
            .into_owned()
    }
    /// `Status::NeedsAttention` sends hosts to the attention icon, which
    /// defaults to empty — leaving the tray blank exactly when there are
    /// updates to notice.
    fn attention_icon_name(&self) -> String {
        self.icon_name()
    }
    fn tool_tip(&self) -> ToolTip {
        let description = if self.updates_count > 0 {
            format!(
                "{} updates available:\n{}",
                self.updates_count,
                self.update_names.join(", ")
            )
        } else {
            "All applications up to date".into()
        };
        ToolTip {
            title: "AppShelf".into(),
            description,
            icon_name: self.icon_name(),
            ..Default::default()
        }
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = Command::new(&self.manager.binary).spawn();
    }
    fn menu(&self) -> Vec<MenuItem<Self>> {
        let binary = self.manager.binary.clone();
        let count = self.updates_count;
        let update_label = if count > 0 {
            format!("Updates available: {count}")
        } else {
            "All apps up to date".into()
        };

        vec![
            StandardItem {
                label: "Open AppShelf".into(),
                activate: Box::new(move |_| {
                    let _ = Command::new(&binary).spawn();
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: update_label,
                enabled: false,
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Check for Updates Now".into(),
                activate: Box::new(|this: &mut Self| {
                    this.check_now(true);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit Tray".into(),
                activate: Box::new(|_| {
                    std::process::exit(0);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

impl AppShelfTray {
    /// Identity of the current shelf contents, used to notice installs,
    /// removals and updates performed by the window without polling for them
    /// through the backend protocol.
    pub fn shelf_fingerprint(manager: &Manager) -> Vec<String> {
        let mut ids: Vec<String> = manager
            .list()
            .iter()
            .map(|app| {
                format!(
                    "{}:{}",
                    app["id"].as_str().unwrap_or_default(),
                    app["name"].as_str().unwrap_or_default()
                )
            })
            .collect();
        ids.sort();
        ids
    }
    pub fn check_now(&mut self, notify_if_none: bool) -> usize {
        let mut count = 0;
        let mut names = Vec::new();
        // System packages are pacman's to update, and the tray does not speak
        // for pacman; they are skipped rather than counted as up to date.
        for app in self.manager.list() {
            if app["kind"].as_str() == Some("package") {
                continue;
            }
            let id = app["id"].as_str().unwrap_or_default();
            let name = app["name"].as_str().unwrap_or_default();
            if let Ok(res) = self.manager.check_update(id) {
                if res["has_update"].as_bool().unwrap_or(false) {
                    count += 1;
                    let ver = res["latest_version"].as_str().unwrap_or("");
                    names.push(format!("{name} ({ver})"));
                }
            }
        }
        self.updates_count = count;
        self.update_names = names.clone();

        let icon = format!("--icon={}", notify_icon(&self.manager));
        if count > 0 {
            let body = format!("Updates available for:\n{}", names.join("\n"));
            let _ = Command::new("notify-send")
                .args([
                    "--app-name=AppShelf",
                    &icon,
                    "AppImage Updates Available",
                    &body,
                ])
                .status();
        } else if notify_if_none {
            let _ = Command::new("notify-send")
                .args([
                    "--app-name=AppShelf",
                    &icon,
                    "AppShelf",
                    "All AppImages are up to date.",
                ])
                .status();
        }
        count
    }
}

/// Notification daemons resolve icon names through the same theme cache tray
/// hosts do, so hand them the file directly when it is there.
fn notify_icon(manager: &Manager) -> String {
    let file = manager.resources.join(format!("packaging/{APP_ID}.png"));
    if file.is_file() {
        file.to_string_lossy().into_owned()
    } else {
        APP_ID.into()
    }
}

pub async fn run_tray(manager: Arc<Manager>) -> anyhow::Result<()> {
    let mut tray = AppShelfTray {
        manager: manager.clone(),
        updates_count: 0,
        update_names: Vec::new(),
    };

    tray.check_now(false);

    let handle = tray.spawn().await?;

    let bg_mgr = manager.clone();
    let bg_handle = handle.clone();

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(4 * 3600));
        interval.tick().await;

        loop {
            interval.tick().await;
            let mgr = bg_mgr.clone();
            let (count, names) = tokio::task::spawn_blocking(move || {
                let mut count = 0;
                let mut names = Vec::new();
                for app in mgr.list() {
                    if app["kind"].as_str() == Some("package") {
                        continue;
                    }
                    let id = app["id"].as_str().unwrap_or_default();
                    let name = app["name"].as_str().unwrap_or_default();
                    if let Ok(res) = mgr.check_update(id) {
                        if res["has_update"].as_bool().unwrap_or(false) {
                            count += 1;
                            let ver = res["latest_version"].as_str().unwrap_or("");
                            names.push(format!("{name} ({ver})"));
                        }
                    }
                }
                if count > 0 {
                    let body = format!("Updates available for:\n{}", names.join("\n"));
                    let icon = format!("--icon={}", notify_icon(&mgr));
                    let _ = Command::new("notify-send")
                        .args([
                            "--app-name=AppShelf",
                            &icon,
                            "AppImage Updates Available",
                            &body,
                        ])
                        .status();
                }
                (count, names)
            })
            .await
            .unwrap_or((0, Vec::new()));

            let _ = bg_handle
                .update(move |tray: &mut AppShelfTray| {
                    tray.updates_count = count;
                    tray.update_names = names;
                })
                .await;
        }
    });

    // The shelf is shared state: the window installs and removes applications
    // behind the tray's back, so follow the directory rather than waiting for
    // the four-hourly update sweep.
    let watch_mgr = manager.clone();
    let watch_handle = handle.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(3));
        let mut known = {
            let mgr = watch_mgr.clone();
            tokio::task::spawn_blocking(move || AppShelfTray::shelf_fingerprint(&mgr))
                .await
                .unwrap_or_default()
        };
        loop {
            interval.tick().await;
            let mgr = watch_mgr.clone();
            let current =
                match tokio::task::spawn_blocking(move || AppShelfTray::shelf_fingerprint(&mgr))
                    .await
                {
                    Ok(value) => value,
                    Err(_) => continue,
                };
            if current == known {
                continue;
            }
            known = current;
            // Counts refer to applications that may no longer be installed;
            // drop them and let the next sweep, or the user, recompute.
            let _ = watch_handle
                .update(move |tray: &mut AppShelfTray| {
                    tray.updates_count = 0;
                    tray.update_names.clear();
                })
                .await;
        }
    });

    let _ = tokio::signal::ctrl_c().await;
    let _ = handle.shutdown().await;
    Ok(())
}
