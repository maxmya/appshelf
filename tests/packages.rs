//! System package support, exercised against packages this file builds itself.
//!
//! Nothing here installs anything: every privileged step goes through pacman in
//! a terminal, and a test must not open one. What is checked is everything up
//! to that point — what AppShelf decides a file is, what it says about it, and
//! whether the package it hands pacman is one pacman actually accepts. That
//! last part is verified with `pacman -Qip` and `pacman -Qlp`, which read a
//! package file and need no privileges at all.

use appshelf::package::{self, Kind};
use std::{
    fs,
    os::unix::fs::{symlink, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use tempfile::TempDir;

fn have(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// A payload with everything that has ever gone wrong in a conversion: a
/// setuid binary, the pre-usr-merge directories Arch keeps as symbolic links,
/// a relative symbolic link, and a desktop entry.
fn payload(root: &Path) -> PathBuf {
    let tree = root.join("payload");
    for directory in [
        "bin",
        "sbin",
        "lib",
        "lib64",
        "usr/sbin",
        "usr/lib64",
        "usr/share/applications",
    ] {
        fs::create_dir_all(tree.join(directory)).unwrap();
    }
    for file in [
        "bin/tool",
        "sbin/admin",
        "lib/libfoo.so.1",
        "lib64/libbar.so",
        "usr/sbin/daemon",
        "usr/lib64/libbaz.so",
        "usr/share/applications/sample.desktop",
    ] {
        fs::write(tree.join(file), b"payload\n").unwrap();
    }
    fs::set_permissions(tree.join("bin/tool"), fs::Permissions::from_mode(0o4755)).unwrap();
    symlink("../lib/libfoo.so.1", tree.join("lib/libfoo.so")).unwrap();
    let archive = root.join("data.tar.xz");
    assert!(Command::new("bsdtar")
        .args(["-cJf"])
        .arg(&archive)
        .arg("-C")
        .arg(&tree)
        .args(["--uid", "0", "--gid", "0", "./bin", "./sbin", "./lib", "./lib64", "./usr"])
        .status()
        .unwrap()
        .success());
    archive
}

fn write_deb(root: &Path, control: &str) -> PathBuf {
    let data = payload(root);
    let control_dir = root.join("control");
    fs::create_dir_all(&control_dir).unwrap();
    fs::write(control_dir.join("control"), control).unwrap();
    let control_archive = root.join("control.tar.xz");
    assert!(Command::new("bsdtar")
        .args(["-cJf"])
        .arg(&control_archive)
        .arg("-C")
        .arg(&control_dir)
        .arg("./control")
        .status()
        .unwrap()
        .success());
    fs::write(root.join("debian-binary"), "2.0\n").unwrap();
    let deb = root.join("sample.deb");
    // `ar` member order is part of the format: debian-binary first.
    assert!(Command::new("ar")
        .arg("rc")
        .arg(&deb)
        .arg(root.join("debian-binary"))
        .arg(&control_archive)
        .arg(&data)
        .status()
        .unwrap()
        .success());
    deb
}

fn field(text: &str, key: &str) -> String {
    text.lines()
        .find(|line| line.starts_with(key))
        .and_then(|line| line.split_once(':'))
        .map(|(_, value)| value.trim().to_string())
        .unwrap_or_default()
}

fn pacman_info(package: &Path) -> String {
    let output = Command::new("pacman")
        .arg("-Qip")
        .arg(package)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "pacman refused the converted package: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn pacman_files(package: &Path) -> Vec<String> {
    let output = Command::new("pacman")
        .arg("-Qlp")
        .arg(package)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_once(' ').map(|(_, path)| path.to_string()))
        .collect()
}

#[test]
fn versions_are_normalised_to_something_pacman_accepts() {
    // Debian and RPM both use characters pacman does not, and both use more
    // hyphens than pacman's single release separator allows.
    assert_eq!(package::arch_version("2.10-3"), "2.10-3");
    assert_eq!(package::arch_version("1.2"), "1.2-1");
    assert_eq!(package::arch_version("1:2.0~beta-3-4"), "1:2.0_beta_3-4");
    assert_eq!(package::arch_version("2.12.1-4.fc40"), "2.12.1-4.fc40");
    assert_eq!(package::arch_version("1.0+dfsg-2"), "1.0+dfsg-2");
    // An epoch is only an epoch when it is a number; a colon anywhere else is
    // just another character that has to go.
    assert_eq!(package::arch_version("weird:thing-1"), "weird_thing-1");
    assert_eq!(package::arch_version(""), "0-1");
    for raw in ["2.10-3", "1:2.0~beta-3-4", "", "~~~-~~~", "a b c"] {
        let version = package::arch_version(raw);
        let (_, rest) = version.split_once(':').unwrap_or(("", &version));
        assert_eq!(rest.matches('-').count(), 1, "{raw} -> {version}");
        assert!(
            rest.chars()
                .all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c)),
            "{raw} -> {version}"
        );
    }
}

#[test]
fn identifiers_never_collide_with_a_managed_appimage() {
    // A managed AppImage is 64 hexadecimal characters; a package is its own
    // name behind a prefix, and neither can be read as the other.
    assert_eq!(package::app_id("hello"), "pkg:hello");
    assert_eq!(package::id_name("pkg:hello"), Some("hello"));
    assert_eq!(package::id_name(&"a".repeat(64)), None);
    assert_eq!(package::id_name("hello"), None);
    // Nothing that could reach a command line as a flag or a path survives.
    assert_eq!(package::id_name("pkg:--help"), None);
    assert_eq!(package::id_name("pkg:../../etc/passwd"), None);
    assert_eq!(package::id_name("pkg:a b"), None);
    assert_eq!(package::id_name("pkg:"), None);
    assert!(package::valid_name("gimp-devel+2"));
    assert!(!package::valid_name(".hidden"));
}

#[test]
fn formats_are_told_apart_by_content_rather_than_by_name() {
    let root = TempDir::new().unwrap();
    let base = root.path();

    // An RPM is its magic, whatever it is called.
    let rpm = base.join("no-extension");
    fs::write(&rpm, b"\xed\xab\xee\xdb\x03\x00\x00\x00rest of the lead").unwrap();
    assert_eq!(package::detect(&rpm), Some(Kind::Rpm));

    // A .deb and a static library are the same container, and only the first
    // member tells them apart.
    let deb = base.join("looks-like-nothing");
    fs::write(
        &deb,
        b"!<arch>\ndebian-binary   1700000000  0     0     100644  4    ",
    )
    .unwrap();
    assert_eq!(package::detect(&deb), Some(Kind::Deb));
    let library = base.join("libfoo.a");
    fs::write(
        &library,
        b"!<arch>\nfoo.o/          1700000000  0     0     100644  4    ",
    )
    .unwrap();
    assert_eq!(package::detect(&library), None);

    // An Arch package has no magic of its own: it is a compressed tarball, and
    // the name is the only claim. A tarball that makes no such claim is not
    // one, however it is compressed.
    let package_named = base.join("thing-1-1-x86_64.pkg.tar.zst");
    fs::write(&package_named, b"\x28\xb5\x2f\xfd\x00\x58\x00\x00").unwrap();
    assert_eq!(package::detect(&package_named), Some(Kind::Arch));
    let plain = base.join("photos.tar.zst");
    fs::write(&plain, b"\x28\xb5\x2f\xfd\x00\x58\x00\x00").unwrap();
    assert_eq!(package::detect(&plain), None);
    // Nor is a file that merely claims to be one without the compression.
    let liar = base.join("thing-1-1-x86_64.pkg.tar.zst.txt");
    fs::write(&liar, b"hello").unwrap();
    assert_eq!(package::detect(&liar), None);

    // An AppImage stays AppShelf's own business and must never be read as a
    // package by the module that would hand it to pacman.
    let appimage = base.join("Sample.AppImage");
    let mut bytes = vec![0u8; 160];
    bytes[..4].copy_from_slice(b"\x7fELF");
    bytes[8..11].copy_from_slice(b"AI\x02");
    fs::write(&appimage, bytes).unwrap();
    assert_eq!(package::detect(&appimage), None);
}

#[test]
fn a_converted_deb_is_a_package_pacman_reads_and_keeps_its_file_modes() {
    if !have("bsdtar") || !have("pacman") || !have("ar") {
        eprintln!("skipped: bsdtar, ar and pacman are required");
        return;
    }
    let root = TempDir::new().unwrap();
    let deb = write_deb(
        root.path(),
        "Package: sample-tool\n\
         Version: 1:2.0~beta-3-4\n\
         Architecture: amd64\n\
         Installed-Size: 12\n\
         Homepage: https://example.invalid/sample\n\
         Depends: libc6 (>= 2.34), libfoo\n\
         Description: a sample tool\n \
         with a longer body that is not the summary\n",
    );

    assert_eq!(package::detect(&deb), Some(Kind::Deb));
    let info = package::inspect(&deb, Kind::Deb).unwrap();
    assert_eq!(info.name, "sample-tool");
    assert_eq!(info.original_version, "1:2.0~beta-3-4");
    assert_eq!(info.version, "1:2.0_beta_3-4");
    assert_eq!(info.arch, "x86_64");
    assert_eq!(info.original_arch, "amd64");
    // Only the first line of a Debian description is a summary, and pkgdesc is
    // one line.
    assert_eq!(info.description, "a sample tool");
    assert_eq!(info.url, "https://example.invalid/sample");
    // Installed-Size is counted in KiB.
    assert_eq!(info.installed_size, 12 * 1024);
    assert_eq!(info.depends, vec!["libc6 (>= 2.34)", "libfoo"]);

    let (built, _stage) = package::convert(&deb, &info).unwrap();
    let text = pacman_info(&built);
    assert_eq!(field(&text, "Name"), "sample-tool");
    assert_eq!(field(&text, "Version"), "1:2.0_beta_3-4");
    assert_eq!(field(&text, "Architecture"), "x86_64");
    assert_eq!(field(&text, "Description"), "a sample tool");
    // Foreign dependency names mean nothing to pacman, so none are claimed:
    // a guess here is what makes pacman refuse a package that is really fine.
    assert_eq!(field(&text, "Depends On"), "None");

    let files = pacman_files(&built);
    // `/bin`, `/sbin`, `/lib` and the 64-bit variants are symbolic links on
    // Arch, and pacman refuses to write through one.
    for expected in [
        "/usr/bin/tool",
        "/usr/bin/admin",
        "/usr/bin/daemon",
        "/usr/lib/libfoo.so.1",
        "/usr/lib/libfoo.so",
        "/usr/lib/libbar.so",
        "/usr/lib/libbaz.so",
        "/usr/share/applications/sample.desktop",
    ] {
        assert!(
            files.iter().any(|f| f == expected),
            "missing {expected} in {files:?}"
        );
    }
    assert!(
        !files.iter().any(|f| f.starts_with("/bin/")
            || f.starts_with("/sbin/")
            || f.starts_with("/lib/")
            || f.starts_with("/lib64/")
            || f.starts_with("/usr/sbin/")
            || f.starts_with("/usr/lib64/")),
        "a pre-usr-merge path survived: {files:?}"
    );

    // The payload is copied archive to archive rather than unpacked, so a mode
    // the test user could not restore on disk is still there afterwards.
    let listing = Command::new("bsdtar")
        .arg("-tvf")
        .arg(&built)
        .output()
        .unwrap();
    let listing = String::from_utf8_lossy(&listing.stdout);
    let tool = listing
        .lines()
        .find(|line| line.ends_with("usr/bin/tool"))
        .expect("usr/bin/tool is in the package");
    assert!(tool.starts_with("-rwsr-xr-x"), "setuid was lost: {tool}");
    assert!(
        tool.contains("root"),
        "ownership was not normalised: {tool}"
    );
    // `.PKGINFO` has to be the archive's first entry.
    assert!(listing.lines().next().unwrap().ends_with(".PKGINFO"));
}

#[test]
fn a_converted_package_cannot_name_a_path_outside_itself() {
    if !have("bsdtar") || !have("pacman") || !have("ar") {
        eprintln!("skipped: bsdtar, ar and pacman are required");
        return;
    }
    let root = TempDir::new().unwrap();
    let base = root.path();
    // A payload that tries to write above its own root, which is the one thing
    // a converted package must never be able to do.
    fs::create_dir_all(base.join("evil")).unwrap();
    fs::write(base.join("evil/passwd"), b"pwned\n").unwrap();
    assert!(Command::new("bsdtar")
        .args(["-cJf"])
        .arg(base.join("data.tar.xz"))
        .arg("-C")
        .arg(base.join("evil"))
        .args(["-s", "|^./passwd|../../../../etc/passwd|", "./passwd"])
        .status()
        .unwrap()
        .success());
    let control_dir = base.join("control");
    fs::create_dir_all(&control_dir).unwrap();
    fs::write(
        control_dir.join("control"),
        "Package: escapee\nVersion: 1\nArchitecture: all\nDescription: nope\n",
    )
    .unwrap();
    assert!(Command::new("bsdtar")
        .args(["-cJf"])
        .arg(base.join("control.tar.xz"))
        .arg("-C")
        .arg(&control_dir)
        .arg("./control")
        .status()
        .unwrap()
        .success());
    fs::write(base.join("debian-binary"), "2.0\n").unwrap();
    let deb = base.join("escapee.deb");
    assert!(Command::new("ar")
        .arg("rc")
        .arg(&deb)
        .arg(base.join("debian-binary"))
        .arg(base.join("control.tar.xz"))
        .arg(base.join("data.tar.xz"))
        .status()
        .unwrap()
        .success());

    let info = package::inspect(&deb, Kind::Deb).unwrap();
    // `all` means every architecture, which pacman spells `any`.
    assert_eq!(info.arch, "any");
    let (built, _stage) = package::convert(&deb, &info).unwrap();
    for path in pacman_files(&built) {
        assert!(!path.contains(".."), "an escaping path survived: {path}");
        assert!(path != "/etc/passwd", "the payload reached /etc/passwd");
    }
}

#[test]
fn an_arch_package_is_read_as_it_is_and_handed_on_unchanged() {
    if !have("bsdtar") || !have("pacman") {
        eprintln!("skipped: bsdtar and pacman are required");
        return;
    }
    let root = TempDir::new().unwrap();
    let base = root.path();
    let stage = base.join("stage");
    fs::create_dir_all(stage.join("usr/bin")).unwrap();
    fs::write(stage.join("usr/bin/native"), b"native\n").unwrap();
    fs::write(
        stage.join(".PKGINFO"),
        "pkgname = native-thing\n\
         pkgver = 3.1-2\n\
         pkgdesc = already an Arch package\n\
         arch = x86_64\n\
         size = 4096\n\
         license = MIT\n\
         depend = glibc\n",
    )
    .unwrap();
    let native = base.join("native-thing-3.1-2-x86_64.pkg.tar.zst");
    assert!(Command::new("bsdtar")
        .args(["-c", "--zstd", "-f"])
        .arg(&native)
        .arg("-C")
        .arg(&stage)
        .args([".PKGINFO", "usr"])
        .status()
        .unwrap()
        .success());

    assert_eq!(package::detect(&native), Some(Kind::Arch));
    let info = package::inspect(&native, Kind::Arch).unwrap();
    assert_eq!(info.name, "native-thing");
    assert_eq!(info.version, "3.1-2");
    assert_eq!(info.license, "MIT");
    assert_eq!(info.depends, vec!["glibc"]);
    assert_eq!(info.installed_size, 4096);

    // Nothing is rewritten: a package that is already the right shape must
    // reach pacman byte for byte, signatures and all.
    let (handed, stage) = package::convert(&native, &info).unwrap();
    assert_eq!(handed, native);
    assert!(stage.is_none());
}

#[test]
fn the_preview_says_what_would_happen_before_anything_does() {
    if !have("bsdtar") || !have("pacman") || !have("ar") {
        eprintln!("skipped: bsdtar, ar and pacman are required");
        return;
    }
    let root = TempDir::new().unwrap();
    let deb = write_deb(
        root.path(),
        "Package: sample-tool\nVersion: 1.0\nArchitecture: amd64\nDescription: a sample tool\n",
    );
    let preview = package::preview(deb.to_str().unwrap())
        .unwrap()
        .expect("a package");
    assert_eq!(preview["kind"], "package");
    assert_eq!(preview["package_kind"], "deb");
    assert_eq!(preview["name"], "sample-tool");
    assert_eq!(preview["version"], "1.0-1");
    assert_eq!(preview["arch_mismatch"], false);
    // Nothing invented is ever installed silently: the conversion says so.
    let warnings = preview["warnings"].as_array().unwrap();
    assert!(
        warnings.iter().any(|w| w
            .as_str()
            .unwrap_or_default()
            .contains("Converted from a Debian package")),
        "{warnings:?}"
    );

    // A package for another machine is refused rather than handed to pacman,
    // which would only refuse it later and less clearly.
    let other = TempDir::new().unwrap();
    let foreign = write_deb(
        other.path(),
        "Package: sample-tool\nVersion: 1.0\nArchitecture: mips64el\nDescription: a sample tool\n",
    );
    let preview = package::preview(foreign.to_str().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(preview["arch_mismatch"], true);
    let error = package::install(foreign.to_str().unwrap()).unwrap_err();
    let error = format!("{error:#}");
    assert!(error.contains("mips64el"), "{error}");

    // An AppImage is not a package, and the preview says so by declining to
    // answer rather than by guessing.
    let appimage = root.path().join("Sample.AppImage");
    let mut bytes = vec![0u8; 160];
    bytes[..4].copy_from_slice(b"\x7fELF");
    bytes[8..11].copy_from_slice(b"AI\x02");
    fs::write(&appimage, bytes).unwrap();
    assert!(package::preview(appimage.to_str().unwrap())
        .unwrap()
        .is_none());
}

/// A minimal but structurally real RPM: the 96-byte lead, an empty signature
/// header, the header proper carrying the tags AppShelf reads, and a cpio
/// payload. Built by hand because the `rpm` tools are not on an Arch box and
/// AppShelf must not need them either.
fn write_rpm(root: &Path, tags: &[(u32, u32, &str)], size: u32) -> PathBuf {
    fn header(entries: &[(u32, u32, u32, u32)], store: &[u8]) -> Vec<u8> {
        let mut out = b"\x8e\xad\xe8\x01\x01\x00\x00\x00".to_vec();
        out.extend_from_slice(&(entries.len() as u32).to_be_bytes());
        out.extend_from_slice(&(store.len() as u32).to_be_bytes());
        for (tag, kind, offset, count) in entries {
            for value in [tag, kind, offset, count] {
                out.extend_from_slice(&value.to_be_bytes());
            }
        }
        out.extend_from_slice(store);
        out
    }

    let mut store: Vec<u8> = Vec::new();
    let mut entries: Vec<(u32, u32, u32, u32)> = Vec::new();
    for (tag, kind, value) in tags {
        entries.push((*tag, *kind, store.len() as u32, 1));
        store.extend_from_slice(value.as_bytes());
        store.push(0);
    }
    // INT32 values are aligned in the store, as every real RPM writes them.
    while !store.len().is_multiple_of(4) {
        store.push(0);
    }
    entries.push((1009, 4, store.len() as u32, 1));
    store.extend_from_slice(&size.to_be_bytes());
    entries.sort_by_key(|entry| entry.0);

    let mut bytes = vec![0u8; 96];
    bytes[..4].copy_from_slice(b"\xed\xab\xee\xdb");
    bytes[4] = 3;
    // An empty signature header, padded to the eight-byte boundary the header
    // proper begins on.
    let signature = header(&[], &[]);
    bytes.extend_from_slice(&signature);
    while !bytes.len().is_multiple_of(8) {
        bytes.push(0);
    }
    bytes.extend_from_slice(&header(&entries, &store));

    let tree = root.join("rpmroot");
    fs::create_dir_all(tree.join("usr/bin")).unwrap();
    fs::write(tree.join("usr/bin/greet"), b"greet\n").unwrap();
    fs::set_permissions(
        tree.join("usr/bin/greet"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let cpio = root.join("payload.cpio");
    assert!(Command::new("bsdtar")
        .args(["-c", "--format=newc", "-f"])
        .arg(&cpio)
        .arg("-C")
        .arg(&tree)
        .args(["--uid", "0", "--gid", "0", "./usr"])
        .status()
        .unwrap()
        .success());
    bytes.extend_from_slice(&fs::read(&cpio).unwrap());

    let rpm = root.join("sample.rpm");
    fs::write(&rpm, bytes).unwrap();
    rpm
}

#[test]
fn an_rpm_header_is_read_without_the_rpm_tools() {
    if !have("bsdtar") || !have("pacman") {
        eprintln!("skipped: bsdtar and pacman are required");
        return;
    }
    let root = TempDir::new().unwrap();
    let rpm = write_rpm(
        root.path(),
        &[
            (1000, 6, "greeter"),
            (1001, 6, "2.12.1"),
            (1002, 6, "4.fc40"),
            (1004, 9, "Prints a greeting"),
            (1014, 6, "GPL-3.0-or-later"),
            (1020, 6, "https://example.invalid/greeter"),
            (1022, 6, "x86_64"),
            // Requirements naming a capability or a file mean nothing to
            // pacman and would only be noise in the window.
            (1049, 8, "coreutils"),
        ],
        201071,
    );

    assert_eq!(package::detect(&rpm), Some(Kind::Rpm));
    let info = package::inspect(&rpm, Kind::Rpm).unwrap();
    assert_eq!(info.name, "greeter");
    assert_eq!(info.original_version, "2.12.1-4.fc40");
    assert_eq!(info.version, "2.12.1-4.fc40");
    assert_eq!(info.arch, "x86_64");
    assert_eq!(info.description, "Prints a greeting");
    assert_eq!(info.license, "GPL-3.0-or-later");
    assert_eq!(info.url, "https://example.invalid/greeter");
    assert_eq!(info.installed_size, 201071);
    assert_eq!(info.depends, vec!["coreutils"]);

    let (built, _stage) = package::convert(&rpm, &info).unwrap();
    let text = pacman_info(&built);
    assert_eq!(field(&text, "Name"), "greeter");
    assert_eq!(field(&text, "Version"), "2.12.1-4.fc40");
    assert_eq!(field(&text, "Licenses"), "GPL-3.0-or-later");
    // The size RPM declared, carried through verbatim. pacman's own rendering
    // of it is pacman's business, so the `.PKGINFO` is what is checked.
    let pkginfo = Command::new("bsdtar")
        .arg("-xOf")
        .arg(&built)
        .arg(".PKGINFO")
        .output()
        .unwrap();
    let pkginfo = String::from_utf8_lossy(&pkginfo.stdout).into_owned();
    assert!(
        pkginfo.lines().any(|line| line == "size = 201071"),
        "{pkginfo}"
    );
    assert!(
        pkginfo.lines().any(|line| line == "arch = x86_64"),
        "{pkginfo}"
    );
    // `pkgname` must be the first field pacman meets after the comment.
    assert!(
        pkginfo.starts_with("# Converted from an RPM package by AppShelf\npkgname = greeter\n"),
        "{pkginfo}"
    );
    // The cpio payload writes `./usr/...`; a package's paths are relative to
    // the root without that prefix.
    assert_eq!(
        pacman_files(&built),
        vec!["/usr/", "/usr/bin/", "/usr/bin/greet"]
    );
}

#[test]
fn an_rpm_that_is_not_one_is_refused_rather_than_misread() {
    let root = TempDir::new().unwrap();
    // The magic without a header behind it: truncation must be an error, not a
    // read past the end of the file.
    let stub = root.path().join("truncated.rpm");
    fs::write(&stub, b"\xed\xab\xee\xdb".repeat(30)).unwrap();
    assert_eq!(package::detect(&stub), Some(Kind::Rpm));
    assert!(package::inspect(&stub, Kind::Rpm).is_err());

    let empty = root.path().join("empty.rpm");
    fs::write(&empty, b"").unwrap();
    assert_eq!(package::detect(&empty), None);
}

#[test]
fn an_unknown_architecture_cannot_become_a_path() {
    if !have("bsdtar") || !have("pacman") || !have("ar") {
        eprintln!("skipped: bsdtar, ar and pacman are required");
        return;
    }
    let root = TempDir::new().unwrap();
    // The architecture is written into the converted package's file name, and
    // a control file is text the package author chose.
    let deb = write_deb(
        root.path(),
        "Package: sneaky\nVersion: 1\nArchitecture: ../../../../tmp/owned\nDescription: nope\n",
    );
    let info = package::inspect(&deb, Kind::Deb).unwrap();
    assert!(!info.arch.contains('/'), "{}", info.arch);
    assert!(!info.arch.contains(".."), "{}", info.arch);
    // The original is kept so the window can say what the file actually claimed.
    assert_eq!(info.original_arch, "../../../../tmp/owned");
    let (built, _stage) = package::convert(&deb, &info).unwrap();
    assert_eq!(built.parent(), _stage.as_ref().map(|s| s.path()));
    // And it is not this machine's architecture, so it is refused outright.
    let error = format!("{:#}", package::install(deb.to_str().unwrap()).unwrap_err());
    assert!(error.contains("sneaky"), "{error}");
}
