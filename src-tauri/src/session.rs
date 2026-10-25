//! Sessions: one shell on a pseudo-terminal per tab.
//!
//! Each session has two threads. alacritty's event loop does the PTY I/O and parsing. A worker
//! owns everything else: it is the only thread that locks the terminal on our side, paces frames
//! to the renderer, answers terminal queries and keeps the title up to date. Commands only send
//! messages to the worker, so they never block on the terminal lock and keep their order.

use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::Scroll;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config, TermMode};
use alacritty_terminal::Term;
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, State, WebviewWindow};

use crate::platform::{self, PtySpec};
use crate::settings::SettingsStore;
use crate::term_core::{self, Frame, Size};
use crate::title::{TitleState, Titles};

/// Minimum time between two frames of one session.
const FRAME_INTERVAL: Duration = Duration::from_millis(8);
/// How soon after output the foreground process and directory are checked again.
const ACTIVE_POLL: Duration = Duration::from_millis(250);
/// How often they are checked while the session is quiet.
const IDLE_POLL: Duration = Duration::from_secs(1);

/// Messages from a session to its tab.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SessionEvent {
    Frame(Frame),
    Title(Titles),
    Exited { code: Option<i32> },
}

type Sink = Box<dyn Fn(SessionEvent) + Send>;

enum WorkerMsg {
    Term(Event),
    Input(Vec<u8>),
    Paste(String),
    Resize(WindowSize),
    Scroll(i32),
    Visible(bool),
    Close,
}

/// Forwards terminal events to the worker. alacritty sends some of them while holding the
/// terminal lock, so this must never lock the terminal itself.
#[derive(Clone)]
struct Listener(Sender<WorkerMsg>);

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let _ = self.0.send(WorkerMsg::Term(event));
    }
}

struct Session {
    window: String,
    tx: Sender<WorkerMsg>,
    master: Arc<File>,
    cwd: Arc<Mutex<Option<PathBuf>>>,
}

/// What a new session runs and how big it starts.
pub struct SpawnOptions {
    pub size: WindowSize,
    pub cwd: PathBuf,
    /// Program and arguments to run instead of the login shell.
    pub shell: Option<(String, Vec<String>)>,
    pub scrollback: usize,
    pub app_name: String,
    pub app_version: String,
}

/// All sessions of the app, keyed by an id that is never reused.
#[derive(Default)]
pub struct Sessions {
    next_id: AtomicU64,
    sessions: Mutex<HashMap<u64, Session>>,
}

impl Sessions {
    fn spawn(&self, window: &str, options: SpawnOptions, sink: Sink) -> io::Result<u64> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let size = options.size;

        let pty = platform::spawn_pty(
            PtySpec {
                shell: options.shell,
                cwd: options.cwd.clone(),
                size,
                app_version: options.app_version,
            },
            id,
        )?;
        // Our own handle on the master side, for `tcgetpgrp`, that outlives the event loop's.
        let master = Arc::new(pty.file().try_clone()?);

        let (tx, rx) = mpsc::channel();
        let listener = Listener(tx.clone());
        // kitty_keyboard stays off: 0.26 can panic when its keyboard mode stack overflows.
        let config = Config {
            scrolling_history: options.scrollback,
            kitty_keyboard: false,
            ..Config::default()
        };
        let grid = Size {
            cols: usize::from(size.num_cols),
            rows: usize::from(size.num_lines),
        };
        let term = Arc::new(FairMutex::new(Term::new(config, &grid, listener.clone())));

        let event_loop = EventLoop::new(term.clone(), listener, pty, true, false)?;
        let pty_tx = event_loop.channel();
        event_loop.spawn();

        let cwd = Arc::new(Mutex::new(Some(options.cwd)));
        let worker = Worker {
            rx,
            term,
            pty: pty_tx,
            master: master.clone(),
            sink,
            size,
            visible: true,
            dirty: false,
            force_full: true,
            offset: 0,
            last_frame: Instant::now() - FRAME_INTERVAL,
            next_poll: Instant::now() + ACTIVE_POLL,
            titles: TitleState::default(),
            published: None,
            exit_code: None,
            home: platform::home_dir(),
            app_name: options.app_name,
            cwd: cwd.clone(),
            running: true,
        };
        std::thread::Builder::new()
            .name(format!("session-{id}"))
            .spawn(move || worker.run())?;

        let session = Session {
            window: window.to_owned(),
            tx,
            master,
            cwd,
        };
        self.sessions.lock().unwrap().insert(id, session);
        Ok(id)
    }

    fn send(&self, id: u64, msg: WorkerMsg) -> Result<(), String> {
        let sessions = self.sessions.lock().unwrap();
        let session = sessions
            .get(&id)
            .ok_or_else(|| format!("no session {id}"))?;
        // The worker has stopped if the shell exited; the tab is about to close anyway.
        let _ = session.tx.send(msg);
        Ok(())
    }

    /// The working directory of the session's foreground process, or the last one known.
    pub fn cwd(&self, id: u64) -> Option<PathBuf> {
        let sessions = self.sessions.lock().unwrap();
        let session = sessions.get(&id)?;
        platform::foreground_pgid(session.master.as_raw_fd())
            .and_then(platform::process_cwd)
            .or_else(|| session.cwd.lock().unwrap().clone())
    }

    fn close(&self, id: u64) {
        if let Some(session) = self.sessions.lock().unwrap().remove(&id) {
            let _ = session.tx.send(WorkerMsg::Close);
        }
    }

    /// Ends every session of a window, e.g. when it closes or its page reloads.
    pub fn close_window(&self, window: &str) {
        let mut sessions = self.sessions.lock().unwrap();
        sessions.retain(|_, session| {
            let keep = session.window != window;
            if !keep {
                let _ = session.tx.send(WorkerMsg::Close);
            }
            keep
        });
    }
}

struct Worker {
    rx: Receiver<WorkerMsg>,
    term: Arc<FairMutex<Term<Listener>>>,
    pty: EventLoopSender,
    master: Arc<File>,
    sink: Sink,
    size: WindowSize,
    /// Hidden tabs get no frames; they get a full one when shown again.
    visible: bool,
    dirty: bool,
    force_full: bool,
    /// Display offset of the last frame; non-zero while scrolled back.
    offset: usize,
    last_frame: Instant,
    next_poll: Instant,
    titles: TitleState,
    published: Option<Titles>,
    exit_code: Option<i32>,
    home: PathBuf,
    app_name: String,
    cwd: Arc<Mutex<Option<PathBuf>>>,
    running: bool,
}

impl Worker {
    fn run(mut self) {
        while self.running {
            let timeout = self
                .next_deadline()
                .saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(timeout) {
                Ok(msg) => self.handle(msg),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            // Take everything already queued before drawing, so bursts become one frame.
            while self.running {
                match self.rx.try_recv() {
                    Ok(msg) => self.handle(msg),
                    Err(_) => break,
                }
            }
            if self.running {
                self.render_if_due();
                self.poll_if_due();
            }
        }
    }

    fn next_deadline(&self) -> Instant {
        if self.dirty && self.visible {
            self.next_poll.min(self.last_frame + FRAME_INTERVAL)
        } else {
            self.next_poll
        }
    }

    fn handle(&mut self, msg: WorkerMsg) {
        match msg {
            WorkerMsg::Term(event) => self.handle_term_event(event),
            WorkerMsg::Input(bytes) => self.write(bytes),
            WorkerMsg::Paste(text) => {
                let bracketed = self.term.lock().mode().contains(TermMode::BRACKETED_PASTE);
                self.write(paste_bytes(&text, bracketed));
            }
            WorkerMsg::Resize(size) => {
                self.size = size;
                let grid = Size {
                    cols: usize::from(size.num_cols),
                    rows: usize::from(size.num_lines),
                };
                self.term.lock().resize(grid);
                let _ = self.pty.send(Msg::Resize(size));
                self.dirty = true;
            }
            WorkerMsg::Scroll(delta) => {
                self.term.lock().scroll_display(Scroll::Delta(delta));
                self.dirty = true;
            }
            WorkerMsg::Visible(visible) => {
                self.visible = visible;
                if visible {
                    self.force_full = true;
                    self.dirty = true;
                }
            }
            WorkerMsg::Close => {
                let _ = self.pty.send(Msg::Shutdown);
                self.running = false;
            }
        }
    }

    fn handle_term_event(&mut self, event: Event) {
        match event {
            Event::Wakeup => {
                self.dirty = true;
                self.next_poll = self.next_poll.min(Instant::now() + ACTIVE_POLL);
            }
            Event::Title(title) => {
                // The owner is whoever is in the foreground right now, not at the last poll.
                let pgid = platform::foreground_pgid(self.master.as_raw_fd());
                self.titles.set_program_title(&title, pgid);
                self.publish_titles();
            }
            Event::ResetTitle => {
                self.titles.reset_program_title();
                self.publish_titles();
            }
            // Replies to device status and attribute queries; programs wait for these.
            Event::PtyWrite(text) => {
                let _ = self.pty.send(Msg::Input(text.into_bytes().into()));
            }
            Event::TextAreaSizeRequest(format) => {
                let _ = self
                    .pty
                    .send(Msg::Input(format(self.size).into_bytes().into()));
            }
            Event::ChildExit(status) => self.exit_code = status.code(),
            Event::Exit => {
                if self.dirty && self.visible {
                    self.render();
                }
                (self.sink)(SessionEvent::Exited {
                    code: self.exit_code,
                });
                self.running = false;
            }
            // Colour queries (THM-06), clipboard (VIM-23), bell (TERM-16) and the rest come later.
            _ => {}
        }
    }

    fn write(&mut self, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        // Typing returns the view to the bottom (TERM-07).
        if self.offset > 0 {
            self.term.lock().scroll_display(Scroll::Bottom);
            self.dirty = true;
        }
        let _ = self.pty.send(Msg::Input(bytes.into()));
    }

    fn render_if_due(&mut self) {
        if self.dirty && self.visible && Instant::now() >= self.last_frame + FRAME_INTERVAL {
            self.render();
        }
    }

    fn render(&mut self) {
        let frame = term_core::frame(&mut self.term.lock(), self.force_full);
        self.offset = frame.offset;
        self.dirty = false;
        self.force_full = false;
        self.last_frame = Instant::now();
        (self.sink)(SessionEvent::Frame(frame));
    }

    fn poll_if_due(&mut self) {
        let now = Instant::now();
        if now < self.next_poll {
            return;
        }
        self.next_poll = now + IDLE_POLL;

        let pgid = platform::foreground_pgid(self.master.as_raw_fd());
        let program = pgid.and_then(platform::process_name);
        let cwd = pgid.and_then(platform::process_cwd);
        self.titles.update_process(pgid, program, cwd);
        if let Some(cwd) = self.titles.cwd() {
            *self.cwd.lock().unwrap() = Some(cwd.to_owned());
        }
        self.publish_titles();
    }

    fn publish_titles(&mut self) {
        let titles = self.titles.compose(&self.home, &self.app_name);
        if self.published.as_ref() != Some(&titles) {
            self.published = Some(titles.clone());
            (self.sink)(SessionEvent::Title(titles));
        }
    }
}

/// Encodes pasted text. Line endings become CR like typed Returns. In bracketed paste mode, ESC
/// and C1 CSI are removed so the text cannot end the paste early and run as commands.
fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed {
        let text: String = text
            .chars()
            .filter(|&c| c != '\x1b' && c != '\u{9b}')
            .collect();
        format!("\x1b[200~{text}\x1b[201~").into_bytes()
    } else {
        text.into_bytes()
    }
}

/// Converts the frontend's measurements to a PTY size. Pixel sizes are clamped so their products
/// fit the kernel's 16-bit window size fields.
fn window_size(cols: u16, rows: u16, cell_width: f64, cell_height: f64) -> WindowSize {
    let cell = |px: f64| px.round().clamp(1.0, 60.0) as u16;
    WindowSize {
        num_cols: cols.clamp(1, 1000),
        num_lines: rows.clamp(1, 1000),
        cell_width: cell(cell_width),
        cell_height: cell(cell_height),
    }
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn session_spawn(
    app: AppHandle,
    window: WebviewWindow,
    sessions: State<'_, Sessions>,
    settings: State<'_, SettingsStore>,
    on_event: Channel<SessionEvent>,
    cols: u16,
    rows: u16,
    cell_width: f64,
    cell_height: f64,
    cwd: Option<String>,
    cwd_from: Option<u64>,
) -> Result<u64, String> {
    let settings = settings.get();
    let cwd = cwd_from
        .and_then(|id| sessions.cwd(id))
        .or_else(|| cwd.map(PathBuf::from))
        .filter(|dir| dir.is_dir())
        .unwrap_or_else(platform::home_dir);
    let info = app.package_info();
    let options = SpawnOptions {
        size: window_size(cols, rows, cell_width, cell_height),
        cwd,
        shell: settings.shell.map(|shell| (shell, vec!["-l".to_owned()])),
        scrollback: settings.scrollback_lines,
        app_name: info.name.clone(),
        app_version: info.version.to_string(),
    };
    let sink: Sink = Box::new(move |event| {
        let _ = on_event.send(event);
    });
    sessions
        .spawn(window.label(), options, sink)
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub fn session_input(sessions: State<'_, Sessions>, id: u64, data: String) -> Result<(), String> {
    sessions.send(id, WorkerMsg::Input(data.into_bytes()))
}

#[tauri::command]
pub fn session_paste(sessions: State<'_, Sessions>, id: u64, text: String) -> Result<(), String> {
    sessions.send(id, WorkerMsg::Paste(text))
}

#[tauri::command]
pub fn session_resize(
    sessions: State<'_, Sessions>,
    id: u64,
    cols: u16,
    rows: u16,
    cell_width: f64,
    cell_height: f64,
) -> Result<(), String> {
    sessions.send(
        id,
        WorkerMsg::Resize(window_size(cols, rows, cell_width, cell_height)),
    )
}

#[tauri::command]
pub fn session_scroll(sessions: State<'_, Sessions>, id: u64, delta: i32) -> Result<(), String> {
    sessions.send(id, WorkerMsg::Scroll(delta))
}

#[tauri::command]
pub fn session_set_visible(
    sessions: State<'_, Sessions>,
    id: u64,
    visible: bool,
) -> Result<(), String> {
    sessions.send(id, WorkerMsg::Visible(visible))
}

#[tauri::command]
pub fn session_close(sessions: State<'_, Sessions>, id: u64) {
    sessions.close(id);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spawn(sessions: &Sessions, program: &str, args: &[&str]) -> Receiver<SessionEvent> {
        let (tx, rx) = mpsc::channel();
        let options = SpawnOptions {
            size: window_size(40, 5, 8.0, 16.0),
            cwd: std::env::temp_dir(),
            shell: Some((
                program.into(),
                args.iter().map(|arg| arg.to_string()).collect(),
            )),
            scrollback: 100,
            app_name: "Twominal".into(),
            app_version: "0.0.0".into(),
        };
        let sink: Sink = Box::new(move |event| {
            let _ = tx.send(event);
        });
        sessions.spawn("win-test", options, sink).unwrap();
        rx
    }

    fn collect_until_exit(rx: &Receiver<SessionEvent>) -> (String, Option<i32>) {
        let mut text = String::new();
        loop {
            match rx
                .recv_timeout(Duration::from_secs(10))
                .expect("session should exit")
            {
                SessionEvent::Frame(frame) => {
                    for row in frame.lines {
                        text.extend(row.1.into_iter().map(|run| run.1));
                        text.push('\n');
                    }
                }
                SessionEvent::Exited { code } => return (text, code),
                SessionEvent::Title(_) => {}
            }
        }
    }

    #[test]
    fn runs_a_program_and_reports_its_output_and_exit_code() {
        let sessions = Sessions::default();
        let rx = spawn(
            &sessions,
            "/bin/sh",
            &["-c", "printf 'hi from %s' \"$TERM_PROGRAM\"; exit 3"],
        );
        let (text, code) = collect_until_exit(&rx);
        assert!(text.contains("hi from Twominal"), "frames were: {text:?}");
        assert_eq!(code, Some(3));
    }

    #[test]
    fn input_reaches_the_program() {
        let sessions = Sessions::default();
        let rx = spawn(
            &sessions,
            "/bin/sh",
            &["-c", "read line; printf 'got:%s' \"$line\""],
        );
        sessions
            .send(1, WorkerMsg::Input(b"ping\r".to_vec()))
            .unwrap();
        let (text, code) = collect_until_exit(&rx);
        assert!(text.contains("got:ping"), "frames were: {text:?}");
        assert_eq!(code, Some(0));
    }

    #[test]
    fn closing_a_window_ends_its_sessions() {
        let sessions = Sessions::default();
        let _rx = spawn(&sessions, "/bin/sleep", &["30"]);
        sessions.close_window("win-test");
        assert!(sessions.sessions.lock().unwrap().is_empty());
    }

    #[test]
    fn paste_is_bracketed_and_cannot_escape() {
        assert_eq!(paste_bytes("a\nb", false), b"a\rb");
        assert_eq!(
            paste_bytes("x\x1b[201~rm -rf ~\n", true),
            b"\x1b[200~x[201~rm -rf ~\r\x1b[201~"
        );
    }

    #[test]
    fn window_size_is_clamped_to_kernel_limits() {
        let size = window_size(0, 5000, 0.2, 400.0);
        assert_eq!(
            (
                size.num_cols,
                size.num_lines,
                size.cell_width,
                size.cell_height
            ),
            (1, 1000, 1, 60)
        );
    }
}
