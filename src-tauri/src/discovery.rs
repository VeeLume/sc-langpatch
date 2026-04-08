use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::{Result, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};
use specta::Type;

/// A discovered Star Citizen installation.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Installation {
    pub channel: String,
    pub path: String,
}

/// Discover all Star Citizen installations by parsing the RSI Launcher log.
///
/// Returns one entry per channel (deduplicated, last-seen path wins).
/// Only includes directories that still exist and contain Data.p4k.
pub fn find_installations() -> Result<Vec<Installation>> {
    let log_path = launcher_log_path();

    if !log_path.exists() {
        bail!(
            "RSI Launcher log not found at {}\n\
             Make sure you have launched Star Citizen at least once.",
            log_path.display()
        );
    }

    let entries = parse_launcher_log(&log_path);

    if entries.is_empty() {
        bail!(
            "No Star Citizen launches found in launcher log.\n\
             Make sure you have launched the game at least once."
        );
    }

    // Deduplicate by channel — last-seen path wins
    let mut by_channel: HashMap<String, PathBuf> = HashMap::new();
    for (channel, path) in entries {
        by_channel.insert(channel.to_uppercase(), path);
    }

    let mut installations: Vec<Installation> = by_channel
        .into_iter()
        .filter(|(channel, path)| {
            if !path.exists() {
                eprintln!("Skipping {channel}: directory not found at {}", path.display());
                return false;
            }
            if !path.join("Data.p4k").exists() {
                eprintln!("Skipping {channel}: no Data.p4k at {}", path.display());
                return false;
            }
            true
        })
        .map(|(channel, path)| Installation {
            channel,
            path: path.to_string_lossy().into_owned(),
        })
        .collect();

    installations.sort_by(|a, b| a.channel.cmp(&b.channel));

    if installations.is_empty() {
        bail!("No valid Star Citizen installations found.");
    }

    Ok(installations)
}

/// The output directory where the patched global.ini should be written.
pub fn output_dir(install_path: &Path) -> PathBuf {
    install_path
        .join("data")
        .join("Localization")
        .join("english")
}

fn launcher_log_path() -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_default();
    PathBuf::from(appdata).join("rsilauncher/logs/log.log")
}

fn parse_launcher_log(log_path: &Path) -> Vec<(String, PathBuf)> {
    let content = match std::fs::read_to_string(log_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    content
        .lines()
        .filter_map(extract_launch_entry)
        .collect()
}

static LAUNCH_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"Launching Star Citizen (\S+) from \((.+?)\)").unwrap()
});

fn extract_launch_entry(line: &str) -> Option<(String, PathBuf)> {
    let caps = LAUNCH_RE.captures(line)?;
    let channel = caps[1].to_string();
    let path_str = caps[2].replace("\\\\", "\\");
    Some((channel, PathBuf::from(path_str)))
}
