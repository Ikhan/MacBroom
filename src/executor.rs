//! Executes actions for selected items. Admin operations are batched into a
//! single macOS auth prompt.

use crate::model::{Item, Report, Target};
use crate::util::{cmd, guard};
use std::path::{Path, PathBuf};
use trash::TrashContext;
use trash::macos::{DeleteMethod, TrashContextExtMacos};

/// Global override: delete permanently instead of moving to Trash.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DeleteMode {
    #[default]
    Trash,
    Permanent,
}

/// Acts on all selected, actionable items.
pub fn execute(items: &[Item], mode: DeleteMode) -> Report {
    let mut report = Report::default();
    let mut admin_script: Vec<String> = Vec::new();
    for item in items.iter().filter(|i| i.selected && i.is_actionable()) {
        match &item.target {
            Target::Files { paths, permanent } => {
                let perm = *permanent || mode == DeleteMode::Permanent;
                remove_files(paths, perm, item.size, &mut report);
            }
            Target::Process { pid } => kill(*pid, &item.label, &mut report),
            Target::LaunchItem { plist, label, system, remove } => {
                launch_item(plist, label, *system, *remove, &mut admin_script, &mut report)
            }
            Target::Command { argv, admin: true } => {
                admin_script.push(argv.iter().map(|a| cmd::shell_quote(a)).collect::<Vec<_>>().join(" "))
            }
            Target::Command { argv, admin: false } => run_command(argv, &mut report),
            Target::Info => {}
        }
    }
    run_admin_batch(&admin_script, &mut report);
    report
}

fn remove_files(paths: &[PathBuf], permanent: bool, size: u64, report: &mut Report) {
    let mut ok = true;
    for path in paths {
        if let Err(reason) = guard::check_deletable(path) {
            report.failed.push(reason);
            ok = false;
            continue;
        }
        let result = if permanent { delete_permanently(path).map_err(|e| e.to_string()) } else { move_to_trash(path) };
        if let Err(e) = result {
            report.failed.push(format!("{}: {e}", path.display()));
            ok = false;
        }
    }
    if ok {
        report.done += 1;
        report.freed += size;
    }
}

fn delete_permanently(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Fast silent trash via NSFileManager; falls back to Finder, which can
/// prompt for a password for root-owned items.
fn move_to_trash(path: &Path) -> Result<(), String> {
    if !path.exists() && std::fs::symlink_metadata(path).is_err() {
        return Ok(());
    }
    let mut fast = TrashContext::default();
    fast.set_delete_method(DeleteMethod::NsFileManager);
    if fast.delete(path).is_ok() {
        return Ok(());
    }
    let mut finder = TrashContext::default();
    finder.set_delete_method(DeleteMethod::Finder);
    finder.delete(path).map_err(|e| e.to_string())
}

fn kill(pid: u32, name: &str, report: &mut Report) {
    let Ok(raw) = libc::pid_t::try_from(pid) else {
        report.failed.push(format!("invalid pid for {name}: {pid}"));
        return;
    };
    if pid == std::process::id() || raw <= 1 {
        report.failed.push(format!("refusing to kill {name} ({pid})"));
        return;
    }
    // SAFETY: plain syscall with a validated positive pid; no memory is shared.
    let rc = unsafe { libc::kill(raw, libc::SIGTERM) };
    if rc == 0 {
        report.done += 1;
        report.messages.push(format!("asked {name} ({pid}) to quit"));
    } else {
        report.failed.push(format!("{name} ({pid}): {}", std::io::Error::last_os_error()));
    }
}

fn uid() -> u32 {
    // SAFETY: getuid has no preconditions and cannot fail.
    unsafe { libc::getuid() }
}

fn launch_item(
    plist: &Path,
    label: &str,
    system: bool,
    remove: bool,
    admin_script: &mut Vec<String>,
    report: &mut Report,
) {
    if system {
        let target = cmd::shell_quote(&format!("system/{label}"));
        let mut script = format!("launchctl bootout {target} 2>/dev/null; launchctl disable {target}");
        if remove {
            if let Err(reason) = guard::check_deletable(plist) {
                report.failed.push(reason);
                return;
            }
            let dest = crate::util::fsx::home_join(&format!(".Trash/{label}-{}.plist", std::process::id()));
            let p = plist.to_string_lossy();
            script.push_str(&format!("; mv -n {} {}", cmd::shell_quote(&p), cmd::shell_quote(&dest.to_string_lossy())));
        }
        admin_script.push(script);
        return;
    }
    let target = format!("gui/{}/{label}", uid());
    let _ = cmd::run("/bin/launchctl", &["bootout", &target]);
    let disabled = cmd::run("/bin/launchctl", &["disable", &target]);
    if remove {
        remove_files(&[plist.to_path_buf()], false, 0, report);
    } else if let Err(e) = disabled {
        report.failed.push(format!("{label}: {e}"));
    } else {
        report.done += 1;
        report.messages.push(format!("disabled {label} (re-enable: launchctl enable {target})"));
    }
}

fn run_command(argv: &[String], report: &mut Report) {
    let Some((prog, args)) = argv.split_first() else { return };
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match cmd::run(prog, &args) {
        Ok(out) => {
            report.done += 1;
            let tail = out.lines().last().unwrap_or("").trim();
            report.messages.push(format!("{} ✓ {tail}", argv.join(" ")));
        }
        Err(e) => report.failed.push(e.to_string()),
    }
}

fn run_admin_batch(script: &[String], report: &mut Report) {
    if script.is_empty() {
        return;
    }
    // Run each step independently so one failure doesn't stop the rest.
    let joined = script
        .iter()
        .map(|s| format!("( {s} ) || echo 'FAILED: {}'", s.replace('\'', "")))
        .collect::<Vec<_>>()
        .join("; ");
    match cmd::run_admin(&joined) {
        Ok(out) => {
            // `do shell script` turns LF into CR.
            let failures: Vec<String> = out
                .split(['\n', '\r'])
                .filter(|l| l.starts_with("FAILED: "))
                .map(str::to_string)
                .collect();
            report.done += script.len() - failures.len();
            report.failed.extend(failures);
            report.messages.push("admin tasks completed".into());
        }
        Err(e) => report.failed.push(format!("admin tasks: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Item;

    #[test]
    fn permanent_delete_frees_and_counts() {
        let home_tmp = crate::util::fsx::home_join(".macbroom-test");
        std::fs::create_dir_all(&home_tmp).unwrap();
        let dir = tempfile::tempdir_in(&home_tmp).unwrap();
        let f = dir.path().join("junk.bin");
        std::fs::write(&f, vec![0u8; 4096]).unwrap();
        let items = vec![Item::files("t", "junk".into(), vec![f.clone()], 4096).permanent()];
        let report = execute(&items, DeleteMode::Trash);
        assert_eq!(report.done, 1);
        assert_eq!(report.freed, 4096);
        assert!(!f.exists());
        let _ = std::fs::remove_dir_all(&home_tmp);
    }

    #[test]
    fn guard_blocks_protected_paths() {
        let items = vec![Item::files("t", "sys".into(), vec![PathBuf::from("/System")], 1).permanent()];
        let report = execute(&items, DeleteMode::Permanent);
        assert_eq!(report.done, 0);
        assert_eq!(report.failed.len(), 1);
    }

    #[test]
    fn unselected_and_info_are_skipped() {
        let a = Item::files("t", "a".into(), vec![PathBuf::from("/System")], 1).with_selected(false);
        let b = Item { target: Target::Info, ..a.clone().with_selected(true) };
        let report = execute(&[a, b], DeleteMode::Permanent);
        assert_eq!(report.done + report.failed.len(), 0);
    }

    #[test]
    fn refuses_to_kill_self() {
        let mut r = Report::default();
        kill(std::process::id(), "me", &mut r);
        assert_eq!(r.failed.len(), 1);
    }

    #[test]
    fn runs_plain_commands() {
        let mut r = Report::default();
        run_command(&["/bin/echo".into(), "ok".into()], &mut r);
        assert_eq!(r.done, 1);
    }

    #[test]
    fn missing_files_and_dead_processes() {
        assert!(move_to_trash(Path::new("/nonexistent/macbroom/x")).is_ok());
        assert!(delete_permanently(Path::new("/nonexistent/macbroom/x")).is_ok());
        let mut r = Report::default();
        kill(4_000_000, "ghost", &mut r);
        assert_eq!(r.failed.len(), 1);
        kill(u32::MAX, "wrap", &mut r);
        assert!(r.failed[1].contains("invalid pid"), "must never become kill(-1)");
        r.failed.pop();
        run_command(&["/usr/bin/false".into()], &mut r);
        assert_eq!(r.failed.len(), 2);
        run_command(&[], &mut r);
        run_admin_batch(&[], &mut r);
        assert_eq!(r.done, 0);
    }

    #[test]
    fn execute_dispatches_processes_and_commands() {
        let items = vec![
            Item { target: Target::Process { pid: 4_000_000 }, ..Item::files("p", "ghost".into(), vec![], 0) },
            Item { target: Target::Command { argv: vec!["/bin/echo".into(), "hi".into()], admin: false }, ..Item::files("c", "echo".into(), vec![], 0) },
        ];
        let r = execute(&items, DeleteMode::Trash);
        assert_eq!(r.done, 1);
        assert_eq!(r.failed.len(), 1);
        assert!(uid() > 0);
    }
}
