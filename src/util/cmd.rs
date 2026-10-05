//! Running external commands, including admin commands via the native
//! macOS password prompt (no terminal sudo needed).

use anyhow::{Context, Result, bail};
use std::process::Command;

/// Runs a program and returns stdout. Fails on non-zero exit.
pub fn run(program: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("failed to start {program}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!("{program} failed: {}", err.trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Whether a program exists on PATH (or at an absolute path).
pub fn exists(program: &str) -> bool {
    if program.starts_with('/') {
        return std::path::Path::new(program).exists();
    }
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| dir.join(program).is_file())
    })
}

/// Quotes a string for safe use inside a POSIX shell command.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Escapes a string for use inside an AppleScript string literal.
fn applescript_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Runs a shell script as root using the standard macOS auth dialog.
/// Batch several commands into one call to prompt only once.
pub fn run_admin(script: &str) -> Result<String> {
    let apple = format!(
        "do shell script \"{}\" with administrator privileges",
        applescript_escape(script)
    );
    run("/usr/bin/osascript", &["-e", &apple])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_shell_strings() {
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn escapes_applescript() {
        assert_eq!(applescript_escape(r#"say "hi" \n"#), r#"say \"hi\" \\n"#);
    }

    #[test]
    fn runs_commands() {
        assert_eq!(run("/bin/echo", &["hello"]).unwrap().trim(), "hello");
        assert!(run("/usr/bin/false", &[]).is_err());
        assert!(exists("/bin/sh"));
        assert!(!exists("definitely-not-a-real-binary-xyz"));
    }
}
