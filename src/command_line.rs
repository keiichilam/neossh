use crate::Args;
use crossterm::{
    event::{self, Event as LocalEvent, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind},
    execute, terminal,
};
use neo_ssh::{
    config::{Config, Profile},
    input::{self, Key, Modifiers, MouseAction},
    preferences::Preferences,
    session::{Connect, Event, Session},
    terminal::Terminal,
    trust::HostKey,
};
use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    io::{self, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use zeroize::Zeroizing;

/// ssh4's upload folder, used by `--once` when neither `--remote-dir` nor the
/// profile names one.
const ONCE_REMOTE_DIR: &str = "/tmp";
/// ssh4 profiles keep their upload folder here; NEO-SSH preserves it as a
/// profile extension and `--save-profile` writes it for `--remote-dir`.
const REMOTE_DIR_KEY: &str = "remote_dir";
/// Per-profile history length shared with ssh4 and `Preferences::history_lines`.
const SCROLLBACK_KEY: &str = "scrollback_lines";

struct Console {
    active: bool,
    #[cfg(windows)]
    output_mode: Option<u32>,
    #[cfg(windows)]
    code_pages: Option<CodePages>,
}
impl Console {
    fn new(active: bool) -> Self {
        Self {
            active,
            #[cfg(windows)]
            output_mode: {
                use windows_sys::Win32::System::Console::{
                    GetConsoleMode, GetStdHandle, STD_OUTPUT_HANDLE,
                };
                let mut mode = 0;
                (unsafe { GetConsoleMode(GetStdHandle(STD_OUTPUT_HANDLE), &mut mode) } != 0)
                    .then_some(mode)
            },
            #[cfg(windows)]
            code_pages: None,
        }
    }
    fn resume(&mut self) -> Result<(), String> {
        terminal::enable_raw_mode().map_err(|e| e.to_string())?;
        self.active = true;
        #[cfg(windows)]
        if self.code_pages.is_none() {
            self.code_pages = CodePages::utf8();
        }
        execute!(
            io::stdout(),
            event::EnableMouseCapture,
            event::EnableBracketedPaste
        )
        .map_err(|e| e.to_string())
    }
    fn suspend(&mut self) {
        if self.active {
            let _ = execute!(
                io::stdout(),
                event::DisableMouseCapture,
                event::DisableBracketedPaste
            );
            let _ = terminal::disable_raw_mode();
            #[cfg(windows)]
            if let Some(mode) = self.output_mode {
                use windows_sys::Win32::System::Console::{
                    GetStdHandle, SetConsoleMode, STD_OUTPUT_HANDLE,
                };
                unsafe {
                    SetConsoleMode(GetStdHandle(STD_OUTPUT_HANDLE), mode);
                }
            }
            #[cfg(windows)]
            if let Some(pages) = self.code_pages.take() {
                pages.restore();
            }
            self.active = false;
        }
    }
}
impl Drop for Console {
    fn drop(&mut self) {
        self.suspend();
    }
}

/// Console code pages in effect before the session switched to UTF-8, as ssh4
/// did, so legacy conhost renders remote UTF-8 instead of locale mojibake.
#[cfg(windows)]
#[derive(Clone, Copy)]
struct CodePages {
    input: u32,
    output: u32,
}
#[cfg(windows)]
impl CodePages {
    const UTF8: u32 = 65001;
    /// Switch input and output to UTF-8, returning the pages to restore.
    fn utf8() -> Option<Self> {
        use windows_sys::Win32::System::Console::{
            GetConsoleCP, GetConsoleOutputCP, SetConsoleCP, SetConsoleOutputCP,
        };
        let original = unsafe {
            Self {
                input: GetConsoleCP(),
                output: GetConsoleOutputCP(),
            }
        };
        // Zero means the process has no console to configure.
        if original.input == 0 || original.output == 0 {
            return None;
        }
        unsafe {
            SetConsoleOutputCP(Self::UTF8);
            SetConsoleCP(Self::UTF8);
        }
        Some(original)
    }
    fn restore(self) {
        use windows_sys::Win32::System::Console::{SetConsoleCP, SetConsoleOutputCP};
        unsafe {
            SetConsoleOutputCP(self.output);
            SetConsoleCP(self.input);
        }
    }
}

pub fn run(args: Args, mut config: Config, config_path: &Path) -> Result<(), String> {
    let profiles = config.profiles()?;
    let profile = args
        .profile
        .as_ref()
        .map(|name| {
            profiles
                .get(name)
                .ok_or_else(|| format!("Unknown profile: {name}"))
        })
        .transpose()?;
    if let Some(name) = &args.save_profile {
        check_profile_name(&profiles, name, args.profile.as_deref())?;
    }
    let remote_dir = upload_folder(args.remote_dir.as_deref(), profile);
    let preferences = Preferences::load(&config);
    let history = history_lines(&preferences, profile, args.scrollback_lines);
    let home = Config::path()
        .map_err(|e| e.to_string())?
        .parent()
        .ok_or("Home directory unavailable.")?
        .to_path_buf();
    let imports = neo_ssh::ssh_config::SshConfig::load(&home.join(".ssh/config"))?;
    for warning in imports.warnings() {
        eprintln!("{warning}");
    }
    let local_user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default();
    let mut save_as = args.save_profile.as_deref();
    loop {
        let mut connect = neo_ssh::connection::resolve(
            profile,
            neo_ssh::connection::Overrides {
                target: args.target.clone(),
                user: args.user.clone(),
                key: args.identity.clone(),
                tmux: args.tmux_name(),
                no_tmux: args.no_tmux,
                jump: args.jump.clone(),
            },
            &imports,
            &home,
            &local_user,
            config
                .get("keep_alive")
                .and_then(toml::Value::as_bool)
                .unwrap_or(false),
        )?;
        customize(&mut connect, &args)?;
        if let Some(name) = save_as.take() {
            let saved = profile_to_save(profile, &args, &connect)?;
            config
                .save_profile(config_path, name, &saved)
                .map_err(|e| format!("Cannot save profile '{name}': {e}"))?;
            eprintln!("Saved profile '{name}' in {}.", config_path.display());
        }
        if args.once {
            return upload_once(connect, remote_dir.as_deref().unwrap_or(ONCE_REMOTE_DIR));
        }
        let identity = connect.target.to_string();
        let (columns, rows) = terminal::size().unwrap_or((80, 24));
        let session = Session::start(connect, columns, rows);
        let mut stats = SessionStats::default();
        let result = interact(
            &session,
            columns,
            rows,
            &View {
                log: args.log.as_ref(),
                preferences: &preferences,
                history,
                remote_dir: remote_dir.as_deref(),
            },
            &mut stats,
        );
        let connected_for = stats.connected.map(|at| at.elapsed());
        drop(session);
        if let Err(error) = &result {
            eprintln!("\n{error}");
        }
        if let Some(duration) = connected_for {
            let traffic = stats.totals();
            eprintln!(
                "{}{}",
                if result.is_ok() { "\n" } else { "" },
                disconnect_summary(&identity, duration, traffic)
            );
        }
        if let Ok(true) = result {
            return Ok(());
        }
        if args.no_reconnect || !confirm("Reconnect? [y/N] ")? {
            return result.map(|_| ());
        }
    }
}

/// Apply command-line forwards and the connect timeout to a resolved route.
/// `--timeout` takes the place of a profile's `connect_timeout_secs` (and the
/// imported `ConnectTimeout`), which governs the destination.
fn customize(connect: &mut Connect, args: &Args) -> Result<(), String> {
    for spec in &args.local_forwards {
        connect.local_forwards.push(spec.parse()?);
    }
    for spec in &args.dynamic_forwards {
        connect.dynamic_forwards.push(spec.parse()?);
    }
    for spec in &args.remote_forwards {
        connect.remote_forwards.push(spec.parse()?);
    }
    if let Some(seconds) = args.timeout {
        connect.timeout = Duration::from_secs(seconds);
    }
    Ok(())
}

/// `--save-profile` never silently replaces another saved profile; updating
/// one requires loading it with `-P` under the same name.
fn check_profile_name(
    existing: &BTreeMap<String, Profile>,
    name: &str,
    loaded: Option<&str>,
) -> Result<(), String> {
    if existing.contains_key(name) && loaded != Some(name) {
        return Err(format!(
            "A saved profile named '{name}' already exists. Load it with -P to update it, or choose another name."
        ));
    }
    Ok(())
}

/// The profile `--save-profile` writes: the loaded profile (if any) plus this
/// command line, pinned to the resolved port, user and host-key policy because a
/// profile takes precedence over those imported settings; likewise, a forward
/// kind given on the command line keeps the imported forwards that were active.
/// Unset keys, jumps, timeouts and other forward kinds keep falling back to
/// ssh_config. Profiles never hold passwords.
fn profile_to_save(
    base: Option<&Profile>,
    args: &Args,
    connect: &Connect,
) -> Result<Profile, String> {
    let mut profile = base.cloned().unwrap_or_default();
    if let Some(raw) = &args.target {
        profile.host = raw.parse::<neo_ssh::target::Target>()?.host;
    }
    profile.port = connect.target.port;
    profile.user = connect.target.user.clone().unwrap_or_default();
    profile.strict_host_key = connect.strict;
    if let Some(path) = &args.identity {
        profile.key_path = saved_key_path(path);
    }
    if args.no_tmux {
        profile.tmux_session.clear();
    } else if let Some(name) = args.tmux_name() {
        profile.tmux_session = name;
    }
    if let Some(jump) = &args.jump {
        profile.jump = jump.clone();
    }
    if let Some(seconds) = args.timeout {
        profile.connect_timeout_secs = seconds;
    }
    save_forwards(
        &mut profile.local_forwards,
        &connect.local_forwards,
        &args.local_forwards,
    )?;
    save_forwards(
        &mut profile.remote_forwards,
        &connect.remote_forwards,
        &args.remote_forwards,
    )?;
    save_forwards(
        &mut profile.dynamic_forwards,
        &connect.dynamic_forwards,
        &args.dynamic_forwards,
    )?;
    if let Some(lines) = args.scrollback_lines {
        profile
            .extensions
            .insert(SCROLLBACK_KEY.into(), toml::Value::Integer(lines.into()));
    }
    if let Some(dir) = &args.remote_dir {
        profile
            .extensions
            .insert(REMOTE_DIR_KEY.into(), toml::Value::String(dir.clone()));
    }
    Ok(profile)
}

/// Adds command-line forwards (`added`) to one saved forward list. A nonempty
/// saved list replaces the imported forwards of its kind, so when it is empty
/// the imported forwards that were active are saved first: `active` holds them
/// followed by the `added` ones that `customize` appended. Forwards already in
/// the list are not saved twice. Kinds without added forwards are untouched,
/// and an empty list keeps following ssh_config.
fn save_forwards<T>(saved: &mut Vec<String>, active: &[T], added: &[String]) -> Result<(), String>
where
    T: std::str::FromStr<Err = String> + PartialEq + std::fmt::Display,
{
    if added.is_empty() {
        return Ok(());
    }
    if saved.is_empty() {
        let inherited = active.len().saturating_sub(added.len());
        saved.extend(active[..inherited].iter().map(T::to_string));
    }
    let mut kept = saved
        .iter()
        .map(|spec| spec.parse())
        .collect::<Result<Vec<T>, _>>()?;
    for spec in added {
        let forward: T = spec.parse()?;
        if !kept.contains(&forward) {
            kept.push(forward);
            saved.push(spec.clone());
        }
    }
    Ok(())
}

/// Saved key paths must not depend on the directory `neo-ssh` ran from; `~` and
/// `%` tokens are left for ssh_config-style expansion at connect time.
fn saved_key_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    if path.is_absolute() || text.starts_with('~') || text.contains('%') {
        return text.into_owned();
    }
    std::env::current_dir()
        .map(|directory| directory.join(path).to_string_lossy().into_owned())
        .unwrap_or_else(|_| text.into_owned())
}

/// Upload folder from `--remote-dir`, else a valid ssh4 profile `remote_dir`.
fn upload_folder(flag: Option<&str>, profile: Option<&Profile>) -> Option<String> {
    if let Some(directory) = flag {
        return Some(directory.to_owned());
    }
    let saved = profile?.extensions.get(REMOTE_DIR_KEY)?.as_str()?;
    match neo_ssh::remote_directory::validate(saved) {
        Ok(()) => Some(saved.to_owned()),
        Err(error) => {
            eprintln!("Ignoring the profile's remote_dir {saved:?}: {error}");
            None
        }
    }
}

/// History length: `--scrollback-lines` overrides the profile's and the global
/// length; the Preferences on/off switch still applies.
fn history_lines(
    preferences: &Preferences,
    profile: Option<&Profile>,
    lines: Option<u32>,
) -> usize {
    let Some(lines) = lines else {
        return preferences.history_lines(profile);
    };
    let mut profile = profile.cloned().unwrap_or_default();
    profile
        .extensions
        .insert(SCROLLBACK_KEY.into(), toml::Value::Integer(lines.into()));
    preferences.history_lines(Some(&profile))
}

/// `--once`: connect with the interactive CLI's trust and authentication
/// prompts, upload the clipboard PNG without replacing remote files, and print
/// its remote path. The local PNG is removed on every return path, and a
/// cancelled remote SCP stage gets the session's bounded cleanup before exit.
fn upload_once(mut connect: Connect, remote_dir: &str) -> Result<(), String> {
    // A one-shot upload needs no tmux attach, startup directory or listeners;
    // a profile's forwards may already be bound by an interactive session.
    connect.tmux = None;
    connect.remote_directory = None;
    connect.local_forwards.clear();
    connect.remote_forwards.clear();
    connect.dynamic_forwards.clear();
    let (columns, rows) = terminal::size().unwrap_or((80, 24));
    let session = Session::start(connect, columns, rows);
    let result = once(&session, remote_dir);
    session.close_after_file_cleanup();
    neo_ssh::session::wait_for_file_cleanup();
    println!("{}", result?);
    Ok(())
}

fn once(session: &Session, remote_dir: &str) -> Result<String, String> {
    let mut connected = false;
    while !connected {
        once_events(session, &mut connected)?;
        std::thread::sleep(Duration::from_millis(10));
    }
    let job = crate::clipboard_image::Job::start()?;
    let keys = RawKeys::enable();
    // Raw mode may be active from here on: end lines explicitly.
    eprint!(
        "Reading clipboard image{}\r\n",
        if keys.is_some() {
            "; Esc or Ctrl+C cancels."
        } else {
            "."
        }
    );
    let cancelled = || keys.as_ref().is_some_and(RawKeys::cancelled);
    let captured = loop {
        once_events(session, &mut connected)?;
        if let Some(result) = job.poll() {
            break result?;
        }
        if cancelled() {
            return Err("Clipboard capture cancelled.".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let remote = join_remote(remote_dir, &clip_file_name(SystemTime::now()));
    eprint!(
        "Uploading {} x {} PNG to {}\r\n",
        captured.width,
        captured.height,
        remote.escape_debug()
    );
    let upload = ClipboardUpload {
        task: session.file_action(neo_ssh::files::Action::UploadScp {
            local: captured.file.path().into(),
            remote: remote.clone(),
        })?,
        _file: captured.file,
        remote,
        insert: false,
    };
    loop {
        once_events(session, &mut connected)?;
        if let Some(result) = upload.task.poll() {
            result.map_err(|error| format!("Clipboard upload failed: {error}"))?;
            break;
        }
        if cancelled() {
            return Err("Clipboard upload cancelled.".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(keys);
    match arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(upload.remote.clone()))
    {
        Ok(()) => eprintln!("Remote path copied to the clipboard."),
        Err(error) => eprintln!("Upload succeeded; cannot copy remote path: {error}"),
    }
    Ok(upload.remote.clone())
}

/// Drain session events for `--once`: answer prompts exactly as the interactive
/// CLI does and discard terminal output so the worker never blocks on it.
fn once_events(session: &Session, connected: &mut bool) -> Result<(), String> {
    for _ in 0..64 {
        match session.try_event() {
            Ok(Event::Phase(text)) => eprint!("{text}\r\n"),
            Ok(Event::Connected) => *connected = true,
            Ok(Event::Output(_) | Event::Heartbeat { .. }) => {}
            Ok(Event::Trust { key, answer }) => {
                let accepted = review_host_key(&key)?;
                let _ = answer.send(accepted);
            }
            Ok(Event::Challenge {
                name,
                instruction,
                prompts,
                answer,
            }) => {
                let _ = answer.send(answer_challenge(&name, &instruction, prompts)?);
            }
            Ok(Event::ShellExited(_)) => {
                return Err("The remote shell closed before the upload finished.".into())
            }
            Ok(Event::Disconnected(reason)) => return Err(reason),
            Err(std::sync::mpsc::TryRecvError::Empty) => break,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                return Err("The session ended before the upload finished.".into())
            }
        }
    }
    Ok(())
}

/// Raw keyboard input while `--once` captures and uploads, so Esc or Ctrl+C
/// cancel through the normal cleanup path instead of terminating the process.
/// Absent when there is no interactive console (for example under a scheduler).
struct RawKeys;
impl RawKeys {
    fn enable() -> Option<Self> {
        terminal::enable_raw_mode().ok().map(|()| Self)
    }
    fn cancelled(&self) -> bool {
        while event::poll(Duration::ZERO).unwrap_or(false) {
            if let Ok(LocalEvent::Key(key)) = event::read() {
                if key.kind != KeyEventKind::Release
                    && (key.code == KeyCode::Esc
                        || key.modifiers.contains(KeyModifiers::CONTROL)
                            && matches!(key.code, KeyCode::Char('c' | 'C')))
                {
                    return true;
                }
            }
        }
        false
    }
}
impl Drop for RawKeys {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

fn review_host_key(key: &HostKey) -> Result<bool, String> {
    let (heading, advice) = key.trust_warning();
    eprintln!(
        "{heading}\n{advice}\n{}\n{}",
        key.algorithm, key.fingerprint
    );
    confirm("Trust this server key? [y/N] ")
}

fn answer_challenge(
    name: &str,
    instruction: &str,
    prompts: Vec<(String, bool)>,
) -> Result<Vec<String>, String> {
    eprintln!("{name}\n{instruction}");
    let mut answers = Vec::new();
    for (prompt, echo) in prompts {
        answers.push(if echo {
            text_prompt(&prompt)?
        } else {
            secret_prompt(&prompt)?.to_string()
        });
    }
    Ok(answers)
}

/// UTC calendar fields for seconds since the Unix epoch.
#[derive(Debug, PartialEq, Eq)]
struct UtcTime {
    year: u64,
    month: u64,
    day: u64,
    hour: u64,
    minute: u64,
    second: u64,
}
impl UtcTime {
    fn from_unix(seconds: u64) -> Self {
        let (days, time) = (seconds / 86_400, seconds % 86_400);
        // Howard Hinnant's civil_from_days, restricted to dates from 1970.
        let shifted = days + 719_468;
        let (era, day_of_era) = (shifted / 146_097, shifted % 146_097);
        let year_of_era =
            (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let month_index = (5 * day_of_year + 2) / 153;
        let month = if month_index < 10 {
            month_index + 3
        } else {
            month_index - 9
        };
        Self {
            year: era * 400 + year_of_era + u64::from(month <= 2),
            month,
            day: day_of_year - (153 * month_index + 2) / 5 + 1,
            hour: time / 3_600,
            minute: time / 60 % 60,
            second: time % 60,
        }
    }
}

/// ssh4's clipboard upload name, `clip_YYYYmmdd_HHMMSS.png`, in UTC.
fn clip_file_name(now: SystemTime) -> String {
    let time = UtcTime::from_unix(now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()));
    format!(
        "clip_{:04}{:02}{:02}_{:02}{:02}{:02}.png",
        time.year, time.month, time.day, time.hour, time.minute, time.second
    )
}

fn join_remote(directory: &str, name: &str) -> String {
    format!("{}/{name}", directory.trim_end_matches('/'))
}

fn upload_prompt(remote_dir: Option<&str>, default_name: &str) -> String {
    match remote_dir {
        Some(directory) => format!(
            "Remote file name or path (blank uses {}): ",
            join_remote(directory, default_name).escape_debug()
        ),
        None => "Remote file path (blank cancels): ".into(),
    }
}

/// Ctrl+Shift+I destination. Without an upload folder a blank answer cancels; with
/// one, blank selects the default name and a bare name is placed in the folder.
fn upload_destination(input: &str, remote_dir: Option<&str>, default_name: &str) -> Option<String> {
    match remote_dir {
        None => (!input.is_empty()).then(|| input.to_owned()),
        Some(directory) if input.is_empty() => Some(join_remote(directory, default_name)),
        Some(directory) if !input.contains('/') => Some(join_remote(directory, input)),
        Some(_) => Some(input.to_owned()),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Traffic {
    received: u64,
    sent: u64,
}

/// Connected time and traffic for the disconnect summary. Worker heartbeats
/// report wire counts at most once per second, so locally observed output and
/// input cover the final second before the session ends.
#[derive(Default)]
struct SessionStats {
    connected: Option<Instant>,
    reported: Traffic,
    local: Traffic,
}
impl SessionStats {
    fn totals(&self) -> Traffic {
        Traffic {
            received: self.reported.received.max(self.local.received),
            sent: self.reported.sent.max(self.local.sent),
        }
    }
}

fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60
    )
}

fn disconnect_summary(identity: &str, duration: Duration, traffic: Traffic) -> String {
    format!(
        "Disconnected from {identity} after {} (sent {} B, received {} B)",
        format_duration(duration),
        traffic.sent,
        traffic.received
    )
}

fn send(session: &Session, stats: &mut SessionStats, bytes: Vec<u8>) -> Result<(), String> {
    let count = bytes.len() as u64;
    session.send(bytes)?;
    stats.local.sent = stats.local.sent.saturating_add(count);
    Ok(())
}

struct View<'a> {
    log: Option<&'a PathBuf>,
    preferences: &'a Preferences,
    history: usize,
    remote_dir: Option<&'a str>,
}

fn interact(
    session: &Session,
    columns: u16,
    rows: u16,
    view: &View,
    stats: &mut SessionStats,
) -> Result<bool, String> {
    let mut console = Console::new(false);
    let mut terminal = Terminal::new(rows, columns, view.history);
    terminal.colors = neo_ssh::preferences::Colors::for_theme(&view.preferences.theme);
    let mut log = view
        .log
        .map(|path| OpenOptions::new().create(true).append(true).open(path))
        .transpose()
        .map_err(|e| e.to_string())?;
    let mut connected = false;
    let mut clipboard_job: Option<crate::clipboard_image::Job> = None;
    let mut upload: Option<ClipboardUpload> = None;
    loop {
        for _ in 0..32 {
            match session.try_event() {
                Ok(Event::Phase(text)) => eprintln!("{text}"),
                Ok(Event::Connected) => {
                    connected = true;
                    stats.connected.get_or_insert_with(Instant::now);
                    console.resume()?;
                }
                Ok(Event::Output(bytes)) => {
                    stats.local.received = stats.local.received.saturating_add(bytes.len() as u64);
                    if let Some(log) = &mut log {
                        log.write_all(&bytes).map_err(|e| e.to_string())?;
                    }
                    let output = terminal.feed(&bytes);
                    io::stdout()
                        .write_all(&output.display)
                        .and_then(|_| io::stdout().flush())
                        .map_err(|e| e.to_string())?;
                    let replies = output.replies.len() as u64;
                    session.reply(output.replies)?;
                    stats.local.sent = stats.local.sent.saturating_add(replies);
                }
                Ok(Event::Trust { key, answer }) => {
                    console.suspend();
                    let accepted = review_host_key(&key)?;
                    let _ = answer.send(accepted);
                    if connected {
                        console.resume()?;
                    }
                }
                Ok(Event::Challenge {
                    name,
                    instruction,
                    prompts,
                    answer,
                }) => {
                    console.suspend();
                    let _ = answer.send(answer_challenge(&name, &instruction, prompts)?);
                    if connected {
                        console.resume()?;
                    }
                }
                Ok(Event::ShellExited(status)) => {
                    return match status {
                        // Remote exit follows the reconnect preference; only an
                        // explicit local quit bypasses the prompt.
                        Some(0) => Ok(false),
                        Some(status) => Err(format!("Remote shell exited with status {status}.")),
                        None => Err("Remote shell closed without an exit status.".into()),
                    };
                }
                Ok(Event::Disconnected(reason)) => return Err(reason),
                Ok(Event::Heartbeat { received, sent }) => {
                    stats.reported = Traffic { received, sent };
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return Ok(false),
            }
        }
        if let Some(result) = clipboard_job.as_ref().and_then(|job| job.poll()) {
            clipboard_job = None;
            console.suspend();
            let reviewed = result.and_then(|captured| {
                eprintln!(
                    "Clipboard PNG: {} x {} pixels",
                    captured.width, captured.height
                );
                let default_name = clip_file_name(SystemTime::now());
                let input = text_prompt(&upload_prompt(view.remote_dir, &default_name))?;
                let Some(remote) = upload_destination(&input, view.remote_dir, &default_name)
                else {
                    return Ok(None);
                };
                eprintln!(
                    "SCP destination on the active connection: {}",
                    remote.escape_debug()
                );
                if !confirm("Upload this PNG? [y/N] ")? {
                    return Ok(None);
                }
                let insert = confirm("Insert the quoted remote path after upload? [y/N] ")?;
                ClipboardUpload::start(session, captured.file, remote, insert).map(Some)
            });
            match reviewed {
                Ok(pending) => upload = pending,
                Err(error) => eprintln!("Clipboard upload: {error}"),
            }
            console.resume()?;
        }
        if let Some(result) = upload.as_ref().and_then(|pending| pending.task.poll()) {
            let completed = upload.take().unwrap();
            match result {
                Ok(()) => {
                    eprintln!("\r\nUploaded PNG to {}", completed.remote.escape_debug());
                    if let Err(error) = arboard::Clipboard::new()
                        .and_then(|mut clipboard| clipboard.set_text(completed.remote.clone()))
                    {
                        eprintln!("\r\nUpload succeeded; cannot copy remote path: {error}");
                    }
                    if completed.insert {
                        send(
                            session,
                            stats,
                            terminal.paste(&quote_remote_path(&completed.remote)),
                        )?;
                    }
                }
                Err(error) => eprintln!("\r\nClipboard upload: {error}"),
            }
        }
        if connected && event::poll(Duration::from_millis(8)).map_err(|e| e.to_string())? {
            match event::read().map_err(|e| e.to_string())? {
                LocalEvent::Key(key) if key.kind != KeyEventKind::Release => {
                    let local = local_key(&key);
                    if local == Some(LocalKey::Quit) {
                        return Ok(true);
                    }
                    if local == Some(LocalKey::ClipboardImage) {
                        if let Some(pending) = &upload {
                            pending.task.cancel();
                            eprintln!("\r\nCancelling clipboard upload...");
                        } else if clipboard_job.take().is_some() {
                            eprintln!("\r\nClipboard capture cancelled.");
                        } else {
                            match crate::clipboard_image::Job::start() {
                                Ok(job) => {
                                    clipboard_job = Some(job);
                                    eprintln!("\r\nReading clipboard image; Ctrl+Shift+I cancels.");
                                }
                                Err(error) => eprintln!("\r\nClipboard image: {error}"),
                            }
                        }
                    } else if let Some(LocalKey::Paste { review }) = local {
                        let text = match arboard::Clipboard::new()
                            .and_then(|mut clipboard| clipboard.get_text())
                        {
                            Ok(text) => text,
                            Err(error) => {
                                eprintln!("\r\nCannot paste clipboard text: {error}");
                                continue;
                            }
                        };
                        let mut accepted = true;
                        if neo_ssh::terminal::needs_paste_confirmation(&text) && review {
                            console.suspend();
                            eprintln!("\n{}", neo_ssh::terminal::paste_preview(&text));
                            accepted = confirm("Send multiline paste? [y/N] ")?;
                            console.resume()?;
                        }
                        if accepted {
                            send(session, stats, terminal.paste(&text))?;
                        }
                    } else if let Some(code) = key_code(key.code) {
                        let mut modifiers = modifiers(key.modifiers);
                        modifiers.shift |= key.code == KeyCode::BackTab;
                        send(
                            session,
                            stats,
                            input::encode_key(
                                code,
                                modifiers,
                                terminal.screen().application_cursor(),
                            ),
                        )?;
                    }
                }
                LocalEvent::Paste(text) => {
                    console.suspend();
                    let accepted = !neo_ssh::terminal::needs_paste_confirmation(&text)
                        || confirm(&format!(
                            "\n{}\nSend multiline paste? [y/N] ",
                            neo_ssh::terminal::paste_preview(&text)
                        ))?;
                    console.resume()?;
                    if accepted {
                        send(session, stats, terminal.paste(&text))?;
                    }
                }
                LocalEvent::Resize(columns, rows) => {
                    terminal.resize(rows, columns);
                    session.resize(columns, rows);
                }
                LocalEvent::Mouse(mouse) => {
                    let button = |b| match b {
                        event::MouseButton::Left => 0,
                        event::MouseButton::Middle => 1,
                        event::MouseButton::Right => 2,
                    };
                    let action = match mouse.kind {
                        MouseEventKind::Down(b) => MouseAction::Press(button(b)),
                        MouseEventKind::Up(b) => MouseAction::Release(button(b)),
                        MouseEventKind::Drag(b) => MouseAction::Move(Some(button(b))),
                        MouseEventKind::Moved => MouseAction::Move(None),
                        MouseEventKind::ScrollUp => MouseAction::WheelUp,
                        MouseEventKind::ScrollDown => MouseAction::WheelDown,
                        _ => continue,
                    };
                    send(
                        session,
                        stats,
                        input::encode_mouse(
                            terminal.modes,
                            action,
                            mouse.column,
                            mouse.row,
                            modifiers(mouse.modifiers),
                        ),
                    )?;
                }
                _ => {}
            }
        } else {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

// Keep PNG staging alive until the worker reports completion. Dropping the task
// first requests cancellation when the terminal exits during an upload.
struct ClipboardUpload {
    task: neo_ssh::files::ActionTask,
    _file: tempfile::NamedTempFile,
    remote: String,
    insert: bool,
}
impl ClipboardUpload {
    fn start(
        session: &Session,
        file: tempfile::NamedTempFile,
        remote: String,
        insert: bool,
    ) -> Result<Self, String> {
        let task = session.file_action(neo_ssh::files::Action::UploadScp {
            local: file.path().into(),
            remote: remote.clone(),
        })?;
        eprintln!("Uploading clipboard PNG; Ctrl+Shift+I cancels.");
        Ok(Self {
            task,
            _file: file,
            remote,
            insert,
        })
    }
}

fn quote_remote_path(path: &str) -> String {
    format!("'{}'", path.replace('\'', "'\\''"))
}

fn modifiers(value: KeyModifiers) -> Modifiers {
    Modifiers {
        shift: value.contains(KeyModifiers::SHIFT),
        alt: value.contains(KeyModifiers::ALT),
        control: value.contains(KeyModifiers::CONTROL),
    }
}
fn key_code(value: KeyCode) -> Option<Key> {
    Some(match value {
        KeyCode::Char(c) => Key::Character(c),
        KeyCode::Enter => Key::Enter,
        KeyCode::Tab => Key::Tab,
        KeyCode::BackTab => return Some(Key::Tab),
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Esc => Key::Escape,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::Insert => Key::Insert,
        KeyCode::Delete => Key::Delete,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::F(n) => Key::Function(n),
        _ => return None,
    })
}
fn text_prompt(prompt: &str) -> Result<String, String> {
    eprint!("{prompt}");
    io::stderr().flush().map_err(|e| e.to_string())?;
    let mut text = String::new();
    if io::stdin()
        .read_line(&mut text)
        .map_err(|e| e.to_string())?
        == 0
    {
        return Err("Input closed.".into());
    }
    Ok(text.trim_end_matches(['\r', '\n']).into())
}
/// A key the CLI handles itself instead of sending to the remote.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LocalKey {
    Quit,
    ClipboardImage,
    Paste { review: bool },
}

/// The CLI's own shortcuts. They use Ctrl+Shift so that plain Ctrl keys stay
/// with the remote shell (Ctrl+P is previous-history). Ctrl+Shift chords need
/// a console that reports Shift with Ctrl, as the Windows console does; other
/// terminals send them as the plain Ctrl key, which then reaches the remote
/// (use `--once` to upload a clipboard image there). Ctrl+V pastes only on
/// Windows: elsewhere the terminal's own paste arrives as a bracketed paste
/// (reviewed like Ctrl+V), and Ctrl+V stays Vim's block selection and
/// readline's quoted insert. Chords with Alt (AltGr) are never taken.
fn local_key(key: &crossterm::event::KeyEvent) -> Option<LocalKey> {
    let held = |modifier| key.modifiers.contains(modifier);
    if !held(KeyModifiers::CONTROL) || held(KeyModifiers::ALT) {
        return None;
    }
    let shift = held(KeyModifiers::SHIFT);
    let KeyCode::Char(character) = key.code else {
        return None;
    };
    match character.to_ascii_lowercase() {
        'q' if shift => Some(LocalKey::Quit),
        'i' if shift => Some(LocalKey::ClipboardImage),
        'v' if cfg!(windows) => Some(LocalKey::Paste { review: !shift }),
        _ => None,
    }
}

fn confirm(prompt: &str) -> Result<bool, String> {
    Ok(matches!(
        text_prompt(prompt)?.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}
pub(crate) fn secret_prompt(prompt: &str) -> Result<Zeroizing<String>, String> {
    eprint!("{prompt}");
    io::stderr().flush().map_err(|e| e.to_string())?;
    terminal::enable_raw_mode().map_err(|e| e.to_string())?;
    let _guard = Console::new(true);
    let mut secret = Zeroizing::new(String::new());
    loop {
        if let LocalEvent::Key(key) = event::read().map_err(|e| e.to_string())? {
            if key.kind == KeyEventKind::Release {
                continue;
            }
            match key.code {
                KeyCode::Enter => {
                    eprint!("\r\n");
                    return Ok(secret);
                }
                KeyCode::Esc => return Err("Authentication cancelled.".into()),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Err("Authentication cancelled.".into())
                }
                KeyCode::Backspace => {
                    secret.pop();
                }
                KeyCode::Char(c) => secret.push(c),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use neo_ssh::{connection, ssh_config::SshConfig};

    fn args(arguments: &[&str]) -> Args {
        Args::try_parse_from(std::iter::once("neo-ssh").chain(arguments.iter().copied())).unwrap()
    }

    fn resolved(profile: Option<&Profile>, args: &Args, imports: &str) -> Connect {
        let mut connect = connection::resolve(
            profile,
            connection::Overrides {
                target: args.target.clone(),
                user: args.user.clone(),
                key: args.identity.clone(),
                tmux: args.tmux_name(),
                no_tmux: args.no_tmux,
                jump: args.jump.clone(),
            },
            &SshConfig::parse(imports).unwrap(),
            Path::new("/home/test"),
            "local",
            false,
        )
        .unwrap();
        customize(&mut connect, args).unwrap();
        connect
    }

    #[test]
    fn uploaded_path_insertion_quotes_shell_metacharacters_without_submitting() {
        let path = "/tmp/image '$(touch sentinel); name.png";
        let quoted = quote_remote_path(path);
        assert_eq!(quoted, "'/tmp/image '\\''$(touch sentinel); name.png'");
        let terminal = Terminal::new(24, 80, 0);
        assert_eq!(terminal.paste(&quoted), quoted.as_bytes());
        assert!(!quoted.ends_with('\n'));
        assert!(!quoted.ends_with('\r'));
    }

    #[test]
    fn utc_conversion_handles_epoch_leap_days_and_century_rules() {
        let at = |seconds| UtcTime::from_unix(seconds);
        let utc = |year, month, day, hour, minute, second| UtcTime {
            year,
            month,
            day,
            hour,
            minute,
            second,
        };
        assert_eq!(at(0), utc(1970, 1, 1, 0, 0, 0));
        assert_eq!(at(951_868_799), utc(2000, 2, 29, 23, 59, 59));
        assert_eq!(at(951_868_800), utc(2000, 3, 1, 0, 0, 0));
        assert_eq!(at(1_781_015_405), utc(2026, 6, 9, 14, 30, 5));
        // 2100 is not a leap year: 28 February is followed by 1 March.
        assert_eq!(at(4_107_542_400), utc(2100, 3, 1, 0, 0, 0));
        assert_eq!(at(4_107_542_399), utc(2100, 2, 28, 23, 59, 59));
        assert_eq!(at(253_402_300_799), utc(9999, 12, 31, 23, 59, 59));
    }

    #[test]
    fn clipboard_names_use_ssh4_format_in_utc() {
        let name = |seconds| clip_file_name(UNIX_EPOCH + Duration::from_secs(seconds));
        assert_eq!(name(1_781_015_405), "clip_20260609_143005.png");
        assert_eq!(name(0), "clip_19700101_000000.png");
        assert_eq!(
            clip_file_name(UNIX_EPOCH - Duration::from_secs(1)),
            "clip_19700101_000000.png"
        );
        assert_eq!(join_remote("/tmp", "a.png"), "/tmp/a.png");
        assert_eq!(join_remote("/tmp//", "a.png"), "/tmp/a.png");
        assert_eq!(join_remote("/", "a.png"), "/a.png");
    }

    #[test]
    fn ctrl_p_destination_defaults_only_with_an_upload_folder() {
        let default = "clip_20260609_143005.png";
        assert_eq!(upload_destination("", None, default), None);
        assert_eq!(
            upload_destination("shot.png", None, default).as_deref(),
            Some("shot.png")
        );
        assert_eq!(
            upload_prompt(None, default),
            "Remote file path (blank cancels): "
        );
        let folder = Some("/data/shots/");
        assert_eq!(
            upload_destination("", folder, default).as_deref(),
            Some("/data/shots/clip_20260609_143005.png")
        );
        assert_eq!(
            upload_destination("a b.png", folder, default).as_deref(),
            Some("/data/shots/a b.png")
        );
        assert_eq!(
            upload_destination("/other/x.png", folder, default).as_deref(),
            Some("/other/x.png")
        );
        assert_eq!(
            upload_destination("sub/x.png", folder, default).as_deref(),
            Some("sub/x.png")
        );
        assert!(upload_prompt(folder, default).contains("/data/shots/clip_20260609_143005.png"));
    }

    #[test]
    fn upload_folder_prefers_flag_then_valid_profile_value() {
        let mut profile = Profile::default();
        assert_eq!(upload_folder(None, None), None);
        assert_eq!(upload_folder(None, Some(&profile)), None);
        profile
            .extensions
            .insert(REMOTE_DIR_KEY.into(), toml::Value::String("/data".into()));
        assert_eq!(
            upload_folder(None, Some(&profile)).as_deref(),
            Some("/data")
        );
        assert_eq!(
            upload_folder(Some("/flag"), Some(&profile)).as_deref(),
            Some("/flag")
        );
        profile.extensions.insert(
            REMOTE_DIR_KEY.into(),
            toml::Value::String("relative".into()),
        );
        assert_eq!(upload_folder(None, Some(&profile)), None);
    }

    #[test]
    fn scrollback_flag_overrides_profile_and_global_but_not_the_off_switch() {
        let config = Config::parse(
            "scrollback_lines = 2000\n[profiles.small]\nhost = 'h'\nscrollback_lines = 500\n",
        )
        .unwrap();
        let profiles = config.profiles().unwrap();
        let small = profiles.get("small");
        let mut preferences = Preferences::load(&config);
        assert_eq!(history_lines(&preferences, None, None), 2000);
        assert_eq!(history_lines(&preferences, small, None), 500);
        assert_eq!(history_lines(&preferences, small, Some(30_000)), 30_000);
        assert_eq!(history_lines(&preferences, None, Some(150)), 150);
        preferences.scrollback = false;
        assert_eq!(history_lines(&preferences, small, Some(30_000)), 0);
    }

    #[test]
    fn save_profile_refuses_other_existing_names() {
        let mut existing = BTreeMap::new();
        existing.insert("prod".to_owned(), Profile::default());
        assert!(check_profile_name(&existing, "prod", None).is_err());
        assert!(check_profile_name(&existing, "prod", Some("staging")).is_err());
        assert!(check_profile_name(&existing, "prod", Some("prod")).is_ok());
        assert!(check_profile_name(&existing, "new", None).is_ok());
    }

    #[test]
    fn saved_profile_keeps_alias_and_pins_resolved_destination() {
        let imports = "Host web\nHostName web.internal\nUser deploy\nPort 2200\nStrictHostKeyChecking yes\nLocalForward 8080 db:5432\n";
        let key = std::env::temp_dir().join("id_web");
        let key = key.to_str().unwrap();
        let args = args(&[
            "web",
            "--save-profile",
            "web",
            "--timeout",
            "7",
            "--tmux-session",
            "ops",
            "-L",
            "9000:localhost:80",
            "--scrollback-lines",
            "2500",
            "--remote-dir",
            "/srv/drop",
            "-k",
            key,
        ]);
        let connect = resolved(None, &args, imports);
        assert_eq!(connect.timeout, Duration::from_secs(7));
        assert_eq!(connect.local_forwards.len(), 2);
        let profile = profile_to_save(None, &args, &connect).unwrap();
        assert_eq!(profile.host, "web");
        assert_eq!(profile.port, 2200);
        assert_eq!(profile.user, "deploy");
        assert!(profile.strict_host_key);
        assert_eq!(profile.key_path, key);
        assert_eq!(profile.tmux_session, "ops");
        assert_eq!(profile.connect_timeout_secs, 7);
        // The imported forward was active too; a saved list replaces imports.
        assert_eq!(
            profile.local_forwards,
            ["127.0.0.1:8080:db:5432", "9000:localhost:80"]
        );
        assert_eq!(
            profile.extensions[SCROLLBACK_KEY],
            toml::Value::Integer(2500)
        );
        assert_eq!(
            profile.extensions[REMOTE_DIR_KEY],
            toml::Value::String("/srv/drop".into())
        );
        // Saving drops no secrets and survives a round trip through the config.
        let mut config = Config::default();
        config.set_profile("web", &profile).unwrap();
        let reloaded = config.profiles().unwrap()["web"].clone();
        let again = resolved(Some(&reloaded), &self::args(&["-P", "web"]), imports);
        assert_eq!(again.target.to_string(), "deploy@web.internal:2200");
        assert_eq!(again.timeout, Duration::from_secs(7));
        assert_eq!(again.tmux.as_deref(), Some("ops"));
        assert!(again.strict);
        assert_eq!(again.local_forwards, connect.local_forwards);
    }

    #[test]
    fn saved_profile_keeps_inherited_forwards_of_every_kind() {
        let imports = "Host web\nLocalForward 8080 localhost:80\nRemoteForward [::1]:9000 [::1]:443\nDynamicForward [::1]:1080\nDynamicForward 1081\n";
        let args = args(&[
            "web",
            "--save-profile",
            "web",
            "-L",
            "8081:localhost:81",
            // Already imported: kept once.
            "-L",
            "127.0.0.1:8080:localhost:80",
            "-R",
            "9001:localhost:90",
            "-D",
            "1082",
        ]);
        let connect = resolved(None, &args, imports);
        let profile = profile_to_save(None, &args, &connect).unwrap();
        assert_eq!(
            profile.local_forwards,
            ["127.0.0.1:8080:localhost:80", "8081:localhost:81"]
        );
        assert_eq!(
            profile.remote_forwards,
            ["[::1]:9000:[::1]:443", "9001:localhost:90"]
        );
        assert_eq!(
            profile.dynamic_forwards,
            ["[::1]:1080", "127.0.0.1:1081", "1082"]
        );
        let again = resolved(Some(&profile), &self::args(&["-P", "web"]), imports);
        assert_eq!(again.local_forwards, connect.local_forwards[..2]);
        assert_eq!(again.remote_forwards, connect.remote_forwards);
        assert_eq!(again.dynamic_forwards, connect.dynamic_forwards);
        // Kinds without command-line forwards stay unset and keep importing.
        let args = self::args(&["web", "--save-profile", "web", "-L", "8081:localhost:81"]);
        let profile = profile_to_save(None, &args, &resolved(None, &args, imports)).unwrap();
        assert!(profile.remote_forwards.is_empty() && profile.dynamic_forwards.is_empty());
    }

    #[test]
    fn saved_profile_updates_loaded_profile_without_losing_its_options() {
        let base = Profile {
            host: "db".into(),
            user: "admin".into(),
            port: 2222,
            term: "vt100".into(),
            tmux_session: "old".into(),
            local_forwards: vec!["5432:localhost:5432".into()],
            ..Default::default()
        };
        let args = args(&[
            "-P",
            "db",
            "--save-profile",
            "db",
            "--no-tmux",
            "-u",
            "operator",
            "--local",
            "5432:localhost:5432",
        ]);
        let connect = resolved(Some(&base), &args, "");
        let profile = profile_to_save(Some(&base), &args, &connect).unwrap();
        assert_eq!(profile.host, "db");
        assert_eq!(profile.port, 2222);
        assert_eq!(profile.user, "operator");
        assert_eq!(profile.term, "vt100");
        assert!(profile.tmux_session.is_empty());
        assert_eq!(profile.local_forwards, ["5432:localhost:5432"]);
        assert_eq!(profile.connect_timeout_secs, 0);
    }

    #[test]
    fn local_keys_leave_shell_control_keys_to_the_remote() {
        use crossterm::event::KeyEvent;
        let key = |code, modifiers| local_key(&KeyEvent::new(code, modifiers));
        let ctrl = KeyModifiers::CONTROL;
        let ctrl_shift = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        assert_eq!(key(KeyCode::Char('Q'), ctrl_shift), Some(LocalKey::Quit));
        assert_eq!(
            key(KeyCode::Char('I'), ctrl_shift),
            Some(LocalKey::ClipboardImage)
        );
        // Ctrl+P is previous-history; AltGr-style chords type characters.
        for (code, modifiers) in [
            (KeyCode::Char('p'), ctrl),
            (KeyCode::Char('q'), ctrl),
            (KeyCode::Char('i'), ctrl),
            (KeyCode::Tab, KeyModifiers::NONE),
            (KeyCode::Char('I'), ctrl_shift | KeyModifiers::ALT),
            (KeyCode::Char('v'), ctrl | KeyModifiers::ALT),
        ] {
            assert_eq!(key(code, modifiers), None, "{code:?} {modifiers:?}");
        }
        // Windows consoles deliver Ctrl+V as a key; elsewhere the terminal's own
        // paste arrives as a reviewed bracketed paste, and Ctrl+V stays Vim's
        // block selection and readline's quoted insert.
        let paste = |review| cfg!(windows).then_some(LocalKey::Paste { review });
        assert_eq!(key(KeyCode::Char('v'), ctrl), paste(true));
        assert_eq!(key(KeyCode::Char('V'), ctrl_shift), paste(false));
    }

    #[test]
    fn relative_key_paths_are_saved_absolute() {
        assert_eq!(saved_key_path(Path::new("~/.ssh/id")), "~/.ssh/id");
        let saved = saved_key_path(Path::new("keys/id"));
        assert!(Path::new(&saved).is_absolute(), "{saved}");
        assert!(saved.ends_with("id"));
    }

    #[cfg(windows)]
    #[test]
    fn utf8_console_code_pages_are_restored() {
        use windows_sys::Win32::System::Console::{GetConsoleCP, GetConsoleOutputCP};
        let current = || unsafe { (GetConsoleCP(), GetConsoleOutputCP()) };
        let before = current();
        let Some(saved) = CodePages::utf8() else {
            // Without a console there is nothing to switch or restore.
            assert!(before.0 == 0 || before.1 == 0);
            return;
        };
        assert_eq!(current(), (CodePages::UTF8, CodePages::UTF8));
        assert_eq!((saved.input, saved.output), before);
        saved.restore();
        assert_eq!(current(), before);
    }

    #[test]
    fn disconnect_summary_reports_duration_and_freshest_traffic() {
        let mut stats = SessionStats {
            reported: Traffic {
                received: 900,
                sent: 40,
            },
            local: Traffic {
                received: 1_000,
                sent: 12,
            },
            ..Default::default()
        };
        assert_eq!(
            stats.totals(),
            Traffic {
                received: 1_000,
                sent: 40
            }
        );
        stats.local.sent = 45;
        assert_eq!(
            disconnect_summary(
                "ops@host:2222",
                Duration::from_secs(3 * 3_600 + 4 * 60 + 5),
                stats.totals()
            ),
            "Disconnected from ops@host:2222 after 03:04:05 (sent 45 B, received 1000 B)"
        );
        assert_eq!(format_duration(Duration::from_millis(999)), "00:00:00");
        assert_eq!(
            format_duration(Duration::from_secs(100 * 3_600)),
            "100:00:00"
        );
    }
}
