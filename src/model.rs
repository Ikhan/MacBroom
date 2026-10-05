//! Shared data model: every scanner yields `Item`s, every action consumes them.

use std::path::PathBuf;

/// What acting on an item does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// Remove files/folders (to Trash unless `permanent`).
    Files { paths: Vec<PathBuf>, permanent: bool },
    /// Terminate a process.
    Process { pid: u32 },
    /// Unload a launchd job; `remove` also trashes its plist.
    LaunchItem { plist: PathBuf, label: String, system: bool, remove: bool },
    /// Run an external command (as root via auth dialog if `admin`).
    Command { argv: Vec<String>, admin: bool },
    /// Informational only; no action.
    Info,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub group: &'static str,
    pub label: String,
    pub detail: String,
    pub size: u64,
    pub selected: bool,
    pub target: Target,
}

impl Item {
    pub fn files(group: &'static str, label: String, paths: Vec<PathBuf>, size: u64) -> Self {
        Self {
            group,
            label,
            detail: String::new(),
            size,
            selected: true,
            target: Target::Files { paths, permanent: false },
        }
    }

    pub fn with_detail(self, detail: impl Into<String>) -> Self {
        Self { detail: detail.into(), ..self }
    }

    pub fn with_selected(self, selected: bool) -> Self {
        Self { selected, ..self }
    }

    pub fn permanent(self) -> Self {
        let target = match self.target {
            Target::Files { paths, .. } => Target::Files { paths, permanent: true },
            other => other,
        };
        Self { target, ..self }
    }

    pub fn is_actionable(&self) -> bool {
        self.target != Target::Info
    }
}

/// Outcome of acting on a batch of items.
#[derive(Debug, Default, Clone)]
pub struct Report {
    pub freed: u64,
    pub done: usize,
    pub failed: Vec<String>,
    pub messages: Vec<String>,
}

impl Report {
    pub fn summary(&self) -> String {
        let mut s = format!("{} done", self.done);
        if self.freed > 0 {
            s.push_str(&format!(", {} freed", crate::util::fmt::bytes(self.freed)));
        }
        if !self.failed.is_empty() {
            s.push_str(&format!(", {} failed", self.failed.len()));
        }
        s
    }
}

/// Sum of sizes of selected items.
pub fn selected_size(items: &[Item]) -> u64 {
    items.iter().filter(|i| i.selected).map(|i| i.size).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_and_selection() {
        let a = Item::files("g", "a".into(), vec![PathBuf::from("/x")], 10);
        let b = a.clone().with_selected(false).with_detail("d").permanent();
        assert!(a.selected && !b.selected);
        assert_eq!(b.detail, "d");
        assert!(matches!(b.target, Target::Files { permanent: true, .. }));
        assert_eq!(selected_size(&[a, b]), 10);
    }

    #[test]
    fn report_summary() {
        let r = Report { freed: 2_000_000, done: 3, failed: vec!["x".into()], ..Default::default() };
        assert_eq!(r.summary(), "3 done, 2.0 MB freed, 1 failed");
    }
}
