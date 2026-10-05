//! Interactive terminal UI. Redraws only when state changes, so it idles
//! at ~0% CPU.

mod state;
mod ui;

use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind};
use std::time::Duration;

pub fn run() -> Result<()> {
    let mut terminal = ratatui::init();
    let mut app = state::App::new();
    let mut dirty = true;
    let result = loop {
        dirty |= app.drain();
        dirty |= app.tick();
        if dirty {
            if let Err(e) = terminal.draw(|f| ui::draw(f, &mut app)) {
                break Err(e.into());
            }
            dirty = false;
        }
        match event::poll(Duration::from_millis(200)) {
            Ok(true) => match event::read() {
                Ok(Event::Key(k)) if k.kind == KeyEventKind::Press => {
                    app.on_key(k);
                    dirty = true;
                }
                Ok(Event::Resize(..)) => dirty = true,
                Ok(_) => {}
                Err(e) => break Err(e.into()),
            },
            Ok(false) => {}
            Err(e) => break Err(e.into()),
        }
        if app.quit {
            break Ok(());
        }
    };
    ratatui::restore();
    result
}
