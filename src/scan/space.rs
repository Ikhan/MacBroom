//! Space Lens: sizes of a folder's immediate children, computed lazily so
//! memory stays flat no matter how big the disk is.

use crate::model::Item;
use crate::util::fmt::bytes;
use crate::util::fsx;
use rayon::prelude::*;
use std::path::Path;

pub fn scan(dir: &Path) -> Vec<Item> {
    let kids = fsx::children(dir);
    let mut items: Vec<Item> = kids
        .into_par_iter()
        .map(|p| {
            let size = fsx::dir_size(&p);
            let is_dir = std::fs::symlink_metadata(&p).is_ok_and(|m| m.is_dir());
            let name = if is_dir { format!("{}/", fsx::name_of(&p)) } else { fsx::name_of(&p) };
            Item::files("Space Lens", name, vec![p], size).with_selected(false)
        })
        .collect();
    items.sort_by(|a, b| b.size.cmp(&a.size));
    let total: u64 = items.iter().map(|i| i.size).sum::<u64>().max(1);
    items
        .into_iter()
        .map(|i| {
            let pct = i.size as f64 / total as f64;
            let detail = format!("{} {:>5.1}%  {}", crate::util::fmt::bar(pct, 20), pct * 100.0, bytes(i.size));
            i.with_detail(detail)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_children_by_size() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("d")).unwrap();
        std::fs::write(tmp.path().join("d/x"), vec![0u8; 100_000]).unwrap();
        std::fs::write(tmp.path().join("f"), vec![0u8; 10]).unwrap();
        let items = scan(tmp.path());
        assert_eq!(items[0].label, "d/");
        assert!(items[0].detail.contains('%'));
    }
}
