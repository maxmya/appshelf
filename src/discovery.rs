use crate::{
    config,
    manager::{atomic_json, data_home, home, local_path, modified, Manager},
    package, runtime, selfupdate,
};
use anyhow::Result;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

/// Paths the user has told the shelf to stop offering. Kept as a plain list of
/// paths rather than content hashes: what is being dismissed is "this file in
/// this folder", and a rebuilt download at the same path is the same offer.
pub fn ignored_path() -> PathBuf {
    data_home().join("appshelf/ignored.json")
}
pub fn ignored() -> BTreeSet<String> {
    fs::read(ignored_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice::<BTreeSet<String>>(&bytes).ok())
        .unwrap_or_default()
}
/// Add or remove one path. A path that no longer exists is dropped on the way
/// past: an ignored download that was deleted has nothing left to ignore.
pub fn set_ignored(value: &str, ignore: bool) -> Result<()> {
    let mut list: BTreeSet<String> = ignored()
        .into_iter()
        .filter(|entry| Path::new(entry).is_file())
        .collect();
    if ignore {
        // Resolved the same way discovery resolves it, or the entry would
        // never match the row it was meant to dismiss.
        let path = local_path(value)?;
        anyhow::ensure!(path.is_file(), "That file no longer exists");
        anyhow::ensure!(
            list.len() < 1000,
            "Too many ignored files; clear some before ignoring more"
        );
        list.insert(path.to_string_lossy().into_owned());
    } else {
        // An entry whose file has gone is already dropped above; one that is
        // still there is removed by the path the window has for it.
        let path = local_path(value)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| value.to_string());
        list.remove(&path);
    }
    let target = ignored_path();
    fs::create_dir_all(target.parent().unwrap())?;
    atomic_json(&target, &list)
}

/// `~/Downloads` and `~/Desktop`, honouring `user-dirs.dirs` so localised or
/// relocated folders are scanned too.
fn user_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![home().join("Downloads"), home().join("Desktop")];
    let config = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"));
    if let Ok(body) = fs::read_to_string(config.join("user-dirs.dirs")) {
        for line in body.lines() {
            let line = line.trim();
            let Some(value) = line
                .strip_prefix("XDG_DOWNLOAD_DIR=")
                .or_else(|| line.strip_prefix("XDG_DESKTOP_DIR="))
            else {
                continue;
            };
            let value = value.trim().trim_matches('"');
            let path = match value.strip_prefix("$HOME/") {
                Some(rest) => home().join(rest),
                None => PathBuf::from(value),
            };
            if path.is_absolute() && !dirs.contains(&path) {
                dirs.push(path);
            }
        }
    }
    dirs
}

/// Whether the user has asked the shelf to look through their filesystem.
/// Callers ask this rather than `discover` asking it for them: what the shelf
/// finds should depend on what is on the disk and nothing else, so the
/// preference belongs at the call site where it can be seen.
pub fn enabled() -> bool {
    config::load().scan
}

/// Bounded, read-only discovery. No shell expansion and no candidate execution.
pub fn discover(manager: &Manager, extra: &[PathBuf]) -> Vec<Value> {
    let mut candidates: BTreeMap<PathBuf, (String, String)> = BTreeMap::new();
    let mut roots = vec![
        home().join("Applications"),
        home().join("AppImages"),
        home().join(".local/bin"),
        home().join(".local/opt"),
        manager
            .root
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("appimages"),
        PathBuf::from("/opt"),
    ];
    // Where AppImages actually land: a browser download, or a file dropped on
    // the desktop. Leaving these out meant a machine full of AppImages still
    // reported nothing found.
    roots.extend(user_dirs());
    roots.extend_from_slice(extra);
    for root in roots {
        for entry in WalkDir::new(root)
            .follow_links(false)
            .max_depth(4)
            .into_iter()
            .filter_map(Result::ok)
            .take(20000)
        {
            if (entry.file_type().is_file() || entry.file_type().is_symlink())
                && !entry.path().starts_with(&manager.root)
            {
                if let Ok(path) = entry.path().canonicalize() {
                    candidates.entry(path).or_default();
                }
            }
        }
    }
    let mut desktop_dirs = vec![manager.launchers.clone()];
    desktop_dirs.extend(
        env::split_paths(
            &env::var_os("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into()),
        )
        .map(|p| p.join("applications")),
    );
    for directory in desktop_dirs {
        for entry in WalkDir::new(directory)
            .max_depth(2)
            .into_iter()
            .filter_map(Result::ok)
            .take(10000)
        {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("desktop")
                || entry
                    .metadata()
                    .map(|m| m.len() > 1024 * 1024)
                    .unwrap_or(true)
            {
                continue;
            }
            let Ok(body) = fs::read_to_string(path) else {
                continue;
            };
            if runtime::desktop_value(&body, "X-AppShelf-Managed").as_deref() == Some("true") {
                continue;
            }
            let name = runtime::desktop_value(&body, "Name").unwrap_or_default();
            // Quoted absolute file arguments cover AppImageLauncher/AppManager launchers and wrappers.
            for key in ["Exec", "TryExec"] {
                if let Some(exec) = runtime::desktop_value(&body, key) {
                    if let Some(words) = shlex::split(&exec.replace("\\\\", "\\")) {
                        for word in words {
                            let source = Path::new(&word);
                            if source.is_absolute() && source.is_file() {
                                if let Ok(source) = source.canonicalize() {
                                    candidates.insert(
                                        source,
                                        (name.clone(), path.to_string_lossy().into_owned()),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    let managed = manager.list();
    let dismissed = ignored();
    // pacman is asked once, and only if a package file actually turns up.
    let mut present: Option<BTreeMap<String, String>> = None;
    let mut found = Vec::new();
    for (path, (name, desktop)) in candidates {
        if path.starts_with(&manager.root)
            || managed.iter().any(|a| {
                a["source"].as_str() == path.to_str()
                    && a["source_modified"].as_u64() == Some(modified(&path))
            })
        {
            continue;
        }
        let filename = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        // Every format the shelf can install is worth offering, not only the
        // ones it keeps a copy of: a `.deb` sitting in Downloads is as
        // installable here as an AppImage beside it.
        let mut declared_name = String::new();
        let (kind, format, package_kind, version) = match runtime::filesystem(&path) {
            Ok((format, _)) => {
                // AppShelf is not one of the applications AppShelf manages:
                // installing its own AppImage would leave a second copy and a
                // second launcher. It reports its own version and updates from
                // Settings instead.
                if selfupdate::is_appshelf(&path) {
                    continue;
                }
                (
                    "appimage",
                    format,
                    String::new(),
                    runtime::version_from_filename(&filename),
                )
            }
            Err(_) => {
                let Some(package) = package::detect(&path) else {
                    continue;
                };
                // A package pacman already has at this version is installed,
                // whoever installed it, and offering it again would be an
                // offer to reinstall.
                let info = package::inspect(&path, package).ok();
                if let Some(info) = &info {
                    let installed = present.get_or_insert_with(package::installed);
                    if installed.get(&info.name) == Some(&info.version) {
                        continue;
                    }
                }
                // `claude-desktop-1.40609.0-1-x86_64.pkg.tar` is a filename;
                // `claude-desktop` is what the package calls itself, and what
                // it will be called once pacman has it.
                if let Some(info) = &info {
                    declared_name = info.name.clone();
                }
                (
                    "package",
                    package.label().to_string(),
                    package.id().to_string(),
                    info.as_ref()
                        .map(|i| i.original_version.clone())
                        .unwrap_or_else(|| runtime::version_from_filename(&filename)),
                )
            }
        };
        let id = format!(
            "external-{:x}",
            Sha256::digest(path.as_os_str().as_encoded_bytes())
        );
        let name = if !name.is_empty() {
            name
        } else if !declared_name.is_empty() {
            declared_name
        } else {
            path.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        };
        let ignored = dismissed.contains(path.to_string_lossy().as_ref());
        found.push(json!({"id":id,"kind":kind,"package_kind":package_kind,"name":name,"version":version,"path":path,"source":path,"desktop":desktop,"size":fs::metadata(&path).map(|m|m.len()).unwrap_or(0),"format":format,"icon":"","unmanaged":true,"ignored":ignored,"missing":false,"environment":{},"isolation":"off","installed":0}));
    }
    found.sort_by_key(|a| a["name"].as_str().unwrap_or_default().to_lowercase());
    found
}
