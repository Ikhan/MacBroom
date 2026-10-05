//! Large & old files finder over the home folder.

use crate::model::Item;
use crate::util::fmt::{days_since, tilde};
use crate::util::fsx::{self, WalkOpts, alloc_size};
use std::path::Path;
use std::sync::Mutex;
use std::time::SystemTime;

pub const DEFAULT_MIN_SIZE: u64 = 100_000_000;
pub const MAX_RESULTS: usize = 500;

#[derive(Clone)]
pub struct Query {
    pub min_size: u64,
    pub older_than_days: u64,
}

impl Default for Query {
    fn default() -> Self {
        Self { min_size: DEFAULT_MIN_SIZE, older_than_days: 0 }
    }
}

/// Walk options used for user-data scans (skips ~/Library and hidden dirs).
pub fn user_walk_opts() -> WalkOpts {
    WalkOpts {
        skip_hidden: true,
        skip_packages: true,
        skip_dirs: vec![fsx::home_join("Library")],
        skip_names: &["node_modules", ".git", "Pods", "DerivedData"],
    }
}

fn last_used(meta: &std::fs::Metadata) -> SystemTime {
    let m = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let a = meta.accessed().unwrap_or(SystemTime::UNIX_EPOCH);
    m.max(a)
}

pub fn scan(root: &Path, q: &Query) -> Vec<Item> {
    let found: Mutex<Vec<(std::path::PathBuf, u64, u64)>> = Mutex::new(Vec::new());
    fsx::walk_files(root, &user_walk_opts(), &|path, meta| {
        let size = alloc_size(meta);
        if size < q.min_size {
            return;
        }
        let age = days_since(last_used(meta));
        if age >= q.older_than_days {
            found.lock().expect("lock poisoned").push((path.to_path_buf(), size, age));
        }
    });
    let mut found = found.into_inner().expect("lock poisoned");
    found.sort_by(|a, b| b.1.cmp(&a.1));
    found.truncate(MAX_RESULTS);
    found
        .into_iter()
        .map(|(path, size, age)| {
            let detail = format!("used {age}d ago · {}", tilde(&path));
            Item::files("Large & Old", fsx::name_of(&path), vec![path], size)
                .with_detail(detail)
                .with_selected(false)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_files_over_threshold() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("big"), vec![1u8; 200_000]).unwrap();
        std::fs::write(tmp.path().join("small"), vec![1u8; 10]).unwrap();
        let items = scan(tmp.path(), &Query { min_size: 100_000, older_than_days: 0 });
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "big");
        assert!(!items[0].selected);
    }

    #[test]
    fn age_filter_excludes_new_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("big"), vec![1u8; 200_000]).unwrap();
        let items = scan(tmp.path(), &Query { min_size: 1, older_than_days: 30 });
        assert!(items.is_empty());
    }
}
