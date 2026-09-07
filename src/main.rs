use anyhow::{bail, Context, Result};
use appshelf::{
    discovery, install,
    manager::{self, Manager, Settings},
    runtime, selfupdate, service,
};
use serde_json::{json, Value};
use std::{
    env, fs,
    io::{self, BufRead, Write},
    os::unix::{fs::symlink, process::CommandExt},
    path::{Path, PathBuf},
    process::Command,
};

fn resources() -> Result<PathBuf> {
    let exe = env::current_exe()?.canonicalize()?;
    let beside = exe.parent().context("No binary directory")?;
    if beside.join("ui/shell.qml").is_file() {
        return Ok(beside.into());
    }
    let checkout = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if checkout.join("ui/shell.qml").is_file() {
        return Ok(checkout);
    }
    bail!("AppShelf UI resources not found")
}
/// Everything the Settings view shows about this installation, refreshed after
/// every command that can change it.
fn preferences(manager: &Manager) -> Value {
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "channel": selfupdate::channel(),
        "tray_running": service::tray_running(),
        "service_installed": service::service_installed(),
        "binary": manager.binary.to_string_lossy(),
        "program": install::program_dir().join("appshelf").to_string_lossy(),
        "data": manager.root.parent().map(|p| p.to_string_lossy().into_owned()),
        // Counted apart, because they are two different claims: one is what
        // AppShelf keeps a copy of, the other what it asked pacman to install.
        "managed": manager
            .list()
            .iter()
            .filter(|app| app["kind"].as_str() != Some("package"))
            .count(),
        "packages": appshelf::package::registry().len(),
    })
}
/// Check every managed application in one pass, so the window can offer the
/// same sweep the tray performs on its own schedule.
fn check_all(manager: &Manager) -> Value {
    let mut updates = Vec::new();
    let mut failed = Vec::new();
    let apps: Vec<_> = manager
        .list()
        .into_iter()
        .filter(|app| app["kind"].as_str() != Some("package"))
        .collect();
    for app in &apps {
        let id = app["id"].as_str().unwrap_or_default();
        let name = app["name"].as_str().unwrap_or_default();
        match manager.check_update(id) {
            Ok(result) if result["has_update"].as_bool().unwrap_or(false) => updates.push(json!({
                "id": id,
                "name": name,
                "latest_version": result["latest_version"],
            })),
            Ok(_) => {}
            Err(error) => failed.push(json!({"name": name, "error": format!("{error:#}")})),
        }
    }
    json!({"checked": apps.len(), "updates": updates, "failed": failed})
}
fn emit(value: Value) {
    println!("{value}");
    let _ = io::stdout().flush();
}
fn serve(manager: Manager) -> Result<()> {
    let mut discovered = discovery::discover(&manager, &[]);
    emit(
        json!({"event":"ready","apps":manager.list(),"discovered":discovered,"preferences":preferences(&manager)}),
    );
    for line in io::stdin().lock().lines() {
        let line = line?;
        let request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                emit(json!({"ok":false,"error":e.to_string()}));
                continue;
            }
        };
        let command = request["command"].as_str().unwrap_or_default();
        if command == "quit" {
            emit(json!({"event":"quit"}));
            break;
        }
        let result = (|| -> Result<Value> {
            let path = || request["path"].as_str().context("Missing path");
            let id = || request["id"].as_str().context("Missing application ID");
            let settings = || -> Result<Settings> {
                Ok(serde_json::from_value(
                    json!({"environment":request.get("environment").cloned().unwrap_or(json!({})),"isolation":request.get("isolation").cloned().unwrap_or(json!("off"))}),
                )?)
            };
            match command {
                "inspect" => manager.inspect(path()?),
                "install" => manager.install(path()?, settings()?),
                "configure" => {
                    manager.configure(id()?, settings()?)?;
                    Ok(Value::Null)
                }
                "uninstall" => {
                    manager.uninstall(id()?)?;
                    Ok(Value::Null)
                }
                "launch" => {
                    manager.launch(id()?)?;
                    Ok(Value::Null)
                }
                "reveal" => {
                    manager.reveal(
                        request["id"].as_str().unwrap_or_default(),
                        request["path"].as_str(),
                    )?;
                    Ok(Value::Null)
                }
                "check-update" => manager.check_update(id()?),
                "update" => manager.update(id()?),
                "check-all-updates" => Ok(check_all(&manager)),
                "preferences" => Ok(preferences(&manager)),
                "tray-start" => {
                    service::start_tray(&manager.binary)?;
                    Ok(preferences(&manager))
                }
                "tray-stop" => {
                    service::stop_tray()?;
                    Ok(preferences(&manager))
                }
                "service-enable" => {
                    service::install_service(&manager.binary)?;
                    Ok(preferences(&manager))
                }
                "service-disable" => {
                    service::uninstall_service()?;
                    Ok(preferences(&manager))
                }
                "self-check-update" => Ok(serde_json::to_value(selfupdate::check()?)?),
                "self-update" => selfupdate::apply(),
                "list" => Ok(Value::Null),
                _ => bail!("Unknown command"),
            }
        })();
        match result {
            Ok(result) => {
                if ["list", "install", "uninstall", "update"].contains(&command) {
                    discovered = discovery::discover(&manager, &[]);
                }
                emit(
                    json!({"ok":true,"command":command,"result":result,"apps":manager.list(),"discovered":discovered,"preferences":preferences(&manager)}),
                );
            }
            Err(error) => emit(json!({"ok":false,"command":command,"error":format!("{error:#}")})),
        }
    }
    Ok(())
}
/// Resolve the QML tree to hand to Quickshell, linking in Omarchy's shared
/// Commons components. When the resources live on a read-only mount — an
/// AppImage — the tree is staged into a writable runtime directory instead.
fn ui_path(resources: &Path) -> Result<PathBuf> {
    let shared = env::var_os("OMARCHY_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/usr/share/omarchy"))
        .join("shell/Commons");
    let link_commons = |ui: &Path| -> Result<()> {
        let commons = ui.join("Commons");
        if commons.exists() {
            return Ok(());
        }
        anyhow::ensure!(
            shared.is_dir(),
            "Omarchy Quickshell Commons components are required"
        );
        symlink(&shared, commons)?;
        Ok(())
    };
    let bundled = resources.join("ui");
    if link_commons(&bundled).is_ok() {
        return Ok(bundled);
    }
    let staged = env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(manager::data_home)
        .join("appshelf/ui");
    fs::create_dir_all(&staged)?;
    for entry in fs::read_dir(&bundled)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            fs::copy(entry.path(), staged.join(entry.file_name()))?;
        }
    }
    link_commons(&staged)?;
    Ok(staged)
}
fn main() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--help")
        || args.first().map(String::as_str) == Some("-h")
    {
        println!("AppShelf — keyboard-first application management for Omarchy\n\nHandles AppImages, Arch packages (.pkg.tar.zst, .pkg.tar.xz),\nDebian packages (.deb) and RPM packages (.rpm).\n\nappshelf [file-or-file-URL]\nappshelf --shelf\nappshelf --tray\nappshelf --enable-service\nappshelf --disable-service\nappshelf --service-status\nappshelf --check-update [ID]\nappshelf --update ID\nappshelf --self-check-update\nappshelf --self-update\nappshelf --backend\nappshelf --launch ID\nappshelf --scan\nappshelf --inspect FILE\nappshelf --convert FILE [DIRECTORY]\nappshelf --fetch-runtime\nappshelf --install [--integrate|--no-integrate]\nappshelf --setup-state\nappshelf --restore-association\nappshelf --version");
        return Ok(());
    }
    if matches!(
        args.first().map(String::as_str),
        Some("--version") | Some("-V")
    ) {
        println!("appshelf {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let resources = resources()?;
    if args.first().map(String::as_str) == Some("--fetch-runtime") {
        return runtime::fetch(&resources);
    }
    if args.first().map(String::as_str) == Some("--install") {
        let association = if args.iter().any(|s| s == "--integrate") {
            install::Association::Claim
        } else if args.iter().any(|s| s == "--no-integrate") {
            install::Association::Release
        } else {
            install::Association::Keep
        };
        return install::install(&resources, association);
    }
    if args.first().map(String::as_str) == Some("--setup-state") {
        println!(
            "{}",
            serde_json::to_string_pretty(&install::state(&resources)?)?
        );
        return Ok(());
    }
    if args.first().map(String::as_str) == Some("--restore-association") {
        return install::restore_association();
    }
    let manager = Manager::new(
        manager::data_home(),
        resources.clone(),
        manager::launcher_path()?,
    )?;
    match args.first().map(String::as_str) {
        Some("--backend") => return serve(manager),
        Some("--scan") => {
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &json!({"apps":manager.list(),"discovered":discovery::discover(&manager,&[])})
                )?
            );
            return Ok(());
        }
        Some("--inspect") => {
            let path = args.get(1).context("Missing file")?;
            println!("{}", serde_json::to_string_pretty(&manager.inspect(path)?)?);
            return Ok(());
        }
        // Conversion on its own, so what pacman would be handed can be read
        // with pacman's own tools before anything is installed.
        Some("--convert") => {
            let source = manager::local_path(args.get(1).context("Missing file")?)?;
            let kind = appshelf::package::detect(&source)
                .context("That file is not a Debian, RPM or Arch package")?;
            let info = appshelf::package::inspect(&source, kind)?;
            let (built, _stage) = appshelf::package::convert(&source, &info)?;
            let destination = args
                .get(2)
                .map(PathBuf::from)
                .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
                .join(built.file_name().context("Converted package has no name")?);
            fs::copy(&built, &destination)?;
            println!("{}", destination.display());
            return Ok(());
        }
        Some("--tray") => {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;
            return rt.block_on(appshelf::tray::run_tray(std::sync::Arc::new(manager)));
        }
        Some("--enable-service") => {
            appshelf::service::install_service(&manager.binary)?;
            println!(
                "AppShelf background service and tray enabled (systemd user service + autostart)"
            );
            return Ok(());
        }
        Some("--disable-service") => {
            appshelf::service::uninstall_service()?;
            println!("AppShelf background service and tray disabled");
            return Ok(());
        }
        Some("--service-status") => {
            return appshelf::service::status_service();
        }
        Some("--check-update") => {
            if let Some(id) = args.get(1) {
                let res = manager.check_update(id)?;
                println!("{}", serde_json::to_string_pretty(&res)?);
            } else {
                for app in manager.list() {
                    let id = app["id"].as_str().unwrap_or_default();
                    let name = app["name"].as_str().unwrap_or_default();
                    match manager.check_update(id) {
                        Ok(res) => {
                            let msg = res["message"].as_str().unwrap_or("");
                            println!("{name}: {msg}");
                        }
                        Err(e) => println!("{name}: Error ({e})"),
                    }
                }
            }
            return Ok(());
        }
        Some("--self-check-update") => {
            let res = appshelf::selfupdate::check()?;
            println!("{}", serde_json::to_string_pretty(&res)?);
            return Ok(());
        }
        Some("--self-update") => {
            let res = appshelf::selfupdate::apply()?;
            println!("{}", serde_json::to_string_pretty(&res)?);
            return Ok(());
        }
        Some("--update") => {
            let id = args.get(1).context("Missing application ID")?;
            let res = manager.update(id)?;
            println!("{}", serde_json::to_string_pretty(&res)?);
            return Ok(());
        }
        Some("--launch") => {
            let result = manager.launch(args.get(1).context("Missing application ID")?);
            if let Err(error) = &result {
                let _ = Command::new("notify-send")
                    .args([
                        "--app-name=AppShelf",
                        "Launch failed",
                        &format!("{error:#}"),
                    ])
                    .status();
            }
            return result;
        }
        // Handled below: the shelf, with the first-run setup window suppressed.
        Some("--shelf") => {}
        Some(s) if s.starts_with("--") => bail!("Unknown option {s}"),
        _ => {}
    }
    let forced_shelf = args.first().map(String::as_str) == Some("--shelf");
    let args = if forced_shelf { &args[1..] } else { &args[..] };
    anyhow::ensure!(args.len() <= 1, "Open one AppImage at a time");
    // Running the AppImage itself is a request to set AppShelf up, not to use a
    // copy that vanishes with its mount — unless --shelf says otherwise.
    let setup = !forced_shelf && args.is_empty() && appshelf::selfupdate::appimage_path().is_some();
    let ui = ui_path(&resources)?;
    if !setup {
        let _ = service::start_tray(&manager.binary);
    }
    // Opening a file goes straight to the compact installer; the full shelf is
    // only worth loading when AppShelf is started on its own.
    let entry = match args.first() {
        _ if setup => ui.join("setup.qml"),
        Some(_) => ui.join("install.qml"),
        None => ui,
    };
    let error = Command::new("quickshell")
        .args(["-p"])
        .arg(entry)
        .env("APPSHELF_BACKEND", env::current_exe()?)
        .env("APPSHELF_OPEN", args.first().cloned().unwrap_or_default())
        .exec();
    Err(error.into())
}
