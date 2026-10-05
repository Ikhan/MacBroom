//! Browser traces (history, cookies, caches) and recent-items lists.
//! Nothing is pre-selected: the user opts in to each item.

use crate::model::Item;
use crate::util::fsx::{self, home_join};
use rayon::prelude::*;
use std::path::PathBuf;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

struct Chromium {
    name: &'static str,
    data: &'static str,
    cache: &'static str,
    process: &'static str,
}

const CHROMIUM: [Chromium; 5] = [
    Chromium { name: "Chrome", data: "Library/Application Support/Google/Chrome", cache: "Library/Caches/Google/Chrome", process: "Google Chrome" },
    Chromium { name: "Brave", data: "Library/Application Support/BraveSoftware/Brave-Browser", cache: "Library/Caches/BraveSoftware/Brave-Browser", process: "Brave Browser" },
    Chromium { name: "Edge", data: "Library/Application Support/Microsoft Edge", cache: "Library/Caches/Microsoft Edge", process: "Microsoft Edge" },
    Chromium { name: "Vivaldi", data: "Library/Application Support/Vivaldi", cache: "Library/Caches/Vivaldi", process: "Vivaldi" },
    Chromium { name: "Arc", data: "Library/Application Support/Arc/User Data", cache: "Library/Caches/Arc", process: "Arc" },
];

fn running_names() -> Vec<String> {
    let mut sys = System::new();
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    sys.processes().values().map(|p| p.name().to_string_lossy().into_owned()).collect()
}

fn existing(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.into_iter().filter(|p| p.exists()).collect()
}

fn trace(browser: &str, kind: &str, paths: Vec<PathBuf>, running: bool) -> Option<Item> {
    let paths = existing(paths);
    if paths.is_empty() {
        return None;
    }
    let size = paths.par_iter().map(|p| fsx::dir_size(p)).sum();
    let note = if running { " — quit the browser first" } else { "" };
    let count = paths.len();
    Some(
        Item::files("Privacy", format!("{browser}: {kind}"), paths, size)
            .with_detail(format!("{count} item(s){note}"))
            .with_selected(false),
    )
}

fn chromium_items(b: &Chromium, running: &[String]) -> Vec<Item> {
    let root = home_join(b.data);
    if !root.exists() {
        return Vec::new();
    }
    let is_running = running.iter().any(|n| n == b.process);
    let profiles: Vec<PathBuf> = fsx::children(&root)
        .into_iter()
        .filter(|p| {
            let n = fsx::name_of(p);
            n == "Default" || n.starts_with("Profile ")
        })
        .collect();
    let pick = |files: &[&str]| -> Vec<PathBuf> {
        profiles.iter().flat_map(|p| files.iter().map(move |f| p.join(f))).collect()
    };
    [
        trace(b.name, "Browsing history", pick(&["History", "History-journal", "Visited Links"]), is_running),
        trace(b.name, "Cookies", pick(&["Cookies", "Cookies-journal", "Network/Cookies", "Network/Cookies-journal"]), is_running),
        trace(b.name, "Cache", vec![home_join(b.cache)], is_running),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn safari_items(running: &[String]) -> Vec<Item> {
    let is_running = running.iter().any(|n| n == "Safari");
    [
        trace("Safari", "Browsing history", vec![
            home_join("Library/Safari/History.db"),
            home_join("Library/Safari/History.db-wal"),
            home_join("Library/Safari/History.db-shm"),
        ], is_running),
        trace("Safari", "Cookies", vec![
            home_join("Library/Containers/com.apple.Safari/Data/Library/Cookies/Cookies.binarycookies"),
        ], is_running),
        trace("Safari", "Cache", vec![
            home_join("Library/Caches/com.apple.Safari"),
            home_join("Library/Containers/com.apple.Safari/Data/Library/Caches"),
        ], is_running),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Firefox: cookies and cache only (places.sqlite also holds bookmarks).
fn firefox_items(running: &[String]) -> Vec<Item> {
    let is_running = running.iter().any(|n| n == "firefox");
    let profiles = fsx::children(&home_join("Library/Application Support/Firefox/Profiles"));
    let cookies = profiles.iter().map(|p| p.join("cookies.sqlite")).collect();
    [
        trace("Firefox", "Cookies", cookies, is_running),
        trace("Firefox", "Cache", vec![home_join("Library/Caches/Firefox")], is_running),
    ]
    .into_iter()
    .flatten()
    .collect()
}

fn recent_items() -> Option<Item> {
    let dir = home_join("Library/Application Support/com.apple.sharedfilelist");
    let files: Vec<PathBuf> = fsx::children(&dir)
        .into_iter()
        .flat_map(|p| if p.is_dir() { fsx::children(&p) } else { vec![p] })
        .filter(|p| p.extension().is_some_and(|e| e.to_string_lossy().starts_with("sfl")))
        .collect();
    trace("macOS", "Recent items lists", files, false)
}

pub fn scan_privacy() -> Vec<Item> {
    let running = running_names();
    let mut items: Vec<Item> = CHROMIUM.par_iter().flat_map(|b| chromium_items(b, &running)).collect();
    items.extend(safari_items(&running));
    items.extend(firefox_items(&running));
    items.extend(recent_items());
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_skips_missing_paths() {
        assert!(trace("X", "Y", vec![PathBuf::from("/nope/nope")], false).is_none());
    }

    #[test]
    fn trace_is_unselected_and_warns_when_running() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let item = trace("X", "Cache", vec![tmp.path().to_path_buf()], true).unwrap();
        assert!(!item.selected);
        assert!(item.detail.contains("quit the browser"));
    }

    #[test]
    fn live_privacy_scan_is_opt_in() {
        assert!(scan_privacy().iter().all(|i| !i.selected));
        assert!(!running_names().is_empty());
    }

    #[test]
    fn chromium_profile_discovery() {
        let b = &CHROMIUM[0];
        let items = chromium_items(b, &[b.process.to_string()]);
        assert!(items.iter().all(|i| i.label.starts_with(b.name)));
    }
}
