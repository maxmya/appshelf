use appshelf::{
    discovery,
    manager::{desktop_exec, local_path, validate_settings, Isolation, Manager, Settings},
    runtime,
};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    os::unix::fs::{symlink, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
use tempfile::TempDir;

fn resources() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
fn manager(base: &Path) -> Manager {
    Manager::new(
        base.join("data with spaces % $ ` \""),
        resources(),
        PathBuf::from(env!("CARGO_BIN_EXE_appshelf")),
    )
    .unwrap()
}
fn stub(base: &Path) -> PathBuf {
    let path = base.join("Sample % $ ` \".AppImage");
    let mut bytes = vec![0u8; 160];
    bytes[..4].copy_from_slice(b"\x7fELF");
    bytes[4] = 2;
    bytes[5] = 1;
    bytes[8..11].copy_from_slice(b"AI\x02");
    bytes[64..68].copy_from_slice(b"hsqs");
    bytes[92] = 4;
    fs::write(&path, bytes).unwrap();
    path
}
fn install(m: &Manager, p: &Path) -> String {
    m.install(p.to_str().unwrap(), Settings::default()).unwrap()["id"]
        .as_str()
        .unwrap()
        .into()
}

#[test]
fn install_remove_preserves_source_and_personal_data() {
    let tmp = TempDir::new().unwrap();
    let m = manager(tmp.path());
    let source = stub(tmp.path());
    let original = fs::read(&source).unwrap();
    let id = install(&m, &source);
    let folder = m.folder(&id).unwrap();
    assert_eq!(fs::read(folder.join("app.AppImage")).unwrap(), original);
    assert_eq!(
        fs::metadata(folder.join("app.AppImage"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    fs::create_dir(folder.join("app.AppImage.home")).unwrap();
    fs::write(folder.join("app.AppImage.home/notes"), "keep").unwrap();
    m.uninstall(&id).unwrap();
    assert!(m.list().is_empty());
    assert_eq!(fs::read(source).unwrap(), original);
    assert_eq!(
        fs::read_to_string(folder.join("app.AppImage.home/notes")).unwrap(),
        "keep"
    );
    assert!(!m.desktop_path(&id).unwrap().exists());
}
#[test]
fn duplicate_and_invalid_files_leave_no_staging() {
    let tmp = TempDir::new().unwrap();
    let m = manager(tmp.path());
    let source = stub(tmp.path());
    install(&m, &source);
    assert!(m
        .install(source.to_str().unwrap(), Settings::default())
        .unwrap_err()
        .to_string()
        .contains("already installed"));
    fs::write(&source, b"#!/bin/sh\necho never execute me").unwrap();
    assert!(m
        .install(source.to_str().unwrap(), Settings::default())
        .is_err());
    assert!(!fs::read_dir(&m.root)
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().starts_with('.')));
}
#[test]
fn urls_and_identifiers_are_validated() {
    let tmp = TempDir::new().unwrap();
    let source = stub(tmp.path());
    let m = manager(tmp.path());
    assert_eq!(
        local_path(url::Url::from_file_path(&source).unwrap().as_str()).unwrap(),
        source
    );
    assert!(local_path("file://remote/tmp/test.AppImage").is_err());
    assert!(local_path("https://example.org/app.AppImage").is_err());
    assert!(m.uninstall("../../foreign").is_err());
}
#[test]
fn foreign_launcher_and_symlink_directory_are_preserved() {
    let tmp = TempDir::new().unwrap();
    let m = manager(tmp.path());
    let source = stub(tmp.path());
    let id = install(&m, &source);
    let launcher = m.desktop_path(&id).unwrap();
    fs::write(&launcher, "[Desktop Entry]\nName=Foreign\n").unwrap();
    assert!(m.uninstall(&id).is_err());
    let folder = m.folder(&id).unwrap();
    let moved = tmp.path().join("foreign");
    fs::rename(&folder, &moved).unwrap();
    symlink(&moved, &folder).unwrap();
    assert!(m.uninstall(&id).is_err());
    assert!(moved.join("app.AppImage").exists());
}
#[test]
fn uninstall_failure_rolls_back_owned_files() {
    let tmp = TempDir::new().unwrap();
    let m = manager(tmp.path());
    let source = stub(tmp.path());
    let id = install(&m, &source);
    let folder = m.folder(&id).unwrap();
    fs::create_dir(folder.join("icon.png")).unwrap();
    assert!(m.uninstall(&id).is_err());
    assert!(folder.join("record.json").exists());
    assert!(folder.join("app.AppImage").exists());
    assert_eq!(m.list().len(), 1);
}
#[test]
fn desktop_entries_escape_metacharacters() {
    let tmp = TempDir::new().unwrap();
    let m = manager(tmp.path());
    let source = stub(tmp.path());
    let id = install(&m, &source);
    let desktop = m.desktop_path(&id).unwrap();
    let output = Command::new("desktop-file-validate")
        .arg(&desktop)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(desktop_exec(&source).contains("%%"));
    assert!(fs::read_to_string(desktop).unwrap().contains("--launch"));
}
#[test]
fn discovery_finds_extensionless_and_launcher_referenced_apps() {
    let tmp = TempDir::new().unwrap();
    let m = manager(tmp.path());
    let source = stub(tmp.path());
    let extensionless = tmp.path().join("ExistingApplication");
    fs::rename(source, &extensionless).unwrap();
    fs::write(
        m.launchers.join("other-manager.desktop"),
        format!(
            "[Desktop Entry]\nName=Existing app\nExec={} %U\n",
            desktop_exec(&extensionless)
        ),
    )
    .unwrap();
    let found = discovery::discover(&m, &[tmp.path().to_path_buf()]);
    assert!(found
        .iter()
        .any(|a| a["path"].as_str() == extensionless.to_str() && a["unmanaged"] == true));
    install(&m, &extensionless);
    let found = discovery::discover(&m, &[tmp.path().to_path_buf()]);
    assert!(!found
        .iter()
        .any(|a| a["path"].as_str() == extensionless.to_str()));
    assert!(extensionless.exists());
}
#[test]
fn settings_and_protocol_reject_invalid_input() {
    for key in ["BAD NAME", "HOME", "TARGET_APPIMAGE"] {
        let settings = Settings {
            environment: BTreeMap::from([(key.into(), "value".into())]),
            isolation: Isolation::Home,
        };
        assert!(validate_settings(&settings).is_err());
    }
    let tmp = TempDir::new().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_appshelf"))
        .arg("--backend")
        .env("XDG_DATA_HOME", tmp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"[]\n{\"command\":\"list\"}\n{\"command\":\"quit\"}\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let replies: Vec<serde_json::Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(replies[0]["event"], "ready");
    assert_eq!(replies[1]["ok"], false);
    assert_eq!(replies[2]["ok"], true);
    assert_eq!(replies[3]["event"], "quit");
}

struct Fixture {
    _temp: TempDir,
    images: Vec<(String, PathBuf)>,
}
impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let dir = temp.path().join("AppDir");
        fs::create_dir(&dir).unwrap();
        fs::write(
            dir.join("AppRun"),
            "#!/bin/sh\n/usr/bin/env > \"$SHELF_RESULT\"\n",
        )
        .unwrap();
        fs::set_permissions(dir.join("AppRun"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(
            dir.join("fixture.desktop"),
            "[Desktop Entry]\nType=Application\nName=Runtime Fixture\nExec=AppRun\nIcon=org.test.Fixture\n",
        )
        .unwrap();
        fs::write(
            dir.join("org.test.Fixture.png"),
            b"\x89PNG\r\n\x1a\nfixture",
        )
        .unwrap();
        let runtime = runtime::runtime_path(&resources()).unwrap();
        let mut images = Vec::new();
        for kind in ["SquashFS", "DwarFS"] {
            let fs_path = temp.path().join(format!("{kind}.fs"));
            let mut command = Command::new(&runtime);
            if kind == "SquashFS" {
                command
                    .arg("--appimage-mksquashfs")
                    .arg(&dir)
                    .arg(&fs_path)
                    .args(["-noappend", "-processors", "1"]);
            } else {
                command
                    .arg("--appimage-mkdwarfs")
                    .arg("-i")
                    .arg(&dir)
                    .arg("-o")
                    .arg(&fs_path)
                    .args(["-l", "1"]);
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let image = temp.path().join(format!("{kind}.AppImage"));
            let mut bytes = fs::read(&runtime).unwrap();
            bytes.extend(fs::read(fs_path).unwrap());
            fs::write(&image, bytes).unwrap();
            images.push((kind.into(), image));
        }
        Self {
            _temp: temp,
            images,
        }
    }
}
#[test]
fn real_squashfs_and_dwarfs_metadata_and_fuse_free_isolation() {
    let fixtures = Fixture::new();
    let runtime = runtime::runtime_path(&resources()).unwrap();
    for (kind, image) in &fixtures.images {
        assert_eq!(&runtime::filesystem(image).unwrap().0, kind);
        let (name, icon) = runtime::metadata(&runtime, image).unwrap();
        assert_eq!(name, "Runtime Fixture");
        assert!(icon.unwrap().starts_with(b"\x89PNG"));
        let tmp = TempDir::new().unwrap();
        let m = manager(tmp.path());
        let output_path = tmp.path().join("result");
        let settings = Settings {
            environment: BTreeMap::from([
                (
                    "SHELF_RESULT".into(),
                    output_path.to_string_lossy().into_owned(),
                ),
                ("CUSTOM_SETTING".into(), "spaces = $literal `value`".into()),
            ]),
            isolation: Isolation::Home,
        };
        let id = m.install(image.to_str().unwrap(), settings).unwrap()["id"]
            .as_str()
            .unwrap()
            .to_string();
        let (runtime, env) = m.launch_spec(&id).unwrap();
        assert_eq!(env["APPIMAGE_EXTRACT_AND_RUN"], "1");
        let output = Command::new(runtime)
            .arg("--appimage-extract-and-run")
            .env_clear()
            .envs(&env)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let observed = fs::read_to_string(&output_path).unwrap();
        let private = m.root.parent().unwrap().join("data").join(&id);
        assert!(observed
            .lines()
            .any(|s| s == format!("HOME={}", private.join("home").display())));
        assert!(observed
            .lines()
            .any(|s| s == format!("XDG_CONFIG_HOME={}", private.join("config").display())));
        assert!(observed.contains("CUSTOM_SETTING=spaces = $literal `value`"));
        m.configure(&id, Settings::default()).unwrap();
        assert_eq!(m.list()[0]["isolation"], "off");
        m.uninstall(&id).unwrap();
        assert!(private.join("home").is_dir());
    }
}
