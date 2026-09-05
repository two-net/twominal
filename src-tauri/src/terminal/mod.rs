mod flow;

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, Receiver, RecvTimeoutError, Sender, TrySendError};
use serde::Serialize;
use tauri::ipc::{Channel, InvokeResponseBody};
use uuid::Uuid;

use crate::error::CommandError;
use crate::logging;
use crate::pty::{NativePtyBackend, ProcessExit, PtyBackend, PtyChild, SpawnedPty, TerminalSize};
use crate::shell::{detect_default_shell, ShellIntegrationPaths, ShellLaunch};
use flow::{FlowError, OutputFlow, OUTPUT_BATCH_BYTES};

const INPUT_QUEUE_DEPTH: usize = 16;
const MAX_INPUT_BYTES: usize = 64 * 1024;
const CLOSED_SESSION_HISTORY: usize = 128;
const CHILD_POLL_INTERVAL: Duration = Duration::from_millis(250);
const CHILD_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_TRANSFER_REPLAY_BYTES: usize = 8 * 1024 * 1024;

const STATE_RUNNING: u8 = 0;
const STATE_EXITED: u8 = 1;
const STATE_FAILED: u8 = 2;
const STATE_CLOSING: u8 = 3;
const STATE_CLOSED: u8 = 4;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDescriptor {
    pub session_id: String,
    pub shell_name: String,
    pub cwd: String,
    pub shell_integration: bool,
    pub shell_integration_nonce: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum LifecycleEvent {
    Exited {
        #[serde(rename = "exitCode")]
        exit_code: u32,
        signal: Option<String>,
    },
    Error {
        code: &'static str,
        message: &'static str,
    },
}

#[derive(Debug)]
enum SupervisorCommand {
    Terminate,
}

struct SessionControl {
    state: AtomicU8,
    flow: Arc<OutputFlow>,
    output: Mutex<Channel<InvokeResponseBody>>,
    lifecycle: Mutex<Channel<LifecycleEvent>>,
    replay: Mutex<ReplayBuffer>,
    last_lifecycle: Mutex<Option<LifecycleEvent>>,
    input: Mutex<Option<Sender<Vec<u8>>>>,
    writer_stop: Sender<()>,
    supervisor: Sender<SupervisorCommand>,
    supervisor_done: Receiver<()>,
}

impl SessionControl {
    fn is_running(&self) -> bool {
        self.state.load(Ordering::Acquire) == STATE_RUNNING
    }

    fn process_exited(&self, exit: ProcessExit) {
        if self
            .state
            .compare_exchange(
                STATE_RUNNING,
                STATE_EXITED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            self.stop_writer();
            logging::info("terminal_process_exited");
            let event = LifecycleEvent::Exited {
                exit_code: exit.exit_code,
                signal: exit.signal,
            };
            *lock(&self.last_lifecycle) = Some(event.clone());
            let _ = lock(&self.lifecycle).send(event);
        }
    }

    fn fail(&self, code: &'static str, message: &'static str) {
        if self
            .state
            .compare_exchange(
                STATE_RUNNING,
                STATE_FAILED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            self.flow.close();
            self.stop_writer();
            logging::error("terminal_session_failed", code);
            let event = LifecycleEvent::Error { code, message };
            *lock(&self.last_lifecycle) = Some(event.clone());
            let _ = lock(&self.lifecycle).send(event);
            let _ = self.supervisor.try_send(SupervisorCommand::Terminate);
        }
    }

    fn begin_close(&self) -> bool {
        loop {
            let state = self.state.load(Ordering::Acquire);
            if state == STATE_CLOSING || state == STATE_CLOSED {
                return false;
            }
            if self
                .state
                .compare_exchange(state, STATE_CLOSING, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return true;
            }
        }
    }

    fn finish_close(&self) {
        self.state.store(STATE_CLOSED, Ordering::Release);
    }

    fn stop_writer(&self) {
        self.lock_input().take();
        let _ = self.writer_stop.try_send(());
    }

    fn lock_input(&self) -> MutexGuard<'_, Option<Sender<Vec<u8>>>> {
        self.input
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[derive(Default)]
struct ReplayBuffer {
    bytes: VecDeque<u8>,
    truncated: bool,
}

impl ReplayBuffer {
    fn push(&mut self, bytes: &[u8]) {
        if bytes.len() >= MAX_TRANSFER_REPLAY_BYTES {
            self.bytes.clear();
            self.bytes.extend(
                bytes[bytes.len() - MAX_TRANSFER_REPLAY_BYTES..]
                    .iter()
                    .copied(),
            );
            self.truncated = true;
            return;
        }

        let overflow = self
            .bytes
            .len()
            .saturating_add(bytes.len())
            .saturating_sub(MAX_TRANSFER_REPLAY_BYTES);
        if overflow > 0 {
            self.bytes.drain(..overflow);
            self.truncated = true;
        }
        self.bytes.extend(bytes.iter().copied());
    }

    fn snapshot(&self) -> Vec<u8> {
        let reset = self.truncated.then_some(b"\x1bc".as_slice());
        let mut snapshot =
            Vec::with_capacity(self.bytes.len() + reset.map_or(0, |bytes| bytes.len()));
        if let Some(reset) = reset {
            snapshot.extend_from_slice(reset);
        }
        snapshot.extend(self.bytes.iter().copied());
        snapshot
    }
}

struct TerminalSession {
    owner: Mutex<String>,
    descriptor: SessionDescriptor,
    cwd: PathBuf,
    master: Mutex<Option<Box<dyn crate::pty::PtyMaster>>>,
    control: Arc<SessionControl>,
}

impl TerminalSession {
    fn start(
        id: Uuid,
        owner: String,
        descriptor: SessionDescriptor,
        cwd: PathBuf,
        spawned: SpawnedPty,
        output: Channel<InvokeResponseBody>,
        lifecycle: Channel<LifecycleEvent>,
    ) -> Result<Arc<Self>, CommandError> {
        let SpawnedPty {
            master,
            reader,
            writer,
            child,
        } = spawned;
        let (input_tx, input_rx) = bounded(INPUT_QUEUE_DEPTH);
        let (writer_stop_tx, writer_stop_rx) = bounded(1);
        let (supervisor_tx, supervisor_rx) = bounded(1);
        let (supervisor_done_tx, supervisor_done_rx) = bounded(1);
        let control = Arc::new(SessionControl {
            state: AtomicU8::new(STATE_RUNNING),
            flow: Arc::new(OutputFlow::default()),
            output: Mutex::new(output),
            lifecycle: Mutex::new(lifecycle),
            replay: Mutex::new(ReplayBuffer::default()),
            last_lifecycle: Mutex::new(None),
            input: Mutex::new(Some(input_tx)),
            writer_stop: writer_stop_tx,
            supervisor: supervisor_tx,
            supervisor_done: supervisor_done_rx,
        });

        let child_holder = Arc::new(Mutex::new(Some(child)));
        let supervisor_control = Arc::clone(&control);
        let supervisor_child = Arc::clone(&child_holder);
        if thread::Builder::new()
            .name(format!("twominal-child-{id}"))
            .spawn(move || {
                let child = supervisor_child
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .take();
                if let Some(child) = child {
                    run_supervisor(child, supervisor_rx, supervisor_control);
                }
                let _ = supervisor_done_tx.try_send(());
            })
            .is_err()
        {
            if let Some(mut child) = child_holder
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take()
            {
                terminate_child(&mut *child);
            }
            logging::error("terminal_start_failed", "supervisor_thread_unavailable");
            return Err(CommandError::new(
                "terminal_start_failed",
                "Twominal could not start the terminal supervisor.",
            ));
        }

        let writer_control = Arc::clone(&control);
        if thread::Builder::new()
            .name(format!("twominal-writer-{id}"))
            .spawn(move || run_writer(writer, input_rx, writer_stop_rx, writer_control))
            .is_err()
        {
            control.fail(
                "pty_writer_unavailable",
                "The terminal input stream could not be started.",
            );
            wait_for_supervisor(&control, CHILD_SHUTDOWN_TIMEOUT);
            return Err(CommandError::new(
                "terminal_start_failed",
                "Twominal could not start the terminal input stream.",
            ));
        }

        let reader_control = Arc::clone(&control);
        if thread::Builder::new()
            .name(format!("twominal-reader-{id}"))
            .spawn(move || run_reader(reader, reader_control))
            .is_err()
        {
            control.fail(
                "pty_reader_unavailable",
                "The terminal output stream could not be started.",
            );
            wait_for_supervisor(&control, CHILD_SHUTDOWN_TIMEOUT);
            return Err(CommandError::new(
                "terminal_start_failed",
                "Twominal could not start the terminal output stream.",
            ));
        }

        Ok(Arc::new(Self {
            owner: Mutex::new(owner),
            descriptor,
            cwd,
            master: Mutex::new(Some(master)),
            control,
        }))
    }

    fn write(&self, bytes: Vec<u8>) -> Result<(), CommandError> {
        if bytes.len() > MAX_INPUT_BYTES {
            return Err(CommandError::new(
                "input_too_large",
                "Terminal input must not exceed 65536 bytes per request.",
            ));
        }
        if bytes.is_empty() {
            return Ok(());
        }
        if !self.control.is_running() {
            return Err(CommandError::session_not_running());
        }

        let input = self.control.lock_input();
        let sender = input
            .as_ref()
            .ok_or_else(CommandError::session_not_running)?;
        match sender.try_send(bytes) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(CommandError::new(
                "input_backpressure",
                "Terminal input is temporarily busy; retry this input.",
            )),
            Err(TrySendError::Disconnected(_)) => Err(CommandError::session_not_running()),
        }
    }

    fn resize(&self, size: TerminalSize) -> Result<(), CommandError> {
        if !size.validate() {
            return Err(CommandError::invalid_size());
        }
        if !self.control.is_running() {
            return Err(CommandError::session_not_running());
        }

        let master = self
            .master
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        master
            .as_ref()
            .ok_or_else(CommandError::session_not_running)?
            .resize(size)
            .map_err(|_| {
                CommandError::new("pty_resize_failed", "The terminal could not be resized.")
            })
    }

    fn acknowledge(&self, bytes: usize) -> Result<(), CommandError> {
        self.control
            .flow
            .acknowledge(bytes)
            .map_err(|error| match error {
                FlowError::InvalidAcknowledgement => CommandError::new(
                    "invalid_output_acknowledgement",
                    "The terminal output acknowledgement is invalid.",
                ),
                FlowError::Closed | FlowError::InvalidReservation => {
                    CommandError::session_not_running()
                }
            })
    }

    fn is_owned_by(&self, owner: &str) -> bool {
        lock(&self.owner).as_str() == owner
    }

    fn set_owner(&self, owner: &str) {
        *lock(&self.owner) = owner.to_owned();
    }

    fn attach(
        &self,
        output: Channel<InvokeResponseBody>,
        lifecycle: Channel<LifecycleEvent>,
        snapshot: Channel<InvokeResponseBody>,
    ) -> SessionDescriptor {
        // Holding the output lock ensures the replay is delivered before any
        // newly-read PTY bytes reach the destination webview.
        let mut current_output = lock(&self.control.output);
        *current_output = output;
        let replay = lock(&self.control.replay).snapshot();
        let _ = snapshot.send(InvokeResponseBody::Raw(replay));
        self.control.flow.reset();
        drop(current_output);

        let mut current_lifecycle = lock(&self.control.lifecycle);
        *current_lifecycle = lifecycle;
        if let Some(event) = lock(&self.control.last_lifecycle).clone() {
            let _ = current_lifecycle.send(event);
        }
        drop(current_lifecycle);

        self.descriptor.clone()
    }

    fn request_close(&self) -> bool {
        if !self.control.begin_close() {
            return false;
        }

        self.control.flow.close();
        self.control.stop_writer();
        self.master
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        let _ = self
            .control
            .supervisor
            .try_send(SupervisorCommand::Terminate);
        true
    }

    fn wait_for_shutdown(&self, timeout: Duration) {
        wait_for_supervisor(&self.control, timeout);
        self.control.finish_close();
    }

    fn close(&self) {
        if self.request_close() {
            self.wait_for_shutdown(CHILD_SHUTDOWN_TIMEOUT);
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        self.close();
    }
}

#[derive(Default)]
struct ManagerState {
    sessions: HashMap<Uuid, Arc<TerminalSession>>,
    closed: VecDeque<(Uuid, String)>,
    transfers: HashMap<Uuid, PendingTransfer>,
}

struct PendingTransfer {
    session_id: Uuid,
    source_owner: String,
    target_owner: String,
}

pub struct TerminalSessionManager {
    backend: Arc<dyn PtyBackend>,
    shell_integration: Option<ShellIntegrationPaths>,
    state: Mutex<ManagerState>,
}

impl Default for TerminalSessionManager {
    fn default() -> Self {
        Self {
            backend: Arc::new(NativePtyBackend),
            shell_integration: None,
            state: Mutex::new(ManagerState::default()),
        }
    }
}

impl TerminalSessionManager {
    #[cfg(test)]
    fn with_backend(backend: Arc<dyn PtyBackend>) -> Self {
        Self {
            backend,
            shell_integration: None,
            state: Mutex::new(ManagerState::default()),
        }
    }

    pub fn new(shell_integration: Option<ShellIntegrationPaths>) -> Self {
        Self {
            backend: Arc::new(NativePtyBackend),
            shell_integration,
            state: Mutex::new(ManagerState::default()),
        }
    }

    pub fn start(
        &self,
        owner: &str,
        size: TerminalSize,
        output: Channel<InvokeResponseBody>,
        lifecycle: Channel<LifecycleEvent>,
    ) -> Result<SessionDescriptor, CommandError> {
        let mut shell = detect_default_shell().map_err(|_| {
            logging::error("terminal_start_failed", "default_shell_not_found");
            CommandError::new(
                "shell_not_found",
                "Twominal could not find an executable default shell.",
            )
        })?;
        if let Some(integration) = &self.shell_integration {
            integration.apply(&mut shell);
        }
        self.start_shell(owner, size, output, lifecycle, shell)
    }

    fn start_shell(
        &self,
        owner: &str,
        size: TerminalSize,
        output: Channel<InvokeResponseBody>,
        lifecycle: Channel<LifecycleEvent>,
        mut shell: ShellLaunch,
    ) -> Result<SessionDescriptor, CommandError> {
        if !size.validate() {
            return Err(CommandError::invalid_size());
        }

        let integration_nonce = shell
            .shell_integration
            .then(|| Uuid::new_v4().simple().to_string());
        if let Some(nonce) = &integration_nonce {
            shell.environment.push((
                "TWOMINAL_SHELL_INTEGRATION_NONCE".into(),
                nonce.clone().into(),
            ));
        }

        let spawned = self.backend.spawn(&shell, size).map_err(|_| {
            logging::error("terminal_start_failed", "pty_spawn_failed");
            CommandError::new(
                "pty_start_failed",
                "Twominal could not start the selected shell in a PTY.",
            )
        })?;
        let id = Uuid::new_v4();
        let descriptor = SessionDescriptor {
            session_id: id.to_string(),
            shell_name: shell.display_name,
            cwd: shell.cwd.to_string_lossy().into_owned(),
            shell_integration: shell.shell_integration,
            shell_integration_nonce: integration_nonce,
        };
        let session = TerminalSession::start(
            id,
            owner.to_owned(),
            descriptor.clone(),
            shell.cwd.clone(),
            spawned,
            output,
            lifecycle,
        )?;
        let active_sessions = {
            let mut state = self.lock_state();
            state.sessions.insert(id, session);
            state.sessions.len()
        };
        logging::sessions("terminal_session_started", active_sessions);

        Ok(descriptor)
    }

    pub fn prepare_transfer(
        &self,
        owner: &str,
        id: Uuid,
        target_owner: &str,
    ) -> Result<String, CommandError> {
        if !valid_window_label(target_owner) || target_owner == owner {
            return Err(CommandError::new(
                "invalid_transfer_target",
                "The terminal transfer target is invalid.",
            ));
        }

        let mut state = self.lock_state();
        let session = state
            .sessions
            .get(&id)
            .ok_or_else(CommandError::session_not_found)?;
        if !session.is_owned_by(owner) {
            return Err(CommandError::session_forbidden());
        }
        if !session.control.is_running() {
            return Err(CommandError::session_not_running());
        }
        if state
            .transfers
            .values()
            .any(|transfer| transfer.session_id == id)
        {
            return Err(CommandError::new(
                "transfer_in_progress",
                "This terminal is already being moved.",
            ));
        }

        let token = Uuid::new_v4();
        state.transfers.insert(
            token,
            PendingTransfer {
                session_id: id,
                source_owner: owner.to_owned(),
                target_owner: target_owner.to_owned(),
            },
        );
        Ok(token.to_string())
    }

    pub fn cancel_transfer(&self, owner: &str, token: Uuid) -> Result<(), CommandError> {
        let mut state = self.lock_state();
        let transfer = state.transfers.get(&token).ok_or_else(transfer_not_found)?;
        if transfer.source_owner != owner {
            return Err(CommandError::session_forbidden());
        }
        state.transfers.remove(&token);
        Ok(())
    }

    pub fn attach_transfer(
        &self,
        owner: &str,
        token: Uuid,
        size: TerminalSize,
        output: Channel<InvokeResponseBody>,
        lifecycle: Channel<LifecycleEvent>,
        snapshot: Channel<InvokeResponseBody>,
    ) -> Result<SessionDescriptor, CommandError> {
        if !size.validate() {
            return Err(CommandError::invalid_size());
        }

        let session = {
            let mut state = self.lock_state();
            let transfer = state.transfers.get(&token).ok_or_else(transfer_not_found)?;
            if transfer.target_owner != owner {
                return Err(CommandError::session_forbidden());
            }
            let session = state
                .sessions
                .get(&transfer.session_id)
                .ok_or_else(CommandError::session_not_found)?;
            if !session.is_owned_by(&transfer.source_owner) {
                return Err(CommandError::session_forbidden());
            }
            let session = Arc::clone(session);
            session.resize(size)?;
            session.set_owner(owner);
            state.transfers.remove(&token);
            session
        };

        Ok(session.attach(output, lifecycle, snapshot))
    }

    pub fn write(&self, owner: &str, id: Uuid, bytes: Vec<u8>) -> Result<(), CommandError> {
        self.session(owner, id)?.write(bytes)
    }

    pub fn resize(&self, owner: &str, id: Uuid, size: TerminalSize) -> Result<(), CommandError> {
        self.session(owner, id)?.resize(size)
    }

    pub fn acknowledge(&self, owner: &str, id: Uuid, bytes: usize) -> Result<(), CommandError> {
        self.session(owner, id)?.acknowledge(bytes)
    }

    pub fn initial_cwd(&self, owner: &str, id: Uuid) -> Result<PathBuf, CommandError> {
        let session = self.session(owner, id)?;
        if !session.control.is_running() {
            return Err(CommandError::session_not_running());
        }
        Ok(session.cwd.clone())
    }

    pub fn close(&self, owner: &str, id: Uuid) -> Result<(), CommandError> {
        let (session, active_sessions) = {
            let mut state = self.lock_state();
            if let Some(session) = state.sessions.get(&id) {
                if !session.is_owned_by(owner) {
                    return Err(CommandError::session_forbidden());
                }
                if state
                    .transfers
                    .values()
                    .any(|transfer| transfer.session_id == id && transfer.source_owner == owner)
                {
                    return Ok(());
                }
            } else if let Some((_, closed_owner)) =
                state.closed.iter().find(|(closed_id, _)| *closed_id == id)
            {
                return if closed_owner == owner {
                    Ok(())
                } else {
                    Err(CommandError::session_forbidden())
                };
            } else {
                return Err(CommandError::session_not_found());
            }

            let session = state.sessions.remove(&id).expect("session checked above");
            state.closed.push_back((id, owner.to_owned()));
            if state.closed.len() > CLOSED_SESSION_HISTORY {
                state.closed.pop_front();
            }
            let active_sessions = state.sessions.len();
            (session, active_sessions)
        };

        session.close();
        logging::sessions("terminal_session_closed", active_sessions);
        Ok(())
    }

    pub fn close_owner(&self, owner: &str) {
        let sessions = {
            let mut state = self.lock_state();
            let ids: Vec<_> = state
                .sessions
                .iter()
                .filter_map(|(id, session)| session.is_owned_by(owner).then_some(*id))
                .collect();
            state
                .transfers
                .retain(|_, transfer| !ids.contains(&transfer.session_id));
            ids.into_iter()
                .filter_map(|id| state.sessions.remove(&id))
                .collect::<Vec<_>>()
        };
        close_sessions(sessions);
        let active_sessions = self.lock_state().sessions.len();
        logging::sessions("terminal_owner_sessions_closed", active_sessions);
    }

    pub fn close_all(&self) {
        let sessions = {
            let mut state = self.lock_state();
            state.transfers.clear();
            state
                .sessions
                .drain()
                .map(|(_, session)| session)
                .collect::<Vec<_>>()
        };
        close_sessions(sessions);
        logging::sessions("terminal_sessions_closed", 0);
    }

    fn session(&self, owner: &str, id: Uuid) -> Result<Arc<TerminalSession>, CommandError> {
        let state = self.lock_state();
        let session = state
            .sessions
            .get(&id)
            .ok_or_else(CommandError::session_not_found)?;
        if !session.is_owned_by(owner) {
            return Err(CommandError::session_forbidden());
        }
        Ok(Arc::clone(session))
    }

    fn lock_state(&self) -> MutexGuard<'_, ManagerState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Drop for TerminalSessionManager {
    fn drop(&mut self) {
        let state = self
            .state
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.transfers.clear();
        let sessions = state.sessions.drain().map(|(_, session)| session).collect();
        close_sessions(sessions);
    }
}

fn close_sessions(sessions: Vec<Arc<TerminalSession>>) {
    let closing = sessions
        .into_iter()
        .filter(|session| session.request_close())
        .collect::<Vec<_>>();
    let deadline = Instant::now() + CHILD_SHUTDOWN_TIMEOUT;
    for session in closing {
        session.wait_for_shutdown(deadline.saturating_duration_since(Instant::now()));
    }
}

fn wait_for_supervisor(control: &SessionControl, timeout: Duration) {
    match control.supervisor_done.recv_timeout(timeout) {
        Ok(()) | Err(RecvTimeoutError::Disconnected) => {}
        Err(RecvTimeoutError::Timeout) => {
            logging::error("terminal_cleanup_timed_out", "child_shutdown_timeout");
        }
    }
}

fn run_reader(mut reader: Box<dyn Read + Send>, control: Arc<SessionControl>) {
    let mut buffer = vec![0; OUTPUT_BATCH_BYTES];
    loop {
        let permitted = match control.flow.reserve() {
            Ok(permitted) => permitted,
            Err(FlowError::Closed) => return,
            Err(FlowError::InvalidAcknowledgement | FlowError::InvalidReservation) => return,
        };
        let read = match reader.read(&mut buffer[..permitted]) {
            Ok(0) => return,
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => {
                control.fail(
                    "pty_read_failed",
                    "The terminal output stream could not be read.",
                );
                return;
            }
        };
        // Serialize commits and sends with transport replacement so a transfer
        // cannot reset flow credit underneath an in-flight output batch.
        let output = lock(&control.output);
        if control.flow.commit(read).is_err() {
            return;
        }
        lock(&control.replay).push(&buffer[..read]);
        if output
            .send(InvokeResponseBody::Raw(buffer[..read].to_vec()))
            .is_err()
        {
            // Window cleanup normally closes this session immediately. During
            // a transfer, however, the old channel can disappear briefly; do
            // not deadlock the PTY while its replacement attaches.
            let _ = control.flow.acknowledge(read);
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn valid_window_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 128
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-/:_".contains(&byte))
}

fn transfer_not_found() -> CommandError {
    CommandError::new(
        "transfer_not_found",
        "The terminal transfer is no longer available.",
    )
}

fn run_writer(
    mut writer: Box<dyn Write + Send>,
    input: Receiver<Vec<u8>>,
    stop: Receiver<()>,
    control: Arc<SessionControl>,
) {
    loop {
        crossbeam_channel::select_biased! {
            recv(stop) -> _ => return,
            recv(input) -> message => match message {
                Ok(bytes) => {
                    if !control.is_running() {
                        return;
                    }
                    if writer.write_all(&bytes).is_err() {
                        control.fail(
                            "pty_write_failed",
                            "The terminal input stream was closed.",
                        );
                        return;
                    }
                }
                Err(_) => return,
            }
        }
    }
}

fn run_supervisor(
    mut child: Box<dyn PtyChild>,
    commands: Receiver<SupervisorCommand>,
    control: Arc<SessionControl>,
) {
    loop {
        match commands.recv_timeout(CHILD_POLL_INTERVAL) {
            Ok(SupervisorCommand::Terminate)
            | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                terminate_child(&mut *child);
                return;
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => match child.try_wait() {
                Ok(Some(exit)) => {
                    control.process_exited(exit);
                    return;
                }
                Ok(None) => {}
                Err(_) => {
                    control.fail(
                        "process_wait_failed",
                        "Twominal could not monitor the shell process.",
                    );
                    terminate_child(&mut *child);
                    return;
                }
            },
        }
    }
}

fn terminate_child(child: &mut dyn PtyChild) {
    if let Ok(Some(_)) = child.try_wait() {
        return;
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use crossbeam_channel::{bounded, unbounded};

    use super::*;
    use crate::pty::{PtyError, PtyMaster};

    #[derive(Default)]
    struct FakeMaster {
        sizes: Arc<Mutex<Vec<TerminalSize>>>,
    }

    impl PtyMaster for FakeMaster {
        fn resize(&self, size: TerminalSize) -> Result<(), PtyError> {
            self.sizes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(size);
            Ok(())
        }
    }

    struct FakeWriter {
        writes: Sender<Vec<u8>>,
    }

    impl Write for FakeWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.writes
                .send(bytes.to_vec())
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "test receiver closed"))?;
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct InterruptOnceReader {
        interrupted: bool,
        remaining: io::Cursor<Vec<u8>>,
    }

    impl Read for InterruptOnceReader {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            if !self.interrupted {
                self.interrupted = true;
                return Err(io::Error::from(io::ErrorKind::Interrupted));
            }
            self.remaining.read(bytes)
        }
    }

    struct FakeChild {
        exits: Receiver<ProcessExit>,
        kill_count: Arc<AtomicUsize>,
    }

    impl PtyChild for FakeChild {
        fn try_wait(&mut self) -> io::Result<Option<ProcessExit>> {
            match self.exits.try_recv() {
                Ok(exit) => Ok(Some(exit)),
                Err(_) => Ok(None),
            }
        }

        fn wait(&mut self) -> io::Result<ProcessExit> {
            Ok(ProcessExit {
                exit_code: 1,
                signal: Some("terminated".to_owned()),
            })
        }

        fn kill(&mut self) -> io::Result<()> {
            self.kill_count.fetch_add(1, Ordering::AcqRel);
            Ok(())
        }
    }

    struct FakeBackend {
        output: Mutex<Option<Vec<u8>>>,
        writes: Sender<Vec<u8>>,
        exits: Receiver<ProcessExit>,
        sizes: Arc<Mutex<Vec<TerminalSize>>>,
        kill_count: Arc<AtomicUsize>,
    }

    impl PtyBackend for FakeBackend {
        fn spawn(&self, _shell: &ShellLaunch, _size: TerminalSize) -> Result<SpawnedPty, PtyError> {
            let output = self
                .output
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take()
                .unwrap_or_default();
            Ok(SpawnedPty {
                master: Box::new(FakeMaster {
                    sizes: Arc::clone(&self.sizes),
                }),
                reader: Box::new(io::Cursor::new(output)),
                writer: Box::new(FakeWriter {
                    writes: self.writes.clone(),
                }),
                child: Box::new(FakeChild {
                    exits: self.exits.clone(),
                    kill_count: Arc::clone(&self.kill_count),
                }),
            })
        }
    }

    struct Harness {
        manager: TerminalSessionManager,
        output: Receiver<Vec<u8>>,
        lifecycle: Receiver<InvokeResponseBody>,
        writes: Receiver<Vec<u8>>,
        exits: Sender<ProcessExit>,
        sizes: Arc<Mutex<Vec<TerminalSize>>>,
        kill_count: Arc<AtomicUsize>,
    }

    impl Harness {
        fn new(pty_output: Vec<u8>) -> Self {
            let (write_tx, writes) = unbounded();
            let (exit_tx, exit_rx) = unbounded();
            let sizes = Arc::new(Mutex::new(Vec::new()));
            let kill_count = Arc::new(AtomicUsize::new(0));
            let backend = Arc::new(FakeBackend {
                output: Mutex::new(Some(pty_output)),
                writes: write_tx,
                exits: exit_rx,
                sizes: Arc::clone(&sizes),
                kill_count: Arc::clone(&kill_count),
            });
            let (output_tx, output) = unbounded();
            let output_channel = Channel::new(move |body| {
                if let InvokeResponseBody::Raw(bytes) = body {
                    output_tx.send(bytes).unwrap();
                }
                Ok(())
            });
            let (lifecycle_tx, lifecycle) = unbounded();
            let lifecycle_channel = Channel::new(move |body| {
                lifecycle_tx.send(body).unwrap();
                Ok(())
            });
            let manager = TerminalSessionManager::with_backend(backend);
            let shell = ShellLaunch {
                executable: PathBuf::from("/test/shell"),
                arguments: Vec::new(),
                environment: Vec::new(),
                display_name: "test-shell".to_owned(),
                cwd: PathBuf::from("/test/home"),
                shell_integration: false,
            };
            manager
                .start_shell(
                    "main",
                    TerminalSize { rows: 24, cols: 80 },
                    output_channel,
                    lifecycle_channel,
                    shell,
                )
                .unwrap();

            Self {
                manager,
                output,
                lifecycle,
                writes,
                exits: exit_tx,
                sizes,
                kill_count,
            }
        }

        fn id(&self) -> Uuid {
            *self.manager.lock_state().sessions.keys().next().unwrap()
        }

        fn start_another(&self) -> Uuid {
            let descriptor = self
                .manager
                .start_shell(
                    "main",
                    TerminalSize {
                        rows: 30,
                        cols: 100,
                    },
                    Channel::new(|_| Ok(())),
                    Channel::new(|_| Ok(())),
                    ShellLaunch {
                        executable: PathBuf::from("/test/shell"),
                        arguments: Vec::new(),
                        environment: Vec::new(),
                        display_name: "test-shell".to_owned(),
                        cwd: PathBuf::from("/test/home"),
                        shell_integration: false,
                    },
                )
                .unwrap();

            Uuid::parse_str(&descriptor.session_id).unwrap()
        }
    }

    #[test]
    fn preserves_raw_output_and_requires_exact_acknowledgements() {
        let expected = vec![0, 0xff, b'a', b'\n'];
        let harness = Harness::new(expected.clone());
        let id = harness.id();

        assert_eq!(
            harness.output.recv_timeout(Duration::from_secs(1)).unwrap(),
            expected
        );
        let error = harness.manager.acknowledge("main", id, 5).unwrap_err();
        assert_eq!(error.code, "invalid_output_acknowledgement");
        harness.manager.acknowledge("main", id, 4).unwrap();
    }

    #[test]
    fn serializes_input_and_forwards_resize() {
        let harness = Harness::new(Vec::new());
        let id = harness.id();

        harness
            .manager
            .write("main", id, b"hello".to_vec())
            .unwrap();
        harness
            .manager
            .write("main", id, b" world".to_vec())
            .unwrap();
        assert_eq!(
            harness.writes.recv_timeout(Duration::from_secs(1)).unwrap(),
            b"hello"
        );
        assert_eq!(
            harness.writes.recv_timeout(Duration::from_secs(1)).unwrap(),
            b" world"
        );

        let size = TerminalSize {
            rows: 42,
            cols: 132,
        };
        harness.manager.resize("main", id, size).unwrap();
        assert_eq!(
            harness
                .sizes
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_slice(),
            &[size]
        );
    }

    #[test]
    fn manages_multiple_sessions_independently() {
        let harness = Harness::new(Vec::new());
        let first = harness.id();
        let second = harness.start_another();

        assert_ne!(first, second);
        assert_eq!(harness.manager.lock_state().sessions.len(), 2);

        harness.manager.close("main", first).unwrap();
        assert_eq!(
            harness
                .manager
                .write("main", first, b"closed".to_vec())
                .unwrap_err()
                .code,
            "session_not_found"
        );

        harness
            .manager
            .write("main", second, b"still-running".to_vec())
            .unwrap();
        assert_eq!(
            harness.writes.recv_timeout(Duration::from_secs(1)).unwrap(),
            b"still-running"
        );

        let size = TerminalSize {
            rows: 50,
            cols: 160,
        };
        harness.manager.resize("main", second, size).unwrap();
        assert!(harness
            .sizes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(&size));
        harness.manager.close("main", second).unwrap();
    }

    #[test]
    fn transfers_a_live_session_with_replay_and_new_owner_enforcement() {
        let expected = b"existing terminal output".to_vec();
        let harness = Harness::new(expected.clone());
        let id = harness.id();
        assert_eq!(
            harness.output.recv_timeout(Duration::from_secs(1)).unwrap(),
            expected
        );

        let token = harness
            .manager
            .prepare_transfer("main", id, "twominal-child")
            .unwrap();
        let token = Uuid::parse_str(&token).unwrap();

        // React unmounts the source pane after destination attachment. A close
        // racing with the handoff must not terminate the process.
        harness.manager.close("main", id).unwrap();
        assert_eq!(harness.kill_count.load(Ordering::Acquire), 0);

        let (snapshot_tx, snapshot_rx) = bounded(1);
        let descriptor = harness
            .manager
            .attach_transfer(
                "twominal-child",
                token,
                TerminalSize {
                    rows: 30,
                    cols: 100,
                },
                Channel::new(|_| Ok(())),
                Channel::new(|_| Ok(())),
                Channel::new(move |body| {
                    if let InvokeResponseBody::Raw(bytes) = body {
                        snapshot_tx.send(bytes).unwrap();
                    }
                    Ok(())
                }),
            )
            .unwrap();

        assert_eq!(descriptor.session_id, id.to_string());
        assert_eq!(
            snapshot_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            expected
        );
        assert_eq!(
            harness
                .manager
                .write("main", id, b"old owner".to_vec())
                .unwrap_err()
                .code,
            "session_forbidden"
        );
        harness
            .manager
            .write("twominal-child", id, b"new owner".to_vec())
            .unwrap();
        assert_eq!(
            harness.writes.recv_timeout(Duration::from_secs(1)).unwrap(),
            b"new owner"
        );

        harness.manager.close_owner("main");
        assert_eq!(harness.kill_count.load(Ordering::Acquire), 0);
        harness.manager.close_owner("twominal-child");
        assert_eq!(harness.kill_count.load(Ordering::Acquire), 1);
    }

    #[test]
    fn canceled_transfer_leaves_the_session_with_its_source_window() {
        let harness = Harness::new(Vec::new());
        let id = harness.id();
        let token = harness
            .manager
            .prepare_transfer("main", id, "twominal-child")
            .unwrap();

        harness
            .manager
            .cancel_transfer("main", Uuid::parse_str(&token).unwrap())
            .unwrap();
        harness.manager.close("main", id).unwrap();
        assert_eq!(harness.kill_count.load(Ordering::Acquire), 1);
    }

    #[test]
    fn destroying_a_source_window_cleans_up_an_unfinished_transfer() {
        let harness = Harness::new(Vec::new());
        let id = harness.id();
        harness
            .manager
            .prepare_transfer("main", id, "twominal-child")
            .unwrap();

        harness.manager.close_owner("main");

        assert_eq!(harness.kill_count.load(Ordering::Acquire), 1);
        assert!(harness.manager.lock_state().transfers.is_empty());
    }

    #[test]
    fn transfer_tokens_are_bound_to_their_source_and_destination_windows() {
        let harness = Harness::new(Vec::new());
        let id = harness.id();
        let token = harness
            .manager
            .prepare_transfer("main", id, "twominal-child")
            .unwrap();
        let token = Uuid::parse_str(&token).unwrap();

        assert_eq!(
            harness
                .manager
                .cancel_transfer("other", token)
                .unwrap_err()
                .code,
            "session_forbidden"
        );
        assert_eq!(
            harness
                .manager
                .attach_transfer(
                    "other",
                    token,
                    TerminalSize { rows: 24, cols: 80 },
                    Channel::new(|_| Ok(())),
                    Channel::new(|_| Ok(())),
                    Channel::new(|_| Ok(())),
                )
                .unwrap_err()
                .code,
            "session_forbidden"
        );
        harness.manager.cancel_transfer("main", token).unwrap();
    }

    #[test]
    fn writer_stop_preempts_already_queued_input() {
        let (input_tx, input_rx) = bounded(2);
        input_tx.send(b"must-not-run".to_vec()).unwrap();
        let (stop_tx, stop_rx) = bounded(1);
        stop_tx.send(()).unwrap();
        let (supervisor_tx, _supervisor_rx) = bounded(1);
        let (_supervisor_done_tx, supervisor_done_rx) = bounded(1);
        let (write_tx, writes) = unbounded();
        let control = Arc::new(SessionControl {
            state: AtomicU8::new(STATE_RUNNING),
            flow: Arc::new(OutputFlow::default()),
            output: Mutex::new(Channel::new(|_| Ok(()))),
            lifecycle: Mutex::new(Channel::new(|_| Ok(()))),
            replay: Mutex::new(ReplayBuffer::default()),
            last_lifecycle: Mutex::new(None),
            input: Mutex::new(Some(input_tx)),
            writer_stop: stop_tx,
            supervisor: supervisor_tx,
            supervisor_done: supervisor_done_rx,
        });

        run_writer(
            Box::new(FakeWriter { writes: write_tx }),
            input_rx,
            stop_rx,
            control,
        );

        assert!(writes.try_recv().is_err());
    }

    #[test]
    fn reader_retries_interrupted_system_calls() {
        let (input_tx, _input_rx) = bounded(1);
        let (stop_tx, _stop_rx) = bounded(1);
        let (supervisor_tx, _supervisor_rx) = bounded(1);
        let (_supervisor_done_tx, supervisor_done_rx) = bounded(1);
        let (output_tx, output) = bounded(1);
        let output_channel = Channel::new(move |body| {
            if let InvokeResponseBody::Raw(bytes) = body {
                output_tx.send(bytes).unwrap();
            }
            Ok(())
        });
        let control = Arc::new(SessionControl {
            state: AtomicU8::new(STATE_RUNNING),
            flow: Arc::new(OutputFlow::default()),
            output: Mutex::new(output_channel),
            lifecycle: Mutex::new(Channel::new(|_| Ok(()))),
            replay: Mutex::new(ReplayBuffer::default()),
            last_lifecycle: Mutex::new(None),
            input: Mutex::new(Some(input_tx)),
            writer_stop: stop_tx,
            supervisor: supervisor_tx,
            supervisor_done: supervisor_done_rx,
        });
        run_reader(
            Box::new(InterruptOnceReader {
                interrupted: false,
                remaining: io::Cursor::new(vec![1, 2, 3]),
            }),
            Arc::clone(&control),
        );

        assert_eq!(
            output.recv_timeout(Duration::from_secs(1)).unwrap(),
            [1, 2, 3]
        );
        assert!(control.is_running());
    }

    #[test]
    fn enforces_owner_and_input_limits() {
        let harness = Harness::new(Vec::new());
        let id = harness.id();

        assert_eq!(
            harness
                .manager
                .write("other", id, vec![1])
                .unwrap_err()
                .code,
            "session_forbidden"
        );
        assert_eq!(
            harness
                .manager
                .write("main", id, vec![0; MAX_INPUT_BYTES + 1])
                .unwrap_err()
                .code,
            "input_too_large"
        );
    }

    #[test]
    fn close_is_idempotent_for_the_owner_and_terminates_the_child() {
        let harness = Harness::new(Vec::new());
        let id = harness.id();

        harness.manager.close("main", id).unwrap();
        harness.manager.close("main", id).unwrap();
        assert_eq!(harness.kill_count.load(Ordering::Acquire), 1);
        assert_eq!(
            harness.manager.close("other", id).unwrap_err().code,
            "session_forbidden"
        );
    }

    #[test]
    fn close_all_waits_for_child_termination_and_clears_the_registry() {
        let harness = Harness::new(Vec::new());
        harness.start_another();

        harness.manager.close_all();

        assert!(harness.manager.lock_state().sessions.is_empty());
        assert_eq!(harness.kill_count.load(Ordering::Acquire), 2);
    }

    #[test]
    fn reports_process_exit_once_and_rejects_further_input() {
        let harness = Harness::new(Vec::new());
        let id = harness.id();
        harness
            .exits
            .send(ProcessExit {
                exit_code: 7,
                signal: None,
            })
            .unwrap();

        let body = harness
            .lifecycle
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        match body {
            InvokeResponseBody::Json(json) => {
                assert!(json.contains("\"type\":\"exited\""));
                assert!(json.contains("\"exitCode\":7"));
            }
            InvokeResponseBody::Raw(_) => panic!("lifecycle events must be JSON"),
        }
        assert_eq!(
            harness.manager.write("main", id, vec![1]).unwrap_err().code,
            "session_not_running"
        );
    }

    #[test]
    fn rejects_invalid_sizes_before_spawning() {
        let (write_tx, _) = bounded(1);
        let (_, exit_rx) = bounded(1);
        let backend = Arc::new(FakeBackend {
            output: Mutex::new(Some(Vec::new())),
            writes: write_tx,
            exits: exit_rx,
            sizes: Arc::new(Mutex::new(Vec::new())),
            kill_count: Arc::new(AtomicUsize::new(0)),
        });
        let manager = TerminalSessionManager::with_backend(backend);
        let output = Channel::new(|_| Ok(()));
        let lifecycle = Channel::new(|_| Ok(()));
        let shell = ShellLaunch {
            executable: PathBuf::from("/test/shell"),
            arguments: Vec::new(),
            environment: Vec::new(),
            display_name: "test".to_owned(),
            cwd: PathBuf::from("/test"),
            shell_integration: false,
        };

        let error = manager
            .start_shell(
                "main",
                TerminalSize { rows: 0, cols: 80 },
                output,
                lifecycle,
                shell,
            )
            .unwrap_err();
        assert_eq!(error.code, "invalid_terminal_size");
    }
}
