use crate::{
    manager::{home, modified, Manager},
    runtime,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

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
        let Ok((format, _)) = runtime::filesystem(&path) else {
            continue;
        };
        let id = format!(
            "external-{:x}",
            Sha256::digest(path.as_os_str().as_encoded_bytes())
        );
        let name = if name.is_empty() {
            path.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        } else {
            name
        };
        found.push(json!({"id":id,"name":name,"path":path,"source":path,"desktop":desktop,"size":fs::metadata(&path).map(|m|m.len()).unwrap_or(0),"format":format,"icon":"","unmanaged":true,"missing":false,"environment":{},"isolation":"off","installed":0}));
    }
    found.sort_by_key(|a| a["name"].as_str().unwrap_or_default().to_lowercase());
    found
}
