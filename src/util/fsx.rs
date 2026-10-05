//! Fast, parallel filesystem primitives with minimal allocations.

use rayon::prelude::*;
use std::fs::{self, Metadata};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

/// The current user's home directory, from the user database (not `$HOME`,
/// which can be unset or spoofed). Falls back to `$HOME`, then `/`.
pub fn home() -> PathBuf {
    passwd_home()
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn passwd_home() -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    // SAFETY: getpwuid returns a pointer to static storage or null; we copy
    // the C string out immediately and never retain the pointer.
    unsafe {
        let pw = libc::getpwuid(libc::getuid());
        if pw.is_null() || (*pw).pw_dir.is_null() {
            return None;
        }
        let dir = std::ffi::CStr::from_ptr((*pw).pw_dir);
        Some(PathBuf::from(std::ffi::OsStr::from_bytes(dir.to_bytes())))
    }
}

/// `~/<rel>` as an absolute path.
pub fn home_join(rel: &str) -> PathBuf {
    home().join(rel)
}

/// Bytes actually allocated on disk (handles sparse/compressed files).
pub fn alloc_size(meta: &Metadata) -> u64 {
    meta.blocks() * 512
}

/// Recursive on-disk size of `path`. Never follows symlinks or crosses devices.
pub fn dir_size(path: &Path) -> u64 {
    match fs::symlink_metadata(path) {
        Ok(meta) => size_of(path, &meta, meta.dev()),
        Err(_) => 0,
    }
}

fn size_of(path: &Path, meta: &Metadata, dev: u64) -> u64 {
    let own = alloc_size(meta);
    if !meta.is_dir() || meta.dev() != dev {
        return own;
    }
    let Ok(read) = fs::read_dir(path) else {
        return own;
    };
    let children: Vec<PathBuf> = read.filter_map(|e| e.ok().map(|e| e.path())).collect();
    own + children
        .par_iter()
        .map(|child| match fs::symlink_metadata(child) {
            Ok(m) => size_of(child, &m, dev),
            Err(_) => 0,
        })
        .sum::<u64>()
}

/// Immediate children of a directory (empty on error).
pub fn children(path: &Path) -> Vec<PathBuf> {
    fs::read_dir(path)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default()
}

/// File name as a lossy string ("" if none).
pub fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Options controlling [`walk_files`].
#[derive(Clone, Default)]
pub struct WalkOpts {
    pub skip_hidden: bool,
    /// Directories (absolute) never descended into.
    pub skip_dirs: Vec<PathBuf>,
    /// Directory extensions treated as opaque packages (e.g. `app`).
    pub skip_packages: bool,
    /// Directory names never descended into (e.g. `node_modules`).
    pub skip_names: &'static [&'static str],
}

const PACKAGE_EXTS: [&str; 8] = [
    "app", "photoslibrary", "musiclibrary", "tvlibrary", "fcpbundle",
    "imovielibrary", "photolibrary", "bundle",
];

fn is_package(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| PACKAGE_EXTS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Walks regular files under `root` in parallel, calling `visit` for each.
/// Stays on the root's device and never follows symlinks.
pub fn walk_files<F>(root: &Path, opts: &WalkOpts, visit: &F)
where
    F: Fn(&Path, &Metadata) + Sync,
{
    if let Ok(meta) = fs::symlink_metadata(root) {
        walk_inner(root, &meta, meta.dev(), opts, visit);
    }
}

fn walk_inner<F>(path: &Path, meta: &Metadata, dev: u64, opts: &WalkOpts, visit: &F)
where
    F: Fn(&Path, &Metadata) + Sync,
{
    if meta.is_file() {
        visit(path, meta);
        return;
    }
    if !meta.is_dir() || meta.dev() != dev || should_skip_dir(path, opts) {
        return;
    }
    children(path).par_iter().for_each(|child| {
        if opts.skip_hidden && name_of(child).starts_with('.') {
            return;
        }
        if let Ok(m) = fs::symlink_metadata(child) {
            walk_inner(child, &m, dev, opts, visit);
        }
    });
}

fn should_skip_dir(path: &Path, opts: &WalkOpts) -> bool {
    (opts.skip_packages && is_package(path))
        || opts.skip_dirs.iter().any(|d| d == path)
        || (!opts.skip_names.is_empty() && opts.skip_names.contains(&name_of(path).as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn dir_size_counts_nested_files() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("a/b")).unwrap();
        fs::write(tmp.path().join("a/b/f"), vec![1u8; 64 * 1024]).unwrap();
        fs::write(tmp.path().join("g"), vec![1u8; 64 * 1024]).unwrap();
        assert!(dir_size(tmp.path()) >= 128 * 1024);
        assert_eq!(dir_size(&tmp.path().join("missing")), 0);
    }

    #[test]
    fn walk_respects_hidden_and_packages() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".hidden")).unwrap();
        fs::create_dir_all(root.join("X.app/Contents")).unwrap();
        fs::write(root.join(".hidden/a"), b"a").unwrap();
        fs::write(root.join("X.app/Contents/b"), b"b").unwrap();
        fs::write(root.join("c"), b"c").unwrap();
        let seen = Mutex::new(Vec::new());
        let opts = WalkOpts { skip_hidden: true, skip_packages: true, ..Default::default() };
        walk_files(root, &opts, &|p, _| seen.lock().unwrap().push(name_of(p)));
        assert_eq!(seen.into_inner().unwrap(), vec!["c".to_string()]);
    }

    #[test]
    fn walk_skips_named_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("p/node_modules")).unwrap();
        fs::write(tmp.path().join("p/node_modules/a"), b"a").unwrap();
        fs::write(tmp.path().join("p/b"), b"b").unwrap();
        let count = Mutex::new(0);
        let opts = WalkOpts { skip_names: &["node_modules"], ..Default::default() };
        walk_files(tmp.path(), &opts, &|_, _| *count.lock().unwrap() += 1);
        assert_eq!(*count.lock().unwrap(), 1);
    }

    #[test]
    fn walk_skips_listed_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("skip")).unwrap();
        fs::write(tmp.path().join("skip/a"), b"a").unwrap();
        let count = Mutex::new(0);
        let opts = WalkOpts { skip_dirs: vec![tmp.path().join("skip")], ..Default::default() };
        walk_files(tmp.path(), &opts, &|_, _| *count.lock().unwrap() += 1);
        assert_eq!(*count.lock().unwrap(), 0);
    }
}
