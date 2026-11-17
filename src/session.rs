//! Cancellable SSH sessions. Network and authentication waits stay on a worker.
use crate::{
    target::Target,
    trust::{HostKey, Status},
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
        Arc, Condvar, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use zeroize::{Zeroize, Zeroizing};

// Track retired sessions too: their tabs may have disappeared before app exit.
static FILE_CLEANUP: CleanupTracker = CleanupTracker::new();
const FILE_CLEANUP_TIMEOUT: Duration = Duration::from_secs(5);

struct CleanupTracker {
    pending: Mutex<usize>,
    changed: Condvar,
}

impl CleanupTracker {
    const fn new() -> Self {
        Self {
            pending: Mutex::new(0),
            changed: Condvar::new(),
        }
    }

    fn register(&self) -> CleanupGuard<'_> {
        *self
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner()) += 1;
        CleanupGuard(self)
    }

    fn wait(&self, timeout: Duration) -> bool {
        let pending = self
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let (pending, _) = self
            .changed
            .wait_timeout_while(pending, timeout, |pending| *pending != 0)
            .unwrap_or_else(|error| error.into_inner());
        *pending == 0
    }
}

struct CleanupGuard<'a>(&'a CleanupTracker);

impl Drop for CleanupGuard<'_> {
    fn drop(&mut self) {
        let mut pending = self
            .0
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *pending -= 1;
        self.0.changed.notify_all();
    }
}

/// Keep the process alive for bounded cleanup after all GUI tabs are dropped.
/// This includes work retired by earlier tab closures or manual disconnects.
pub fn wait_for_file_cleanup() {
    FILE_CLEANUP.wait(FILE_CLEANUP_TIMEOUT);
}

#[derive(Clone)]
pub struct Connect {
    pub jumps: Vec<Connect>,
    pub target: Target,
    pub password: Zeroizing<String>,
    pub key_path: Option<PathBuf>,
    pub known_hosts: PathBuf,
    pub strict: bool,
    pub tmux: Option<String>,
    pub remote_directory: Option<String>,
    pub term: String,
    pub timeout: Duration,
    pub keepalive: Option<Duration>,
    pub accept_new: bool,
    pub environment: Vec<(String, String)>,
    pub auth: Vec<crate::connection::AuthMethod>,
    pub use_agent: bool,
    pub local_forwards: Vec<crate::forward::LocalForward>,
    pub dynamic_forwards: Vec<crate::forward::DynamicForward>,
    pub remote_forwards: Vec<crate::forward::LocalForward>,
}

pub enum Event {
    /// The remote shell channel closed; the SSH transport may still be open.
    ShellExited(Option<u32>),
    Phase(String),
    Trust {
        key: Box<HostKey>,
        answer: SyncSender<bool>,
    },
    Challenge {
        name: String,
        instruction: String,
        prompts: Vec<(String, bool)>,
        answer: SyncSender<Vec<String>>,
    },
    Connected,
    Output(Vec<u8>),
    Heartbeat {
        received: u64,
        sent: u64,
    },
    Disconnected(String),
}

type Wakeup = Arc<Mutex<Option<Arc<dyn Fn() + Send + Sync>>>>;

#[derive(Clone)]
struct EventSender {
    sender: SyncSender<Event>,
    wakeup: Wakeup,
}
impl EventSender {
    fn wake(&self) {
        let callback = self.wakeup.lock().unwrap().clone();
        if let Some(callback) = callback {
            callback();
        }
    }
    fn try_send(&self, event: Event) -> Result<(), TrySendError<Event>> {
        self.sender.try_send(event)?;
        self.wake();
        Ok(())
    }
}

pub struct Session {
    wakeup: Wakeup,
    events: Receiver<Event>,
    completion: Receiver<String>,
    input: Arc<crate::input_queue::Queue>,
    files: SyncSender<crate::files::Request>,
    file_busy: Arc<AtomicBool>,
    key_installs: SyncSender<crate::key_install::Request>,
    cancelled: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    size: Arc<Mutex<Option<(u16, u16)>>>,
}

impl Session {
    pub fn start(connect: Connect, columns: u16, rows: u16) -> Self {
        let (sender, events) = mpsc::sync_channel(64);
        let wakeup = Wakeup::default();
        let output = EventSender {
            sender,
            wakeup: wakeup.clone(),
        };
        let (completed, completion) = mpsc::channel();
        let input = Arc::new(crate::input_queue::Queue::default());
        let commands = input.clone();
        let (files, file_requests) = mpsc::sync_channel(1);
        let file_busy = Arc::new(AtomicBool::new(false));
        let worker_file_busy = file_busy.clone();
        let (key_installs, key_requests) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let size = Arc::new(Mutex::new(Some((columns.max(1), rows.max(1)))));
        let stop = cancelled.clone();
        let finished = Arc::new(AtomicBool::new(false));
        let finished_worker = finished.clone();
        let dimensions = size.clone();
        thread::spawn(move || {
            finish_worker(
                || {
                    run(
                        connect,
                        &commands,
                        (file_requests, worker_file_busy),
                        key_requests,
                        &output,
                        &stop,
                        &dimensions,
                    )
                },
                &completed,
                &finished_worker,
            );
            commands.close();
            let callback = output.wakeup.lock().unwrap().clone();
            drop(output);
            if let Some(callback) = callback {
                callback();
            }
        });
        Self {
            wakeup,
            events,
            completion,
            input,
            files,
            file_busy,
            key_installs,
            cancelled,
            finished,
            size,
        }
    }

    /// Install a UI wakeup without coupling the SSH worker to a GUI framework.
    pub fn set_wakeup(&self, callback: Arc<dyn Fn() + Send + Sync>) {
        *self.wakeup.lock().unwrap() = Some(callback);
    }

    /// Drain queued output before reporting completion. The separate one-shot
    /// completion channel cannot lose the final status to output backpressure.
    pub fn try_event(&self) -> Result<Event, mpsc::TryRecvError> {
        self.events.try_recv().or_else(|error| {
            if error != mpsc::TryRecvError::Disconnected {
                return Err(error);
            }
            self.completion
                .try_recv()
                .map(Event::Disconnected)
                .map_err(|_| error)
        })
    }

    pub fn recv_event(&self, timeout: Duration) -> Result<Event, mpsc::RecvTimeoutError> {
        self.events.recv_timeout(timeout).or_else(|error| {
            if error != mpsc::RecvTimeoutError::Disconnected {
                return Err(error);
            }
            self.completion
                .try_recv()
                .map(Event::Disconnected)
                .map_err(|_| error)
        })
    }

    pub fn send(&self, bytes: Vec<u8>) -> Result<(), String> {
        if bytes.is_empty() {
            return Ok(());
        }
        if self.cancelled.load(Ordering::Acquire) || self.is_finished() {
            return Err("Session is disconnected.".into());
        }
        self.input.push(bytes)
    }

    /// A query in buffered final output cannot be answered after shutdown.
    /// Preserve the queued exit status instead of replacing it with a send error.
    pub fn reply(&self, bytes: Vec<u8>) -> Result<(), String> {
        self.send(bytes).or_else(|error| {
            if self.is_finished() || self.cancelled.load(Ordering::Acquire) {
                Ok(())
            } else {
                Err(error)
            }
        })
    }

    pub fn resize(&self, columns: u16, rows: u16) {
        if let Ok(mut size) = self.size.lock() {
            *size = Some((columns.max(1), rows.max(1)));
        }
    }
    pub fn list_directory(&self, path: String) -> Result<crate::files::DirectoryTask, String> {
        crate::files::submit(&self.files, path)
    }
    pub fn file_action(
        &self,
        action: crate::files::Action,
    ) -> Result<crate::files::ActionTask, String> {
        crate::files::submit_action(&self.files, action)
    }
    /// Close after cancelled file work has had a bounded chance to clean up.
    /// The caller must first drop its file task handles to request cancellation.
    pub fn close_after_file_cleanup(self) {
        if !self.file_busy.load(Ordering::Acquire) || self.is_finished() {
            return;
        }
        // Register before spawning so shutdown cannot miss an unstarted thread.
        let cleanup = FILE_CLEANUP.register();
        thread::spawn(move || {
            let _cleanup = cleanup;
            let deadline = Instant::now() + FILE_CLEANUP_TIMEOUT;
            while self.file_busy.load(Ordering::Acquire)
                && !self.is_finished()
                && Instant::now() < deadline
            {
                // Keep terminal output from blocking the worker's file cleanup.
                for _ in 0..64 {
                    if self.try_event().is_err() {
                        break;
                    }
                }
                thread::sleep(Duration::from_millis(4));
            }
            self.disconnect();
        });
    }
    pub fn disconnect(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
    pub fn install_public_key(
        &self,
        key: crate::key_install::Key,
    ) -> Result<crate::key_install::Task, String> {
        crate::key_install::submit(&self.key_installs, key)
    }
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }
}

fn finish_worker(
    action: impl FnOnce() -> Result<(), String>,
    completed: &mpsc::Sender<String>,
    finished: &AtomicBool,
) {
    let detail = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(action)) {
        Ok(Ok(())) => "Session closed.".into(),
        Ok(Err(error)) => error,
        Err(_) => "The SSH worker stopped unexpectedly. You can reconnect.".into(),
    };
    // Do not include arbitrary panic payloads: dependencies may embed secrets.
    let _ = completed.send(detail);
    finished.store(true, Ordering::Release);
}

impl Drop for Session {
    fn drop(&mut self) {
        self.disconnect();
    }
}

fn deliver(sender: &EventSender, mut event: Event, stop: &AtomicBool) -> Result<(), String> {
    loop {
        if stop.load(Ordering::Acquire) {
            return Err("Connection cancelled.".into());
        }
        match sender.try_send(event) {
            Ok(()) => return Ok(()),
            Err(TrySendError::Disconnected(_)) => return Err("Workspace closed.".into()),
            Err(TrySendError::Full(value)) => {
                event = value;
                thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

fn answer<T>(receiver: Receiver<T>, stop: &AtomicBool) -> Result<T, String> {
    loop {
        if stop.load(Ordering::Acquire) {
            return Err("Connection cancelled.".into());
        }
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(value) => return Ok(value),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => return Err("Authentication cancelled.".into()),
        }
    }
}

struct Hooks;
impl Drop for Hooks {
    fn drop(&mut self) {
        ssh::set_host_key_verifier(None);
        ssh::set_pending_peer(None);
        ssh::set_kbd_prompter(None);
        ssh::set_agent_signer(None);
        ssh::clear_host_key_proof();
    }
}

trait Transport: std::io::Read + std::io::Write {}
impl<T: std::io::Read + std::io::Write> Transport for T {}
type BoxTransport = Box<dyn Transport>;

fn phase_observer(
    output: &EventSender,
    stop: &Arc<AtomicBool>,
    peer: String,
) -> Arc<dyn Fn(ssh::ConnectionPhase) + Send + Sync> {
    let output = output.clone();
    let stop = stop.clone();
    let started = Instant::now();
    Arc::new(move |phase| {
        let text = match phase {
            ssh::ConnectionPhase::Version => "SSH version negotiation",
            ssh::ConnectionPhase::KeyExchange => "Key exchange",
            ssh::ConnectionPhase::Authentication => "SSH authentication request",
            ssh::ConnectionPhase::PtyRequest => "PTY request",
            ssh::ConnectionPhase::ShellRequest => "Shell open request",
            ssh::ConnectionPhase::ShellRequestSent => {
                "Shell open request sent (no acknowledgement requested)"
            }
        };
        let _ = deliver(
            &output,
            Event::Phase(format!(
                "{peer}: {text} ({} ms)",
                started.elapsed().as_millis()
            )),
            &stop,
        );
    })
}

fn authenticate(
    connect: &mut Connect,
    output: &EventSender,
    stop: &Arc<AtomicBool>,
    transport: &mut impl FnMut() -> Result<BoxTransport, String>,
) -> Result<ssh::LocalSession<BoxTransport>, String> {
    let started = Instant::now();
    let peer = connect.target.to_string();
    let phase = |text: &str| {
        deliver(
            output,
            Event::Phase(format!(
                "{peer}: {text} ({} ms)",
                started.elapsed().as_millis()
            )),
            stop,
        )
    };
    let user = connect
        .target
        .user
        .as_deref()
        .ok_or("A username is required.")?;
    let pending = Arc::new(Mutex::new(None::<HostKey>));
    let accepted = Arc::new(Mutex::new(None::<String>));
    let _hooks = Hooks;
    ssh::set_pending_peer(Some(connect.target.to_string()));
    let (tx, cancellation, staged, approved) = (
        output.clone(),
        stop.clone(),
        pending.clone(),
        accepted.clone(),
    );
    let (host, port, path, strict, accept_new) = (
        connect.target.host.clone(),
        connect.target.port,
        connect.known_hosts.clone(),
        connect.strict,
        connect.accept_new,
    );
    ssh::set_host_key_verifier(Some(Box::new(move |_, blob| {
        deliver(
            &tx,
            Event::Phase(format!("{host}:{port}: Host key verification")),
            &cancellation,
        )?;
        let mut key = HostKey::new(&host, port, blob)?;
        let status = key.check_file(&path)?;
        match status {
            Status::Known => Ok(()),
            Status::Revoked => Err(format!("Host key is revoked for {}.", key.peer)),
            Status::Changed => Err(format!(
                "Host key changed for {} ({}). Review known_hosts before reconnecting.",
                key.peer, key.fingerprint
            )),
            Status::Unknown if strict => Err(format!(
                "Strict verification rejected unknown host {} ({}).",
                key.peer, key.fingerprint
            )),
            Status::NewKeyType if strict => Err(format!(
                "Strict verification rejected a {} key not yet known for {} ({}).",
                key.algorithm, key.peer, key.fingerprint
            )),
            Status::Unknown | Status::NewKeyType => {
                // accept-new covers new hosts only, never a known host's new key type.
                key.other_types_known = status == Status::NewKeyType;
                let previously_accepted = approved
                    .lock()
                    .map_err(|_| "Trust state unavailable.")?
                    .as_deref()
                    == Some(key.fingerprint.as_str());
                if !previously_accepted && (key.other_types_known || !accept_new) {
                    let (answer_tx, answer_rx) = mpsc::sync_channel(1);
                    deliver(
                        &tx,
                        Event::Trust {
                            key: Box::new(key.clone()),
                            answer: answer_tx,
                        },
                        &cancellation,
                    )?;
                    if !answer(answer_rx, &cancellation)? {
                        return Err("Host key was not accepted.".into());
                    }
                    *approved.lock().map_err(|_| "Trust state unavailable.")? =
                        Some(key.fingerprint.clone());
                }
                *staged.lock().map_err(|_| "Trust state unavailable.")? = Some(key);
                Ok(())
            }
        }
    })));
    let (tx, cancellation) = (output.clone(), stop.clone());
    ssh::set_kbd_prompter(Some(Box::new(move |name, instruction, prompts| {
        let count = prompts.len();
        let (answer_tx, answer_rx) = mpsc::sync_channel(1);
        deliver(
            &tx,
            Event::Challenge {
                name,
                instruction,
                prompts,
                answer: answer_tx,
            },
            &cancellation,
        )?;
        let values = answer(answer_rx, &cancellation).inspect_err(|_error| {
            cancellation.store(true, Ordering::Release);
        })?;
        if values.len() != count {
            return Err("Authentication answer count does not match.".into());
        }
        Ok(values)
    })));
    phase("Connecting and verifying host")?;
    let mut authenticated = None;
    let selected_agent = Arc::new(Mutex::new(None::<String>));
    for method in &connect.auth {
        use crate::connection::AuthMethod;
        if stop.load(Ordering::Acquire) {
            return Err("Authentication cancelled.".into());
        }
        let mut builder = ssh::create_session()
            .username(user)
            .timeout(Some(connect.timeout))
            .keyboard_interactive(false);
        match method {
            AuthMethod::Agent => {
                if !connect.use_agent {
                    continue;
                }
                phase("Discovering SSH agent identities")?;
                let identities = match crate::agent::identities(stop) {
                    Ok(keys) if !keys.is_empty() => keys,
                    _ => {
                        phase("No agent identities available; trying next configured method")?;
                        continue;
                    }
                };
                builder = builder
                    .agent_identities(identities.iter().map(|key| key.blob.clone()).collect());
                let selected = selected_agent.clone();
                let cancellation = stop.clone();
                ssh::set_agent_signer(Some(Box::new(move |blob, data, flags| {
                    for identity in identities.iter().filter(|identity| identity.blob == blob) {
                        if cancellation.load(Ordering::Acquire) {
                            return Err("Authentication cancelled.".into());
                        }
                        if let Ok(signature) = identity.sign(data, flags, &cancellation) {
                            if cancellation.load(Ordering::Acquire) {
                                return Err("Authentication cancelled.".into());
                            }
                            if let Ok(mut value) = selected.lock() {
                                *value = Some(identity.fingerprint());
                            }
                            return Ok(signature);
                        }
                    }
                    Err("Available SSH agents declined signing with this identity.".into())
                })));
            }
            AuthMethod::Key => {
                let Some(path) = &connect.key_path else {
                    continue;
                };
                if !path.is_file() {
                    phase("Private key unavailable; trying next configured method")?;
                    continue;
                }
                phase("Authenticating with private key")?;
                let mut key_text = crate::keyfile::read(path)?;
                if let Some(encrypted) = crate::keyfile::encrypted(&key_text) {
                    let (tx, rx) = mpsc::sync_channel(1);
                    deliver(
                        output,
                        Event::Challenge {
                            name: "Unlock private key".into(),
                            instruction: format!(
                                "Enter the passphrase for local key {}. It will not be saved.",
                                path.display()
                            ),
                            prompts: vec![("Key passphrase".into(), false)],
                            answer: tx,
                        },
                        stop,
                    )?;
                    let mut values = Zeroizing::new(answer(rx, stop)?);
                    if values.len() != 1 {
                        return Err("Key passphrase prompt was cancelled.".into());
                    }
                    let password = Zeroizing::new(values.remove(0));
                    key_text = crate::keyfile::decrypt(encrypted, password, stop)?;
                }
                builder = builder.private_key(key_text.as_str());
            }
            AuthMethod::Interactive => {
                phase("Keyboard-interactive authentication")?;
                builder = builder.keyboard_interactive(true);
            }
            AuthMethod::Password => {
                if connect.password.is_empty() {
                    let (tx, rx) = mpsc::sync_channel(1);
                    deliver(
                        output,
                        Event::Challenge {
                            name: "Password authentication".into(),
                            instruction: format!("Enter the password for {}.", connect.target),
                            prompts: vec![("Password".into(), false)],
                            answer: tx,
                        },
                        stop,
                    )?;
                    let mut values = answer(rx, stop)?;
                    if values.len() != 1 {
                        return Err("Password prompt was cancelled.".into());
                    }
                    connect.password = Zeroizing::new(values.remove(0));
                }
                phase("Authenticating with password")?;
                builder = builder.password(&connect.password);
            }
        }
        ssh::clear_host_key_proof();
        let stream = transport()?;
        phase("Negotiating SSH and authenticating")?;
        let connector =
            ssh::with_phase_observer(phase_observer(output, stop, peer.clone()), || {
                builder.connect_bio(stream)
            });
        if stop.load(Ordering::Acquire) {
            return Err("Authentication cancelled.".into());
        }
        if ssh::host_key_proof_ok() {
            let verified = pending
                .lock()
                .map_err(|_| "Trust state unavailable.")?
                .take();
            if let Some(key) = verified {
                key.persist_verified(&connect.known_hosts)?;
            }
        }
        match connector {
            Ok(value) => {
                if *method == crate::connection::AuthMethod::Agent {
                    if let Some(fingerprint) = selected_agent
                        .lock()
                        .map_err(|_| "Agent identity state unavailable.")?
                        .as_ref()
                    {
                        phase(&format!("Authenticated with agent identity {fingerprint}"))?;
                    }
                }
                authenticated = Some(value);
                break;
            }
            Err(ssh::SshError::AuthError) => {
                phase("Authentication method rejected; trying next configured method")?;
            }
            Err(error) => {
                let text = error.to_string();
                return Err(if connect.password.is_empty() {
                    text
                } else {
                    text.replace(connect.password.as_str(), "[redacted]")
                });
            }
        }
    }
    let connection = authenticated
        .ok_or("Authentication failed: no configured method succeeded.")?
        .run_local();
    connect.password.zeroize();
    Ok(connection)
}

fn run(
    mut connect: Connect,
    input: &crate::input_queue::Queue,
    file_requests: (Receiver<crate::files::Request>, Arc<AtomicBool>),
    key_requests: Receiver<crate::key_install::Request>,
    output: &EventSender,
    stop: &Arc<AtomicBool>,
    dimensions: &Mutex<Option<(u16, u16)>>,
) -> Result<(), String> {
    let (file_requests, file_busy) = file_requests;
    let started = Instant::now();
    let phase = |text: &str| {
        deliver(
            output,
            Event::Phase(format!("{text} · {} ms", started.elapsed().as_millis())),
            stop,
        )
    };
    let mut hops = std::mem::take(&mut connect.jumps);
    if hops.len() > 8 || hops.iter().any(|hop| !hop.jumps.is_empty()) {
        return Err("Jump route must be a flat chain of at most eight hops.".into());
    }
    let mut upstream: Option<ssh::LocalSession<BoxTransport>> = None;
    for (index, hop) in hops
        .iter_mut()
        .chain(std::iter::once(&mut connect))
        .enumerate()
    {
        let target = hop.target.clone();
        let timeout = hop.timeout;
        phase(&format!("Route step {}: {}", index + 1, target))?;
        let mut transport = || -> Result<BoxTransport, String> {
            if stop.load(Ordering::Acquire) {
                return Err("Connection cancelled.".into());
            }
            if let Some(session) = &mut upstream {
                phase(&format!(
                    "Opening SSH tunnel to {}:{}",
                    target.host, target.port
                ))?;
                session
                    .open_direct_tcpip(&target.host, target.port)
                    .map(|stream| Box::new(stream) as BoxTransport)
                    .map_err(|e| e.to_string())
            } else {
                phase("Resolving host and opening TCP connection")?;
                crate::network::connect(target.host.clone(), target.port, timeout, stop)
                    .map(|stream| Box::new(stream) as BoxTransport)
            }
        };
        let result = authenticate(hop, output, stop, &mut transport)
            .map_err(|error| format!("Route step {} ({}): {error}", index + 1, target))?;
        upstream = Some(result);
    }
    let mut connection = upstream.ok_or("Empty connection route.")?;
    if stop.load(Ordering::Acquire) {
        return Err("Connection cancelled.".into());
    }
    phase("Opening terminal")?;
    let mut forwards = crate::forward::Forwarder::bind(&connect.local_forwards)?;
    for address in forwards.bind_remote(&mut connection, &connect.remote_forwards)? {
        phase(&format!("Remote forward listening on {address}"))?;
    }
    for address in forwards.bind_dynamic(&connect.dynamic_forwards)? {
        phase(&format!("SOCKS forward listening on {address}"))?;
    }
    for address in forwards.addresses() {
        phase(&format!("Local forward listening on {address}"))?;
    }
    let (columns, rows) = dimensions
        .lock()
        .map_err(|_| "Terminal size unavailable.")?
        .take()
        .unwrap_or((80, 24));
    let mut shell = ssh::with_phase_observer(
        phase_observer(output, stop, connect.target.to_string()),
        || {
            connection.open_shell_terminal_env(
                ssh::TerminalSize::from(columns.into(), rows.into()),
                Some(&connect.term),
                &connect.environment,
            )
        },
    )
    .map_err(|e| e.to_string())?;
    let startup = crate::remote_directory::startup(
        connect.remote_directory.as_deref(),
        connect.tmux.as_deref(),
    )?;
    if !startup.is_empty() {
        shell.write(&startup).map_err(|e| e.to_string())?;
    }
    deliver(output, Event::Connected, stop)?;
    let (mut received, mut sent) = (0u64, 0u64);
    let (mut heartbeat, mut keepalive) = (Instant::now(), Instant::now());
    let mut reported_traffic = (0, 0);
    let mut files = crate::files::Worker::new(file_requests);
    let mut key_installs = crate::key_install::Worker::new(key_requests);
    let mut pending_input = Zeroizing::new(Vec::new());
    let mut input_offset = 0;
    while !stop.load(Ordering::Acquire) {
        file_busy.store(true, Ordering::Release);
        files.poll(&mut connection);
        file_busy.store(files.has_pending_work(), Ordering::Release);
        key_installs.poll(&mut connection);
        for error in forwards.poll(&mut connection) {
            phase(&error)?;
        }
        for _ in 0..16 {
            if input_offset == pending_input.len() {
                let Some(bytes) = input.pop()? else {
                    break;
                };
                pending_input = Zeroizing::new(bytes);
                input_offset = 0;
            }
            let count = shell
                .try_write(&pending_input[input_offset..])
                .map_err(|e| e.to_string())?;
            if count == 0 {
                break;
            }
            input_offset += count;
            sent = sent.saturating_add(count as u64);
            if input_offset == pending_input.len() {
                pending_input.zeroize();
                input_offset = 0;
            }
        }
        // Release the lock before the network write: the UI thread takes it in
        // `Session::resize` and must never wait on a stalled connection.
        let resized = dimensions
            .lock()
            .map_err(|_| "Terminal size unavailable.")?
            .take();
        if let Some((columns, rows)) = resized {
            shell
                .window_change(ssh::TerminalSize::from(columns.into(), rows.into()))
                .map_err(|e| e.to_string())?;
        }
        if let Some(bytes) = shell.try_read().map_err(|e| e.to_string())? {
            received = received.saturating_add(bytes.len() as u64);
            for chunk in bytes.chunks(8192) {
                deliver(output, Event::Output(chunk.to_vec()), stop)?;
            }
        }
        if shell.remote_closed() {
            let status = shell.received_exit_status();
            deliver(output, Event::ShellExited(status), stop)?;
            return match status {
                Some(0) => Ok(()),
                Some(status) => Err(format!("Remote shell exited with status {status}.")),
                None => Err("Remote shell closed without an exit status.".into()),
            };
        }
        if heartbeat.elapsed() >= Duration::from_secs(1) {
            // These are traffic snapshots, not protocol keepalives. Avoid
            // waking the UI repeatedly when an idle session has no new counts.
            if reported_traffic != (received, sent) {
                deliver(output, Event::Heartbeat { received, sent }, stop)?;
                reported_traffic = (received, sent);
            }
            heartbeat = Instant::now();
        }
        if connect
            .keepalive
            .is_some_and(|interval| keepalive.elapsed() >= interval)
        {
            shell.keepalive().map_err(|e| e.to_string())?;
            keepalive = Instant::now();
        }
        thread::sleep(Duration::from_millis(4));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn event_delivery_wakes_after_enqueue_and_not_on_failed_send() {
        use std::sync::atomic::AtomicUsize;
        let (sender, receiver) = mpsc::sync_channel(1);
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let output = EventSender {
            sender,
            wakeup: Arc::new(Mutex::new(Some(Arc::new(move || {
                count.fetch_add(1, Ordering::SeqCst);
            })))),
        };
        deliver(&output, Event::Connected, &AtomicBool::new(false)).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(matches!(receiver.try_recv().unwrap(), Event::Connected));
        drop(receiver);
        assert!(deliver(&output, Event::Connected, &AtomicBool::new(false)).is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    use super::*;
    #[test]
    fn application_cleanup_waits_for_all_retired_sessions_and_has_a_shared_bound() {
        let tracker = CleanupTracker::new();
        let first = tracker.register();
        let second = tracker.register();
        assert!(!tracker.wait(Duration::from_millis(20)));
        drop(first);
        assert!(!tracker.wait(Duration::ZERO));
        thread::scope(|scope| {
            scope.spawn(|| {
                thread::sleep(Duration::from_millis(20));
                drop(second);
            });
            assert!(tracker.wait(Duration::from_secs(2)));
        });
        assert!(tracker.wait(Duration::ZERO));
    }

    #[test]
    fn tab_close_drains_output_and_waits_for_file_cleanup_without_blocking_caller() {
        let (output, events) = mpsc::sync_channel(1);
        let (_completed, completion) = mpsc::channel();
        let (files, _) = mpsc::sync_channel(1);
        let (key_installs, _) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let file_busy = Arc::new(AtomicBool::new(true));
        let session = Session {
            wakeup: Wakeup::default(),
            events,
            completion,
            files,
            key_installs,
            file_busy: file_busy.clone(),
            input: Arc::new(crate::input_queue::Queue::default()),
            cancelled: cancelled.clone(),
            finished: Arc::new(AtomicBool::new(false)),
            size: Arc::new(Mutex::new(None)),
        };
        output.try_send(Event::Output(vec![1])).unwrap();
        session.close_after_file_cleanup();
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if output.try_send(Event::Output(vec![2])).is_ok() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Closed tab stopped draining output"
            );
            thread::sleep(Duration::from_millis(4));
        }
        assert!(
            !cancelled.load(Ordering::Acquire),
            "Transport closed before cleanup"
        );
        file_busy.store(false, Ordering::Release);
        while !cancelled.load(Ordering::Acquire) {
            assert!(
                Instant::now() < deadline,
                "Transport remained open after cleanup"
            );
            thread::sleep(Duration::from_millis(4));
        }
    }

    #[test]
    fn final_query_does_not_replace_remote_exit_status() {
        let (output, events) = mpsc::sync_channel(2);
        let (completed, completion) = mpsc::channel();
        let (files, _) = mpsc::sync_channel(1);
        let (key_installs, _) = mpsc::sync_channel(1);
        let session = Session {
            wakeup: Wakeup::default(),
            events,
            completion,
            files,
            file_busy: Arc::new(AtomicBool::new(false)),
            key_installs,
            input: Arc::new(crate::input_queue::Queue::default()),
            cancelled: Arc::new(AtomicBool::new(false)),
            finished: Arc::new(AtomicBool::new(false)),
            size: Arc::new(Mutex::new(None)),
        };
        // Active sessions must still report backpressure rather than lose replies.
        for _ in 0..8 {
            session.send(vec![b'x'; 1024 * 1024]).unwrap();
        }
        assert!(session
            .reply(b"reply".to_vec())
            .unwrap_err()
            .contains("full"));
        output
            .send(Event::Output(b"last output\x1b[6n".to_vec()))
            .unwrap();
        output.send(Event::ShellExited(Some(0))).unwrap();
        finish_worker(|| Ok(()), &completed, &session.finished);
        session.input.close();
        drop(output);
        let Ok(Event::Output(bytes)) = session.try_event() else {
            panic!("final output missing")
        };
        let mut terminal = crate::terminal::Terminal::new(24, 80, 100);
        let parsed = terminal.feed(&bytes);
        assert!(!parsed.replies.is_empty());
        assert!(session.send(parsed.replies.clone()).is_err());
        session.reply(parsed.replies).unwrap();
        assert!(matches!(
            session.try_event(),
            Ok(Event::ShellExited(Some(0)))
        ));
        assert!(matches!(session.try_event(), Ok(Event::Disconnected(_))));
    }
    #[test]
    fn completion_cannot_overtake_the_last_event_sender() {
        let (output, events) = mpsc::sync_channel(1);
        let (completed, completion) = mpsc::channel();
        let (files, _) = mpsc::sync_channel(1);
        let (key_installs, _) = mpsc::sync_channel(1);
        let session = Session {
            wakeup: Wakeup::default(),
            events,
            completion,
            files,
            file_busy: Arc::new(AtomicBool::new(false)),
            key_installs,
            input: Arc::new(crate::input_queue::Queue::default()),
            cancelled: Arc::new(AtomicBool::new(false)),
            finished: Arc::new(AtomicBool::new(false)),
            size: Arc::new(Mutex::new(None)),
        };
        completed.send("Session closed.".into()).unwrap();
        // An initially empty queue is not proof that the producer has dropped
        // its final sender. Both receive paths must preserve event ordering.
        assert!(matches!(
            session.try_event(),
            Err(mpsc::TryRecvError::Empty)
        ));
        assert!(matches!(
            session.recv_event(Duration::from_millis(1)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        output.send(Event::ShellExited(Some(0))).unwrap();
        drop(output);
        assert!(matches!(
            session.try_event(),
            Ok(Event::ShellExited(Some(0)))
        ));
        assert!(matches!(session.try_event(), Ok(Event::Disconnected(_))));
    }

    #[test]
    fn panic_and_full_output_queue_preserve_final_status_and_output_order() {
        let (output, events) = mpsc::sync_channel(1);
        let (completed, completion) = mpsc::channel();
        let input = Arc::new(crate::input_queue::Queue::default());
        let (files, _file_requests) = mpsc::sync_channel(1);
        let (key_installs, _key_requests) = mpsc::sync_channel(1);
        let session = Session {
            wakeup: Wakeup::default(),
            events,
            completion,
            input,
            files,
            file_busy: Arc::new(AtomicBool::new(false)),
            key_installs,
            cancelled: Arc::new(AtomicBool::new(false)),
            finished: Arc::new(AtomicBool::new(false)),
            size: Arc::new(Mutex::new(None)),
        };
        output.send(Event::Output(b"last output".to_vec())).unwrap();
        finish_worker(
            || panic!("sensitive panic payload"),
            &completed,
            &session.finished,
        );
        drop(output);
        assert!(session.is_finished());
        assert!(matches!(session.try_event(), Ok(Event::Output(bytes)) if bytes == b"last output"));
        assert!(
            matches!(session.recv_event(Duration::from_millis(1)), Ok(Event::Disconnected(detail)) if detail.contains("reconnect") && !detail.contains("sensitive"))
        );
        assert!(session.try_event().is_err());
    }
    #[test]
    fn cancelled_worker_can_escape_full_output_queue() {
        let (tx, _rx) = mpsc::sync_channel(1);
        tx.send(Event::Connected).unwrap();
        let stop = AtomicBool::new(true);
        assert!(deliver(
            &EventSender {
                sender: tx,
                wakeup: Wakeup::default()
            },
            Event::Connected,
            &stop
        )
        .is_err());
    }
    #[test]
    fn closing_auth_dialog_aborts_wait() {
        let (tx, rx) = mpsc::sync_channel::<bool>(1);
        drop(tx);
        assert!(answer(rx, &AtomicBool::new(false)).is_err());
    }
}
