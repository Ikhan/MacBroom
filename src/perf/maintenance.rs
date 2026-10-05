//! One-click maintenance scripts. Root tasks are batched into a single
//! native password prompt by the executor.

use crate::model::{Item, Target};
use crate::util::cmd;

const LSREGISTER: &str = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";

struct Task {
    name: &'static str,
    about: &'static str,
    script: String,
    admin: bool,
}

fn sh(script: impl Into<String>) -> Vec<String> {
    vec!["/bin/sh".into(), "-c".into(), script.into()]
}

fn tasks() -> Vec<Task> {
    // SAFETY: getuid has no preconditions.
    let uid = unsafe { libc::getuid() };
    let mut list = vec![
        Task { name: "Free up RAM", about: "purge inactive memory (root)", script: "purge".into(), admin: true },
        Task { name: "Flush DNS cache", about: "fixes stale websites / network names (root)", script: "dscacheutil -flushcache; killall -HUP mDNSResponder".into(), admin: true },
        Task { name: "Thin Time Machine snapshots", about: "reclaim purgeable space held by local snapshots (root)", script: "tmutil thinlocalsnapshots / 999999999999 4".into(), admin: true },
        Task { name: "Reindex Spotlight", about: "rebuild search index; slow searches/wrong results (root)", script: "mdutil -E /".into(), admin: true },
        Task { name: "Clear font caches", about: "fixes garbled fonts; restart afterwards (root)", script: "atsutil databases -remove".into(), admin: true },
        Task { name: "Repair home permissions", about: "reset ACLs on your home folder (root)", script: format!("diskutil resetUserPermissions / {uid}"), admin: true },
        Task { name: "Rebuild Launch Services", about: "fixes duplicate 'Open With' entries", script: format!("{LSREGISTER} -r -domain local -domain system -domain user"), admin: false },
        Task { name: "Verify startup disk", about: "check APFS volume for errors (read-only)", script: "diskutil verifyVolume /".into(), admin: false },
        Task { name: "Restart Finder & Dock", about: "fixes hung Finder / Dock UI", script: "killall Finder; killall Dock".into(), admin: false },
    ];
    if cmd::exists("/usr/sbin/periodic") {
        list.push(Task { name: "Run maintenance scripts", about: "rotate logs & tidy system dbs (root)", script: "periodic daily weekly monthly".into(), admin: true });
    }
    list
}

pub fn scan() -> Vec<Item> {
    tasks()
        .into_iter()
        .map(|t| Item {
            group: "Maintenance",
            label: t.name.into(),
            detail: t.about.into(),
            size: 0,
            selected: false,
            target: Target::Command { argv: sh(t.script), admin: t.admin },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tasks_are_unselected_commands() {
        let items = scan();
        assert!(items.len() >= 9);
        assert!(items.iter().all(|i| !i.selected && matches!(i.target, Target::Command { .. })));
    }
}
