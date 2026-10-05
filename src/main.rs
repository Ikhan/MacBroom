//! macbroom — a fast, low-memory CleanMyMac-style utility for macOS.

mod apps;
mod cli;
mod executor;
mod model;
mod modules;
mod monitor;
mod perf;
mod scan;
mod shred;
mod tui;
mod util;

use clap::Parser;

fn main() {
    let cli = cli::Cli::parse();
    let result = match cli.command {
        Some(cmd) => cli::run(cmd),
        None => tui::run(),
    };
    if let Err(e) = result {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}
