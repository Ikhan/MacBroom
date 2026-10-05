//! Duplicate finder: size bucket → head/tail hash → full streaming hash.
//! Only candidate paths are kept in memory; hashing is parallel.

use crate::model::Item;
use crate::util::fmt::{bytes, tilde};
use crate::util::fsx;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;
use xxhash_rust::xxh3::Xxh3;

pub const DEFAULT_MIN_SIZE: u64 = 1_000_000;
const SAMPLE: u64 = 16 * 1024;
const BUF: usize = 256 * 1024;

#[derive(Clone)]
struct Candidate {
    path: PathBuf,
    len: u64,
    modified: SystemTime,
}

/// Hash of the first and last 16 KB: cheap filter before a full read.
fn sample_hash(path: &Path, len: u64) -> Option<u64> {
    let mut f = File::open(path).ok()?;
    let mut h = Xxh3::new();
    let mut buf = vec![0u8; SAMPLE as usize];
    let n = f.read(&mut buf).ok()?;
    h.update(&buf[..n]);
    if len > SAMPLE * 2 {
        f.seek(SeekFrom::End(-(SAMPLE as i64))).ok()?;
        let n = f.read(&mut buf).ok()?;
        h.update(&buf[..n]);
    }
    Some(h.digest())
}

fn full_hash(path: &Path) -> Option<u128> {
    let mut f = File::open(path).ok()?;
    let mut h = Xxh3::new();
    let mut buf = vec![0u8; BUF];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            return Some(h.digest128());
        }
        h.update(&buf[..n]);
    }
}

fn collect(root: &Path, min_size: u64) -> HashMap<u64, Vec<Candidate>> {
    let seen_inodes: Mutex<HashSet<(u64, u64)>> = Mutex::new(HashSet::new());
    let by_size: Mutex<HashMap<u64, Vec<Candidate>>> = Mutex::new(HashMap::new());
    fsx::walk_files(root, &super::large_old::user_walk_opts(), &|path, meta| {
        let len = meta.len();
        if len < min_size || !seen_inodes.lock().expect("lock").insert((meta.dev(), meta.ino())) {
            return;
        }
        let c = Candidate {
            path: path.to_path_buf(),
            len,
            modified: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
        };
        by_size.lock().expect("lock").entry(len).or_default().push(c);
    });
    by_size.into_inner().expect("lock")
}

/// Splits each group by `key`, keeping only sub-groups with 2+ members.
fn refine<K, F>(groups: Vec<Vec<Candidate>>, key: F) -> Vec<Vec<Candidate>>
where
    K: std::hash::Hash + Eq + Send,
    F: Fn(&Candidate) -> Option<K> + Sync,
{
    groups
        .into_par_iter()
        .flat_map(|group| {
            let keyed: Vec<(K, Candidate)> =
                group.into_par_iter().filter_map(|c| key(&c).map(|k| (k, c))).collect();
            let mut buckets: HashMap<K, Vec<Candidate>> = HashMap::new();
            for (k, c) in keyed {
                buckets.entry(k).or_default().push(c);
            }
            buckets.into_values().filter(|g| g.len() > 1).collect::<Vec<_>>()
        })
        .collect()
}

/// Returns duplicate groups, each sorted oldest-first (the one to keep).
pub fn find_groups(root: &Path, min_size: u64) -> Vec<Vec<PathBuf>> {
    let size_groups: Vec<Vec<Candidate>> =
        collect(root, min_size).into_values().filter(|g| g.len() > 1).collect();
    let sampled = refine(size_groups, |c| sample_hash(&c.path, c.len));
    let mut exact = refine(sampled, |c| full_hash(&c.path));
    exact.sort_by_key(|g| std::cmp::Reverse(g[0].len * (g.len() as u64 - 1)));
    exact
        .into_iter()
        .map(|mut g| {
            g.sort_by_key(|c| (c.modified, c.path.as_os_str().len()));
            g.into_iter().map(|c| c.path).collect()
        })
        .collect()
}

pub fn scan(root: &Path, min_size: u64) -> Vec<Item> {
    find_groups(root, min_size)
        .into_iter()
        .enumerate()
        .flat_map(|(gi, group)| {
            let size = std::fs::metadata(&group[0]).map(|m| m.len()).unwrap_or(0);
            group.into_iter().enumerate().map(move |(i, path)| {
                let role = if i == 0 { "original (kept)" } else { "duplicate" };
                let detail = format!("set #{} · {} · {role} · {}", gi + 1, bytes(size), tilde(&path));
                let item = Item::files("Duplicates", fsx::name_of(&path), vec![path], size).with_detail(detail);
                // The kept copy is informational so no selection mode can remove every copy.
                if i == 0 { Item { target: crate::model::Target::Info, selected: false, ..item } } else { item }
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_exact_duplicates_only() {
        let tmp = tempfile::tempdir().unwrap();
        let data = vec![7u8; 100_000];
        let mut other = data.clone();
        other[50_000] = 8; // same size, same head/tail, different middle
        std::fs::write(tmp.path().join("a"), &data).unwrap();
        std::fs::write(tmp.path().join("b"), &data).unwrap();
        std::fs::write(tmp.path().join("c"), &other).unwrap();
        let groups = find_groups(tmp.path(), 1);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].len(), 2);
        let items = scan(tmp.path(), 1);
        assert_eq!(items.iter().filter(|i| i.selected).count(), 1);
        assert_eq!(items.iter().filter(|i| !i.is_actionable()).count(), 1, "original must be kept");
    }

    #[test]
    fn ignores_hard_links() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a"), vec![1u8; 5000]).unwrap();
        std::fs::hard_link(tmp.path().join("a"), tmp.path().join("b")).unwrap();
        assert!(find_groups(tmp.path(), 1).is_empty());
    }

    #[test]
    fn respects_min_size() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a"), b"same").unwrap();
        std::fs::write(tmp.path().join("b"), b"same").unwrap();
        assert!(find_groups(tmp.path(), 1000).is_empty());
    }
}
