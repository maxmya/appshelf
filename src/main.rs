use anyhow::{bail, Context, Result};
use appshelf::{
    discovery, install,
    manager::{self, Manager, Settings},
    runtime,
};
use serde_json::{json, Value};
use std::{
    env,
    io::{self, BufRead, Write},
    os::unix::{fs::symlink, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
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
fn emit(value: Value) {
    println!("{value}");
    let _ = io::stdout().flush();
}
fn serve(manager: Manager) -> Result<()> {
    let mut discovered = discovery::discover(&manager, &[]);
    emit(json!({"event":"ready","apps":manager.list(),"discovered":discovered}));
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
                    json!({"ok":true,"command":command,"result":result,"apps":manager.list(),"discovered":discovered}),
                );
            }
            Err(error) => emit(json!({"ok":false,"command":command,"error":format!("{error:#}")})),
        }
    }
    Ok(())
}
fn ensure_tray_running(binary: &Path) {
    let check = Command::new("pgrep")
        .args(["-f", "--", "appshelf --tray"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if check.map(|s| s.success()).unwrap_or(false) {
        return;
    }
    let service_file = appshelf::service::service_dir().join("appshelf-tray.service");
    if service_file.is_file() {
        let _ = Command::new("systemctl")
            .args(["--user", "start", "appshelf-tray.service"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    } else {
        let _ = Command::new(binary)
            .arg("--tray")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
}
fn main() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--help")
        || args.first().map(String::as_str) == Some("-h")
    {
        println!("AppShelf — keyboard-first AppImage management for Omarchy\n\nappshelf [AppImage-or-file-URL]\nappshelf --tray\nappshelf --enable-service\nappshelf --disable-service\nappshelf --service-status\nappshelf --check-update [ID]\nappshelf --update ID\nappshelf --backend\nappshelf --launch ID\nappshelf --scan\nappshelf --fetch-runtime\nappshelf --install [--integrate]\nappshelf --restore-association\nappshelf --version");
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
        return install::install(&resources, args.iter().any(|s| s == "--integrate"));
    }
    if args.first().map(String::as_str) == Some("--restore-association") {
        return install::restore_association();
    }
    let manager = Manager::new(
        manager::data_home(),
        resources.clone(),
        env::current_exe()?.canonicalize()?,
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
        Some("--tray") => {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;
            return rt.block_on(appshelf::tray::run_tray(std::sync::Arc::new(manager)));
        }
        Some("--enable-service") => {
            return appshelf::service::install_service(&manager.binary);
        }
        Some("--disable-service") => {
            return appshelf::service::uninstall_service();
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
        Some(s) if s.starts_with("--") => bail!("Unknown option {s}"),
        _ => {}
    }
    anyhow::ensure!(args.len() <= 1, "Open one AppImage at a time");
    let commons = resources.join("ui/Commons");
    if !commons.exists() {
        let shared = env::var_os("OMARCHY_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/usr/share/omarchy"))
            .join("shell/Commons");
        anyhow::ensure!(
            shared.is_dir(),
            "Omarchy Quickshell Commons components are required"
        );
        symlink(shared, commons)?;
    }
    let exe = env::current_exe()?;
    ensure_tray_running(&exe);
    let error = Command::new("quickshell")
        .args(["-p"])
        .arg(resources.join("ui"))
        .env("APPSHELF_BACKEND", env::current_exe()?)
        .env("APPSHELF_OPEN", args.first().cloned().unwrap_or_default())
        .exec();
    Err(error.into())
}
