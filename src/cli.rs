//! Command-line interface. Every module is dry-run by default; pass
//! `--yes` to act on the pre-selected items.

use crate::executor::{self, DeleteMode};
use crate::model::{Item, selected_size};
use crate::modules::Module;
use crate::util::fmt::{bytes, parse_size};
use crate::util::fsx;
use crate::{scan, shred};
use anyhow::{Result, bail};
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "macbroom", version, about = "Fast, low-memory Mac cleaner & optimizer. Run without arguments for the interactive app.")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Cmd>,
}

#[derive(Args, Clone, Default)]
pub struct Act {
    /// Perform the action (default is a dry run that only reports)
    #[arg(long, short = 'y')]
    pub yes: bool,
    /// Also include items that are not selected by default
    #[arg(long)]
    pub all: bool,
    /// Only items whose name contains this text (case-insensitive)
    #[arg(long)]
    pub only: Option<String>,
    /// Delete permanently instead of moving to Trash
    #[arg(long)]
    pub permanent: bool,
    /// List every item, not just group totals
    #[arg(long, short)]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// Smart Care: junk + trash + mail attachments + threats
    Scan(Act),
    /// System junk: caches, logs, temp, developer junk, broken downloads
    Junk(Act),
    /// Mail attachments downloaded by Mail.app
    Mail(Act),
    /// Empty Trash bins (permanent)
    Trash(Act),
    /// Heuristic malware / adware scan
    Malware(Act),
    /// Browser history, cookies, caches; recent items
    Privacy(Act),
    /// Heavy CPU / memory consumers (action: quit)
    Top(Act),
    /// Launch agents & daemons (action: disable)
    Agents(Act),
    /// Maintenance tasks, e.g. `maintain --only dns --yes`
    Maintain(Act),
    /// List apps with leftovers (action: uninstall; use --only <name>)
    Apps(Act),
    /// Leftovers of apps that were already deleted
    Leftovers(Act),
    /// Outdated Homebrew / App Store / macOS software
    Updates(Act),
    /// Quick Look, preference panes, plug-ins
    Extensions(Act),
    /// Space Lens: folder sizes
    Space { #[arg(default_value = "~")] path: String },
    /// Large & old files
    Large {
        #[arg(default_value = "~")] path: String,
        #[arg(long, default_value = "100MB")] min_size: String,
        #[arg(long, default_value_t = 0)] older_than: u64,
        #[command(flatten)] act: Act,
    },
    /// Duplicate files
    Dupes {
        #[arg(default_value = "~")] path: String,
        #[arg(long, default_value = "1MB")] min_size: String,
        #[command(flatten)] act: Act,
    },
    /// Live system monitor
    Monitor { #[arg(long)] once: bool },
    /// Securely overwrite and delete files
    Shred { paths: Vec<PathBuf>, #[arg(long, short = 'y')] yes: bool },
}

pub fn expand(path: &str) -> PathBuf {
    match path.strip_prefix('~') {
        Some(rest) => fsx::home().join(rest.trim_start_matches('/')),
        None => PathBuf::from(path),
    }
}

/// Modules where `--only` must match a name exactly (uninstalling every app
/// containing "code" would be a disaster).
fn exact_only(module: Module) -> bool {
    matches!(module, Module::Uninstaller | Module::Duplicates | Module::Processes)
}

/// Applies `--all` / `--only` to the default selection.
pub fn apply_filters(items: Vec<Item>, act: &Act, exact: bool) -> Vec<Item> {
    let only = act.only.as_ref().map(|s| s.trim().to_ascii_lowercase());
    items
        .into_iter()
        .map(|i| {
            let label = i.label.to_ascii_lowercase();
            let sel = match &only {
                Some(q) if exact => label == *q,
                Some(q) => label.contains(q),
                None => i.selected || act.all,
            };
            let actionable = i.is_actionable();
            i.with_selected(sel && actionable)
        })
        .collect()
}

fn print_items(items: &[Item], verbose: bool) {
    let mut groups: Vec<&'static str> = Vec::new();
    for i in items {
        if !groups.contains(&i.group) {
            groups.push(i.group);
        }
    }
    for g in groups {
        let members: Vec<&Item> = items.iter().filter(|i| i.group == g).collect();
        let total: u64 = members.iter().map(|i| i.size).sum();
        println!("\x1b[1m{g:<22}\x1b[0m {:>10}  ({} items)", bytes(total), members.len());
        let show = if verbose { members.len() } else { members.len().min(5) };
        for i in &members[..show] {
            let mark = if i.selected { "\x1b[32m[x]\x1b[0m" } else { "[ ]" };
            let size = if i.size > 0 { bytes(i.size) } else { String::new() };
            println!("  {mark} {:<40} {:>10}  \x1b[2m{}\x1b[0m", truncate(&i.label, 40), size, i.detail);
        }
        if show < members.len() {
            println!("  \x1b[2m… {} more (use -v)\x1b[0m", members.len() - show);
        }
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n - 1).collect::<String>()) }
}

fn run_module(module: Module, items: Vec<Item>, act: &Act) -> Result<()> {
    if act.only.as_ref().is_some_and(|q| q.trim().is_empty()) {
        bail!("--only needs a non-empty name");
    }
    let items = apply_filters(items, act, exact_only(module));
    if items.is_empty() {
        println!("Nothing found. ✨");
        return Ok(());
    }
    print_items(&items, act.verbose);
    let n = items.iter().filter(|i| i.selected).count();
    println!("\nSelected: {n} item(s), {}", bytes(selected_size(&items)));
    if !act.yes {
        if n > 0 && !module.verb().is_empty() {
            println!("Dry run. Re-run with --yes to {}.", module.verb().to_lowercase());
        }
        return Ok(());
    }
    let mode = if act.permanent { DeleteMode::Permanent } else { DeleteMode::Trash };
    let report = executor::execute(&items, mode);
    report.messages.iter().for_each(|m| println!("  {m}"));
    report.failed.iter().for_each(|f| eprintln!("  \x1b[31m✗\x1b[0m {f}"));
    println!("\x1b[1m{}\x1b[0m", report.summary());
    Ok(())
}

pub fn run(cmd: Cmd) -> Result<()> {
    let home = fsx::home();
    let simple = |m: Module, act: Act| run_module(m, m.scan(&home), &act);
    match cmd {
        Cmd::Scan(a) => simple(Module::SmartCare, a),
        Cmd::Junk(a) => simple(Module::Junk, a),
        Cmd::Mail(a) => simple(Module::Mail, a),
        Cmd::Trash(a) => simple(Module::Trash, a),
        Cmd::Malware(a) => simple(Module::Malware, a),
        Cmd::Privacy(a) => simple(Module::Privacy, a),
        Cmd::Top(a) => simple(Module::Processes, a),
        Cmd::Agents(a) => simple(Module::LaunchAgents, a),
        Cmd::Maintain(a) => simple(Module::Maintenance, a),
        Cmd::Apps(a) => simple(Module::Uninstaller, a),
        Cmd::Leftovers(a) => simple(Module::Leftovers, a),
        Cmd::Updates(a) => simple(Module::Updates, a),
        Cmd::Extensions(a) => simple(Module::Extensions, a),
        Cmd::Space { path } => {
            let p = expand(&path);
            println!("{}  {}", p.display(), bytes(fsx::dir_size(&p)));
            for i in scan::space::scan(&p).iter().take(40) {
                println!("  {:<40} {}", truncate(&i.label, 40), i.detail);
            }
            Ok(())
        }
        Cmd::Large { path, min_size, older_than, act } => {
            let q = scan::large_old::Query { min_size: parse_size(&min_size).map_err(anyhow::Error::msg)?, older_than_days: older_than };
            run_module(Module::LargeOld, scan::large_old::scan(&expand(&path), &q), &Act { verbose: true, ..act })
        }
        Cmd::Dupes { path, min_size, act } => {
            let min = parse_size(&min_size).map_err(anyhow::Error::msg)?;
            run_module(Module::Duplicates, scan::duplicates::scan(&expand(&path), min), &act)
        }
        Cmd::Monitor { once } => monitor(once),
        Cmd::Shred { paths, yes } => shred_cmd(&paths, yes),
    }
}

fn monitor(once: bool) -> Result<()> {
    let mut m = crate::monitor::Monitor::new();
    m.sample(false);
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let lines = m.sample(true).lines();
        if !once {
            print!("\x1b[2J\x1b[H");
        }
        lines.iter().for_each(|l| println!("{l}"));
        if once {
            return Ok(());
        }
    }
}

fn shred_cmd(paths: &[PathBuf], yes: bool) -> Result<()> {
    if paths.is_empty() {
        bail!("give at least one path to shred");
    }
    if !yes {
        println!("This permanently destroys: {:?}\nRe-run with --yes to confirm.", paths);
        println!("Note: on SSDs, FileVault encryption is the only reliable protection.");
        return Ok(());
    }
    for p in paths {
        let abs = std::fs::canonicalize(p)?;
        let n = shred::shred(&abs)?;
        println!("shredded {} ({n} files)", abs.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(label: &str, selected: bool) -> Item {
        Item::files("g", label.into(), vec![PathBuf::from("/x")], 1).with_selected(selected)
    }

    #[test]
    fn filters_default_all_and_only() {
        let items = vec![item("Chrome", true), item("Slack", false)];
        let d = apply_filters(items.clone(), &Act::default(), false);
        assert_eq!(d.iter().map(|i| i.selected).collect::<Vec<_>>(), vec![true, false]);
        let a = apply_filters(items.clone(), &Act { all: true, ..Default::default() }, false);
        assert!(a.iter().all(|i| i.selected));
        let o = apply_filters(items.clone(), &Act { only: Some("slack".into()), ..Default::default() }, false);
        assert_eq!(o.iter().map(|i| i.selected).collect::<Vec<_>>(), vec![false, true]);
        let sub = apply_filters(items.clone(), &Act { only: Some("sla".into()), ..Default::default() }, true);
        assert!(sub.iter().all(|i| !i.selected), "exact mode rejects substrings");
        let ex = apply_filters(items, &Act { only: Some("Slack".into()), ..Default::default() }, true);
        assert_eq!(ex.iter().filter(|i| i.selected).count(), 1);
    }

    #[test]
    fn empty_only_is_rejected() {
        let act = Act { only: Some(" ".into()), ..Default::default() };
        assert!(run_module(Module::Junk, vec![item("a", true)], &act).is_err());
    }

    #[test]
    fn expands_tilde() {
        assert_eq!(expand("~"), fsx::home());
        assert_eq!(expand("~/Downloads"), fsx::home().join("Downloads"));
        assert_eq!(expand("/tmp"), PathBuf::from("/tmp"));
    }

    #[test]
    fn truncates_long_labels() {
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(truncate("ab", 4), "ab");
    }

    fn tmp_items(dir: &std::path::Path) -> Vec<Item> {
        let f = dir.join("f");
        std::fs::write(&f, vec![0u8; 10]).unwrap();
        vec![
            Item::files("Group A", "f".into(), vec![f], 10),
            Item { target: crate::model::Target::Info, ..Item::files("Group B", "info".into(), vec![], 0) },
        ]
    }

    #[test]
    fn module_dry_run_reports_without_acting() {
        let tmp = tempfile::tempdir().unwrap();
        let items = tmp_items(tmp.path());
        print_items(&items, true);
        print_items(&items, false);
        run_module(Module::Junk, items, &Act::default()).unwrap();
        assert!(tmp.path().join("f").exists());
        run_module(Module::Junk, vec![], &Act::default()).unwrap();
    }

    #[test]
    fn module_yes_acts() {
        let base = fsx::home_join(".macbroom-cli-test");
        std::fs::create_dir_all(&base).unwrap();
        let tmp = tempfile::tempdir_in(&base).unwrap();
        let items = tmp_items(tmp.path());
        run_module(Module::Junk, items, &Act { yes: true, permanent: true, ..Default::default() }).unwrap();
        assert!(!tmp.path().join("f").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn subcommands_run_on_temp_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a"), vec![1u8; 2000]).unwrap();
        std::fs::write(tmp.path().join("b"), vec![1u8; 2000]).unwrap();
        let p = tmp.path().to_string_lossy().to_string();
        run(Cmd::Space { path: p.clone() }).unwrap();
        run(Cmd::Large { path: p.clone(), min_size: "1K".into(), older_than: 0, act: Act::default() }).unwrap();
        run(Cmd::Dupes { path: p.clone(), min_size: "1K".into(), act: Act::default() }).unwrap();
        assert!(run(Cmd::Large { path: p, min_size: "bad".into(), older_than: 0, act: Act::default() }).is_err());
        run(Cmd::Maintain(Act::default())).unwrap();
        run(Cmd::Extensions(Act::default())).unwrap();
        run(Cmd::Agents(Act::default())).unwrap();
        run(Cmd::Monitor { once: true }).unwrap();
    }

    #[test]
    fn shred_requires_paths_and_confirmation() {
        assert!(shred_cmd(&[], true).is_err());
        let tmp = tempfile::NamedTempFile::new().unwrap();
        shred_cmd(&[tmp.path().to_path_buf()], false).unwrap();
        assert!(tmp.path().exists());
    }
}
