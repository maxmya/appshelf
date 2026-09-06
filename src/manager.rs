use crate::runtime;
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    env,
    fs::{self, File, OpenOptions},
    io::Write,
    os::{
        fd::AsRawFd,
        unix::{fs::PermissionsExt, process::CommandExt},
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const APP_ID: &str = "org.omarchy.appshelf";
pub type Environment = BTreeMap<String, String>;
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub environment: Environment,
    #[serde(default)]
    pub isolation: Isolation,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Isolation {
    #[default]
    Off,
    Config,
    Home,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Record {
    pub name: String,
    pub size: u64,
    pub installed: u64,
    pub format: String,
    #[serde(flatten)]
    pub settings: Settings,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub source_modified: u64,
}

pub fn home() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}
pub fn data_home() -> PathBuf {
    env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/share"))
}
pub fn local_path(value: &str) -> Result<PathBuf> {
    let path = if value.starts_with("file:") {
        let url = url::Url::parse(value)?;
        ensure!(
            url.host_str().is_none() || url.host_str() == Some("localhost"),
            "Choose a local file"
        );
        ensure!(
            url.query().is_none() && url.fragment().is_none(),
            "Choose a local file"
        );
        url.to_file_path()
            .map_err(|_| anyhow::anyhow!("Invalid file URL"))?
    } else {
        ensure!(
            !value.contains("://"),
            "Choose a downloaded AppImage, not a web address"
        );
        if let Some(s) = value.strip_prefix("~/") {
            home().join(s)
        } else {
            PathBuf::from(value)
        }
    };
    path.canonicalize()
        .context("File does not exist or cannot be read")
}
pub fn modified(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}
pub fn desktop_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}
pub fn desktop_exec(path: &Path) -> String {
    let mut value = String::from("\"");
    for c in path.to_string_lossy().chars() {
        if "\\\"`$".contains(c) {
            value.push('\\');
        }
        value.push(c);
        if c == '%' {
            value.push('%');
        }
    }
    value.push('"');
    desktop_string(&value)
}
pub fn validate_settings(settings: &Settings) -> Result<()> {
    ensure!(
        settings.environment.len() <= 100,
        "At most 100 environment variables are supported"
    );
    for (key, value) in &settings.environment {
        ensure!(
            !key.is_empty()
                && key.chars().enumerate().all(|(i, c)| c == '_'
                    || c.is_ascii_alphabetic()
                    || (i > 0 && c.is_ascii_digit())),
            "Invalid environment variable name: {key}"
        );
        ensure!(
            !value.contains('\0')
                && !value.contains('\n')
                && !value.contains('\r')
                && value.len() <= 16384,
            "Environment values must be single-line text up to 16 KiB"
        );
        ensure!(
            ![
                "APPIMAGE",
                "TARGET_APPIMAGE",
                "URUNTIME",
                "RUNTIME_",
                "RUNIMAGE",
                "TARGET_RUNIMAGE",
                "APPSHELF_"
            ]
            .iter()
            .any(|p| key.starts_with(p))
                && ![
                    "HOME",
                    "XDG_CONFIG_HOME",
                    "XDG_DATA_HOME",
                    "XDG_CACHE_HOME",
                    "XDG_STATE_HOME",
                    "NO_CLEANUP",
                    "NO_UNMOUNT"
                ]
                .contains(&key.as_str()),
            "{key} is managed by AppShelf; use the isolation setting"
        );
    }
    Ok(())
}
pub fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    serde_json::to_writer(&mut temp, value)?;
    temp.flush()?;
    temp.as_file().sync_all()?;
    temp.persist(path)?;
    Ok(())
}
pub struct Manager {
    pub root: PathBuf,
    pub launchers: PathBuf,
    pub resources: PathBuf,
    pub binary: PathBuf,
}
impl Manager {
    pub fn new(data: PathBuf, resources: PathBuf, binary: PathBuf) -> Result<Self> {
        let manager = Self {
            root: data.join("appshelf/apps"),
            launchers: data.join("applications"),
            resources,
            binary,
        };
        fs::create_dir_all(&manager.root)?;
        fs::create_dir_all(&manager.launchers)?;
        Ok(manager)
    }
    fn lock(&self) -> Result<File> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.root.parent().unwrap().join(".lock"))?;
        // SAFETY: file owns a valid descriptor for the lifetime of the lock guard.
        ensure!(
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0,
            "Unable to lock AppShelf registry"
        );
        Ok(file)
    }
    pub fn folder(&self, id: &str) -> Result<PathBuf> {
        ensure!(
            id.len() == 64
                && id
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
            "Invalid application ID"
        );
        let folder = self.root.join(id);
        ensure!(!folder.is_symlink(), "Managed directory is a symbolic link");
        Ok(folder)
    }
    pub fn desktop_path(&self, id: &str) -> Result<PathBuf> {
        self.folder(id)?;
        Ok(self.launchers.join(format!("{APP_ID}.app.{id}.desktop")))
    }
    pub fn record(&self, id: &str) -> Result<Record> {
        let path = self.folder(id)?.join("record.json");
        ensure!(
            !path.is_symlink(),
            "Registry was replaced by a symbolic link"
        );
        Ok(serde_json::from_reader(
            File::open(path).context("Application is not managed by AppShelf")?,
        )?)
    }
    pub fn inspect(&self, value: &str) -> Result<Value> {
        let path = local_path(value)?;
        ensure!(path.is_file(), "Choose an AppImage file");
        let (format, _) = runtime::filesystem(&path)?;
        let runtime = runtime::runtime_path(&self.resources)?;
        let (name, note) = match runtime::metadata(&runtime, &path) {
            Ok((name, _)) => (name, String::new()),
            Err(_) => (
                path.file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                "Filename used; embedded metadata could not be read.".into(),
            ),
        };
        Ok(
            json!({"path":path,"name":name,"size":path.metadata()?.len(),"format":format,"note":note}),
        )
    }
    pub fn list(&self) -> Vec<Value> {
        let mut apps = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                let id = entry.file_name().to_string_lossy().to_string();
                if let Ok(record) = self.record(&id) {
                    let folder = entry.path();
                    let path = folder.join("app.AppImage");
                    let icon = folder.join("icon.png");
                    let update_info = runtime::update_info(&path).ok().flatten();
                    apps.push(json!({"id":id,"name":record.name,"size":record.size,"installed":record.installed,"format":record.format,"path":path,"icon":if icon.is_file(){url::Url::from_file_path(icon).ok().map(|u|u.to_string()).unwrap_or_default()}else{String::new()},"missing":!path.is_file(),"environment":record.settings.environment,"isolation":record.settings.isolation,"unmanaged":false,"source":record.source,"source_modified":record.source_modified,"update_info":update_info}));
                }
            }
        }
        apps.sort_by_key(|a| a["name"].as_str().unwrap_or_default().to_lowercase());
        apps
    }
    pub fn install(&self, value: &str, settings: Settings) -> Result<Value> {
        validate_settings(&settings)?;
        let source = local_path(value)?;
        ensure!(source.is_file(), "Choose an AppImage file");
        let _lock = self.lock()?;
        let stage = tempfile::Builder::new()
            .prefix(".install-")
            .tempdir_in(&self.root)?;
        let target = stage.path().join("app.AppImage");
        fs::copy(&source, &target)?;
        let (format, _) = runtime::filesystem(&target)?;
        let runtime = runtime::runtime_path(&self.resources)?;
        let id = runtime::sha256(&target)?;
        let folder = self.folder(&id)?;
        ensure!(!folder.exists(), "This exact AppImage is already installed");
        let (name, icon) = runtime::metadata(&runtime, &target).unwrap_or_else(|_| {
            (
                source
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                None,
            )
        });
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755))?;
        let record = Record {
            name: name.clone(),
            size: target.metadata()?.len(),
            installed: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            format,
            settings,
            source: source.to_string_lossy().into_owned(),
            source_modified: modified(&source),
        };
        atomic_json(&stage.path().join("record.json"), &record)?;
        if let Some(bytes) = &icon {
            fs::write(stage.path().join("icon.png"), bytes)?;
        }
        let launcher = self.desktop_path(&id)?;
        ensure!(
            !launcher.exists() && !launcher.is_symlink(),
            "A launcher with this ID already exists; nothing was replaced"
        );
        fs::rename(stage.path(), &folder)?;
        if let Err(error) = self.write_launcher(&id, &name, icon.is_some()) {
            let _ = fs::remove_dir_all(&folder);
            return Err(error);
        }
        self.refresh_database();
        Ok(json!({"id":id,"name":name}))
    }
    pub fn write_launcher(&self, id: &str, name: &str, icon: bool) -> Result<()> {
        let folder = self.folder(id)?;
        let icon = if icon {
            folder.join("icon.png").to_string_lossy().into_owned()
        } else {
            "application-x-executable".into()
        };
        let body=format!("[Desktop Entry]\nType=Application\nVersion=1.0\nName={}\nExec={} --launch {id}\nIcon={}\nTerminal=false\nCategories=Utility;\nX-AppShelf-Managed=true\n",desktop_string(name),desktop_exec(&self.binary),desktop_string(&icon));
        // Publish complete content atomically without replacing another application's entry.
        let mut temp = tempfile::NamedTempFile::new_in(&self.launchers)?;
        temp.write_all(body.as_bytes())?;
        temp.as_file().sync_all()?;
        temp.persist_noclobber(self.desktop_path(id)?)?;
        Ok(())
    }
    pub fn configure(&self, id: &str, settings: Settings) -> Result<()> {
        validate_settings(&settings)?;
        let _lock = self.lock()?;
        let mut record = self.record(id)?;
        record.settings = settings;
        atomic_json(&self.folder(id)?.join("record.json"), &record)
    }
    pub fn uninstall(&self, id: &str) -> Result<()> {
        let _lock = self.lock()?;
        self.record(id)?;
        let folder = self.folder(id)?;
        let launcher = self.desktop_path(id)?;
        ensure!(
            !launcher.is_symlink(),
            "Launcher is a symbolic link; removal stopped"
        );
        if launcher.exists() {
            ensure!(
                fs::read_to_string(&launcher)?
                    .lines()
                    .any(|s| s == "X-AppShelf-Managed=true"),
                "Launcher was modified by another application; removal stopped"
            );
        }
        let tomb = tempfile::Builder::new()
            .prefix(".remove-")
            .tempdir_in(&self.root)?;
        let mut moved = Vec::new();
        let result = (|| -> Result<()> {
            for name in ["record.json", "app.AppImage", "icon.png"] {
                let path = folder.join(name);
                if path.exists() || path.is_symlink() {
                    ensure!(
                        !path.is_dir() || path.is_symlink(),
                        "A managed file was replaced by a directory"
                    );
                    fs::rename(&path, tomb.path().join(name))?;
                    moved.push(name);
                }
            }
            if launcher.exists() {
                fs::remove_file(&launcher)?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            for name in moved {
                fs::rename(tomb.path().join(name), folder.join(name))?;
            }
            return Err(error);
        }
        tomb.close()?;
        let _ = fs::remove_dir(folder);
        self.refresh_database();
        Ok(())
    }
    pub fn launch_spec(&self, id: &str) -> Result<(PathBuf, Environment)> {
        let folder = self.folder(id)?;
        let record = self.record(id)?;
        validate_settings(&record.settings)?;
        ensure!(
            folder.join("app.AppImage").is_file() && !folder.join("app.AppImage").is_symlink(),
            "Managed AppImage is missing or has been replaced"
        );
        let mut env: Environment = env::vars()
            .filter(|(k, _)| {
                ![
                    "APPIMAGE",
                    "TARGET_APPIMAGE",
                    "URUNTIME",
                    "RUNTIME_",
                    "RUNIMAGE",
                    "TARGET_RUNIMAGE",
                    "APPSHELF_",
                ]
                .iter()
                .any(|p| k.starts_with(p))
                    && !["NO_CLEANUP", "NO_UNMOUNT"].contains(&k.as_str())
            })
            .collect();
        env.extend(record.settings.environment);
        let private = self.root.parent().unwrap().join("data").join(id);
        if record.settings.isolation != Isolation::Off {
            let mut dirs = vec![
                ("XDG_CONFIG_HOME", "config"),
                ("XDG_DATA_HOME", "share"),
                ("XDG_CACHE_HOME", "cache"),
                ("XDG_STATE_HOME", "state"),
            ];
            if record.settings.isolation == Isolation::Home {
                dirs.push(("HOME", "home"));
            }
            for (key, suffix) in dirs {
                let dir = private.join(suffix);
                fs::create_dir_all(&dir)?;
                fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
                env.insert(key.into(), dir.to_string_lossy().into_owned());
            }
        }
        env.insert(
            "TARGET_APPIMAGE".into(),
            folder.join("app.AppImage").to_string_lossy().into_owned(),
        );
        env.insert("APPIMAGE_EXTRACT_AND_RUN".into(), "1".into());
        Ok((runtime::runtime_path(&self.resources)?, env))
    }
    pub fn launch(&self, id: &str) -> Result<()> {
        let (runtime, env) = self.launch_spec(id)?;
        let logs = self.root.parent().unwrap().join("logs");
        fs::create_dir_all(&logs)?;
        let path = logs.join(format!("{id}.log"));
        let log = File::create(&path)?;
        let mut child = Command::new(runtime)
            .arg("--appimage-extract-and-run")
            .env_clear()
            .envs(&env)
            .current_dir(env.get("HOME").map(PathBuf::from).unwrap_or_else(home))
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .process_group(0)
            .spawn()?;
        thread::sleep(Duration::from_millis(300));
        if let Some(status) = child.try_wait()? {
            ensure!(
                status.success(),
                "App exited with {status}. See {}",
                path.display()
            );
        } else {
            thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Ok(())
    }
    pub fn reveal(&self, id: &str, external: Option<&str>) -> Result<()> {
        let folder = if let Some(path) = external {
            local_path(path)?
                .parent()
                .context("No parent directory")?
                .to_path_buf()
        } else if id.is_empty() {
            self.root.clone()
        } else {
            self.folder(id)?
        };
        let mut command = if which("flea").is_some() {
            let mut c = Command::new("flea");
            c.arg("--gui");
            c
        } else {
            Command::new("xdg-open")
        };
        let mut child = command
            .arg(folder)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()?;
        thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
    pub fn refresh_database(&self) {
        let _ = Command::new("update-desktop-database")
            .arg(&self.launchers)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    pub fn check_update(&self, id: &str) -> Result<Value> {
        let folder = self.folder(id)?;
        let path = folder.join("app.AppImage");
        ensure!(path.is_file(), "Installed AppImage file not found");
        let res = crate::update::check_update(&path)?;
        Ok(serde_json::to_value(&res)?)
    }
    pub fn update(&self, id: &str) -> Result<Value> {
        let _lock = self.lock()?;
        crate::update::apply_update(self, id)
    }
}
pub fn which(name: &str) -> Option<PathBuf> {
    env::split_paths(&env::var_os("PATH").unwrap_or_default())
        .map(|p| p.join(name))
        .find(|p| p.is_file())
}
