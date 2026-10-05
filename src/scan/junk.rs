//! System junk, trash bins and mail attachments: regenerable data that is
//! safe to remove.

use super::{children_items, paths_items};
use crate::model::Item;
use crate::util::fsx::{self, home_join};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Caches that are protected by macOS or actively needed; never offered.
/// Caches that are protected by macOS or hold data users would lose
/// (e.g. Spotify offline downloads); never offered.
const CACHE_DENYLIST: [&str; 5] = ["CloudKit", "FamilyCircle", "com.apple.HomeKit", "com.apple.containermanagerd", "com.spotify.client"];

fn cache_ok(p: &Path) -> bool {
    let name = fsx::name_of(p);
    !name.starts_with("com.apple.") && !CACHE_DENYLIST.contains(&name.as_str())
}

fn older_than(p: &Path, days: u64) -> bool {
    let cutoff = SystemTime::now() - Duration::from_secs(days * 86_400);
    newest_mtime(p, 3).is_some_and(|t| t < cutoff)
}

/// Newest modification time in a tree (bounded depth), so a folder with a
/// recently touched file inside is never considered stale.
fn newest_mtime(p: &Path, depth: u32) -> Option<SystemTime> {
    let meta = std::fs::symlink_metadata(p).ok()?;
    let own = meta.modified().ok()?;
    if !meta.is_dir() || depth == 0 {
        return Some(own);
    }
    Some(fsx::children(p).iter().filter_map(|c| newest_mtime(c, depth - 1)).fold(own, SystemTime::max))
}

fn has_ext(p: &Path, exts: &[&str]) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| exts.contains(&e.to_ascii_lowercase().as_str()))
}

/// Developer tool caches: all regenerable.
fn developer_caches() -> Vec<PathBuf> {
    [
        "Library/Developer/Xcode/DerivedData",
        "Library/Developer/Xcode/iOS DeviceSupport",
        "Library/Developer/CoreSimulator/Caches",
        ".npm/_cacache",
        "Library/pnpm/store",
        ".cargo/registry/cache",
        ".gradle/caches",
        ".cache",
    ]
    .iter()
    .map(|r| home_join(r))
    .filter(|p| p.exists())
    .collect()
}

fn temp_dir() -> Option<PathBuf> {
    std::env::var_os("TMPDIR").and_then(|t| std::fs::canonicalize(t).ok())
}

/// All system-junk categories.
pub fn scan_junk() -> Vec<Item> {
    let tasks: Vec<Box<dyn Fn() -> Vec<Item> + Send + Sync>> = vec![
        Box::new(|| children_items("User Cache", &home_join("Library/Caches"), cache_ok)),
        Box::new(|| children_items("User Logs", &home_join("Library/Logs"), |_| true)),
        Box::new(|| children_items("Saved App State", &home_join("Library/Saved Application State"), |_| true)),
        Box::new(|| {
            // ~/.cache can hold ML models and other non-regenerable data.
            paths_items("Developer Junk", developer_caches())
                .into_iter()
                .map(|i| { let keep = i.label != ".cache"; i.with_selected(keep) })
                .collect()
        }),
        Box::new(|| {
            temp_dir()
                .map(|t| children_items("Temporary Files", &t, |p| cache_ok(p) && older_than(p, 3)))
                .unwrap_or_default()
        }),
        Box::new(|| {
            // Only stale ones: a fresh .part may be a download in progress.
            children_items("Broken Downloads", &home_join("Downloads"), |p| {
                has_ext(p, &["download", "crdownload", "part", "partial"]) && older_than(p, 1)
            })
        }),
        Box::new(|| {
            children_items("Old Installers", &home_join("Downloads"), |p| has_ext(p, &["dmg", "pkg", "mpkg"]))
                .into_iter()
                .map(|i| i.with_selected(false))
                .collect()
        }),
        Box::new(|| {
            children_items("Xcode Archives", &home_join("Library/Developer/Xcode/Archives"), |_| true)
                .into_iter()
                .map(|i| i.with_selected(false))
                .collect()
        }),
        Box::new(|| {
            children_items("iOS Backups", &home_join("Library/Application Support/MobileSync/Backup"), |_| true)
                .into_iter()
                .map(|i| i.with_selected(false))
                .collect()
        }),
    ];
    tasks.par_iter().flat_map(|t| t()).collect()
}

/// User trash plus trashes on external volumes. Emptied permanently.
pub fn scan_trash() -> Vec<Item> {
    // SAFETY: getuid has no preconditions.
    let uid = unsafe { libc::getuid() };
    let mut items = children_items("Trash", &home_join(".Trash"), |_| true);
    for vol in fsx::children(Path::new("/Volumes")) {
        let t = vol.join(".Trashes").join(uid.to_string());
        items.extend(children_items("Volume Trash", &t, |_| true));
    }
    items.into_iter().map(Item::permanent).collect()
}

/// Trash for Smart Care: listed but not pre-selected (emptying is permanent).
pub fn scan_trash_unselected() -> Vec<Item> {
    scan_trash().into_iter().map(|i| i.with_selected(false)).collect()
}

/// Attachments Mail.app has downloaded. Not pre-selected: for POP/local
/// mailboxes these may be the only copy.
pub fn scan_mail() -> Vec<Item> {
    let dirs = [
        "Library/Containers/com.apple.mail/Data/Library/Mail Downloads",
        "Library/Mail Downloads",
    ];
    dirs.iter()
        .flat_map(|d| children_items("Mail Attachments", &home_join(d), |_| true))
        .map(|i| i.with_selected(false))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_filter_skips_apple() {
        assert!(!cache_ok(Path::new("/x/com.apple.Safari")));
        assert!(!cache_ok(Path::new("/x/CloudKit")));
        assert!(!cache_ok(Path::new("/x/com.spotify.client")), "offline music lives here");
        assert!(cache_ok(Path::new("/x/com.example.app")));
    }

    #[test]
    fn ext_matching_is_case_insensitive() {
        assert!(has_ext(Path::new("/a/b.DMG"), &["dmg"]));
        assert!(!has_ext(Path::new("/a/b"), &["dmg"]));
    }

    #[test]
    fn age_filter() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        assert!(!older_than(tmp.path(), 3));
    }

    #[test]
    fn live_scans_are_safe_and_well_formed() {
        for item in scan_junk().into_iter().chain(scan_mail()) {
            if let crate::model::Target::Files { paths, .. } = &item.target {
                assert!(paths.iter().all(|p| crate::util::guard::check_deletable(p).is_ok()), "{paths:?}");
            }
        }
        assert!(scan_trash().iter().all(|i| matches!(i.target, crate::model::Target::Files { permanent: true, .. })));
        assert!(temp_dir().is_some_and(|t| t.is_absolute()));
        assert!(developer_caches().iter().all(|p| p.exists()));
    }
}
