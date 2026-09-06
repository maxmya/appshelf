use crate::{manager::Manager, runtime};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    env, fs,
    io::{BufRead, BufReader},
    os::unix::fs::PermissionsExt,
    path::Path,
    process::Command,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdateCheckResult {
    pub supported: bool,
    pub has_update: bool,
    pub update_info: Option<String>,
    pub current_version: Option<String>,
    pub latest_version: Option<String>,
    pub download_url: Option<String>,
    pub zsync_url: Option<String>,
    pub download_size: Option<u64>,
    pub message: String,
}

fn glob_match(pattern: &str, text: &str) -> bool {
    let p_chars: Vec<char> = pattern.chars().collect();
    let t_chars: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star_pi, mut star_ti) = (None, 0);

    while ti < t_chars.len() {
        if pi < p_chars.len() && (p_chars[pi] == t_chars[ti] || p_chars[pi] == '?') {
            pi += 1;
            ti += 1;
        } else if pi < p_chars.len() && p_chars[pi] == '*' {
            star_pi = Some(pi + 1);
            pi += 1;
            star_ti = ti;
        } else if let Some(sp) = star_pi {
            pi = sp;
            star_ti += 1;
            ti = star_ti;
        } else {
            return false;
        }
    }
    while pi < p_chars.len() && p_chars[pi] == '*' {
        pi += 1;
    }
    pi == p_chars.len()
}

fn curl_get(url: &str, headers: &[&str], range: Option<&str>) -> Result<Vec<u8>> {
    let mut cmd = Command::new("curl");
    cmd.args([
        "--fail",
        "--location",
        "--proto",
        "=https",
        "--tlsv1.2",
        "--max-time",
        "20",
        "--silent",
        "--show-error",
    ]);
    for h in headers {
        cmd.args(["-H", h]);
    }
    if let Some(r) = range {
        cmd.args(["-r", r]);
    }
    cmd.arg(url);

    let output = cmd.output().context("curl failed to execute")?;
    ensure!(
        output.status.success(),
        "Download failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(output.stdout)
}

#[derive(Default, Debug)]
struct ZsyncHeader {
    filename: Option<String>,
    url: Option<String>,
    sha1: Option<String>,
    length: Option<u64>,
}

fn parse_zsync_header(data: &[u8]) -> ZsyncHeader {
    let mut header = ZsyncHeader::default();
    let reader = BufReader::new(data);
    for line in reader.lines().map_while(Result::ok) {
        if line.is_empty() || line.starts_with('\0') {
            break;
        }
        if let Some((key, val)) = line.split_once(':') {
            let key = key.trim().to_lowercase();
            let val = val.trim().to_string();
            match key.as_str() {
                "filename" => header.filename = Some(val),
                "url" => header.url = Some(val),
                "sha-1" => header.sha1 = Some(val.to_lowercase()),
                "length" => header.length = val.parse().ok(),
                _ => {}
            }
        }
    }
    header
}

pub fn check_update(app_path: &Path) -> Result<UpdateCheckResult> {
    let update_info = runtime::update_info(app_path)?;
    let info = match update_info {
        Some(info) if !info.trim().is_empty() => info.trim().to_string(),
        _ => {
            return Ok(UpdateCheckResult {
                supported: false,
                has_update: false,
                update_info: None,
                current_version: None,
                latest_version: None,
                download_url: None,
                zsync_url: None,
                download_size: None,
                message: "No embedded update information (.upd_info)".into(),
            })
        }
    };

    let parts: Vec<&str> = info.split('|').collect();
    match parts[0] {
        "gh-releases-zsync" if parts.len() >= 5 => {
            let owner = parts[1].trim();
            let repo = parts[2].trim();
            let tag = parts[3].trim();
            let pattern = parts[4].trim();

            let api_url = if tag == "latest" {
                format!("https://api.github.com/repos/{owner}/{repo}/releases/latest")
            } else {
                format!("https://api.github.com/repos/{owner}/{repo}/releases/tags/{tag}")
            };

            let mut headers = vec![
                "User-Agent: AppShelf",
                "Accept: application/vnd.github+json",
            ];
            let token_header;
            if let Ok(token) = env::var("GITHUB_TOKEN").or_else(|_| env::var("GH_TOKEN")) {
                if !token.is_empty() {
                    token_header = format!("Authorization: Bearer {token}");
                    headers.push(&token_header);
                }
            }

            let body = curl_get(&api_url, &headers, None)?;
            let release: Value =
                serde_json::from_slice(&body).context("Failed to parse GitHub release JSON")?;

            let tag_name = release["tag_name"].as_str().unwrap_or("latest").to_string();
            let assets = release["assets"]
                .as_array()
                .context("Missing assets in GitHub release")?;

            let mut zsync_url = None;
            let mut appimage_url = None;
            let mut appimage_size = None;

            for asset in assets {
                let name = asset["name"].as_str().unwrap_or_default();
                let download = asset["browser_download_url"].as_str().unwrap_or_default();
                let size = asset["size"].as_u64();

                if glob_match(pattern, name) {
                    // A pattern is conventionally the .zsync file, but some
                    // images point it straight at the AppImage. Classify by
                    // what the asset actually is, or the download never
                    // resolves and updating fails with no URL.
                    if name.ends_with(".zsync") {
                        zsync_url = Some(download.to_string());
                    } else {
                        appimage_url = Some(download.to_string());
                        appimage_size = size;
                    }
                } else if (name.ends_with(".AppImage") || name.ends_with(".appimage"))
                    && (name.contains("x86_64")
                        || name.contains("amd64")
                        || !name.contains("aarch64"))
                {
                    appimage_url = Some(download.to_string());
                    appimage_size = size;
                }
            }

            if let Some(zurl) = &zsync_url {
                if let Ok(zbytes) = curl_get(zurl, &["User-Agent: AppShelf"], Some("0-2047")) {
                    let zhdr = parse_zsync_header(&zbytes);

                    if let Some(remote_sha1) = &zhdr.sha1 {
                        if let Ok(local_sha1) = runtime::sha1(app_path) {
                            if local_sha1.eq_ignore_ascii_case(remote_sha1) {
                                return Ok(UpdateCheckResult {
                                    supported: true,
                                    has_update: false,
                                    update_info: Some(info),
                                    current_version: None,
                                    latest_version: Some(tag_name),
                                    download_url: appimage_url,
                                    zsync_url,
                                    download_size: zhdr.length.or(appimage_size),
                                    message: "AppImage is already up to date".into(),
                                });
                            }
                        }
                    }

                    if appimage_url.is_none() {
                        if let Some(fname) = &zhdr.filename {
                            for asset in assets {
                                if asset["name"].as_str() == Some(fname.as_str()) {
                                    appimage_url =
                                        asset["browser_download_url"].as_str().map(String::from);
                                    appimage_size = asset["size"].as_u64();
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            Ok(UpdateCheckResult {
                supported: true,
                has_update: true,
                update_info: Some(info),
                current_version: None,
                latest_version: Some(tag_name.clone()),
                download_url: appimage_url,
                zsync_url,
                download_size: appimage_size,
                message: format!("Update available: {tag_name}"),
            })
        }
        "zsync" if parts.len() >= 2 => {
            let zsync_url = parts[1].trim().to_string();
            let zbytes = curl_get(&zsync_url, &["User-Agent: AppShelf"], Some("0-2047"))?;
            let zhdr = parse_zsync_header(&zbytes);

            let has_update = if let Some(remote_sha1) = &zhdr.sha1 {
                let local_sha1 = runtime::sha1(app_path)?;
                !local_sha1.eq_ignore_ascii_case(remote_sha1)
            } else {
                true
            };

            let appimage_url = if let Some(rel_url) = &zhdr.url {
                if rel_url.starts_with("http://") || rel_url.starts_with("https://") {
                    Some(rel_url.clone())
                } else if let Ok(base) = url::Url::parse(&zsync_url) {
                    base.join(rel_url).ok().map(|u| u.to_string())
                } else {
                    None
                }
            } else {
                None
            };

            Ok(UpdateCheckResult {
                supported: true,
                has_update,
                update_info: Some(info),
                current_version: None,
                latest_version: zhdr.filename,
                download_url: appimage_url,
                zsync_url: Some(zsync_url),
                download_size: zhdr.length,
                message: if has_update {
                    "Update available".into()
                } else {
                    "AppImage is already up to date".into()
                },
            })
        }
        _ => Ok(UpdateCheckResult {
            supported: false,
            has_update: false,
            update_info: Some(info),
            current_version: None,
            latest_version: None,
            download_url: None,
            zsync_url: None,
            download_size: None,
            message: "Unsupported update protocol".into(),
        }),
    }
}

pub fn apply_update(manager: &Manager, id: &str) -> Result<Value> {
    let folder = manager.folder(id)?;
    let target = folder.join("app.AppImage");
    ensure!(target.is_file(), "Installed AppImage file not found");

    let check = check_update(&target)?;
    ensure!(
        check.supported,
        "This application does not embed update capabilities"
    );
    ensure!(
        check.has_update,
        "Application is already up to date ({})",
        check.message
    );

    let download_url = check
        .download_url
        .context("No direct download URL resolved for this update")?;

    let stage = tempfile::Builder::new()
        .prefix(".update-")
        .tempdir_in(&manager.root)?;
    let new_target = stage.path().join("app.AppImage");

    let mut cmd = Command::new("curl");
    cmd.args([
        "--fail",
        "--location",
        "--proto",
        "=https",
        "--tlsv1.2",
        "--max-time",
        "600",
        "--output",
    ]);
    cmd.arg(&new_target);
    cmd.arg(&download_url);

    let status = cmd.status().context("curl failed to execute download")?;
    ensure!(status.success(), "Update download failed");

    let (format, _) = runtime::filesystem(&new_target)?;
    fs::set_permissions(&new_target, fs::Permissions::from_mode(0o755))?;

    let new_id = runtime::sha256(&new_target)?;
    let runtime = runtime::runtime_path(&manager.resources)?;

    let old_record = manager.record(id)?;
    let settings = old_record.settings.clone();
    let name = old_record.name.clone();

    if new_id == id {
        fs::copy(&new_target, &target)?;
        return Ok(
            json!({"ok": true, "id": id, "name": name, "updated": false, "version": check.latest_version}),
        );
    }

    let new_folder = manager.folder(&new_id)?;
    ensure!(
        !new_folder.exists(),
        "An installation with the updated version hash already exists"
    );

    let (_, icon) =
        runtime::metadata(&runtime, &new_target).unwrap_or_else(|_| (name.clone(), None));

    let new_record = crate::manager::Record {
        name: name.clone(),
        size: new_target.metadata()?.len(),
        installed: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs(),
        format,
        settings,
        source: old_record.source,
        source_modified: old_record.source_modified,
    };

    crate::manager::atomic_json(&stage.path().join("record.json"), &new_record)?;
    if let Some(bytes) = &icon {
        fs::write(stage.path().join("icon.png"), bytes)?;
    } else if folder.join("icon.png").is_file() {
        let _ = fs::copy(folder.join("icon.png"), stage.path().join("icon.png"));
    }

    let old_data = manager.root.parent().unwrap().join("data").join(id);
    let new_data = manager.root.parent().unwrap().join("data").join(&new_id);
    if old_data.exists() && !new_data.exists() {
        let _ = fs::rename(&old_data, &new_data);
    }

    fs::rename(stage.path(), &new_folder)?;

    let _ = manager.write_launcher(
        &new_id,
        &name,
        icon.is_some() || new_folder.join("icon.png").is_file(),
    );

    let _ = fs::remove_dir_all(&folder);
    if let Ok(old_launcher) = manager.desktop_path(id) {
        if old_launcher.exists() {
            let _ = fs::remove_file(old_launcher);
        }
    }

    manager.refresh_database();

    Ok(json!({
        "ok": true,
        "id": new_id,
        "old_id": id,
        "name": name,
        "version": check.latest_version
    }))
}
