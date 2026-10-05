//! Locates files apps leave behind in Library folders, and orphaned
//! leftovers from apps that are no longer installed.

use super::installed_apps;
use crate::model::Item;
use crate::util::{cmd, fsx};
use rayon::prelude::*;
use std::collections::HashSet;
use std::path::PathBuf;

/// Library sub-folders where apps store data. `true` = also match by app name.
const USER_DIRS: [(&str, bool); 13] = [
    ("Library/Application Support", true),
    ("Library/Caches", true),
    ("Library/Preferences", false),
    ("Library/Preferences/ByHost", false),
    ("Library/Containers", false),
    ("Library/Group Containers", false),
    ("Library/Saved Application State", false),
    ("Library/Logs", true),
    ("Library/HTTPStorages", false),
    ("Library/WebKit", false),
    ("Library/Cookies", false),
    ("Library/LaunchAgents", false),
    ("Library/Application Scripts", false),
];

const SYSTEM_DIRS: [(&str, bool); 5] = [
    ("/Library/Application Support", true),
    ("/Library/Caches", false),
    ("/Library/LaunchAgents", false),
    ("/Library/LaunchDaemons", false),
    ("/Library/PrivilegedHelperTools", false),
];

/// Strips known suffixes so `com.foo.bar.plist` matches `com.foo.bar`.
fn stem(name: &str) -> &str {
    [".plist", ".savedState", ".binarycookies"]
        .iter()
        .find_map(|s| name.strip_suffix(s))
        .unwrap_or(name)
}

/// Pure matching rule (tested): exact bundle id, bundle-id prefix with a
/// dot, group-container suffix, or exact app name where allowed.
pub fn is_match(entry: &str, bundle_id: &str, app_name: &str, by_name: bool) -> bool {
    let e = stem(entry).to_ascii_lowercase();
    let bid = bundle_id.to_ascii_lowercase();
    if e == bid || e.starts_with(&format!("{bid}.")) || e.ends_with(&format!(".{bid}")) {
        return true;
    }
    by_name && app_name.len() >= 4 && e == app_name.to_ascii_lowercase()
}

/// One-time listing of all Library folders, shared across apps.
pub struct LibraryIndex {
    entries: Vec<(PathBuf, String, bool)>,
}

impl LibraryIndex {
    pub fn build() -> Self {
        let mut dirs: Vec<(PathBuf, bool)> = USER_DIRS.iter().map(|(d, n)| (fsx::home_join(d), *n)).collect();
        dirs.extend(SYSTEM_DIRS.iter().map(|(d, n)| (PathBuf::from(d), *n)));
        let entries = dirs
            .into_iter()
            .flat_map(|(dir, by_name)| {
                fsx::children(&dir).into_iter().map(move |p| {
                    let name = fsx::name_of(&p);
                    (p, name, by_name)
                })
            })
            .collect();
        Self { entries }
    }

    /// Leftovers for one app. `others` are the bundle ids of all other
    /// installed apps: entries that belong to a more specific installed app
    /// (e.g. `com.google.Chrome.canary` when removing Chrome) are excluded.
    pub fn leftovers_for(&self, bundle_id: &str, app_name: &str, others: &[String]) -> Vec<PathBuf> {
        let bid = bundle_id.to_ascii_lowercase();
        self.entries
            .iter()
            .filter(|(_, name, by_name)| is_match(name, bundle_id, app_name, *by_name))
            .filter(|(_, name, _)| !claimed_by_other(name, &bid, others))
            .map(|(p, _, _)| p.clone())
            .collect()
    }

    fn bundle_like(&self) -> impl Iterator<Item = &(PathBuf, String, bool)> {
        self.entries.iter().filter(|(p, name, _)| {
            let s = stem(name);
            looks_like_bundle_id(s) && p.starts_with(fsx::home())
        })
    }
}

/// True if another installed app with a longer (more specific) bundle id owns the entry.
fn claimed_by_other(entry: &str, bid: &str, others: &[String]) -> bool {
    let e = stem(entry).to_ascii_lowercase();
    others.iter().any(|o| {
        o.len() > bid.len() && o != bid && (e == *o || e.starts_with(&format!("{o}.")) || e.ends_with(&format!(".{o}")))
    })
}

/// Reverse-DNS names like `com.vendor.app` (not Apple's own).
pub fn looks_like_bundle_id(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    let tld = ["com", "org", "net", "io", "co", "de", "app", "dev", "me"];
    parts.len() >= 3
        && tld.contains(&parts[0].to_ascii_lowercase().as_str())
        && !s.to_ascii_lowercase().starts_with("com.apple.")
        && parts.iter().all(|p| !p.is_empty())
}

/// Bundle id of any app on disk (incl. helpers/system apps) via Spotlight.
fn spotlight_has_app(bundle_id: &str) -> bool {
    let q = format!("kMDItemCFBundleIdentifier == '{}'", bundle_id.replace('\'', ""));
    cmd::run("/usr/bin/mdfind", &[&q]).is_ok_and(|out| !out.trim().is_empty())
}

fn owner_installed(name: &str, installed: &HashSet<String>) -> bool {
    let n = stem(name).to_ascii_lowercase();
    installed.iter().any(|bid| n == *bid || n.starts_with(&format!("{bid}.")) || n.ends_with(&format!(".{bid}")))
}

/// Orphaned leftovers: bundle-id-named data whose app is gone.
pub fn scan_orphans() -> Vec<Item> {
    let index = LibraryIndex::build();
    let installed: HashSet<String> = installed_apps().into_iter().map(|a| a.bundle_id.to_ascii_lowercase()).collect();
    let candidates: Vec<&(PathBuf, String, bool)> =
        index.bundle_like().filter(|(_, n, _)| !owner_installed(n, &installed)).collect();
    let mut items: Vec<Item> = candidates
        .into_par_iter()
        .filter(|(_, name, _)| !spotlight_has_app(stem(name)))
        .map(|(path, name, _)| {
            let size = fsx::dir_size(path);
            Item::files("Leftovers", stem(name).to_string(), vec![path.clone()], size)
                .with_detail(format!("app not found · {}", crate::util::fmt::tilde(path)))
                .with_selected(false)
        })
        .collect();
    items.sort_by(|a, b| b.size.cmp(&a.size));
    items
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_bundle_id_variants() {
        let bid = "com.spotify.client";
        assert!(is_match("com.spotify.client", bid, "Spotify", false));
        assert!(is_match("com.spotify.client.plist", bid, "Spotify", false));
        assert!(is_match("com.spotify.client.helper", bid, "Spotify", false));
        assert!(is_match("ABCDE12345.com.spotify.client", bid, "Spotify", false));
        assert!(is_match("Spotify", bid, "Spotify", true));
        assert!(!is_match("Spotify", bid, "Spotify", false));
        assert!(!is_match("com.spotify.clientx", bid, "Spotify", false));
    }

    #[test]
    fn sibling_apps_keep_their_data() {
        let others = vec!["com.google.chrome.canary".to_string()];
        assert!(claimed_by_other("com.google.Chrome.canary.plist", "com.google.chrome", &others));
        assert!(!claimed_by_other("com.google.Chrome.plist", "com.google.chrome", &others));
    }

    #[test]
    fn short_names_never_match_by_name() {
        assert!(!is_match("Zip", "com.x.zip", "Zip", true));
    }

    #[test]
    fn bundle_id_heuristic() {
        assert!(looks_like_bundle_id("com.vendor.app"));
        assert!(!looks_like_bundle_id("com.apple.finder"));
        assert!(!looks_like_bundle_id("Google"));
        assert!(!looks_like_bundle_id("com..x"));
    }

    #[test]
    fn owner_detection() {
        let installed: HashSet<String> = ["com.foo.bar".to_string()].into();
        assert!(owner_installed("com.foo.bar.helper.plist", &installed));
        assert!(!owner_installed("com.gone.app", &installed));
    }

    #[test]
    fn index_and_live_orphans() {
        let index = LibraryIndex::build();
        assert!(index.leftovers_for("com.definitely.not.installed.xyz", "Nope", &[]).is_empty());
        assert!(scan_orphans().iter().all(|i| !i.selected && !i.label.starts_with("com.apple.")));
        assert!(!spotlight_has_app("com.definitely.not.installed.xyz"));
    }
}
