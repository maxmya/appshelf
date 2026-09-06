use crate::manager::Manager;
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
        "org.omarchy.appshelf".into()
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
        if self.updates_count > 0 {
            "software-update-available".into()
        } else {
            "org.omarchy.appshelf".into()
        }
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
    pub fn check_now(&mut self, notify_if_none: bool) -> usize {
        let mut count = 0;
        let mut names = Vec::new();
        for app in self.manager.list() {
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

        if count > 0 {
            let body = format!("Updates available for:\n{}", names.join("\n"));
            let _ = Command::new("notify-send")
                .args([
                    "--app-name=AppShelf",
                    "--icon=software-update-available",
                    "AppImage Updates Available",
                    &body,
                ])
                .status();
        } else if notify_if_none {
            let _ = Command::new("notify-send")
                .args([
                    "--app-name=AppShelf",
                    "--icon=org.omarchy.appshelf",
                    "AppShelf",
                    "All AppImages are up to date.",
                ])
                .status();
        }
        count
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
                    let _ = Command::new("notify-send")
                        .args([
                            "--app-name=AppShelf",
                            "--icon=software-update-available",
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

    let _ = tokio::signal::ctrl_c().await;
    let _ = handle.shutdown().await;
    Ok(())
}
