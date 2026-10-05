//! Launch agents & daemons (background items that start with the Mac).

use crate::model::{Item, Target};
use crate::util::{cmd, fsx};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Job {
    pub plist: PathBuf,
    pub label: String,
    pub program: Option<PathBuf>,
    pub run_at_load: bool,
    pub keep_alive: bool,
    pub system: bool,
}

/// (directory, is_system_domain)
pub fn job_dirs() -> Vec<(PathBuf, bool)> {
    vec![
        (fsx::home_join("Library/LaunchAgents"), false),
        (PathBuf::from("/Library/LaunchAgents"), true),
        (PathBuf::from("/Library/LaunchDaemons"), true),
    ]
}

pub fn parse_job(plist: &Path, system: bool) -> Option<Job> {
    let value = plist::Value::from_file(plist).ok()?;
    let dict = value.as_dictionary()?;
    let label = dict.get("Label")?.as_string()?.to_string();
    let program = dict
        .get("Program")
        .and_then(|v| v.as_string())
        .or_else(|| dict.get("ProgramArguments")?.as_array()?.first()?.as_string())
        .map(PathBuf::from);
    let truthy = |k: &str| match dict.get(k) {
        Some(plist::Value::Boolean(b)) => *b,
        Some(plist::Value::Dictionary(_)) => true,
        _ => false,
    };
    Some(Job {
        plist: plist.to_path_buf(),
        label,
        program,
        run_at_load: truthy("RunAtLoad"),
        keep_alive: truthy("KeepAlive"),
        system,
    })
}

pub fn all_jobs() -> Vec<Job> {
    job_dirs()
        .into_iter()
        .flat_map(|(dir, system)| {
            fsx::children(&dir)
                .into_iter()
                .filter(|p| p.extension().is_some_and(|e| e == "plist"))
                .filter_map(move |p| parse_job(&p, system))
        })
        .collect()
}

/// Labels currently loaded in the user's launchd domain.
fn loaded_labels() -> HashSet<String> {
    cmd::run("/bin/launchctl", &["list"])
        .map(|out| {
            out.lines()
                .skip(1)
                .filter_map(|l| l.split('\t').nth(2).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

pub fn scan() -> Vec<Item> {
    let loaded = loaded_labels();
    let mut items: Vec<Item> = all_jobs()
        .into_iter()
        .map(|job| {
            let mut flags = vec![if job.system { "system" } else { "user" }];
            if job.run_at_load { flags.push("starts at login"); }
            if job.keep_alive { flags.push("always running"); }
            if loaded.contains(&job.label) { flags.push("loaded"); }
            let prog = job.program.as_deref().map(crate::util::fmt::tilde).unwrap_or_default();
            Item {
                group: "Launch Agents",
                label: job.label.clone(),
                detail: format!("{} · {prog}", flags.join(", ")),
                size: 0,
                selected: false,
                target: Target::LaunchItem { plist: job.plist, label: job.label, system: job.system, remove: false },
            }
        })
        .collect();
    items.sort_by(|a, b| a.label.cmp(&b.label));
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_program_arguments() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("com.example.agent.plist");
        std::fs::write(&p, r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>Label</key><string>com.example.agent</string>
<key>ProgramArguments</key><array><string>/usr/local/bin/agent</string><string>-d</string></array>
<key>RunAtLoad</key><true/>
<key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>
</dict></plist>"#).unwrap();
        let job = parse_job(&p, false).unwrap();
        assert_eq!(job.label, "com.example.agent");
        assert_eq!(job.program.unwrap(), PathBuf::from("/usr/local/bin/agent"));
        assert!(job.run_at_load && job.keep_alive);
    }

    #[test]
    fn rejects_plist_without_label() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("x.plist");
        std::fs::write(&p, r#"<?xml version="1.0"?><plist version="1.0"><dict></dict></plist>"#).unwrap();
        assert!(parse_job(&p, false).is_none());
    }

    #[test]
    fn live_agents_scan() {
        let items = scan();
        assert!(items.iter().all(|i| matches!(i.target, Target::LaunchItem { remove: false, .. })));
        assert!(items.windows(2).all(|w| w[0].label <= w[1].label));
        let _ = loaded_labels();
        assert_eq!(job_dirs().len(), 3);
    }
}
