//! Extensions & plug-ins: Quick Look, preference panes, internet plug-ins,
//! Spotlight importers, screen savers, audio plug-ins, input methods.

use crate::model::Item;
use crate::util::fsx;
use std::path::PathBuf;

const KINDS: [(&str, &str); 8] = [
    ("Quick Look", "Library/QuickLook"),
    ("Preference Panes", "Library/PreferencePanes"),
    ("Internet Plug-ins", "Library/Internet Plug-Ins"),
    ("Spotlight Importers", "Library/Spotlight"),
    ("Screen Savers", "Library/Screen Savers"),
    ("Input Methods", "Library/Input Methods"),
    ("Audio Plug-ins", "Library/Audio/Plug-Ins/HAL"),
    ("Audio Units", "Library/Audio/Plug-Ins/Components"),
];

fn roots(rel: &str) -> [PathBuf; 2] {
    [fsx::home_join(rel), PathBuf::from("/").join(rel)]
}

pub fn scan() -> Vec<Item> {
    KINDS
        .iter()
        .flat_map(|(kind, rel)| {
            roots(rel).into_iter().flat_map(move |dir| {
                fsx::children(&dir)
                    .into_iter()
                    .filter(|p| !fsx::name_of(p).starts_with('.'))
                    .map(move |p| {
                        let size = fsx::dir_size(&p);
                        Item::files("Extensions", fsx::name_of(&p), vec![p.clone()], size)
                            .with_detail(format!("{kind} · {}", crate::util::fmt::tilde(&p)))
                            .with_selected(false)
                    })
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_items_are_unselected() {
        assert!(scan().iter().all(|i| !i.selected));
    }
}
