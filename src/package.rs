//! System packages: Arch `.pkg.tar.*`, Debian `.deb` and RPM `.rpm`.
//!
//! AppImages live on AppShelf's own shelf, as self-contained copies it can
//! launch itself. A system package cannot work that way: it is a tree of files
//! with absolute paths that belongs to the distribution's package manager. So
//! AppShelf does the four things it can honestly do for one — read what the
//! file contains, convert the foreign formats into something pacman accepts,
//! hand the transaction to pacman in a terminal the user can watch, and
//! remember afterwards which packages it was that put them there.
//!
//! Conversion is done with `bsdtar`, which pacman already depends on through
//! libarchive, so nothing has to be installed from the AUR. libarchive reads
//! `ar`, `cpio` and every payload compression these formats use, and its
//! `@archive` source copies entries from one archive straight into another —
//! so the payload is never unpacked onto the filesystem, and modes the current
//! user could not reproduce (setuid bits, root ownership) survive the round
//! trip untouched.

use crate::manager::{data_home, home, local_path, which};
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    env,
    ffi::OsStr,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// How long a pacman transaction may stay open in its terminal before AppShelf
/// stops waiting for it. Long enough for a big download over a slow link, short
/// enough that a terminal the user walked away from does not wedge the backend
/// for the rest of the session.
const TRANSACTION_TIMEOUT: Duration = Duration::from_secs(45 * 60);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Arch,
    Deb,
    Rpm,
}
impl Kind {
    pub fn id(self) -> &'static str {
        match self {
            Kind::Arch => "arch",
            Kind::Deb => "deb",
            Kind::Rpm => "rpm",
        }
    }
    /// What the shelf calls this format.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Arch => "Arch package",
            Kind::Deb => "Debian package",
            Kind::Rpm => "RPM package",
        }
    }
    /// The label with the article an English sentence needs in front of it.
    pub fn with_article(self) -> &'static str {
        match self {
            Kind::Arch => "an Arch package",
            Kind::Deb => "a Debian package",
            Kind::Rpm => "an RPM package",
        }
    }
}

/// Everything the installer window shows about a package file, and everything
/// the converter needs to write a `.PKGINFO`.
#[derive(Clone, Debug)]
pub struct Info {
    pub kind: Kind,
    pub name: String,
    /// `pkgver-pkgrel`, already normalised to something pacman will accept.
    pub version: String,
    /// What the file itself called that version, before normalisation.
    pub original_version: String,
    pub arch: String,
    pub original_arch: String,
    pub description: String,
    pub url: String,
    pub license: String,
    pub installed_size: u64,
    /// Dependencies as the source distribution declared them. They are not
    /// translated to Arch package names — see `convert`.
    pub depends: Vec<String>,
}

/// The identifier the window and the registry use for an installed package.
/// Deliberately unlike a managed AppImage's 64-character content hash, so the
/// two can never be confused for one another.
pub fn app_id(name: &str) -> String {
    format!("pkg:{name}")
}
pub fn id_name(id: &str) -> Option<&str> {
    id.strip_prefix("pkg:").filter(|n| valid_name(n))
}

/// pacman's own rule for a package name, applied before any of it reaches a
/// command line.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with(['-', '.'])
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"@._+-".contains(&c))
}

/// Recognise a package by content rather than by name: a download that arrived
/// as `foo.deb.1` or with no extension at all is still the package it is.
pub fn detect(path: &Path) -> Option<Kind> {
    let mut head = [0u8; 32];
    let read = {
        let mut file = fs::File::open(path).ok()?;
        let mut filled = 0;
        loop {
            match file.read(&mut head[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(_) => return None,
            }
            if filled == head.len() {
                break;
            }
        }
        filled
    };
    let head = &head[..read];
    if head.starts_with(b"\xed\xab\xee\xdb") {
        return Some(Kind::Rpm);
    }
    // An `ar` archive whose first member is `debian-binary`; a static library
    // is the same container and must not be mistaken for one.
    if head.starts_with(b"!<arch>\n") && head.len() >= 21 && &head[8..21] == b"debian-binary" {
        return Some(Kind::Deb);
    }
    // An Arch package is a bare compressed tarball with no magic of its own,
    // so the name carries the claim and `inspect` proves it by reading the
    // `.PKGINFO` the format requires.
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    let compressed = head.starts_with(b"\x28\xb5\x2f\xfd")       // zstd
        || head.starts_with(b"\xfd7zXZ\x00")                     // xz
        || head.starts_with(b"\x1f\x8b")                         // gzip
        || head.starts_with(b"BZh")                              // bzip2
        || (head.len() > 262 && &head[257..262] == b"ustar"); // uncompressed tar
    if compressed && (name.contains(".pkg.tar") || name.ends_with(".pkg")) {
        return Some(Kind::Arch);
    }
    None
}

/// `key = value` as `.PKGINFO` and `key: value` as Debian control both parse
/// into the same shape; only the separator and the continuation rule differ.
fn bsdtar_capture(mut command: Command, limit: usize) -> Result<Vec<u8>> {
    let output = command
        .stderr(Stdio::piped())
        .output()
        .context("bsdtar could not be run; install libarchive")?;
    ensure!(
        output.status.success(),
        "Could not read the package: {}",
        String::from_utf8_lossy(&output.stderr)
            .lines()
            .next()
            .unwrap_or("bsdtar failed")
            .trim()
    );
    ensure!(
        output.stdout.len() <= limit,
        "Package metadata is too large"
    );
    Ok(output.stdout)
}

fn bsdtar() -> Command {
    Command::new("bsdtar")
}

fn pkginfo_fields(body: &str) -> BTreeMap<String, Vec<String>> {
    let mut fields: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for line in body.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            fields
                .entry(key.trim().to_lowercase())
                .or_default()
                .push(value.trim().to_string());
        }
    }
    fields
}

/// Debian control paragraphs fold continuation lines under the field they
/// belong to; only the first line of `Description` is a summary.
fn control_fields(body: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    let mut current = String::new();
    for line in body.lines() {
        if line.starts_with([' ', '\t']) {
            if let Some(value) = fields.get_mut(&current) {
                let value: &mut String = value;
                value.push('\n');
                value.push_str(line.trim());
            }
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            current = key.trim().to_lowercase();
            fields.insert(current.clone(), value.trim().to_string());
        }
    }
    fields
}

/// pacman accepts alphanumerics, `.`, `_` and `+` in a version, one `-` as the
/// release separator and one leading `epoch:`. Debian and RPM both use more
/// than that — `~` for pre-releases, extra hyphens inside an upstream version —
/// so anything else folds to `_` rather than being rejected. The version the
/// file actually claimed is kept alongside in `original_version`, so nothing is
/// silently rewritten out of sight.
pub fn arch_version(raw: &str) -> String {
    let raw = raw.trim();
    let (epoch, rest) = match raw.split_once(':') {
        Some((epoch, rest)) if !epoch.is_empty() && epoch.bytes().all(|c| c.is_ascii_digit()) => {
            (Some(epoch), rest)
        }
        _ => (None, raw),
    };
    let (version, release) = match rest.rsplit_once('-') {
        Some((version, release)) if !version.is_empty() && !release.is_empty() => {
            (version, release)
        }
        _ => (rest, "1"),
    };
    let clean = |s: &str| -> String {
        let out: String = s
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '+' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        out.trim_matches('_').to_string()
    };
    let version = clean(version);
    let release = clean(release);
    let version = if version.is_empty() {
        "0".to_string()
    } else {
        version
    };
    let release = if release.is_empty() {
        "1".to_string()
    } else {
        release
    };
    match epoch {
        Some(epoch) => format!("{epoch}:{version}-{release}"),
        None => format!("{version}-{release}"),
    }
}

/// Foreign architecture names, mapped onto the ones pacman knows. Anything
/// unrecognised is kept verbatim so `inspect` can say it does not match this
/// machine rather than quietly claiming it does.
fn arch_name(raw: &str) -> String {
    match raw.trim() {
        "amd64" | "x86_64" => "x86_64".to_string(),
        "arm64" | "aarch64" => "aarch64".to_string(),
        "i386" | "i486" | "i586" | "i686" => "i686".to_string(),
        "armhf" | "armv7hl" | "armv7h" => "armv7h".to_string(),
        "all" | "any" | "noarch" => "any".to_string(),
        // Kept rather than rejected, so `inspect` can say this package is for
        // another machine instead of quietly claiming it is for this one. But a
        // control file is text the package author chose and this ends up in the
        // converted package's file name, so it is reduced to a plain token
        // first: nothing here may look like a path.
        other => {
            let cleaned: String = other
                .chars()
                .take(32)
                .map(|c| {
                    if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-') {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
            let cleaned = cleaned.trim_matches(['_', '-', '.']).to_string();
            if cleaned.is_empty() {
                "unknown".to_string()
            } else {
                cleaned
            }
        }
    }
}

pub fn host_arch() -> &'static str {
    env::consts::ARCH
}

fn one_line(text: &str, limit: usize) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(limit)
        .collect()
}

fn inspect_arch(path: &Path) -> Result<Info> {
    let body = String::from_utf8_lossy(&bsdtar_capture(
        {
            let mut c = bsdtar();
            c.arg("-xOf").arg(path).arg(".PKGINFO");
            c
        },
        256 * 1024,
    )?)
    .into_owned();
    let fields = pkginfo_fields(&body);
    let first = |key: &str| fields.get(key).and_then(|v| v.first()).cloned();
    let name = first("pkgname").context("Package has no .PKGINFO name")?;
    ensure!(valid_name(&name), "Package name is not a valid pacman name");
    let version = first("pkgver").unwrap_or_else(|| "0-1".into());
    Ok(Info {
        kind: Kind::Arch,
        name,
        version: version.clone(),
        original_version: version,
        arch: first("arch").unwrap_or_else(|| "any".into()),
        original_arch: first("arch").unwrap_or_else(|| "any".into()),
        description: first("pkgdesc").unwrap_or_default(),
        url: first("url").unwrap_or_default(),
        license: fields
            .get("license")
            .cloned()
            .unwrap_or_default()
            .join(", "),
        installed_size: first("size").and_then(|s| s.parse().ok()).unwrap_or(0),
        depends: fields.get("depend").cloned().unwrap_or_default(),
    })
}

fn inspect_deb(path: &Path) -> Result<Info> {
    let mut outer = bsdtar()
        .arg("-xOf")
        .arg(path)
        .arg("control.tar*")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("bsdtar could not be run; install libarchive")?;
    let stream = outer.stdout.take().context("bsdtar produced no output")?;
    let body = {
        let mut inner = bsdtar();
        inner.arg("-xOf").arg("-").arg("control").stdin(stream);
        let bytes = bsdtar_capture(inner, 1024 * 1024);
        let _ = outer.wait();
        String::from_utf8_lossy(&bytes.context("This .deb has no control file")?).into_owned()
    };
    let fields = control_fields(&body);
    let get = |key: &str| fields.get(key).cloned().unwrap_or_default();
    let name = get("package");
    ensure!(!name.is_empty(), "This .deb declares no package name");
    ensure!(
        valid_name(&name),
        "Package name {name} is not a valid pacman name"
    );
    let original_version = get("version");
    let original_arch = get("architecture");
    // Debian's Description is a one-line summary followed by an indented body;
    // pacman's pkgdesc is one line, so only the summary crosses over.
    let description = one_line(get("description").lines().next().unwrap_or_default(), 400);
    Ok(Info {
        kind: Kind::Deb,
        name,
        version: arch_version(&original_version),
        original_version,
        arch: arch_name(&original_arch),
        original_arch,
        description,
        url: one_line(&get("homepage"), 400),
        license: String::new(),
        // Debian counts an installed size in KiB.
        installed_size: get("installed-size")
            .parse::<u64>()
            .unwrap_or(0)
            .saturating_mul(1024),
        depends: get("depends")
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
    })
}

/// The RPM header: a lead, a signature header, then the header proper. Both
/// headers are the same structure — an eight byte introduction, a run of
/// sixteen byte index entries, then the store the entries point into.
struct RpmHeader {
    entries: Vec<(u32, u32, u32, u32)>,
    store: Vec<u8>,
}
impl RpmHeader {
    fn parse(bytes: &[u8], offset: usize) -> Result<(Self, usize)> {
        ensure!(
            bytes.len() >= offset + 16 && bytes[offset..offset + 4] == *b"\x8e\xad\xe8\x01",
            "Malformed RPM header"
        );
        let be = |at: usize| -> u32 {
            u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap_or([0; 4]))
        };
        let count = be(offset + 8) as usize;
        let store_size = be(offset + 12) as usize;
        ensure!(
            count <= 65536 && store_size <= 64 * 1024 * 1024,
            "RPM header is implausibly large"
        );
        let index = offset + 16;
        let store = index + count * 16;
        let end = store + store_size;
        ensure!(bytes.len() >= end, "Truncated RPM header");
        let entries = (0..count)
            .map(|i| {
                let at = index + i * 16;
                (be(at), be(at + 4), be(at + 8), be(at + 12))
            })
            .collect();
        Ok((
            Self {
                entries,
                store: bytes[store..end].to_vec(),
            },
            end,
        ))
    }
    fn strings(&self, tag: u32) -> Vec<String> {
        let Some(&(_, kind, offset, count)) = self.entries.iter().find(|e| e.0 == tag) else {
            return Vec::new();
        };
        // 6 STRING, 8 STRING_ARRAY, 9 I18NSTRING — all NUL-terminated in the
        // store; a plain STRING is always a single one.
        if !matches!(kind, 6 | 8 | 9) {
            return Vec::new();
        }
        let wanted = if kind == 6 { 1 } else { count as usize };
        let mut at = offset as usize;
        let mut out = Vec::new();
        for _ in 0..wanted.min(4096) {
            let Some(end) = self
                .store
                .get(at..)
                .and_then(|s| s.iter().position(|&b| b == 0))
            else {
                break;
            };
            out.push(String::from_utf8_lossy(&self.store[at..at + end]).into_owned());
            at += end + 1;
        }
        out
    }
    fn string(&self, tag: u32) -> String {
        self.strings(tag).into_iter().next().unwrap_or_default()
    }
    fn int(&self, tag: u32) -> u64 {
        let Some(&(_, 4, offset, _)) = self.entries.iter().find(|e| e.0 == tag) else {
            return 0;
        };
        self.store
            .get(offset as usize..offset as usize + 4)
            .map(|b| u32::from_be_bytes(b.try_into().unwrap_or([0; 4])) as u64)
            .unwrap_or(0)
    }
}

const RPM_NAME: u32 = 1000;
const RPM_VERSION: u32 = 1001;
const RPM_RELEASE: u32 = 1002;
const RPM_EPOCH: u32 = 1003;
const RPM_SUMMARY: u32 = 1004;
const RPM_SIZE: u32 = 1009;
const RPM_LICENSE: u32 = 1014;
const RPM_URL: u32 = 1020;
const RPM_ARCH: u32 = 1022;
const RPM_REQUIRENAME: u32 = 1049;

fn inspect_rpm(path: &Path) -> Result<Info> {
    // Only the headers are read, and an RPM header is small; a whole package
    // must never be pulled into memory to answer what it is called.
    let mut file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(16 * 1024 * 1024)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() > 96 && bytes.starts_with(b"\xed\xab\xee\xdb"),
        "Not an RPM package"
    );
    let (_, after_signature) = RpmHeader::parse(&bytes, 96).context("Unreadable RPM signature")?;
    // The signature header is padded to an eight byte boundary; the header
    // proper is not.
    let (header, _) = RpmHeader::parse(&bytes, after_signature.div_ceil(8) * 8)
        .context("Unreadable RPM header")?;
    let name = header.string(RPM_NAME);
    ensure!(!name.is_empty(), "This .rpm declares no package name");
    ensure!(
        valid_name(&name),
        "Package name {name} is not a valid pacman name"
    );
    let epoch = header.int(RPM_EPOCH);
    let version = header.string(RPM_VERSION);
    let release = header.string(RPM_RELEASE);
    let original_version = match (epoch, release.is_empty()) {
        (0, true) => version.clone(),
        (0, false) => format!("{version}-{release}"),
        (e, true) => format!("{e}:{version}"),
        (e, false) => format!("{e}:{version}-{release}"),
    };
    let original_arch = header.string(RPM_ARCH);
    Ok(Info {
        kind: Kind::Rpm,
        name,
        version: arch_version(&original_version),
        original_version,
        arch: arch_name(&original_arch),
        original_arch,
        description: one_line(&header.string(RPM_SUMMARY), 400),
        url: one_line(&header.string(RPM_URL), 400),
        license: one_line(&header.string(RPM_LICENSE), 200),
        installed_size: header.int(RPM_SIZE),
        // RPM requires files, shared objects and named capabilities alongside
        // real packages. A capability is always written with parentheses and a
        // file requirement is always absolute, so what survives is the list of
        // package names — the only part a person reading it can act on.
        depends: header
            .strings(RPM_REQUIRENAME)
            .into_iter()
            .filter(|d| !d.contains('(') && !d.starts_with('/') && !d.contains(".so"))
            .collect(),
    })
}

pub fn inspect(path: &Path, kind: Kind) -> Result<Info> {
    match kind {
        Kind::Arch => inspect_arch(path),
        Kind::Deb => inspect_deb(path),
        Kind::Rpm => inspect_rpm(path),
    }
}

/// Where a converted package is written. Under the cache directory, because it
/// is a build product: losing it costs one conversion, and nothing else.
fn convert_dir() -> PathBuf {
    env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".cache"))
        .join("appshelf/convert")
}

/// Paths a Debian or RPM payload uses that Arch keeps somewhere else. `/bin`,
/// `/sbin` and `/lib` are symbolic links into `/usr` here, and pacman refuses
/// to install a package that would write through one of them, so the payload is
/// rewritten as it is copied rather than left to fail at install time.
const PATH_RULES: &[&str] = &[
    r"|^\./||",
    // Nothing in a package AppShelf converted may name a path above the
    // package root. bsdtar already drops a leading `/` when it writes an
    // archive; this is the other half, and it runs before the moves below so
    // no rewritten path can reintroduce one. The `g` flag matters: a single
    // substitution would leave `../../etc` as `../etc`.
    r"|\.\./|__/|g",
    "|^bin/|usr/bin/|",
    "|^sbin/|usr/bin/|",
    "|^lib/|usr/lib/|",
    "|^lib64/|usr/lib/|",
    "|^usr/sbin/|usr/bin/|",
    "|^usr/lib64/|usr/lib/|",
];

fn pkginfo_body(info: &Info) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut body = String::new();
    body.push_str(&format!(
        "# Converted from {} by AppShelf\n",
        info.kind.with_article()
    ));
    body.push_str(&format!("pkgname = {}\n", info.name));
    body.push_str(&format!("pkgbase = {}\n", info.name));
    body.push_str(&format!("pkgver = {}\n", info.version));
    if !info.description.is_empty() {
        body.push_str(&format!("pkgdesc = {}\n", info.description));
    }
    if !info.url.is_empty() {
        body.push_str(&format!("url = {}\n", info.url));
    }
    body.push_str(&format!("builddate = {now}\n"));
    body.push_str(&format!(
        "packager = AppShelf <converted {} {}>\n",
        info.kind.id(),
        info.original_version
    ));
    body.push_str(&format!("size = {}\n", info.installed_size));
    body.push_str(&format!("arch = {}\n", info.arch));
    if !info.license.is_empty() {
        body.push_str(&format!("license = {}\n", info.license));
    }
    // Deliberately no `depend` lines. Debian and RPM name their dependencies
    // from their own archives — `libc6`, `libc.so.6()(64bit)` — and no honest
    // mapping onto Arch's names exists for an arbitrary package. Emitting a
    // guess would make pacman refuse the install for a package that is not
    // really missing anything; emitting none installs the files and leaves the
    // real requirements visible in the window instead.
    body
}

/// Turn a foreign package into one pacman will install, and answer where it
/// was written. An Arch package needs nothing done to it and is handed back
/// unchanged.
///
/// Nothing is unpacked to disk: `bsdtar` copies entries from the payload
/// straight into the new archive, so file modes the calling user could not
/// restore — setuid bits, root ownership — cross over exactly as they were.
pub fn convert(source: &Path, info: &Info) -> Result<(PathBuf, Option<tempfile::TempDir>)> {
    if info.kind == Kind::Arch {
        return Ok((source.to_path_buf(), None));
    }
    let root = convert_dir();
    fs::create_dir_all(&root)?;
    let stage = tempfile::Builder::new()
        .prefix("convert-")
        .tempdir_in(&root)?;
    fs::write(stage.path().join(".PKGINFO"), pkginfo_body(info))?;
    let target = format!("{}-{}-{}.pkg.tar.zst", info.name, info.version, info.arch);

    let mut build = bsdtar();
    build
        .current_dir(stage.path())
        .args(["-c", "--format=gnutar", "--zstd"])
        .args([
            "--uid", "0", "--gid", "0", "--uname", "root", "--gname", "root",
        ]);
    for rule in PATH_RULES {
        build.args(["-s", rule]);
    }
    // `.PKGINFO` is named first because pacman requires it to be the archive's
    // first entry.
    build.arg("-f").arg(&target).arg(".PKGINFO");

    let status = match info.kind {
        Kind::Deb => {
            // A .deb is an `ar` archive holding a second, compressed tarball;
            // the payload is piped from one bsdtar to the next so a large
            // package never lands on disk twice.
            let mut payload = bsdtar()
                .arg("-xOf")
                .arg(source)
                .arg("data.tar*")
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .context("bsdtar could not be run; install libarchive")?;
            let stream = payload.stdout.take().context("bsdtar produced no output")?;
            let status = build.arg("@-").stdin(stream).status();
            let _ = payload.wait();
            status
        }
        // libarchive reads an RPM's cpio payload directly.
        Kind::Rpm => build.arg(format!("@{}", source.display())).status(),
        Kind::Arch => unreachable!(),
    }
    .context("bsdtar could not be run; install libarchive")?;
    ensure!(status.success(), "Converting the package failed");

    let built = stage.path().join(&target);
    ensure!(built.is_file(), "The converted package was not written");
    Ok((built, Some(stage)))
}

/// Every package pacman currently has installed, as name to version.
pub fn installed() -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let Ok(output) = Command::new("pacman").arg("-Q").output() else {
        return map;
    };
    if !output.status.success() {
        return map;
    }
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some((name, version)) = line.split_once(' ') {
            map.insert(name.to_string(), version.trim().to_string());
        }
    }
    map
}

pub fn installed_version(name: &str) -> Option<String> {
    if !valid_name(name) {
        return None;
    }
    let output = Command::new("pacman")
        .args(["-Q", "--"])
        .arg(name)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .split_once(' ')
        .map(|(_, version)| version.trim().to_string())
}

/// Whether a repository already carries this name. Installing a converted
/// package over one is allowed, but it is the kind of thing a user should be
/// told before rather than after: the next `pacman -Syu` will replace it with
/// the repository's build.
pub fn in_repositories(name: &str) -> bool {
    if !valid_name(name) {
        return false;
    }
    Command::new("pacman")
        .args(["-Si", "--"])
        .arg(name)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// The files a package owns, as absolute paths.
pub fn files(name: &str) -> Vec<String> {
    if !valid_name(name) {
        return Vec::new();
    }
    let Ok(output) = Command::new("pacman")
        .args(["-Qlq", "--"])
        .arg(name)
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

/// The desktop entries a package installed, which is what "launch" means for
/// something that is not a self-contained image.
pub fn desktop_entries(name: &str) -> Vec<String> {
    files(name)
        .into_iter()
        .filter(|f| f.ends_with(".desktop") && f.contains("/share/applications/"))
        .collect()
}

pub fn launch(name: &str) -> Result<()> {
    let entries = desktop_entries(name);
    if let Some(entry) = entries.first() {
        let mut child = Command::new("gio")
            .arg("launch")
            .arg(entry)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("gio could not be run")?;
        thread::spawn(move || {
            let _ = child.wait();
        });
        return Ok(());
    }
    // No launcher: fall back to the first program the package put on PATH.
    let program = files(name)
        .into_iter()
        .find(|f| f.starts_with("/usr/bin/") && f.len() > "/usr/bin/".len() && !f.ends_with('/'))
        .context("This package installed no application to launch")?;
    let mut child = Command::new(&program)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("Could not start {program}"))?;
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Shell quoting for the one script AppShelf writes. Single quotes make every
/// character literal, so only a single quote itself needs an escape.
fn quote(value: &OsStr) -> String {
    format!("'{}'", value.to_string_lossy().replace('\'', r"'\''"))
}

/// Terminals to try, in the order Omarchy would. `xdg-terminal-exec` is the
/// desktop's own answer and execs the terminal in place, so waiting on it waits
/// on the terminal.
fn terminal_command(script: &Path) -> Option<Command> {
    if let Some(path) = which("xdg-terminal-exec") {
        let mut command = Command::new(path);
        command.arg("bash").arg(script);
        return Some(command);
    }
    for (program, flag) in [
        ("alacritty", "-e"),
        ("ghostty", "-e"),
        ("foot", "-e"),
        ("kitty", "--"),
        ("wezterm", "-e"),
        ("xterm", "-e"),
    ] {
        if let Some(path) = which(program) {
            let mut command = Command::new(path);
            command.arg(flag).arg("bash").arg(script);
            return Some(command);
        }
    }
    None
}

/// Run one pacman transaction where the user can see it.
///
/// pacman needs root and it needs to be able to ask questions — about replacing
/// files, about dependencies it will pull in. Answering those on the user's
/// behalf from a window that cannot show them is how a package manager destroys
/// a system, so the transaction runs in a real terminal with a real `sudo`
/// prompt, and AppShelf waits for the answer rather than inventing one. The
/// script records the exit status before it can be closed, so closing the
/// window is reported as the refusal it is.
fn pacman_transaction(title: &str, arguments: &[&OsStr]) -> Result<()> {
    ensure!(
        which("pacman").is_some(),
        "pacman was not found; system packages need an Arch-based system"
    );
    let stage = tempfile::Builder::new()
        .prefix("appshelf-pacman-")
        .tempdir_in(env::temp_dir())?;
    let status_file = stage.path().join("status");
    let script_file = stage.path().join("run.sh");
    let command = arguments
        .iter()
        .map(|a| quote(a))
        .collect::<Vec<_>>()
        .join(" ");
    let script = format!(
        "#!/usr/bin/env bash\n\
         status={status}\n\
         rc=130\n\
         trap 'printf \"%s\" \"$rc\" > \"$status\"' EXIT\n\
         printf '\\033]0;AppShelf\\007'\n\
         printf '%s\\n\\n' {title}\n\
         sudo pacman {command}\n\
         rc=$?\n\
         printf '\\n'\n\
         if [ \"$rc\" -eq 0 ]; then printf 'Done.\\n'; else printf 'pacman exited with %s.\\n' \"$rc\"; fi\n\
         printf 'Press Enter to close this window… '\n\
         read -r _\n",
        status = quote(status_file.as_os_str()),
        title = quote(OsStr::new(&format!("AppShelf — {title}"))),
        command = command,
    );
    fs::write(&script_file, script)?;

    let mut terminal = terminal_command(&script_file)
        .context("No terminal emulator was found to run pacman in")?;
    let mut child = terminal
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Could not open a terminal for pacman")?;

    // The status file, not the terminal's own exit, is what is waited on: some
    // terminals fork and return immediately, and the script's EXIT trap fires
    // even if the window is closed underneath it.
    let started = Instant::now();
    let mut terminal_gone = None;
    let status = loop {
        if let Ok(body) = fs::read_to_string(&status_file) {
            if let Ok(code) = body.trim().parse::<i32>() {
                break code;
            }
        }
        if terminal_gone.is_none() && child.try_wait().ok().flatten().is_some() {
            terminal_gone = Some(Instant::now());
        }
        if let Some(gone) = terminal_gone {
            // A terminal that both exited and left no status behind never ran
            // the script; give it a moment for a slow filesystem, then say so.
            if gone.elapsed() > Duration::from_secs(3) {
                bail!("The terminal closed before pacman could run");
            }
        }
        if started.elapsed() > TRANSACTION_TIMEOUT {
            bail!("pacman did not finish within 45 minutes");
        }
        thread::sleep(Duration::from_millis(200));
    };
    let _ = child.wait();
    ensure!(
        status == 0,
        "{}",
        match status {
            130 => "The terminal was closed before pacman finished".to_string(),
            1 => "pacman refused the transaction; its terminal shows why".to_string(),
            other => format!("pacman exited with {other}; its terminal shows why"),
        }
    );
    Ok(())
}

pub fn install_file(title: &str, package: &Path) -> Result<()> {
    pacman_transaction(
        title,
        &[OsStr::new("-U"), OsStr::new("--"), package.as_os_str()],
    )
}

pub fn remove(name: &str) -> Result<()> {
    ensure!(valid_name(name), "Invalid package name");
    pacman_transaction(
        &format!("removing {name}"),
        &[OsStr::new("-Rns"), OsStr::new("--"), OsStr::new(name)],
    )
}

/// What AppShelf put on the system, and where each of them came from. pacman
/// owns the packages themselves; this only records which of them arrived
/// through AppShelf, so the shelf can show them without claiming every package
/// on the machine.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub kind: Kind,
    pub version: String,
    #[serde(default)]
    pub original_version: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub installed: u64,
    #[serde(default)]
    pub size: u64,
    /// Whether the package installed a desktop entry, recorded once at install
    /// time. `list` runs on every backend reply and on the tray's three-second
    /// watch, so asking pacman for a file list there would be a subprocess per
    /// package per tick; a package's launcher does not change once it is in.
    #[serde(default = "yes")]
    pub launchable: bool,
}
/// A registry written before `launchable` existed says nothing about it, and
/// offering a launch that then fails is a better answer than hiding one that
/// would have worked.
fn yes() -> bool {
    true
}

pub fn registry_path() -> PathBuf {
    data_home().join("appshelf/packages.json")
}

pub fn registry() -> BTreeMap<String, Entry> {
    fs::read(registry_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn write_registry(entries: &BTreeMap<String, Entry>) -> Result<()> {
    let path = registry_path();
    fs::create_dir_all(path.parent().unwrap())?;
    crate::manager::atomic_json(&path, entries)
}

pub fn record(entry: Entry) -> Result<()> {
    let mut entries = registry();
    entries.insert(entry.name.clone(), entry);
    write_registry(&entries)
}

pub fn forget(name: &str) -> Result<()> {
    let mut entries = registry();
    if entries.remove(name).is_some() {
        write_registry(&entries)?;
    }
    Ok(())
}

/// The packages AppShelf installed that pacman still has, as shelf rows.
///
/// A package removed with pacman directly is not reported as missing, it is
/// forgotten: pacman is the authority on what is installed, and a shelf that
/// argued with it would only ever be wrong.
pub fn list() -> Vec<Value> {
    let entries = registry();
    if entries.is_empty() {
        return Vec::new();
    }
    let known = entries.len();
    let present = installed();
    let mut rows = Vec::new();
    let mut keep: BTreeMap<String, Entry> = BTreeMap::new();
    for (name, entry) in entries {
        let Some(version) = present.get(&name) else {
            continue;
        };
        rows.push(json!({
            "id": app_id(&name),
            "name": name,
            "package": name,
            "kind": "package",
            "package_kind": entry.kind.id(),
            "format": entry.kind.label(),
            "version": version,
            "size": entry.size,
            "installed": entry.installed,
            "description": entry.description,
            "source": entry.source,
            "source_modified": 0,
            "path": "",
            "icon": "",
            "missing": false,
            "unmanaged": false,
            "environment": {},
            "isolation": "off",
            "update_info": Value::Null,
            "launchable": entry.launchable,
        }));
        keep.insert(name, entry);
    }
    if keep.len() != known {
        let _ = write_registry(&keep);
    }
    rows.sort_by_key(|a| a["name"].as_str().unwrap_or_default().to_lowercase());
    rows
}

/// Everything the installer window needs to decide whether to go ahead.
pub fn preview(value: &str) -> Result<Option<Value>> {
    let path = local_path(value)?;
    ensure!(path.is_file(), "Choose a package file");
    let Some(kind) = detect(&path) else {
        return Ok(None);
    };
    let info = inspect(&path, kind)?;
    let current = installed_version(&info.name);
    let mismatch = info.arch != "any" && info.arch != host_arch();
    let mut warnings = Vec::new();
    if mismatch {
        warnings.push(format!(
            "Built for {} — this machine is {}.",
            info.original_arch,
            host_arch()
        ));
    }
    if info.kind != Kind::Arch {
        // One line. The detail belongs in the README, not in front of someone
        // deciding whether to press a button.
        warnings.push(format!(
            "Converted from {}; its dependencies and install scripts are not carried over.",
            info.kind.with_article()
        ));
    }
    if in_repositories(&info.name) {
        warnings.push(format!(
            "A repository package is also called {}; the next system upgrade replaces this one.",
            info.name
        ));
    }
    Ok(Some(json!({
        "kind": "package",
        "package_kind": info.kind.id(),
        "format": info.kind.label(),
        "path": path,
        "name": info.name,
        "version": info.version,
        "original_version": info.original_version,
        "arch": info.arch,
        "original_arch": info.original_arch,
        "description": info.description,
        "url": info.url,
        "license": info.license,
        "installed_size": info.installed_size,
        "depends": info.depends,
        "size": path.metadata()?.len(),
        "note": "",
        "icon": Value::Null,
        "id": app_id(&info.name),
        // `installed_id` means "this exact thing is already here", the same as
        // it does for an AppImage's content hash. A package of the same name at
        // another version is an upgrade or a downgrade, not the same thing, and
        // `installed_version` is what says so.
        "installed_id": (current.as_deref() == Some(info.version.as_str()))
            .then(|| app_id(&info.name)),
        "installed_name": current.as_ref().map(|_| info.name.clone()),
        "installed_version": current,
        "launchable": !desktop_entries(&info.name).is_empty(),
        "arch_mismatch": mismatch,
        "warnings": warnings,
    })))
}

/// Convert if it has to, then hand the transaction to pacman.
pub fn install(value: &str) -> Result<Value> {
    let path = local_path(value)?;
    ensure!(path.is_file(), "Choose a package file");
    let kind = detect(&path).context("That file is not a package AppShelf understands")?;
    let info = inspect(&path, kind)?;
    ensure!(
        info.arch == "any" || info.arch == host_arch(),
        "{} is built for {}, and this machine is {}",
        info.name,
        info.original_arch,
        host_arch()
    );
    let (package, _stage) = convert(&path, &info)?;
    install_file(
        &format!("installing {} {}", info.name, info.version),
        &package,
    )?;
    let version = installed_version(&info.name)
        .context("pacman reported success but the package is not installed")?;
    record(Entry {
        name: info.name.clone(),
        kind: info.kind,
        version: version.clone(),
        original_version: info.original_version,
        source: path.to_string_lossy().into_owned(),
        description: info.description,
        installed: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        size: info.installed_size,
        launchable: !desktop_entries(&info.name).is_empty(),
    })?;
    Ok(json!({"id": app_id(&info.name), "name": info.name, "version": version, "kind": "package"}))
}

pub fn uninstall(name: &str) -> Result<()> {
    ensure!(valid_name(name), "Invalid package name");
    ensure!(
        installed_version(name).is_some(),
        "{name} is no longer installed"
    );
    remove(name)?;
    ensure!(
        installed_version(name).is_none(),
        "{name} is still installed"
    );
    forget(name)
}

/// Show a package's files where the user can read them: its own directory in
/// the file manager, or the launcher it installed.
pub fn reveal(name: &str) -> Result<PathBuf> {
    ensure!(valid_name(name), "Invalid package name");
    let owned = files(name);
    // A directory of the package's own is worth opening. `/usr/share/applications`,
    // where its launcher sits among every other package's, is not.
    let own_directory = owned.iter().find(|f| {
        f.ends_with('/')
            && f.matches('/').count() == 4
            && (f.starts_with("/usr/share/") || f.starts_with("/usr/lib/"))
            && !f.starts_with("/usr/share/applications")
            && !f.starts_with("/usr/share/licenses")
            && !f.starts_with("/usr/share/doc")
    });
    let directory = own_directory
        .map(PathBuf::from)
        .or_else(|| {
            desktop_entries(name)
                .first()
                .and_then(|entry| Path::new(entry).parent().map(PathBuf::from))
        })
        .or_else(|| {
            owned
                .iter()
                .find(|f| !f.ends_with('/'))
                .and_then(|f| Path::new(f).parent().map(PathBuf::from))
        })
        .context("This package owns no files to show")?;
    Ok(directory)
}
