//! Secure shredder: overwrite, rename, then unlink.
//! NOTE: on SSDs/APFS, wear-levelling and copy-on-write mean overwriting
//! cannot guarantee old blocks are erased. FileVault is the real defence.

use crate::util::{fsx, guard};
use anyhow::{Context, Result, bail};
use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const CHUNK: usize = 1 << 20;

fn overwrite(path: &Path) -> Result<()> {
    let len = fs::metadata(path)?.len();
    let mut f = OpenOptions::new().write(true).open(path)?;
    let mut buf = vec![0u8; CHUNK];
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15 ^ len;
    for pass in 0..2 {
        f.seek(SeekFrom::Start(0))?;
        let mut left = len;
        while left > 0 {
            if pass == 1 {
                for b in buf.iter_mut() {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    *b = seed as u8;
                }
            }
            let n = left.min(CHUNK as u64) as usize;
            f.write_all(&buf[..n])?;
            left -= n as u64;
        }
        f.sync_all()?;
    }
    Ok(())
}

/// A name that does not exist yet in the same folder (never clobbers).
fn scrambled_name(path: &Path) -> PathBuf {
    let n = fsx::name_of(path).len().max(8);
    (0..10_000u32)
        .map(|i| path.with_file_name(format!("{:0>n$}", i)))
        .find(|p| fs::symlink_metadata(p).is_err())
        .unwrap_or_else(|| path.to_path_buf())
}

fn shred_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    if fs::metadata(path)?.nlink() > 1 {
        bail!("{} has other hard links; overwriting would destroy them", path.display());
    }
    overwrite(path).with_context(|| format!("overwrite {}", path.display()))?;
    let tmp = scrambled_name(path);
    fs::rename(path, &tmp)?;
    fs::remove_file(&tmp)?;
    Ok(())
}

/// Shreds a file or folder recursively. Returns number of files shredded.
pub fn shred(path: &Path) -> Result<usize> {
    if let Err(reason) = guard::check_deletable(path) {
        bail!(reason);
    }
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        fs::remove_file(path)?;
        return Ok(0);
    }
    if meta.is_file() {
        shred_file(path)?;
        return Ok(1);
    }
    let mut count = 0;
    for child in fsx::children(path) {
        count += shred(&child)?;
    }
    fs::remove_dir(path)?;
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shreds_files_and_dirs() {
        let base = fsx::home_join(".macbroom-shred-test");
        fs::create_dir_all(&base).unwrap();
        let dir = tempfile::tempdir_in(&base).unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("secret"), b"top secret").unwrap();
        fs::write(dir.path().join("a"), vec![1u8; 3 * CHUNK / 2]).unwrap();
        let target = dir.path().to_path_buf();
        assert_eq!(shred(&target).unwrap(), 2);
        assert!(!target.exists());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn refuses_protected() {
        assert!(shred(Path::new("/System")).is_err());
    }
}
