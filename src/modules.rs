//! Registry of all features. The TUI sidebar and CLI both use it.

use crate::model::Item;
use crate::{apps, perf, scan};
use rayon::prelude::*;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Module {
    SmartCare,
    Junk,
    Mail,
    Trash,
    Malware,
    Privacy,
    Processes,
    LaunchAgents,
    Maintenance,
    Uninstaller,
    Leftovers,
    Updates,
    Extensions,
    SpaceLens,
    LargeOld,
    Duplicates,
    Monitor,
}

pub const ALL: [Module; 17] = [
    Module::SmartCare, Module::Junk, Module::Mail, Module::Trash, Module::Malware,
    Module::Privacy, Module::Processes, Module::LaunchAgents, Module::Maintenance,
    Module::Uninstaller, Module::Leftovers, Module::Updates, Module::Extensions,
    Module::SpaceLens, Module::LargeOld, Module::Duplicates, Module::Monitor,
];

impl Module {
    pub fn title(self) -> &'static str {
        match self {
            Module::SmartCare => "Smart Care",
            Module::Junk => "System Junk",
            Module::Mail => "Mail Attachments",
            Module::Trash => "Trash Bins",
            Module::Malware => "Malware Removal",
            Module::Privacy => "Privacy",
            Module::Processes => "Heavy Consumers",
            Module::LaunchAgents => "Launch Agents",
            Module::Maintenance => "Maintenance",
            Module::Uninstaller => "Uninstaller",
            Module::Leftovers => "Leftovers",
            Module::Updates => "Updater",
            Module::Extensions => "Extensions",
            Module::SpaceLens => "Space Lens",
            Module::LargeOld => "Large & Old Files",
            Module::Duplicates => "Duplicates",
            Module::Monitor => "Monitor",
        }
    }

    pub fn section(self) -> &'static str {
        match self {
            Module::SmartCare => "",
            Module::Junk | Module::Mail | Module::Trash => "CLEANUP",
            Module::Malware | Module::Privacy => "PROTECTION",
            Module::Processes | Module::LaunchAgents | Module::Maintenance => "PERFORMANCE",
            Module::Uninstaller | Module::Leftovers | Module::Updates | Module::Extensions => "APPLICATIONS",
            Module::SpaceLens | Module::LargeOld | Module::Duplicates => "FILES",
            Module::Monitor => "SYSTEM",
        }
    }

    pub fn about(self) -> &'static str {
        match self {
            Module::SmartCare => "Safe junk is pre-selected; trash, mail and threats are listed for review.",
            Module::Junk => "Caches, logs, temp files, developer junk, broken downloads.",
            Module::Mail => "Attachments Mail downloaded. IMAP keeps originals on the server; POP may not.",
            Module::Trash => "Empty Trash on this Mac and connected drives (permanent).",
            Module::Malware => "Heuristic scan of persistence points for adware & suspicious agents.",
            Module::Privacy => "Browser history, cookies and caches; recent-items lists.",
            Module::Processes => "Apps and processes using the most CPU and memory.",
            Module::LaunchAgents => "Background items that start automatically. Disable is reversible.",
            Module::Maintenance => "Flush DNS, free RAM, reindex Spotlight, thin snapshots and more.",
            Module::Uninstaller => "Remove apps completely, including all leftover files.",
            Module::Leftovers => "Data left behind by apps you already deleted.",
            Module::Updates => "Outdated Homebrew formulae/casks, App Store apps, macOS.",
            Module::Extensions => "Quick Look, preference panes, plug-ins, screen savers.",
            Module::SpaceLens => "See what takes up space. Enter = open folder, Backspace = up.",
            Module::LargeOld => "Files over 100 MB in your home folder, biggest first.",
            Module::Duplicates => "Identical files (content-hashed). Oldest copy is kept.",
            Module::Monitor => "Live CPU, memory, disk, network, battery and temperature.",
        }
    }

    pub fn verb(self) -> &'static str {
        match self {
            Module::Processes => "Quit",
            Module::LaunchAgents => "Disable",
            Module::Maintenance => "Run",
            Module::Uninstaller => "Uninstall",
            Module::Updates => "Update",
            Module::Malware => "Remove",
            Module::Monitor => "",
            _ => "Clean",
        }
    }

    /// Runs the scanner. `root` is used by Space Lens, Large & Old and Duplicates.
    pub fn scan(self, root: &Path) -> Vec<Item> {
        match self {
            Module::SmartCare => smart_care(),
            Module::Junk => scan::junk::scan_junk(),
            Module::Mail => scan::junk::scan_mail(),
            Module::Trash => scan::junk::scan_trash(),
            Module::Malware => scan::malware::scan(),
            Module::Privacy => scan::privacy::scan_privacy(),
            Module::Processes => perf::processes::scan(),
            Module::LaunchAgents => perf::launchd::scan(),
            Module::Maintenance => perf::maintenance::scan(),
            Module::Uninstaller => apps::scan(),
            Module::Leftovers => apps::leftovers::scan_orphans(),
            Module::Updates => apps::updates::scan(),
            Module::Extensions => apps::extensions::scan(),
            Module::SpaceLens => scan::space::scan(root),
            Module::LargeOld => scan::large_old::scan(root, &Default::default()),
            Module::Duplicates => scan::duplicates::scan(root, scan::duplicates::DEFAULT_MIN_SIZE),
            Module::Monitor => Vec::new(),
        }
    }
}

fn smart_care() -> Vec<Item> {
    let parts: [fn() -> Vec<Item>; 4] = [
        scan::junk::scan_junk,
        scan::junk::scan_trash_unselected,
        scan::junk::scan_mail,
        scan::malware::scan,
    ];
    parts.par_iter().flat_map(|f| f()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_module_has_metadata() {
        for m in ALL {
            assert!(!m.title().is_empty());
            assert!(!m.about().is_empty());
        }
        assert_eq!(Module::Monitor.scan(Path::new("/")).len(), 0);
    }
}
