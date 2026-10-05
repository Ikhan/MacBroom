//! Safety guard: refuses to delete anything that could damage the system.
//! Checks run on the *resolved* path (parent symlinks followed) and are
//! case-insensitive, matching APFS semantics.

use super::fsx::home;
use std::path::{Component, Path, PathBuf};

const SYSTEM_PREFIXES: [&str; 9] = [
    "/system", "/usr", "/bin", "/sbin", "/etc", "/var", "/private", "/library/apple", "/cores",
];

const ALLOWED_ROOTS: [&str; 5] = [
    "/applications", "/library", "/users/shared", "/private/var/folders", "/volumes",
];

/// Minimum component counts (incl. root) so container folders themselves
/// can never be removed, e.g. `/Volumes/Disk` or `$TMPDIR` itself.
const MIN_DEPTH: [(&str, usize); 4] = [
    ("/applications", 3),
    ("/volumes", 4),
    ("/private/var/folders", 8),
    ("/users/shared", 3),
];

/// Home-relative folders that must never be removed themselves.
const PROTECTED_HOME: [&str; 26] = [
    "", "Library", "Library/Caches", "Library/Logs", "Library/Application Support",
    "Library/Preferences", "Library/Containers", "Library/Group Containers",
    "Library/Mobile Documents", "Library/Keychains", "Library/Mail", "Library/Messages",
    "Library/Photos", "Library/CloudStorage", "Library/Safari", "Library/LaunchAgents",
    "Documents", "Desktop", "Downloads", "Movies", "Music", "Pictures", "Applications",
    ".Trash", ".ssh", ".gnupg",
];

/// Home folders whose *contents* are never deletable (secrets, mail, photos).
const PROTECTED_HOME_TREES: [&str; 5] = [
    "Library/Keychains", "Library/Mail", "Library/Messages", ".ssh", ".gnupg",
];

const PROTECTED_ABS: [&str; 7] = [
    "/", "/applications", "/library", "/users", "/volumes", "/library/caches", "/users/shared",
];

fn lower(p: &Path) -> PathBuf {
    PathBuf::from(p.to_string_lossy().to_lowercase())
}

/// Resolves symlinks in the parent (not the leaf, so a symlink itself can
/// still be removed without touching its target).
fn resolve(path: &Path) -> PathBuf {
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => std::fs::canonicalize(parent)
            .map(|p| p.join(name))
            .unwrap_or_else(|_| path.to_path_buf()),
        _ => path.to_path_buf(),
    }
}

/// Returns `Err(reason)` if `path` must not be deleted.
pub fn check_deletable(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err(format!("refusing relative path: {}", path.display()));
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(format!("refusing path with '..': {}", path.display()));
    }
    let real = lower(&resolve(path));
    let home = lower(&home());
    let deny = |why: &str| Err(format!("{why}: {}", path.display()));

    if home.components().count() < 3 {
        return deny("cannot determine a safe home folder");
    }
    let protected = PROTECTED_HOME.iter().map(|r| if r.is_empty() { home.clone() } else { lower(&home.join(r)) });
    if protected.chain(PROTECTED_ABS.iter().map(PathBuf::from)).any(|p| p == real) {
        return deny("protected folder");
    }
    if PROTECTED_HOME_TREES.iter().any(|r| real.starts_with(lower(&home.join(r)))) {
        return deny("protected data");
    }
    let in_temp = real.starts_with("/private/var/folders");
    if SYSTEM_PREFIXES.iter().any(|p| real.starts_with(p)) && !in_temp {
        return deny("system path");
    }
    let under_home = real.starts_with(&home);
    if !under_home && !ALLOWED_ROOTS.iter().any(|r| real.starts_with(r)) {
        return deny("outside allowed locations");
    }
    if MIN_DEPTH.iter().any(|(root, d)| real.starts_with(root) && real.components().count() < *d) {
        return deny("protected folder");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_system_and_roots() {
        for p in ["/", "/System/Library", "/usr/bin/ls", "/etc/hosts", "/Applications", "/Library", "/Users/Shared"] {
            assert!(check_deletable(Path::new(p)).is_err(), "{p} should be blocked");
        }
        assert!(check_deletable(&home()).is_err());
        assert!(check_deletable(&home().join("Library")).is_err());
        assert!(check_deletable(&home().join("Documents")).is_err());
    }

    #[test]
    fn blocks_case_variants_and_secrets() {
        assert!(check_deletable(&home().join("library")).is_err());
        assert!(check_deletable(&home().join("DOCUMENTS")).is_err());
        assert!(check_deletable(Path::new("/system/Library/x")).is_err());
        assert!(check_deletable(Path::new("/library/apple/x")).is_err());
        assert!(check_deletable(&home().join("Library/Keychains/login.keychain-db")).is_err());
        assert!(check_deletable(&home().join(".ssh/id_ed25519")).is_err());
    }

    #[test]
    fn blocks_volume_symlink_escape_and_roots() {
        assert!(check_deletable(Path::new("/Volumes/Macintosh HD/System/Library")).is_err());
        assert!(check_deletable(Path::new("/Volumes/Macintosh HD/usr/bin")).is_err());
        assert!(check_deletable(Path::new("/Volumes/SomeDisk")).is_err());
        assert!(check_deletable(Path::new("/private/var/folders/ab/xyz/T")).is_err());
    }

    #[test]
    fn blocks_relative_and_traversal() {
        assert!(check_deletable(Path::new("foo/bar")).is_err());
        assert!(check_deletable(&home().join("Library/Caches/../../Documents")).is_err());
    }

    #[test]
    fn allows_caches_and_apps() {
        assert!(check_deletable(&home().join("Library/Caches/com.example.app")).is_ok());
        assert!(check_deletable(Path::new("/Applications/Foo.app")).is_ok());
        assert!(check_deletable(Path::new("/private/var/folders/ab/xyz/T/tmp1")).is_ok());
        assert!(check_deletable(Path::new("/opt/homebrew/thing")).is_err());
    }
}
