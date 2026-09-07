//! Preferences the window sets and every entry point reads.
//!
//! Distinct from `manager::Settings`, which belongs to one application. These
//! are AppShelf's own, and they are deliberately few: a preference is only
//! worth keeping when leaving it out would make the shelf wrong for someone.

use crate::manager::{atomic_json, data_home};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    /// Whether the shelf looks through the filesystem at all. Off means no
    /// walk, no desktop entries read, nothing under "Found on your computer" —
    /// the shelf shows only what it manages and what pacman installed for it.
    #[serde(default = "yes")]
    pub scan: bool,
}
fn yes() -> bool {
    true
}
impl Default for Config {
    fn default() -> Self {
        Self { scan: yes() }
    }
}

pub fn path() -> PathBuf {
    data_home().join("appshelf/settings.json")
}
/// A missing or unreadable file is the defaults, never an error: a preference
/// no one has set yet is not a reason to refuse to open the shelf.
pub fn load() -> Config {
    fs::read(path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}
pub fn save(config: &Config) -> Result<()> {
    let target = path();
    fs::create_dir_all(target.parent().unwrap())?;
    atomic_json(&target, config)
}
