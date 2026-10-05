pub mod duplicates;
pub mod junk;
pub mod large_old;
pub mod malware;
pub mod privacy;
pub mod space;

use crate::model::Item;
use crate::util::fsx;
use rayon::prelude::*;
use std::path::{Path, PathBuf};

/// Sizes each child of `dir` in parallel and returns one item per non-empty child.
pub fn children_items<F>(group: &'static str, dir: &Path, keep: F) -> Vec<Item>
where
    F: Fn(&Path) -> bool + Sync,
{
    let kids: Vec<PathBuf> = fsx::children(dir).into_iter().filter(|p| keep(p)).collect();
    paths_items(group, kids)
}

/// Sizes the given paths in parallel; drops empty ones; sorts by size desc.
pub fn paths_items(group: &'static str, paths: Vec<PathBuf>) -> Vec<Item> {
    let mut items: Vec<Item> = paths
        .into_par_iter()
        .filter_map(|p| {
            let size = fsx::dir_size(&p);
            (size > 0).then(|| {
                Item::files(group, fsx::name_of(&p), vec![p.clone()], size)
                    .with_detail(crate::util::fmt::tilde(&p))
            })
        })
        .collect();
    items.sort_by(|a, b| b.size.cmp(&a.size));
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_children_and_drops_empty() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("big"), vec![1u8; 50_000]).unwrap();
        std::fs::create_dir(tmp.path().join("empty")).unwrap();
        std::fs::write(tmp.path().join("skip.me"), vec![1u8; 50_000]).unwrap();
        let items = children_items("G", tmp.path(), |p| !fsx::name_of(p).ends_with(".me"));
        assert!(items.iter().any(|i| i.label == "big"));
        assert!(items.iter().all(|i| i.label != "skip.me"));
        assert!(items.windows(2).all(|w| w[0].size >= w[1].size));
    }
}
