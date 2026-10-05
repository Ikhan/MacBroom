pub mod extensions;
pub mod leftovers;
pub mod updates;

use crate::model::Item;
use crate::util::fmt::bytes;
use crate::util::fsx;
use leftovers::LibraryIndex;
use rayon::prelude::*;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct App {
    pub path: PathBuf,
    pub name: String,
    pub bundle_id: String,
    pub version: String,
}

/// Apps macOS protects with SIP; never offered for removal.
const PROTECTED: [&str; 2] = ["com.apple.Safari", "com.apple.systempreferences"];

pub fn read_app(path: &Path) -> Option<App> {
    let info = plist::Value::from_file(path.join("Contents/Info.plist")).ok()?;
    let d = info.as_dictionary()?;
    let get = |k: &str| d.get(k).and_then(|v| v.as_string()).map(str::to_string);
    let bundle_id = get("CFBundleIdentifier")?;
    let fallback = fsx::name_of(path).trim_end_matches(".app").to_string();
    Some(App {
        path: path.to_path_buf(),
        name: get("CFBundleDisplayName").or_else(|| get("CFBundleName")).unwrap_or(fallback),
        bundle_id,
        version: get("CFBundleShortVersionString").unwrap_or_default(),
    })
}

fn is_app(p: &Path) -> bool {
    p.extension().is_some_and(|e| e == "app")
}

/// `.app` bundles in a folder, plus one level of sub-folders (e.g. "Adobe X/").
pub fn apps_in(dir: &Path) -> Vec<PathBuf> {
    fsx::children(dir)
        .into_iter()
        .flat_map(|p| {
            if is_app(&p) {
                vec![p]
            } else if p.is_dir() {
                fsx::children(&p).into_iter().filter(|c| is_app(c)).collect()
            } else {
                vec![]
            }
        })
        .collect()
}

pub fn installed_apps() -> Vec<App> {
    let mut paths = apps_in(Path::new("/Applications"));
    paths.extend(apps_in(&fsx::home_join("Applications")));
    paths
        .par_iter()
        .filter_map(|p| read_app(p))
        .filter(|a| !PROTECTED.contains(&a.bundle_id.as_str()))
        .collect()
}

/// Uninstaller: each app with all of its leftovers in one removable item.
pub fn scan() -> Vec<Item> {
    let index = LibraryIndex::build();
    let apps = installed_apps();
    let bids: Vec<String> = apps.iter().map(|a| a.bundle_id.to_ascii_lowercase()).collect();
    let mut items: Vec<Item> = apps
        .into_par_iter()
        .map(|app| {
            let leftovers = index.leftovers_for(&app.bundle_id, &app.name, &bids);
            let app_size = fsx::dir_size(&app.path);
            let left_size: u64 = leftovers.iter().map(|p| fsx::dir_size(p)).sum();
            let detail = format!(
                "{} · {} · app {} + {} leftovers ({})",
                app.version, app.bundle_id, bytes(app_size), leftovers.len(), bytes(left_size)
            );
            let mut paths = vec![app.path.clone()];
            paths.extend(leftovers);
            Item::files("Applications", app.name, paths, app_size + left_size)
                .with_detail(detail)
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
    fn reads_app_bundle() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path().join("Foo.app");
        std::fs::create_dir_all(app.join("Contents")).unwrap();
        std::fs::write(app.join("Contents/Info.plist"), r#"<?xml version="1.0"?><plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.example.foo</string>
<key>CFBundleShortVersionString</key><string>1.2</string></dict></plist>"#).unwrap();
        let a = read_app(&app).unwrap();
        assert_eq!(a.bundle_id, "com.example.foo");
        assert_eq!(a.name, "Foo");
        assert_eq!(a.version, "1.2");
        assert_eq!(apps_in(tmp.path()), vec![app]);
    }

    #[test]
    fn live_app_scan() {
        let apps = installed_apps();
        assert!(apps.iter().all(|a| !PROTECTED.contains(&a.bundle_id.as_str())));
        let items = scan();
        assert_eq!(items.len(), apps.len());
        assert!(items.iter().all(|i| !i.selected));
    }
}
