//! Human-friendly formatting and parsing helpers.

use std::time::{Duration, SystemTime};

const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];

/// Formats bytes using decimal units, matching what Finder shows.
pub fn bytes(n: u64) -> String {
    if n < 1000 {
        return format!("{n} B");
    }
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    let precision = if value >= 100.0 { 0 } else { 1 };
    format!("{value:.precision$} {}", UNITS[unit])
}

/// Parses sizes like `500`, `10K`, `100MB`, `1.5G` (decimal units).
pub fn parse_size(input: &str) -> Result<u64, String> {
    let s = input.trim().to_ascii_uppercase();
    let split = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(s.len());
    let (num, unit) = s.split_at(split);
    let value: f64 = num.parse().map_err(|_| format!("invalid size: {input}"))?;
    let mult: f64 = match unit.trim_end_matches('B') {
        "" => 1.0,
        "K" => 1e3,
        "M" => 1e6,
        "G" => 1e9,
        "T" => 1e12,
        _ => return Err(format!("invalid size unit: {input}")),
    };
    if value < 0.0 {
        return Err(format!("size must be positive: {input}"));
    }
    Ok((value * mult) as u64)
}

/// Whole days elapsed since `t` (0 if in the future).
pub fn days_since(t: SystemTime) -> u64 {
    SystemTime::now()
        .duration_since(t)
        .unwrap_or(Duration::ZERO)
        .as_secs()
        / 86_400
}

/// Shortens a path by replacing the home directory with `~`.
pub fn tilde(path: &std::path::Path) -> String {
    let home = super::fsx::home();
    match path.strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// Text bar of `width` cells filled proportionally to `ratio` (0..=1).
pub fn bar(ratio: f64, width: usize) -> String {
    let filled = ((ratio.clamp(0.0, 1.0)) * width as f64).round() as usize;
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_bytes() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(999), "999 B");
        assert_eq!(bytes(1_500), "1.5 KB");
        assert_eq!(bytes(250_000_000), "250 MB");
        assert_eq!(bytes(3_210_000_000), "3.2 GB");
    }

    #[test]
    fn parses_sizes() {
        assert_eq!(parse_size("500").unwrap(), 500);
        assert_eq!(parse_size("10k").unwrap(), 10_000);
        assert_eq!(parse_size("100MB").unwrap(), 100_000_000);
        assert_eq!(parse_size("1.5G").unwrap(), 1_500_000_000);
        assert!(parse_size("abc").is_err());
        assert!(parse_size("10Q").is_err());
    }

    #[test]
    fn days_since_future_is_zero() {
        let future = SystemTime::now() + Duration::from_secs(10_000);
        assert_eq!(days_since(future), 0);
        let past = SystemTime::now() - Duration::from_secs(3 * 86_400 + 5);
        assert_eq!(days_since(past), 3);
    }

    #[test]
    fn bar_is_proportional() {
        assert_eq!(bar(0.5, 4), "██░░");
        assert_eq!(bar(2.0, 2), "██");
    }
}
