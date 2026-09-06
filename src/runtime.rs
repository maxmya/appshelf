use anyhow::{bail, ensure, Context, Result};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

pub const VERSION: &str = "v0.6.1";
pub fn checksum() -> Result<&'static str> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("9763e3e6605efa970d98c2e51abd8385f991b5c2df7684858950f4372ff67982"),
        "aarch64" => Ok("39b0184ef33d77b694716b8b69e151a8a3492acec6f3ad1cd3ebc18a91792df2"),
        _ => bail!("uruntime supports x86_64 and aarch64 in this release"),
    }
}
pub fn sha256(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
pub fn runtime_path(resources: &Path) -> Result<PathBuf> {
    let path = resources
        .join("vendor")
        .join(format!("uruntime-{}", std::env::consts::ARCH));
    ensure!(
        path.is_file(),
        "Pinned uruntime is missing. Run appshelf --fetch-runtime."
    );
    ensure!(
        sha256(&path)? == checksum()?,
        "uruntime checksum mismatch; run appshelf --fetch-runtime again"
    );
    Ok(path)
}

/// Skip the ELF section table so filesystem signatures inside the runtime aren't mistaken for payloads.
pub fn filesystem(path: &Path) -> Result<(String, u64)> {
    let mut file = File::open(path)?;
    let mut header = [0u8; 64];
    file.read_exact(&mut header).context("Not an AppImage")?;
    ensure!(
        &header[..4] == b"\x7fELF" && &header[8..11] == b"AI\x02",
        "Choose a type-2 SquashFS or DwarFS AppImage"
    );
    ensure!(header[5] == 1, "Only little-endian AppImages are supported");
    let u16_at = |n| u16::from_le_bytes(header[n..n + 2].try_into().unwrap()) as u64;
    let end = match header[4] {
        2 => u64::from_le_bytes(header[40..48].try_into().unwrap())
            .checked_add(u16_at(58) * u16_at(60)),
        1 => (u32::from_le_bytes(header[32..36].try_into().unwrap()) as u64)
            .checked_add(u16_at(46) * u16_at(48)),
        _ => bail!("Invalid ELF class"),
    }
    .context("Invalid ELF section table")?
    .max(64);
    ensure!(
        end <= file.metadata()?.len(),
        "Truncated AppImage ELF section table"
    );
    file.seek(SeekFrom::Start(end))?;
    let mut bytes = Vec::new();
    file.take(1024 * 1024).read_to_end(&mut bytes)?;
    for (i, window) in bytes.windows(32).enumerate() {
        if &window[..4] == b"hsqs" && u16::from_le_bytes(window[28..30].try_into().unwrap()) == 4 {
            return Ok(("SquashFS".into(), end + i as u64));
        }
        if &window[..6] == b"DWARFS" {
            return Ok(("DwarFS".into(), end + i as u64));
        }
    }
    bail!("No supported SquashFS or DwarFS filesystem found")
}

pub fn bounded(mut command: Command, limit: usize) -> Result<Vec<u8>> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()?;
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = tx.send(result);
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    let received = rx.recv_timeout(Duration::from_secs(15));
    let output = match received {
        Ok(Ok(bytes)) if bytes.len() <= limit => bytes,
        _ => {
            kill_group(child.id());
            let _ = child.wait();
            bail!("Metadata exceeds the inspection time or size limit");
        }
    };
    loop {
        if let Some(status) = child.try_wait()? {
            ensure!(
                status.success(),
                "Could not read optional AppImage metadata"
            );
            return Ok(output);
        }
        if Instant::now() >= deadline {
            kill_group(child.id());
            let _ = child.wait();
            bail!("Metadata inspection timed out");
        }
        thread::sleep(Duration::from_millis(10));
    }
}
fn kill_group(pid: u32) {
    // SAFETY: child was started in a new process group with its pid as group id.
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
}
pub fn tool(runtime: &Path, name: &str, args: &[String], limit: usize) -> Result<Vec<u8>> {
    let mut command = Command::new(runtime);
    command.arg(format!("--appimage-{name}")).args(args);
    bounded(command, limit)
}

/// Reads one regular member from uruntime's ustar stream. Never extracts filesystem paths.
fn tar_member(bytes: &[u8], wanted: &str, limit: usize) -> Result<Vec<u8>> {
    let mut offset = 0;
    while offset + 512 <= bytes.len() {
        let header = &bytes[offset..offset + 512];
        if header.iter().all(|b| *b == 0) {
            break;
        }
        let name = String::from_utf8_lossy(&header[..100])
            .trim_end_matches('\0')
            .to_string();
        let prefix = String::from_utf8_lossy(&header[345..500])
            .trim_end_matches('\0')
            .to_string();
        let name = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let size = usize::from_str_radix(
            std::str::from_utf8(&header[124..136])?.trim_matches(['\0', ' ']),
            8,
        )?;
        let start = offset + 512;
        ensure!(
            size <= bytes.len().saturating_sub(start),
            "Truncated metadata archive"
        );
        if name.trim_start_matches("./").trim_start_matches('/') == wanted
            && (header[156] == 0 || header[156] == b'0')
        {
            ensure!(size <= limit, "Metadata member too large");
            return Ok(bytes[start..start + size].to_vec());
        }
        offset = start + size.div_ceil(512) * 512;
    }
    bail!("Metadata member not found")
}
pub fn desktop_value(body: &str, key: &str) -> Option<String> {
    let mut active = false;
    for line in body.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            active = line == "[Desktop Entry]";
        }
        if active {
            if let Some((k, v)) = line.split_once('=') {
                if k.trim() == key {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}
pub fn metadata(runtime: &Path, path: &Path) -> Result<(String, Option<Vec<u8>>)> {
    let (kind, offset) = filesystem(path)?;
    let p = path.to_string_lossy().to_string();
    let o = offset.to_string();
    let names: Vec<String> = if kind == "SquashFS" {
        String::from_utf8_lossy(&tool(
            runtime,
            "unsquashfs",
            &["-o".into(), o.clone(), "-ls".into(), p.clone()],
            1024 * 1024,
        )?)
        .lines()
        .filter_map(|s| s.strip_prefix("squashfs-root/").map(str::to_owned))
        .collect()
    } else {
        String::from_utf8_lossy(&tool(
            runtime,
            "dwarfsck",
            &["-i".into(), p.clone(), "-O".into(), o.clone(), "-l".into()],
            1024 * 1024,
        )?)
        .lines()
        .map(|s| {
            s.trim_start_matches("./")
                .trim_start_matches('/')
                .to_string()
        })
        .collect()
    };
    let read = |name: &str, limit| -> Result<Vec<u8>> {
        if kind == "SquashFS" {
            tool(
                runtime,
                "unsquashfs",
                &[
                    "-o".into(),
                    o.clone(),
                    "-cat".into(),
                    p.clone(),
                    name.into(),
                ],
                limit,
            )
        } else {
            let tar = tool(
                runtime,
                "dwarfsextract",
                &[
                    "-i".into(),
                    p.clone(),
                    "-O".into(),
                    o.clone(),
                    "--pattern".into(),
                    name.into(),
                    "--skip-devices".into(),
                    "--skip-specials".into(),
                    "-f".into(),
                    "ustar".into(),
                    "-o".into(),
                    "-".into(),
                ],
                limit + 16384,
            )?;
            tar_member(&tar, name, limit)
        }
    };
    let desktop = names
        .iter()
        .find(|s| !s.contains('/') && s.ends_with(".desktop"))
        .context("No desktop metadata")?;
    let body = String::from_utf8(read(desktop, 65536)?)?;
    let name = desktop_value(&body, "Name")
        .context("No app name")?
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(150)
        .collect();
    let icon_name = desktop_value(&body, "Icon").unwrap_or_default();
    let stem = Path::new(&icon_name).file_stem();
    let icon = names
        .iter()
        .find(|s| s.ends_with(".png") && Path::new(s).file_stem() == stem)
        .and_then(|s| read(s, 2 * 1024 * 1024).ok())
        .filter(|b| b.starts_with(b"\x89PNG\r\n\x1a\n"));
    Ok((name, icon))
}
pub fn fetch(resources: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(resources.join("vendor"))?;
    let target = resources
        .join("vendor")
        .join(format!("uruntime-{}", std::env::consts::ARCH));
    let temp = tempfile::NamedTempFile::new_in(target.parent().unwrap())?;
    let url = format!(
        "https://github.com/VHSgunzo/uruntime/releases/download/{VERSION}/uruntime-appimage-{}",
        std::env::consts::ARCH
    );
    ensure!(
        Command::new("curl")
            .args([
                "--fail",
                "--location",
                "--proto",
                "=https",
                "--tlsv1.2",
                "--max-time",
                "120",
                "--output"
            ])
            .arg(temp.path())
            .arg(url)
            .status()?
            .success(),
        "Runtime download failed"
    );
    ensure!(
        sha256(temp.path())? == checksum()?,
        "Downloaded runtime checksum mismatch"
    );
    temp.as_file()
        .set_permissions(fs::Permissions::from_mode(0o755))?;
    temp.persist(target)?;
    println!("Verified uruntime {VERSION}");
    Ok(())
}
