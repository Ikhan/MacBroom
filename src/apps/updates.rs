//! Update checker: Homebrew formulae & casks, Mac App Store (via `mas`),
//! and macOS software updates.

use crate::model::{Item, Target};
use crate::util::cmd;
use rayon::prelude::*;

fn brew() -> Option<&'static str> {
    ["/usr/local/bin/brew", "/opt/homebrew/bin/brew"].into_iter().find(|p| cmd::exists(p))
}

/// Parses `brew outdated --verbose` lines: `name (1.0) < 1.1` or `name (1.0) != 1.1`.
pub fn parse_brew(out: &str) -> Vec<(String, String)> {
    out.lines()
        .filter_map(|l| {
            let (name, rest) = l.trim().split_once(' ')?;
            Some((name.to_string(), rest.trim().to_string()))
        })
        .collect()
}

/// Parses `mas outdated`: `497799835 Xcode (15.0 -> 15.1)`.
pub fn parse_mas(out: &str) -> Vec<(String, String, String)> {
    out.lines()
        .filter_map(|l| {
            let (id, rest) = l.trim().split_once(' ')?;
            let (name, ver) = rest.rsplit_once(" (").unwrap_or((rest, ""));
            Some((id.to_string(), name.trim().to_string(), ver.trim_end_matches(')').to_string()))
        })
        .collect()
}

/// Parses `softwareupdate -l`: lines like `* Label: macOS 15.1-24B83`.
pub fn parse_softwareupdate(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|l| l.trim().strip_prefix("* Label: ").map(str::to_string))
        .collect()
}

fn brew_items(cask: bool) -> Vec<Item> {
    let Some(brew) = brew() else { return vec![] };
    let mut args = vec!["outdated", "--verbose"];
    if cask {
        args.extend(["--cask", "--greedy"]);
    } else {
        args.push("--formula");
    }
    let out = cmd::run(brew, &args).unwrap_or_default();
    let kind = if cask { "--cask" } else { "--formula" };
    parse_brew(&out)
        .into_iter()
        .map(|(name, ver)| Item {
            group: if cask { "Homebrew Casks" } else { "Homebrew" },
            label: name.clone(),
            detail: ver,
            size: 0,
            selected: false,
            target: Target::Command { argv: vec![brew.into(), "upgrade".into(), kind.into(), name], admin: false },
        })
        .collect()
}

fn mas_items() -> Vec<Item> {
    if !cmd::exists("mas") {
        return vec![];
    }
    parse_mas(&cmd::run("mas", &["outdated"]).unwrap_or_default())
        .into_iter()
        .map(|(id, name, ver)| Item {
            group: "App Store",
            label: name,
            detail: ver,
            size: 0,
            selected: false,
            target: Target::Command { argv: vec!["mas".into(), "upgrade".into(), id], admin: false },
        })
        .collect()
}

fn macos_items() -> Vec<Item> {
    let out = cmd::run("/usr/sbin/softwareupdate", &["-l"]).unwrap_or_default();
    parse_softwareupdate(&out)
        .into_iter()
        .map(|label| Item {
            group: "macOS",
            label: label.clone(),
            detail: "system update (may restart)".into(),
            size: 0,
            selected: false,
            target: Target::Command { argv: vec!["/usr/sbin/softwareupdate".into(), "-i".into(), label], admin: true },
        })
        .collect()
}

pub fn scan() -> Vec<Item> {
    let sources: [fn() -> Vec<Item>; 4] = [|| brew_items(false), || brew_items(true), mas_items, macos_items];
    sources.par_iter().flat_map(|f| f()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_brew_outdated() {
        let v = parse_brew("git (2.40.0) < 2.41.0\nfirefox (120.0) != 121.0\n");
        assert_eq!(v, vec![("git".into(), "(2.40.0) < 2.41.0".into()), ("firefox".into(), "(120.0) != 121.0".into())]);
    }

    #[test]
    fn parses_mas_outdated() {
        let v = parse_mas("497799835 Xcode (15.0 -> 15.1)\n");
        assert_eq!(v, vec![("497799835".into(), "Xcode".into(), "15.0 -> 15.1".into())]);
    }

    #[test]
    fn parses_softwareupdate() {
        let out = "Software Update found the following:\n* Label: macOS Sequoia 15.1-24B83\n\tTitle: macOS\n";
        assert_eq!(parse_softwareupdate(out), vec!["macOS Sequoia 15.1-24B83".to_string()]);
    }

    #[test]
    fn mas_and_brew_lookup_do_not_panic() {
        let _ = mas_items();
        let _ = brew();
    }
}
