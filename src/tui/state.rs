//! TUI application state and key handling (no drawing here).

use crate::executor::{self, DeleteMode};
use crate::model::{Item, Report, Target};
use crate::modules::{ALL, Module};
use crate::monitor::{Monitor, Snapshot};
use crate::util::fsx;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::TableState;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

pub enum Status {
    Idle,
    Busy(&'static str),
    Done(String),
}

pub struct ModState {
    pub items: Vec<Item>,
    pub table: TableState,
    pub status: Status,
    pub scanned: bool,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum Focus {
    Sidebar,
    List,
}

enum Msg {
    Scanned(usize, Vec<Item>),
    Acted(usize, Report),
}

pub struct App {
    pub current: usize,
    pub focus: Focus,
    pub mods: Vec<ModState>,
    pub space_path: PathBuf,
    pub confirm: bool,
    pub permanent: bool,
    pub help: bool,
    pub quit: bool,
    pub snapshot: Snapshot,
    monitor: Option<Monitor>,
    last_sample: Instant,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

impl App {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        let mods = ALL
            .iter()
            .map(|_| ModState { items: Vec::new(), table: TableState::default(), status: Status::Idle, scanned: false })
            .collect();
        Self {
            current: 0,
            focus: Focus::Sidebar,
            mods,
            space_path: fsx::home(),
            confirm: false,
            permanent: false,
            help: false,
            quit: false,
            snapshot: Snapshot::default(),
            monitor: None,
            last_sample: Instant::now(),
            tx,
            rx,
        }
    }

    pub fn module(&self) -> Module {
        ALL[self.current]
    }

    pub fn state(&self) -> &ModState {
        &self.mods[self.current]
    }

    fn state_mut(&mut self) -> &mut ModState {
        &mut self.mods[self.current]
    }

    fn busy(&self) -> bool {
        matches!(self.state().status, Status::Busy(_))
    }

    /// Applies finished background work. Returns true if anything changed.
    pub fn drain(&mut self) -> bool {
        let mut changed = false;
        while let Ok(msg) = self.rx.try_recv() {
            changed = true;
            match msg {
                Msg::Scanned(idx, items) => {
                    let st = &mut self.mods[idx];
                    let n = items.len();
                    st.items = items;
                    st.scanned = true;
                    st.table.select(if n > 0 { Some(0) } else { None });
                    st.status = Status::Done(format!("Found {n} item(s)"));
                }
                Msg::Acted(idx, report) => {
                    let mut text = report.summary();
                    if let Some(f) = report.failed.first() {
                        text.push_str(&format!(" — {f}"));
                    } else if let Some(m) = report.messages.last() {
                        text.push_str(&format!(" — {m}"));
                    }
                    self.mods[idx].status = Status::Done(text);
                    self.rescan(idx);
                }
            }
        }
        changed
    }

    /// Samples the monitor at most once a second, only while visible.
    pub fn tick(&mut self) -> bool {
        if self.module() != Module::Monitor || self.last_sample.elapsed().as_millis() < 1000 {
            return false;
        }
        let mon = self.monitor.get_or_insert_with(Monitor::new);
        self.snapshot = mon.sample(true);
        self.last_sample = Instant::now();
        true
    }

    fn rescan(&mut self, idx: usize) {
        let module = ALL[idx];
        // Processes/agents/apps change after acting; junk lists do too.
        if module != Module::Maintenance && module != Module::Updates {
            self.start_scan(idx);
        }
    }

    pub fn start_scan(&mut self, idx: usize) {
        let module = ALL[idx];
        if module == Module::Monitor || matches!(self.mods[idx].status, Status::Busy(_)) {
            return;
        }
        self.mods[idx].status = Status::Busy("Scanning…");
        let tx = self.tx.clone();
        let root = if module == Module::SpaceLens { self.space_path.clone() } else { fsx::home() };
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Scanned(idx, module.scan(&root)));
        });
    }

    fn start_action(&mut self) {
        let idx = self.current;
        let items: Vec<Item> = self.mods[idx].items.iter().filter(|i| i.selected).cloned().collect();
        if items.is_empty() {
            return;
        }
        self.mods[idx].status = Status::Busy("Working…");
        let mode = if self.permanent { DeleteMode::Permanent } else { DeleteMode::Trash };
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Acted(idx, executor::execute(&items, mode)));
        });
    }

    fn move_cursor(&mut self, delta: isize) {
        if self.focus == Focus::Sidebar {
            let n = ALL.len() as isize;
            self.current = ((self.current as isize + delta).rem_euclid(n)) as usize;
            if !self.state().scanned && self.module() == Module::Maintenance {
                self.start_scan(self.current);
            }
            return;
        }
        let st = self.state_mut();
        let len = st.items.len();
        if len == 0 {
            return;
        }
        let cur = st.table.selected().unwrap_or(0) as isize;
        st.table.select(Some((cur + delta).clamp(0, len as isize - 1) as usize));
    }

    fn toggle_current(&mut self) {
        let st = self.state_mut();
        if let Some(i) = st.table.selected().and_then(|i| st.items.get_mut(i)) {
            if i.is_actionable() {
                i.selected = !i.selected;
            }
        }
    }

    fn select_all(&mut self, on: bool) {
        self.state_mut().items.iter_mut().filter(|i| i.is_actionable()).for_each(|i| i.selected = on);
    }

    fn space_enter(&mut self) {
        let st = self.state();
        let Some(item) = st.table.selected().and_then(|i| st.items.get(i)) else { return };
        if let Target::Files { paths, .. } = &item.target {
            if paths[0].is_dir() {
                self.space_path = paths[0].clone();
                self.start_scan(self.current);
            }
        }
    }

    fn space_up(&mut self) {
        if let Some(parent) = self.space_path.parent() {
            self.space_path = parent.to_path_buf();
            self.start_scan(self.current);
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if self.confirm {
            match key.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    self.confirm = false;
                    self.start_action();
                }
                _ => self.confirm = false,
            }
            return;
        }
        if self.help {
            self.help = false;
            return;
        }
        self.on_normal_key(key.code);
    }

    fn on_normal_key(&mut self, code: KeyCode) {
        let in_space = self.module() == Module::SpaceLens;
        match code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Char('?') => self.help = true,
            KeyCode::Tab => self.focus = if self.focus == Focus::Sidebar { Focus::List } else { Focus::Sidebar },
            KeyCode::Left | KeyCode::Char('h') => self.focus = Focus::Sidebar,
            KeyCode::Right | KeyCode::Char('l') => self.focus = Focus::List,
            KeyCode::Up | KeyCode::Char('k') => self.move_cursor(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_cursor(1),
            KeyCode::PageUp => self.move_cursor(-15),
            KeyCode::PageDown => self.move_cursor(15),
            KeyCode::Char('s') | KeyCode::Char('r') => self.start_scan(self.current),
            KeyCode::Char('p') => self.permanent = !self.permanent,
            KeyCode::Char(' ') if self.focus == Focus::List => self.toggle_current(),
            KeyCode::Char('a') => self.select_all(true),
            KeyCode::Char('n') => self.select_all(false),
            KeyCode::Char('c') | KeyCode::Char('x') | KeyCode::Delete => {
                let any = self.state().items.iter().any(|i| i.selected);
                if any && !self.busy() && !self.module().verb().is_empty() {
                    self.confirm = true;
                }
            }
            KeyCode::Backspace if in_space => self.space_up(),
            KeyCode::Enter if self.focus == Focus::Sidebar => {
                self.focus = Focus::List;
                if !self.state().scanned {
                    self.start_scan(self.current);
                }
            }
            KeyCode::Enter if in_space => self.space_enter(),
            KeyCode::Enter => self.toggle_current(),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    #[test]
    fn sidebar_navigation_wraps() {
        let mut app = App::new();
        app.on_key(key(KeyCode::Up));
        assert_eq!(app.module(), Module::Monitor);
        app.on_key(key(KeyCode::Down));
        assert_eq!(app.module(), Module::SmartCare);
    }

    #[test]
    fn selection_and_confirm_flow() {
        let mut app = App::new();
        app.mods[0].items = vec![
            Item::files("g", "a".into(), vec![PathBuf::from("/x")], 1).with_selected(false),
        ];
        app.mods[0].table.select(Some(0));
        app.focus = Focus::List;
        app.on_key(key(KeyCode::Char('c')));
        assert!(!app.confirm, "nothing selected yet");
        app.on_key(key(KeyCode::Char(' ')));
        assert!(app.mods[0].items[0].selected);
        app.on_key(key(KeyCode::Char('c')));
        assert!(app.confirm);
        app.on_key(key(KeyCode::Char('n')));
        assert!(!app.confirm);
        app.on_key(key(KeyCode::Char('n')));
        assert!(!app.mods[0].items[0].selected);
    }

    #[test]
    fn quits_and_toggles_permanent() {
        let mut app = App::new();
        app.on_key(key(KeyCode::Char('p')));
        assert!(app.permanent);
        app.on_key(key(KeyCode::Char('q')));
        assert!(app.quit);
    }

    fn wait_for(app: &mut App, idx: usize) {
        for _ in 0..200 {
            app.drain();
            if !matches!(app.mods[idx].status, Status::Busy(_)) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        panic!("background job did not finish");
    }

    fn index_of(m: Module) -> usize {
        ALL.iter().position(|x| *x == m).unwrap()
    }

    #[test]
    fn background_scan_and_action_round_trip() {
        let mut app = App::new();
        let idx = index_of(Module::Maintenance);
        app.current = idx;
        app.focus = Focus::Sidebar;
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.focus, Focus::List);
        wait_for(&mut app, idx);
        assert!(app.mods[idx].scanned && !app.mods[idx].items.is_empty());
        app.mods[idx].items = vec![Item {
            target: Target::Command { argv: vec!["/bin/echo".into(), "ok".into()], admin: false },
            ..Item::files("c", "echo".into(), vec![], 0)
        }];
        app.on_key(key(KeyCode::Char('c')));
        app.on_key(key(KeyCode::Char('y')));
        wait_for(&mut app, idx);
        assert!(matches!(&app.mods[idx].status, Status::Done(t) if t.contains("1 done")));
    }

    #[test]
    fn space_lens_navigation() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("sub")).unwrap();
        std::fs::write(tmp.path().join("sub/f"), vec![1u8; 5000]).unwrap();
        let mut app = App::new();
        let idx = index_of(Module::SpaceLens);
        app.current = idx;
        app.focus = Focus::List;
        app.space_path = tmp.path().to_path_buf();
        app.on_key(key(KeyCode::Char('s')));
        wait_for(&mut app, idx);
        app.on_key(key(KeyCode::Enter));
        wait_for(&mut app, idx);
        assert_eq!(app.space_path, tmp.path().join("sub"));
        app.on_key(key(KeyCode::Backspace));
        wait_for(&mut app, idx);
        assert_eq!(app.space_path, tmp.path());
    }

    #[test]
    fn cursor_help_and_monitor_tick() {
        let mut app = App::new();
        app.mods[0].items = (0..5).map(|i| Item::files("g", i.to_string(), vec![], 0)).collect();
        app.mods[0].table.select(Some(0));
        app.on_key(key(KeyCode::Tab));
        app.on_key(key(KeyCode::PageDown));
        assert_eq!(app.mods[0].table.selected(), Some(4));
        app.on_key(key(KeyCode::Char('k')));
        assert_eq!(app.mods[0].table.selected(), Some(3));
        app.on_key(key(KeyCode::Char('a')));
        assert!(app.mods[0].items.iter().all(|i| i.selected));
        app.on_key(key(KeyCode::Enter));
        assert!(!app.mods[0].items[3].selected);
        app.on_key(key(KeyCode::Char('h')));
        assert_eq!(app.focus, Focus::Sidebar);
        app.on_key(key(KeyCode::Char('?')));
        assert!(app.help);
        app.on_key(key(KeyCode::Char('x')));
        assert!(!app.help);
        assert!(!app.tick(), "no sampling off the monitor tab");
        app.current = index_of(Module::Monitor);
        app.last_sample = Instant::now() - std::time::Duration::from_secs(2);
        assert!(app.tick());
        app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(app.quit);
    }
}
