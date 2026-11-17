use egui::{Color32, FontId, RichText, Vec2};
use neo_ssh::{
    config::{Config, Profile, Snippet},
    input::{self, Key, Modifiers, MouseAction, MouseTracker},
    search::{SearchJob, SearchResults},
    session::{Event, Session},
    target::Target,
    terminal::{self, Terminal},
    trust::HostKey,
    workspace::{self, SavedTab, Snapshot},
};
use std::{path::PathBuf, sync::mpsc::SyncSender, time::Duration};
use zeroize::{Zeroize, Zeroizing};

const BACKGROUND: Color32 = Color32::from_rgb(13, 19, 28);
const PANEL: Color32 = Color32::from_rgb(20, 29, 41);
const ACCENT: Color32 = Color32::from_rgb(83, 210, 185);
const INK: Color32 = Color32::from_rgb(218, 227, 237);
const MUTED: Color32 = Color32::from_rgb(134, 153, 175);
const BRACKET: Color32 = Color32::from_rgb(76, 53, 103);

#[derive(Default)]
pub struct Draft {
    pub target: String,
    pub user: String,
    pub password: Zeroizing<String>,
    pub key_path: String,
    pub name: String,
    pub original_name: String,
    pub tmux: String,
    pub remote_directory: String,
    pub strict: bool,
    pub options: Profile,
    pub saved_profile: bool,
    pub global_keepalive: bool,
}

struct Tab {
    generation: u64,
    capture_input: bool,
    user_input: Vec<UserInput>,
    color: String,
    draft: Draft,
    terminal: Terminal,
    session: Option<Session>,
    connected: bool,
    review_disconnected_output: bool,
    detail: String,
    size: (u16, u16),
    history: usize,
    traffic: (u64, u64),
    trust: Option<(HostKey, SyncSender<bool>)>,
    challenge: Option<Challenge>,
    selection: Option<((u16, u16), (u16, u16))>,
    selecting: bool,
    copied: Option<String>,
    scroll: usize,
    mouse: MouseTracker,
    link_press: Option<(neo_ssh::links::Link, egui::Pos2, bool)>,
    pending_remote_path: Option<String>,
    wheel: f32,
    search_open: bool,
    search_query: String,
    search_case: bool,
    search_focus: bool,
    search_job: Option<SearchJob>,
    search_results: Option<SearchResults>,
    reconnect_at: Option<std::time::Instant>,
    last_error: Option<String>,
    timeline: std::collections::VecDeque<(Duration, String)>,
    opened: std::time::Instant,
    files: Option<crate::file_browser::Browser>,
    keygen: Option<crate::keygen_dialog::Dialog>,
    key_install: Option<crate::key_install_dialog::Dialog>,
    connected_account: String,
    diagnostic_export: Option<neo_ssh::diagnostics::ExportJob>,
    diagnostic_path: Option<String>,
    ime_active: bool,
    ime_preedit: Zeroizing<String>,
    ime_cancelled: bool,
    /// Alt+hover "row last changed" times (ssh4).
    row_activity: neo_ssh::row_activity::RowActivity,
    rows_dirty: bool,
    /// Mouse pointer hidden over the terminal after typing (ssh4).
    pointer_hidden: bool,
    /// Ctrl+Shift+I pressed in the terminal: upload a clipboard image (Ctrl+P in
    /// ssh4 and ssh4gui, moved so Ctrl+P stays previous-history for the remote).
    clipboard_image_request: bool,
    /// The session just connected: give the terminal keyboard focus the next
    /// time it is shown without a dialog over it.
    focus_terminal: bool,
    /// One-line command entry under the terminal (ssh4gui).
    command_line: String,
    /// Remote edits that keep syncing after the Remote files window closed.
    background_edits: Vec<neo_ssh::remote_edit::EditSession>,
    #[cfg(test)]
    sent_input: Vec<u8>,
}
struct Challenge {
    name: String,
    instruction: String,
    prompts: Vec<(String, bool)>,
    answers: Vec<String>,
    reply: SyncSender<Vec<String>>,
}
impl Drop for Challenge {
    fn drop(&mut self) {
        self.answers.zeroize();
    }
}

impl Drop for Tab {
    fn drop(&mut self) {
        self.files.take();
        // Dropping an edit session keeps any copy that may hold unsynced edits.
        self.background_edits.clear();
        if let Some(session) = self.session.take() {
            session.close_after_file_cleanup();
        }
    }
}

impl Tab {
    fn new(draft: Draft) -> Self {
        Self {
            draft,
            generation: 0,
            capture_input: false,
            user_input: Vec::new(),
            color: String::new(),
            files: None,
            keygen: None,
            key_install: None,
            connected_account: String::new(),
            diagnostic_export: None,
            diagnostic_path: None,
            ime_active: false,
            ime_preedit: Zeroizing::new(String::new()),
            ime_cancelled: true,
            row_activity: Default::default(),
            rows_dirty: false,
            pointer_hidden: false,
            clipboard_image_request: false,
            focus_terminal: false,
            command_line: String::new(),
            background_edits: Vec::new(),
            selecting: false,
            #[cfg(test)]
            sent_input: Vec::new(),
            terminal: Terminal::new(24, 80, 10_000),
            history: 10_000,
            session: None,
            connected: false,
            review_disconnected_output: false,
            detail: "Ready to connect".into(),
            size: (80, 24),
            traffic: (0, 0),
            trust: None,
            challenge: None,
            selection: None,
            copied: None,
            scroll: 0,
            mouse: MouseTracker::default(),
            link_press: None,
            pending_remote_path: None,
            wheel: 0.0,
            search_open: false,
            search_query: String::new(),
            search_case: false,
            search_focus: false,
            search_job: None,
            search_results: None,
            reconnect_at: None,
            last_error: None,
            timeline: Default::default(),
            opened: std::time::Instant::now(),
        }
    }
    fn title(&self) -> &str {
        if !self.draft.name.is_empty() {
            &self.draft.name
        } else if !self.draft.target.is_empty() {
            &self.draft.target
        } else {
            "New connection"
        }
    }
    fn display_screen(&self) -> &vt100::Screen {
        self.search_results
            .as_ref()
            .map(|results| &results.screen)
            .unwrap_or_else(|| self.terminal.screen())
    }
    fn scroll_to(&mut self, offset: usize) {
        if let Some(results) = &mut self.search_results {
            results.screen.set_scrollback(offset);
            results.origin.0 = 0;
        } else {
            self.terminal.scrollback(offset);
        }
        self.scroll = self.display_screen().scrollback();
    }
    fn close_search(&mut self) {
        self.search_open = false;
        self.search_job = None;
        self.search_results = None;
        self.selection = None;
        self.copied = None;
        self.scroll_to(0);
    }
    fn start_search(&mut self) {
        self.search_job = Some(SearchJob::start(
            self.terminal.screen().clone(),
            self.search_query.clone(),
            self.search_case,
        ));
        self.search_results = None;
        self.selection = None;
        self.copied = None;
        let bytes = self.mouse.release_all(self.terminal.modes);
        self.send(bytes);
    }
    fn invalidate_search(&mut self) {
        self.search_job = None;
        self.search_results = None;
        self.selection = None;
        self.copied = None;
        self.scroll = self.terminal.screen().scrollback();
    }
    fn send(&mut self, bytes: Vec<u8>) {
        #[cfg(test)]
        self.sent_input.extend_from_slice(&bytes);
        if let Some(session) = &self.session {
            if let Err(error) = session.send(bytes) {
                self.detail = error;
            }
        }
    }
    /// The Remote files window, created on demand while connected; edits that
    /// were syncing in the background move back into it.
    fn open_files(&mut self) -> Option<&mut crate::file_browser::Browser> {
        let session = self.session.as_ref().filter(|_| self.connected)?;
        if self.files.is_none() {
            let mut browser = crate::file_browser::Browser::new(session);
            browser.adopt_edits(std::mem::take(&mut self.background_edits));
            self.files = Some(browser);
        }
        self.files.as_mut()
    }
    fn poll_background_edits(&mut self) {
        if self.background_edits.is_empty() {
            return;
        }
        let now = std::time::Instant::now();
        let session = self.session.as_ref();
        let connected = self.connected && session.is_some();
        let mut submit = |action| match session {
            Some(session) => session.file_action(action),
            None => Err("The session has ended.".to_owned()),
        };
        for edit in &mut self.background_edits {
            edit.poll(now, connected, &mut submit);
        }
    }
    /// Close the Remote files window without ending its edits: they move to
    /// the background list (shown as EDITING in the header).
    fn close_files(&mut self) {
        if let Some(mut browser) = self.files.take() {
            self.background_edits.extend(browser.take_edits());
        }
    }
    fn stop_background_edits(&mut self) {
        let edits = std::mem::take(&mut self.background_edits);
        for edit in edits {
            let message = edit.stop();
            self.record(message);
        }
    }
    fn send_user(&mut self, input: UserInput) {
        self.pointer_hidden = true;
        self.send(input.encode(&self.terminal));
        if self.capture_input {
            self.user_input.push(input);
        }
    }
    fn resolve_connection(&mut self) -> Result<neo_ssh::session::Connect, String> {
        let (mut connect, warnings) = resolve_draft(&self.draft)?;
        for warning in warnings {
            self.record(warning);
        }
        if self.draft.strict {
            neo_ssh::connection::require_known_host_keys(&mut connect);
        }
        connect.use_agent = self.draft.options.use_agent;
        if !self.draft.options.term.is_empty() {
            connect.term = self.draft.options.term.clone();
        }
        if !self.draft.options.preferred_auth.is_empty() {
            connect.auth = neo_ssh::connection::auth_methods(&self.draft.options.preferred_auth)?;
        }
        if !self.draft.options.remote_env.is_empty() {
            connect.environment = neo_ssh::connection::environment(&self.draft.options.remote_env)?;
        }
        if self.draft.options.connect_timeout_secs > 0 {
            connect.timeout = Duration::from_secs(self.draft.options.connect_timeout_secs);
        }
        if self.draft.options.keep_alive_secs > 0 {
            connect.keepalive = Some(Duration::from_secs(self.draft.options.keep_alive_secs));
        }
        connect.password = self.draft.password.clone();
        if !self.draft.remote_directory.is_empty() {
            neo_ssh::remote_directory::validate(&self.draft.remote_directory)?;
            connect.remote_directory = Some(self.draft.remote_directory.clone());
        }
        Ok(connect)
    }
    fn connection_form_visible(&self) -> bool {
        self.session.is_none() && !self.review_disconnected_output
    }
    fn disconnect(&mut self) {
        self.close_files();
        self.stop_background_edits();
        if let Some(session) = self.session.take() {
            session.close_after_file_cleanup();
        }
        self.connected = false;
        self.review_disconnected_output = false;
        self.reconnect_at = None;
        self.trust = None;
        self.challenge = None;
        self.draft.password.zeroize();
        self.record("Disconnected by user".into());
    }
    fn connect(&mut self) -> Result<(), String> {
        self.reconnect_at = None;
        self.record("Resolving connection configuration".into());
        let connect = self.resolve_connection()?;
        self.record("Connection configuration resolved".into());
        self.review_disconnected_output = false;
        self.connected_account = connect.target.to_string();
        self.generation = self.generation.wrapping_add(1);
        self.key_install = None;
        self.trust = None;
        self.challenge = None;
        self.session = Some(Session::start(connect, self.size.0, self.size.1));
        self.connected = false;
        self.record("Connecting…".into());
        Ok(())
    }
    fn record(&mut self, text: String) {
        self.detail = text.clone();
        if self.timeline.len() == 200 {
            self.timeline.pop_front();
        }
        self.timeline.push_back((self.opened.elapsed(), text));
    }
    fn retry_delay(&self) -> u64 {
        self.draft
            .options
            .extensions
            .get("reconnect_delay_secs")
            .and_then(toml::Value::as_integer)
            .unwrap_or(0)
            .clamp(0, 300) as u64
    }
    fn failed(&mut self, error: String, now: std::time::Instant) {
        let established = self.connected;
        self.review_disconnected_output = false;
        self.connected = false;
        self.session = None;
        self.trust = None;
        self.challenge = None;
        self.last_error = Some(error.clone());
        self.record(error);
        let delay = self.retry_delay();
        self.reconnect_at = if established && delay > 0 {
            Some(now + Duration::from_secs(delay))
        } else {
            None
        };
    }
    fn retry_due(&mut self, now: std::time::Instant) -> bool {
        if self.reconnect_at.is_some_and(|deadline| now >= deadline) {
            self.reconnect_at = None;
            return self.session.is_none();
        }
        false
    }
    fn poll(&mut self, deadline: std::time::Instant) {
        if let Some(result) = self.diagnostic_export.as_ref().and_then(|job| job.poll()) {
            self.diagnostic_export = None;
            self.detail = match result {
                Ok(path) => format!("Diagnostic report saved: {}", path.display()),
                Err(error) => format!("Diagnostic export failed: {error}"),
            };
        }
        if let Some(results) = self.search_job.as_ref().and_then(SearchJob::poll) {
            self.scroll = results.screen.scrollback();
            self.search_results = Some(results);
            self.search_job = None;
        }
        for _ in 0..32 {
            if std::time::Instant::now() >= deadline {
                break;
            }
            let event = self
                .session
                .as_ref()
                .and_then(|session| session.try_event().ok());
            match event {
                None => break,
                Some(Event::Phase(text)) => self.record(text),
                Some(Event::Connected) => {
                    // The window belongs to the previous connection. Its edits
                    // resume on this one: every save is still checked against
                    // the server copy, and transfers cut off by the drop have
                    // already failed (retry is manual).
                    self.close_files();
                    self.close_search();
                    self.connected = true;
                    self.focus_terminal = true;
                    self.record("Connected".into());
                    if !self.background_edits.is_empty() {
                        self.record(format!(
                            "{} remote edit(s) keep syncing on this connection.",
                            self.background_edits.len()
                        ));
                    }
                    self.last_error = None;
                    let colors = self.terminal.colors;
                    self.terminal = Terminal::new(self.size.1, self.size.0, self.history);
                    self.terminal.colors = colors;
                    self.row_activity.reset();
                    self.mouse = MouseTracker::default();
                    self.wheel = 0.0;
                    self.scroll = 0;
                    self.selection = None;
                    self.copied = None;
                    self.draft.password.zeroize();
                }
                Some(Event::Output(bytes)) => {
                    let output = self.terminal.feed(&bytes);
                    self.rows_dirty = true;
                    if let Some(path) = &self.terminal.remote_directory {
                        self.draft.remote_directory = path.clone();
                    }
                    self.send(output.replies);
                }
                Some(Event::Heartbeat { received, sent }) => self.traffic = (received, sent),
                Some(Event::ShellExited(status)) => self.record(match status {
                    Some(status) => format!("Remote shell exited with status {status}"),
                    None => "Remote shell closed without an exit status".into(),
                }),
                Some(Event::Trust { key, answer }) => {
                    self.record("Awaiting host-key decision".into());
                    self.trust = Some((*key, answer));
                }
                Some(Event::Challenge {
                    name,
                    instruction,
                    prompts,
                    answer,
                }) => {
                    self.record("Awaiting authentication response".into());
                    let answers = vec![String::new(); prompts.len()];
                    self.challenge = Some(Challenge {
                        name,
                        instruction,
                        prompts,
                        answers,
                        reply: answer,
                    });
                }
                Some(Event::Disconnected(error)) => {
                    self.failed(error, std::time::Instant::now());
                }
            }
        }
        // Stamp changed rows once per poll, only while the live screen is shown.
        if self.rows_dirty && self.scroll == 0 {
            self.rows_dirty = false;
            self.row_activity
                .update_screen(self.terminal.screen(), std::time::Instant::now());
        }
        if self.retry_due(std::time::Instant::now()) {
            if let Err(error) = self.connect() {
                self.failed(error, std::time::Instant::now());
            }
        }
    }
}

pub struct Workspace {
    sync_input: bool,
    tabs: Vec<Tab>,
    active: usize,
    config: Config,
    config_path: PathBuf,
    error: Option<String>,
    host_filter: String,
    tab_filter: String,
    focus: bool,
    palette: bool,
    /// Command palette filter text and highlighted row (ssh4-style palette).
    palette_query: String,
    palette_selected: usize,
    palette_focus: bool,
    /// Keyboard shortcut overview (ssh4 help overlay).
    help: bool,
    tab_filter_focus: bool,
    /// Show the one-line command entry under the terminal (ssh4gui).
    command_line_visible: bool,
    /// Result of the last Copy public key / install command action.
    key_status: String,
    paste: Option<PasteReview>,
    font_size: f32,
    preferences: neo_ssh::preferences::Preferences,
    settings: Option<crate::settings_dialog::Dialog>,
    qa_screenshot: Option<(PathBuf, u8)>,
    imported_aliases: Vec<String>,
    snippets: Option<SnippetEditor>,
    delete_profile: Option<String>,
    restore_prompt: bool,
    workspace_check: std::time::Instant,
    next_poll: usize,
    workspace_save_enabled: bool,
    debug_log: Option<neo_ssh::debug_log::Log>,
    debug_snapshot: Vec<neo_ssh::debug_log::Record>,
    debug_status: String,
}

struct SnippetEditor {
    selected: Option<usize>,
    draft: Snippet,
    execute: bool,
    status: String,
    focus_initial: bool,
}
impl Default for SnippetEditor {
    fn default() -> Self {
        Self {
            selected: None,
            draft: Snippet::default(),
            execute: false,
            status: String::new(),
            focus_initial: true,
        }
    }
}

pub fn run(config: Config, path: PathBuf, mut draft: Draft) -> Result<(), String> {
    draft.global_keepalive = config
        .get("keep_alive")
        .and_then(toml::Value::as_bool)
        .unwrap_or(false);
    let home = Config::path()
        .map_err(|e| e.to_string())?
        .parent()
        .ok_or("Home directory unavailable.")?
        .to_path_buf();
    let imported_aliases =
        neo_ssh::ssh_config::SshConfig::load(&home.join(".ssh/config"))?.aliases();
    let qa_size = std::env::var_os("NEO_SSH_QA_SCREENSHOT")
        .and_then(|_| std::env::var("NEO_SSH_QA_SIZE").ok())
        .and_then(|value| {
            let (width, height) = value.split_once('x')?;
            Some([
                width.parse::<u16>().ok()?.clamp(760, 2400) as f32,
                height.parse::<u16>().ok()?.clamp(480, 1600) as f32,
            ])
        })
        .unwrap_or([1180.0, 780.0]);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(qa_size)
            .with_min_inner_size([760.0, 480.0])
            .with_icon(app_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "NEO-SSH · Workspace",
        options,
        Box::new(move |cc| {
            let mut fonts = egui::FontDefinitions::default();
            for (name, path, family) in [
                (
                    "system-ui",
                    "C:/Windows/Fonts/segoeui.ttf",
                    egui::FontFamily::Proportional,
                ),
                (
                    "terminal",
                    "C:/Windows/Fonts/consola.ttf",
                    egui::FontFamily::Monospace,
                ),
            ] {
                if let Ok(bytes) = std::fs::read(path) {
                    fonts
                        .font_data
                        .insert(name.into(), egui::FontData::from_owned(bytes));
                    fonts
                        .families
                        .entry(family)
                        .or_default()
                        .insert(0, name.into());
                }
            }
            for (name, paths) in [
                (
                    "cjk",
                    &[
                        "C:/Windows/Fonts/msyh.ttc",
                        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
                        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
                        "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
                        "/System/Library/Fonts/PingFang.ttc",
                    ][..],
                ),
                (
                    "symbols",
                    &[
                        "C:/Windows/Fonts/seguiemj.ttf",
                        "/usr/share/fonts/truetype/ancient-scripts/Symbola_hint.ttf",
                        "/usr/share/fonts/opentype/unifont/unifont_upper.otf",
                    ][..],
                ),
            ] {
                for path in paths {
                    if let Ok(bytes) = std::fs::read(path) {
                        fonts
                            .font_data
                            .insert(name.into(), egui::FontData::from_owned(bytes));
                        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace]
                        {
                            fonts.families.entry(family).or_default().push(name.into());
                        }
                        break;
                    }
                }
            }
            cc.egui_ctx.set_fonts(fonts);
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            let mut style = (*cc.egui_ctx.style()).clone();
            style.visuals.panel_fill = PANEL;
            style.visuals.window_fill = PANEL;
            style.visuals.extreme_bg_color = Color32::from_rgb(24, 35, 49);
            style.visuals.widgets.inactive.bg_stroke =
                egui::Stroke::new(1.0, Color32::from_rgb(52, 69, 89));
            style.visuals.widgets.inactive.fg_stroke.color = INK;
            style.visuals.widgets.noninteractive.fg_stroke.color = MUTED;
            style
                .text_styles
                .insert(egui::TextStyle::Body, FontId::proportional(14.0));
            style
                .text_styles
                .insert(egui::TextStyle::Button, FontId::proportional(13.0));
            style.visuals.selection.bg_fill = Color32::from_rgb(35, 79, 91);
            style.visuals.selection.stroke.color = ACCENT;
            style.spacing.item_spacing = Vec2::new(10.0, 10.0);
            style.spacing.button_padding = Vec2::new(12.0, 8.0);
            cc.egui_ctx.set_style(style);
            let preferences = neo_ssh::preferences::Preferences::load(&config);
            let font_size = preferences.font_size;
            cc.egui_ctx.set_zoom_factor(preferences.zoom);
            // egui's own Ctrl+Plus/Minus/0 zoom also fires with Shift (taking
            // Ctrl+_ from the terminal) and is never saved; Ctrl+= / Ctrl+- /
            // Ctrl+0 size the terminal text instead (see APP_SHORTCUTS).
            cc.egui_ctx
                .options_mut(|options| options.zoom_with_keyboard = false);
            let explicit_target = !draft.target.trim().is_empty();
            let saved =
                if !explicit_target && workspace::enabled(&config, "restore_workspace", true) {
                    Snapshot::load(&config)
                } else {
                    Ok(Snapshot::default())
                };
            let mut initial_error = None;
            let saved = saved.unwrap_or_else(|error| {
                initial_error = Some(error);
                Snapshot::default()
            });
            let restoring = !saved.tabs.is_empty();
            let mut tabs: Vec<Tab> = saved
                .tabs
                .into_iter()
                .map(|saved| {
                    let target = if saved.target.is_empty() {
                        format_target(&saved.connection)
                    } else {
                        saved.target
                    };
                    let mut tab = Tab::new(Draft {
                        target,
                        name: saved.title,
                        original_name: saved.profile_name.clone(),
                        saved_profile: !saved.profile_name.is_empty(),
                        user: saved.connection.user.clone(),
                        key_path: saved.connection.key_path.clone(),
                        tmux: saved.connection.tmux_session.clone(),
                        remote_directory: saved.remote_directory,
                        strict: saved.connection.strict_host_key,
                        global_keepalive: draft.global_keepalive,
                        options: saved.connection,
                        ..Default::default()
                    });
                    tab.color = saved.tab_color;
                    tab
                })
                .collect();
            if tabs.is_empty() {
                tabs.push(Tab::new(draft));
            }
            let active = saved.active.min(tabs.len() - 1);
            let auto = restoring && workspace::enabled(&config, "auto_reconnect_on_restore", false);
            let restore_prompt =
                auto && workspace::enabled(&config, "ask_before_reconnect_on_restore", true);
            if auto
                && !restore_prompt
                && (std::env::var_os("NEO_SSH_QA_SCREENSHOT").is_none()
                    || std::env::var_os("NEO_SSH_QA_RESTORE_CONNECT").is_some())
            {
                for tab in &mut tabs {
                    if let Err(error) = tab.connect() {
                        tab.detail = error;
                    }
                }
            }
            if explicit_target
                && std::env::var_os("NEO_SSH_QA_SCREENSHOT").is_some()
                && std::env::var_os("NEO_SSH_QA_CONNECT").is_some()
            {
                if let Err(error) = tabs[active].connect() {
                    initial_error = Some(error);
                }
            }
            Ok(Box::new(Workspace {
                sync_input: false,
                tabs,
                active,
                config,
                config_path: path,
                workspace_save_enabled: initial_error.is_none(),
                error: initial_error,
                host_filter: String::new(),
                tab_filter: String::new(),
                focus: false,
                palette: false,
                palette_query: String::new(),
                palette_selected: 0,
                palette_focus: false,
                help: false,
                tab_filter_focus: false,
                command_line_visible: false,
                key_status: String::new(),
                paste: None,
                font_size,
                settings: (std::env::var_os("NEO_SSH_QA_SCREENSHOT").is_some()
                    && std::env::var_os("NEO_SSH_QA_SETTINGS").is_some())
                .then(|| crate::settings_dialog::Dialog::new(preferences.clone())),
                preferences,
                imported_aliases,
                snippets: None,
                delete_profile: None,
                restore_prompt,
                workspace_check: std::time::Instant::now(),
                next_poll: 0,
                debug_log: None,
                debug_snapshot: Vec::new(),
                debug_status: String::new(),
                qa_screenshot: std::env::var_os("NEO_SSH_QA_SCREENSHOT")
                    .map(|path| (PathBuf::from(path), 0)),
            }))
        }),
    )
    .map_err(|error| error.to_string())
}

impl Drop for Workspace {
    fn drop(&mut self) {
        // Cancel all file handles and retire sessions before waiting once for
        // the whole workspace. Never multiply the timeout by the tab count.
        self.tabs.clear();
        neo_ssh::session::wait_for_file_cleanup();
    }
}

impl eframe::App for Workspace {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if let Some((path, frames)) = &mut self.qa_screenshot {
            if std::env::var_os("NEO_SSH_QA_FRAME_TIMING").is_some()
                && self.tabs[self.active].terminal.received > 0
            {
                if let Some(seconds) = frame.info().cpu_usage {
                    eprintln!("QA_FRAME_SECONDS={seconds}");
                    eprintln!(
                        "QA_TAB_BYTES={:?}",
                        self.tabs
                            .iter()
                            .map(|tab| tab.terminal.received)
                            .collect::<Vec<_>>()
                    );
                }
            }
            let capture = ctx.input(|input| {
                input.events.iter().find_map(|event| {
                    if let egui::Event::Screenshot { image, .. } = event {
                        Some(image.clone())
                    } else {
                        None
                    }
                })
            });
            if let Some(capture) = capture {
                let rgba: Vec<u8> = capture
                    .pixels
                    .iter()
                    .flat_map(|color| color.to_array())
                    .collect();
                if let Err(error) = image::save_buffer(
                    path,
                    &rgba,
                    capture.size[0] as u32,
                    capture.size[1] as u32,
                    image::ColorType::Rgba8,
                ) {
                    eprintln!("QA capture failed: {error}");
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                let minimum_bytes = std::env::var("NEO_SSH_QA_MIN_BYTES")
                    .ok()
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(1);
                let ready = (std::env::var_os("NEO_SSH_QA_CONNECT").is_none()
                    && std::env::var_os("NEO_SSH_QA_RESTORE_CONNECT").is_none())
                    || self
                        .tabs
                        .iter()
                        .all(|tab| tab.connected && tab.terminal.received >= minimum_bytes)
                    || self.tabs.iter().any(|tab| tab.last_error.is_some())
                    || self.error.is_some();
                *frames = if ready { frames.saturating_add(1) } else { 0 };
                if *frames == 5 {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
                }
            }
        }
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            tab.terminal.colors = neo_ssh::preferences::Colors::for_theme(&self.preferences.theme);
            tab.history = self.preferences.history_lines(Some(&tab.draft.options));
            tab.draft.global_keepalive = self.preferences.keep_alive;
            if index != self.active {
                tab.selecting = false;
                tab.ime_cancelled = true;
                tab.ime_active = false;
                tab.ime_preedit.zeroize();
                let bytes = tab.mouse.release_all(tab.terminal.modes);
                tab.send(bytes);
            }
        }
        // Bound total SSH parsing work independently of the number of tabs.
        // Resume with the next tab so sustained output cannot starve peers.
        let received_before: u64 = self.tabs.iter().map(|tab| tab.terminal.received).sum();
        for tab in &self.tabs {
            if let Some(session) = &tab.session {
                let repaint = ctx.clone();
                session.set_wakeup(std::sync::Arc::new(move || repaint.request_repaint()));
            }
        }
        let deadline = std::time::Instant::now() + Duration::from_millis(8);
        let connected = self
            .tabs
            .iter()
            .filter(|tab| tab.session.is_some())
            .count()
            .max(1);
        let slice = Duration::from_micros((8000 / connected as u64).max(2000));
        for _ in 0..self.tabs.len() {
            let now = std::time::Instant::now();
            if now >= deadline {
                break;
            }
            let index = self.next_poll % self.tabs.len();
            self.next_poll = (index + 1) % self.tabs.len();
            self.tabs[index].poll(deadline.min(now + slice));
        }
        // Remote edits keep syncing while their tab is in the background or
        // their window is closed; the active tab's window polls its own edits.
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            tab.poll_background_edits();
            if index != self.active {
                if let Some(browser) = tab.files.as_mut() {
                    browser.poll_edits(tab.session.as_ref(), tab.connected);
                }
            }
        }
        let received_after: u64 = self.tabs.iter().map(|tab| tab.terminal.received).sum();
        // SSH arrivals wake the UI directly. Idle repainting only needs to
        // cover the two-second workspace autosave interval; jobs and countdowns
        // retain their existing one-second polling interval.
        let background_pending = self.tabs.iter().any(|tab| {
            tab.reconnect_at.is_some()
                || tab.search_job.is_some()
                || tab.files.is_some()
                || !tab.background_edits.is_empty()
                || tab.keygen.is_some()
                || tab.key_install.is_some()
                || tab.diagnostic_export.is_some()
        }) || self
            .debug_log
            .as_ref()
            .is_some_and(|log| !log.enabled() || !log.ready());
        ctx.request_repaint_after(if received_after != received_before {
            Duration::from_millis(33)
        } else if background_pending {
            Duration::from_secs(1)
        } else {
            Duration::from_secs(2)
        });
        if let Some(result) = self.debug_log.as_ref().and_then(|log| log.poll()) {
            self.debug_log = None;
            self.debug_status = match result {
                Ok(()) => "Debug log stopped and saved.".into(),
                Err(error) => {
                    self.error = Some(format!("Debug logging failed: {error}"));
                    "Debug logging failed.".into()
                }
            };
        }
        if let Some(log) = &self.debug_log {
            if log.enabled() {
                let snapshot: Vec<_> = self
                    .tabs
                    .iter()
                    .enumerate()
                    .map(|(index, tab)| {
                        let (rows, columns) = tab.terminal.screen().size();
                        neo_ssh::debug_log::Record {
                            tab: index,
                            connected: tab.connected,
                            connecting: tab.session.is_some() && !tab.connected,
                            trust_prompt: tab.trust.is_some(),
                            auth_prompt: tab.challenge.is_some(),
                            failed: tab.last_error.is_some(),
                            received: tab.traffic.0,
                            sent: tab.traffic.1,
                            rows,
                            columns,
                        }
                    })
                    .collect();
                for (index, record) in snapshot.iter().enumerate() {
                    if self.debug_snapshot.get(index) != Some(record) {
                        log.record(record.clone());
                    }
                }
                self.debug_snapshot = snapshot;
            }
        }
        let dropped = ctx.input_mut(|input| std::mem::take(&mut input.raw.dropped_files));
        if !dropped.is_empty() {
            let tab = &mut self.tabs[self.active];
            let paths: Option<Vec<PathBuf>> = dropped.into_iter().map(|file| file.path).collect();
            if !tab.connected {
                self.error = Some("Connect to a server before dropping files to upload.".into());
            } else if let (Some(paths), Some(browser)) = (paths, tab.open_files()) {
                if let Err(error) = browser.queue_uploads(paths) {
                    self.error = Some(error);
                }
            } else {
                self.error = Some("This drop does not contain local file paths. Save the files locally, then drop them here.".into());
            }
        }
        if self.settings.is_none() && self.delete_profile.is_none() {
            for shortcut in take_app_shortcuts(ctx) {
                match shortcut {
                    AppShortcut::NewTab => self.new_tab(Draft::default()),
                    AppShortcut::Palette if self.palette => self.palette = false,
                    AppShortcut::Palette => self.open_palette(),
                    AppShortcut::Find => {
                        let tab = &mut self.tabs[self.active];
                        tab.search_open = true;
                        tab.search_focus = true;
                    }
                    AppShortcut::FocusMode => self.focus = !self.focus,
                    AppShortcut::LargerText => self.run_command(Command::IncreaseText),
                    AppShortcut::SmallerText => self.run_command(Command::DecreaseText),
                    AppShortcut::ResetText => self.font_size = self.preferences.font_size,
                }
            }
            dismiss_palette(ctx, &mut self.palette);
        }
        if !self.focus {
            egui::TopBottomPanel::top("header")
                .frame(egui::Frame::none().fill(BACKGROUND).inner_margin(16.0))
                .show(ctx, |ui| {
                    if self.settings.is_some() || self.delete_profile.is_some() {
                        ui.disable();
                    }
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("NEO-SSH").size(25.0).strong().color(ACCENT));
                        ui.label(RichText::new("/   WORKSPACE").size(11.0).color(MUTED));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Commands  Ctrl+Shift+P").clicked() {
                                self.open_palette();
                            }
                            if ui.button("Shortcuts").clicked() {
                                self.help = true;
                            }
                            if ui.button("+ New connection").clicked() {
                                self.new_tab(Draft::default());
                            }
                        });
                    });
                });
            egui::SidePanel::left("hosts")
                .exact_width(235.0)
                .frame(egui::Frame::none().fill(PANEL).inner_margin(18.0))
                .show(ctx, |ui| {
                    if self.settings.is_some() || self.delete_profile.is_some() {
                        ui.disable();
                    }
                    egui::ScrollArea::vertical()
                        .id_source("host_sidebar_scroll")
                        .auto_shrink([false, false])
                        .max_height((ui.available_height() - 60.0).max(0.0))
                        .show(ui, |ui| {
                            ui.add_space(6.0);
                            if ui.button("Command snippets").clicked() {
                                self.snippets = Some(SnippetEditor::default());
                            }
                            if ui.button("Preferences").clicked() {
                                let mut preferences = self.preferences.clone();
                                preferences.font_size = self.font_size;
                                self.settings =
                                    Some(crate::settings_dialog::Dialog::new(preferences));
                            }
                            ui.collapsing("Workspace settings", |ui| {
                                let mut updated = self.config.clone();
                                let mut changed = false;
                                for (key, label, default) in [
                                    ("restore_workspace", "Restore previous tabs", true),
                                    (
                                        "auto_reconnect_on_restore",
                                        "Reconnect restored tabs",
                                        false,
                                    ),
                                    (
                                        "ask_before_reconnect_on_restore",
                                        "Ask before reconnecting",
                                        true,
                                    ),
                                ] {
                                    let mut value = workspace::enabled(&updated, key, default);
                                    if ui.checkbox(&mut value, label).changed() {
                                        updated.set(key, toml::Value::Boolean(value));
                                        changed = true;
                                    }
                                }
                                if changed {
                                    match updated.save(&self.config_path) {
                                        Ok(()) => self.config = updated,
                                        Err(error) => self.error = Some(error),
                                    }
                                }
                                if ui.button("Save workspace now").clicked() {
                                    match self.save_workspace() {
                                        Ok(()) => self.workspace_save_enabled = true,
                                        Err(error) => self.error = Some(error),
                                    }
                                }
                            });
                            ui.collapsing("Diagnostics", |ui| {
                                let mut enabled =
                                    self.debug_log.as_ref().is_some_and(|log| log.enabled());
                                let closing =
                                    self.debug_log.as_ref().is_some_and(|log| !log.enabled());
                                if ui
                                    .add_enabled(
                                        !closing,
                                        egui::Checkbox::new(&mut enabled, "Debug logging"),
                                    )
                                    .changed()
                                {
                                    if enabled {
                                        match neo_ssh::debug_log::Log::start(
                                            self.config_path.with_extension("debug.log"),
                                        ) {
                                            Ok(log) => {
                                                self.debug_log = Some(log);
                                                self.debug_snapshot.clear();
                                                self.debug_status.clear();
                                            }
                                            Err(error) => self.error = Some(error),
                                        }
                                    } else if let Some(log) = &mut self.debug_log {
                                        log.stop();
                                    }
                                }
                                ui.label(
                            "Connection state and counters only. No terminal text or credentials.",
                        );
                                if let Some(log) = &self.debug_log {
                                    ui.label(if !log.enabled() {
                                        "Finishing log…"
                                    } else if log.ready() {
                                        "Logging enabled"
                                    } else {
                                        "Opening log…"
                                    });
                                    ui.label(log.path.display().to_string());
                                } else if !self.debug_status.is_empty() {
                                    ui.label(&self.debug_status);
                                }
                            });
                            ui.label(RichText::new("YOUR HOSTS").size(11.0).color(MUTED));
                            ui.add(
                                egui::TextEdit::singleline(&mut self.host_filter)
                                    .hint_text("Find a connection…")
                                    .desired_width(f32::INFINITY),
                            );
                            ui.add_space(10.0);
                            let profiles = self.config.profiles().unwrap_or_default();
                            if profiles.is_empty() {
                                ui.label(
                                    RichText::new("Save a connection to keep it here.")
                                        .color(MUTED),
                                );
                            }
                            for (name, profile) in profiles {
                                if !format!("{name} {} {}", profile.user, profile.host)
                                    .to_lowercase()
                                    .contains(&self.host_filter.to_lowercase())
                                {
                                    continue;
                                }
                                let response = ui.add_sized(
                                    [195.0, 50.0],
                                    egui::Button::new(format!(
                                        "{name}\n{}@{}",
                                        profile.user, profile.host
                                    )),
                                );
                                response.context_menu(|ui| {
                                    if ui.button("Delete saved connection?").clicked() {
                                        self.delete_profile = Some(name.clone());
                                        ui.close_menu();
                                    }
                                });
                                if response.clicked() {
                                    self.new_tab(Draft {
                                        options: profile.clone(),
                                        saved_profile: true,
                                        original_name: name.clone(),
                                        target: format_target(&profile),
                                        user: profile.user,
                                        key_path: profile.key_path,
                                        name,
                                        tmux: profile.tmux_session,
                                        strict: profile.strict_host_key,
                                        ..Default::default()
                                    });
                                }
                            }
                            if !self.imported_aliases.is_empty() {
                                ui.separator();
                                ui.label(
                                    RichText::new("FROM ~/.SSH/CONFIG").size(11.0).color(MUTED),
                                );
                                for alias in self.imported_aliases.clone() {
                                    if alias
                                        .to_lowercase()
                                        .contains(&self.host_filter.to_lowercase())
                                        && ui.button(&alias).clicked()
                                    {
                                        self.new_tab(Draft {
                                            target: alias,
                                            ..Default::default()
                                        });
                                    }
                                }
                            }
                        });
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(
                            "Ctrl+Shift+T  New connection\nCtrl+Shift+P  Commands\nF11               Focus mode",
                        )
                        .size(11.0)
                        .color(MUTED),
                    );
                });
        }
        egui::TopBottomPanel::bottom("status")
            .frame(egui::Frame::none().fill(PANEL).inner_margin(8.0))
            .show(ctx, |ui| {
                if self.settings.is_some() || self.delete_profile.is_some() { ui.disable(); }
                let tab = &self.tabs[self.active];
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.sync_input, "Sync input").on_hover_text("Send terminal keys and pastes to every connected tab. Authentication and mouse events stay on their own connection.");
                    ui.label(
                        RichText::new(if tab.connected { "●" } else { "○" })
                            .color(if tab.connected { ACCENT } else { MUTED }),
                    );
                    ui.label(RichText::new(&tab.detail).size(12.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!(
                                "Rx {} B   Tx {} B   {} x {}",
                                tab.traffic.0, tab.traffic.1, tab.size.0, tab.size.1
                            ))
                            .size(11.0)
                            .color(MUTED),
                        );
                    });
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BACKGROUND).inner_margin(14.0))
            .show(ctx, |ui| {
                if self.settings.is_some() || self.delete_profile.is_some() {
                    ui.disable();
                }
                let mut close = None;
                let mut set_color = None;
                if self.sync_input {
                    let names = self
                        .tabs
                        .iter()
                        .filter(|tab| accepts_sync(tab))
                        .map(|tab| {
                            if tab.connected_account.is_empty() {
                                tab.draft.target.as_str()
                            } else {
                                tab.connected_account.as_str()
                            }
                        })
                        .collect::<Vec<_>>();
                    ui.colored_label(
                        Color32::from_rgb(244, 196, 114),
                        format!("Sync input: {} connected destinations", names.len()),
                    );
                    ui.label(names.join(", "));
                }
                ui.horizontal(|ui| {
                    let filter = ui.add(
                        egui::TextEdit::singleline(&mut self.tab_filter)
                            .hint_text("Find open tabs")
                            .desired_width(180.0),
                    );
                    if std::mem::take(&mut self.tab_filter_focus) {
                        filter.request_focus();
                    }
                    // Enter switches to the first matching tab (ssh4 tab search).
                    if filter.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                        && !self.tab_filter.is_empty()
                    {
                        if let Some(index) = self
                            .tabs
                            .iter()
                            .position(|tab| tab_matches(tab, &self.tab_filter))
                        {
                            self.active = index;
                        }
                    }
                    if !self.tab_filter.is_empty() && ui.small_button("Clear").clicked() {
                        self.tab_filter.clear();
                    }
                    ui.label(format!("{} open", self.tabs.len()));
                });
                ui.horizontal_wrapped(|ui| {
                    for (index, tab) in self.tabs.iter().enumerate() {
                        if !tab_matches(tab, &self.tab_filter) {
                            continue;
                        }
                        let response = ui.selectable_label(
                            self.active == index,
                            RichText::new(tab.title()).color(tab_color(&tab.color)),
                        );
                        if response.clicked() {
                            self.active = index;
                        }
                        if response.middle_clicked() {
                            close = Some(CloseTabs::One(index));
                        }
                        response.context_menu(|ui| {
                            ui.menu_button("Tab color", |ui| {
                                for (value, label) in [
                                    ("", "Default"),
                                    ("mint", "Mint"),
                                    ("blue", "Blue"),
                                    ("amber", "Amber"),
                                    ("pink", "Pink"),
                                ] {
                                    if ui
                                        .selectable_label(
                                            tab.color == value,
                                            RichText::new(label).color(tab_color(value)),
                                        )
                                        .clicked()
                                    {
                                        set_color = Some((index, value.to_owned()));
                                        ui.close_menu();
                                    }
                                }
                            });
                            if ui.button("Close tab").clicked() {
                                close = Some(CloseTabs::One(index));
                                ui.close_menu();
                            }
                            if ui
                                .add_enabled(
                                    self.tabs.len() > 1,
                                    egui::Button::new("Close other tabs"),
                                )
                                .clicked()
                            {
                                close = Some(CloseTabs::Others(index));
                                ui.close_menu();
                            }
                            if ui
                                .add_enabled(
                                    index + 1 < self.tabs.len(),
                                    egui::Button::new("Close tabs to the right"),
                                )
                                .clicked()
                            {
                                close = Some(CloseTabs::Right(index));
                                ui.close_menu();
                            }
                        });
                        if ui
                            .small_button("×")
                            .on_hover_text("Close connection")
                            .clicked()
                        {
                            close = Some(CloseTabs::One(index));
                        }
                    }
                });
                if let Some((index, color)) = set_color {
                    self.tabs[index].color = color;
                }
                if let Some(action) = close {
                    close_tabs(&mut self.tabs, &mut self.active, action);
                    self.paste = None;
                }
                if !self.tab_filter.is_empty()
                    && !self
                        .tabs
                        .iter()
                        .any(|tab| tab_matches(tab, &self.tab_filter))
                {
                    ui.label("No matching tabs. Clear the search to see all connections.");
                }
                ui.separator();
                let modal = self.error.is_some()
                    || self.settings.is_some()
                    || self.delete_profile.is_some()
                    || self.tabs[self.active].keygen.is_some()
                    || self.tabs[self.active].key_install.is_some()
                    || self.tabs[self.active].files.is_some()
                    || self.tabs[self.active].diagnostic_path.is_some()
                    || self.restore_prompt
                    || self.snippets.is_some()
                    || self.palette
                    || self.help
                    || self.paste.is_some()
                    || self.tabs[self.active].trust.is_some()
                    || self.tabs[self.active].challenge.is_some();
                {
                    let tab = &mut self.tabs[self.active];
                    if tab.connected && ui.button("Remote files").clicked() {
                        // Reuse an open window so active remote edits survive.
                        tab.open_files();
                    }
                    if let Some(error) = &tab.last_error {
                        ui.label(
                            RichText::new(format!("Last connection error: {error}"))
                                .color(Color32::from_rgb(245, 176, 140)),
                        );
                    }
                    if let Some(deadline) = tab.reconnect_at {
                        ui.horizontal(|ui| {
                            let remaining = deadline
                                .saturating_duration_since(std::time::Instant::now())
                                .as_secs_f32()
                                .ceil() as u64;
                            ui.label(format!("Reconnecting in {remaining}s"));
                            if ui.button("Cancel retry").clicked() {
                                tab.reconnect_at = None;
                                tab.record("Automatic retry cancelled".into());
                            }
                        });
                    }
                    if !tab.timeline.is_empty() {
                        ui.collapsing("Connection history", |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(160.0)
                                .stick_to_bottom(true)
                                .show(ui, |ui| {
                                    for (elapsed, message) in &tab.timeline {
                                        ui.label(format!(
                                            "{:.1}s  {message}",
                                            elapsed.as_secs_f32()
                                        ));
                                    }
                                });
                        });
                    }
                }
                if self.tabs[self.active].connection_form_visible() {
                    if self.tabs[self.active].terminal.received > 0
                        && ui.button("Review previous output").clicked()
                    {
                        self.tabs[self.active].review_disconnected_output = true;
                    }
                    let form_height = (ui.available_height() - 62.0).max(0.0);
                    egui::ScrollArea::vertical()
                        .max_height(form_height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| self.connection_form(ui));
                    ui.separator();
                    self.connection_actions(ui);
                } else {
                    ui.horizontal(|ui| {
                        let tab = &mut self.tabs[self.active];
                        ui.label(RichText::new(tab.title()).strong().color(INK));
                        if tab.session.is_some() {
                            if ui.button("Disconnect").clicked() {
                                tab.disconnect();
                            }
                        } else if ui
                            .button(if tab.draft.tmux.trim().is_empty() {
                                "Reconnect"
                            } else {
                                "Resume tmux"
                            })
                            .clicked()
                        {
                            if let Err(error) = tab.connect() {
                                self.error = Some(error);
                            }
                        }
                        if tab.session.is_none() && ui.button("Edit connection").clicked() {
                            tab.review_disconnected_output = false;
                        }
                        if ui
                            .button("Find")
                            .on_hover_text("Search output (Ctrl+Shift+F)")
                            .clicked()
                        {
                            tab.search_open = true;
                            tab.search_focus = true;
                        }
                        if (tab.scroll > 0 || tab.search_open)
                            && ui.button("Back to live output").clicked()
                        {
                            tab.close_search();
                        }
                        let tmux = tab.draft.tmux.trim();
                        if !tmux.is_empty() {
                            ui.label(RichText::new(format!("tmux:{tmux}")).color(MUTED))
                                .on_hover_text("Configured tmux session. Startup errors appear in the terminal output.");
                        }
                        // Status chips (ssh4gui): terminal modes, scroll offset and forwards.
                        for (chip, hint) in status_chips(tab) {
                            ui.label(RichText::new(chip).small().color(MUTED))
                                .on_hover_text(hint);
                        }
                        ui.toggle_value(&mut self.command_line_visible, "Command line")
                            .on_hover_text("Show a one-line command entry under the terminal");
                    });
                    search_bar(ui, &mut self.tabs[self.active], modal);
                    self.tabs[self.active].capture_input = self.sync_input;
                    let pending = if self.command_line_visible {
                        let size = Vec2::new(
                            ui.available_width(),
                            (ui.available_height() - 36.0).max(1.0),
                        );
                        let pending = ui
                            .allocate_ui(size, |ui| {
                                terminal_view(ui, &mut self.tabs[self.active], self.font_size, modal)
                            })
                            .inner;
                        let typed = command_line_bar(ui, &mut self.tabs[self.active], modal);
                        pending.or(typed)
                    } else {
                        terminal_view(ui, &mut self.tabs[self.active], self.font_size, modal)
                    };
                    self.tabs[self.active].capture_input = false;
                    let events = std::mem::take(&mut self.tabs[self.active].user_input);
                    for event in events {
                        broadcast_input(&mut self.tabs, self.active, &event);
                    }
                    if let Some(text) = pending {
                        self.paste = Some(PasteReview {
                            recipients: input_recipients(&self.tabs, self.active, self.sync_input),
                            text,
                        });
                    }
                }
            });
        if std::mem::take(&mut self.tabs[self.active].clipboard_image_request) {
            self.upload_clipboard_image();
        }
        let tab = &mut self.tabs[self.active];
        if let Some(path) = tab.pending_remote_path.take() {
            if let Some(session) = tab.session.as_ref().filter(|_| tab.connected) {
                match crate::file_browser::Browser::locate(session, &path) {
                    Ok(mut browser) => {
                        if let Some(previous) = tab.files.as_mut() {
                            browser.adopt_edits(previous.take_edits());
                        }
                        browser.adopt_edits(std::mem::take(&mut tab.background_edits));
                        tab.files = Some(browser);
                    }
                    Err(error) => tab.detail = error,
                }
            }
        }
        self.delete_profile_dialog(ctx);
        self.dialogs(ctx);
        if let Some(dialog) = &mut self.settings {
            let (open, save) = dialog.show(ctx);
            if save {
                let preferences = dialog.preferences.clone();
                let mut config = self.config.clone();
                preferences.apply(&mut config);
                match config.save(&self.config_path) {
                    Ok(()) => {
                        self.config = config;
                        self.font_size = preferences.font_size;
                        ctx.set_zoom_factor(preferences.zoom);
                        self.preferences = preferences;
                        self.settings = None;
                    }
                    Err(error) => self.error = Some(error),
                }
            } else if !open {
                self.settings = None;
            }
        }
        self.snippet_dialog(ctx);
        let tab = &mut self.tabs[self.active];
        let title = tab.title().to_owned();
        if let Some(dialog) = &mut tab.key_install {
            if !dialog.show(
                ctx,
                tab.session.as_ref(),
                tab.connected,
                &tab.connected_account,
            ) {
                tab.key_install = None;
            }
        }
        if let Some(dialog) = &mut tab.keygen {
            let (open, key) = dialog.show(ctx);
            if let Some(path) = key {
                tab.draft.key_path = path.to_string_lossy().into_owned();
            }
            if !open {
                tab.keygen = None;
            }
        }
        if let Some(browser) = &mut tab.files {
            let open = browser.show(ctx, tab.session.as_ref(), tab.connected, &title);
            let insert = browser.take_terminal_insert();
            if !open {
                let edits = browser.take_edits();
                tab.background_edits.extend(edits);
                tab.files = None;
            }
            if let Some(text) = insert {
                tab.send(tab.terminal.paste(&text));
            }
        }
        if let Some(mut path) = tab.diagnostic_path.take() {
            let mut open = true;
            let mut save = false;
            let mut cancel = false;
            egui::Window::new("Save diagnostic report")
                .id(egui::Id::new("diagnostic-export"))
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label("Includes visible terminal text and recent remote output.");
                    ui.label("Remote output may contain echoed secrets. Review before sharing.");
                    ui.label("Local input buffers and connection credentials are excluded.");
                    ui.label("Save to a new file (existing files are never replaced):");
                    ui.add(egui::TextEdit::singleline(&mut path).desired_width(500.0));
                    if ui.button("Choose destination...").clicked() {
                        if let Some(selected) = rfd::FileDialog::new()
                            .set_title("Save diagnostic report")
                            .set_file_name("neo-ssh-diagnostic.txt")
                            .save_file()
                        {
                            path = selected.to_string_lossy().into_owned();
                        }
                    }
                    ui.horizontal(|ui| {
                        save = ui
                            .add_enabled(
                                std::path::Path::new(&path).is_absolute()
                                    && tab.diagnostic_export.is_none(),
                                egui::Button::new("Save report"),
                            )
                            .clicked();
                        cancel = ui.button("Cancel").clicked();
                    });
                });
            if save {
                let report = neo_ssh::diagnostics::terminal_report(
                    &tab.terminal,
                    tab.connected,
                    tab.traffic.1,
                    tab.traffic.0,
                );
                match neo_ssh::diagnostics::ExportJob::start(path.into(), report) {
                    Ok(job) => {
                        tab.diagnostic_export = Some(job);
                        tab.detail = "Saving diagnostic report...".into();
                    }
                    Err(error) => tab.detail = format!("Diagnostic export failed: {error}"),
                }
            } else if open && !cancel && !ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
                tab.diagnostic_path = Some(path);
            }
        }
        if self.restore_prompt {
            egui::Window::new("Reconnect restored tabs?")
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("Your previous tabs are open. Reconnect them now?");
                    for tab in &self.tabs {
                        ui.label(tab.title());
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Reconnect tabs").clicked() {
                            self.restore_prompt = false;
                            for tab in &mut self.tabs {
                                if let Err(error) = tab.connect() {
                                    tab.detail = error;
                                }
                            }
                        }
                        if ui.button("Keep disconnected").clicked() {
                            self.restore_prompt = false;
                        }
                    });
                });
        }
        if self.qa_screenshot.is_none() && self.workspace_save_enabled {
            let closing = ctx.input(|input| input.viewport().close_requested());
            if closing || self.workspace_check.elapsed() >= Duration::from_secs(2) {
                self.workspace_check = std::time::Instant::now();
                if let Err(error) = self.save_workspace() {
                    self.workspace_save_enabled = false;
                    self.error = Some(format!("Workspace could not be saved: {error}. Autosave is paused. Use Save workspace now to retry, or close again to exit without saving."));
                    if closing {
                        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                    }
                }
            }
        }
    }
}

impl Workspace {
    fn snapshot(&self) -> Snapshot {
        snapshot_tabs(&self.tabs, self.active)
    }
    fn save_workspace(&mut self) -> Result<(), String> {
        let mut updated = self.config.clone();
        self.snapshot().store(&mut updated)?;
        if updated.get("last_workspace") != self.config.get("last_workspace") {
            updated.save(&self.config_path)?;
            self.config = updated;
        }
        Ok(())
    }
    fn save_snippets(&mut self, snippets: &[Snippet]) -> Result<(), String> {
        let mut updated = self.config.clone();
        updated.set_snippets(snippets)?;
        updated.save(&self.config_path)?;
        self.config = updated;
        Ok(())
    }

    fn open_palette(&mut self) {
        self.palette = true;
        self.palette_query.clear();
        self.palette_selected = 0;
        self.palette_focus = true;
    }

    /// Persist only the theme key so unsaved zoom/text changes stay unsaved.
    fn set_theme(&mut self, theme: &str) {
        let mut config = self.config.clone();
        config.set("theme", theme.into());
        match config.save(&self.config_path) {
            Ok(()) => {
                self.config = config;
                self.preferences.theme = theme.into();
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn run_command(&mut self, command: Command) {
        match command {
            Command::NewConnection => self.new_tab(Draft::default()),
            Command::CloseTab => {
                let active = self.active;
                close_tabs(&mut self.tabs, &mut self.active, CloseTabs::One(active));
                self.paste = None;
            }
            Command::ToggleFocus => self.focus = !self.focus,
            Command::ToggleSync => self.sync_input = !self.sync_input,
            Command::FindInTerminal => {
                let tab = &mut self.tabs[self.active];
                tab.search_open = true;
                tab.search_focus = true;
            }
            Command::FindTabs => self.tab_filter_focus = true,
            Command::RemoteFiles => self.open_remote_files(),
            Command::UploadClipboardImage => self.upload_clipboard_image(),
            Command::Snippets => self.snippets = Some(SnippetEditor::default()),
            Command::RunSnippet(index, name, command) => {
                let mut editor = SnippetEditor {
                    selected: Some(index),
                    ..Default::default()
                };
                editor.draft.name = name;
                editor.draft.command = command;
                self.snippets = Some(editor);
            }
            Command::Preferences => {
                let mut preferences = self.preferences.clone();
                preferences.font_size = self.font_size;
                self.settings = Some(crate::settings_dialog::Dialog::new(preferences));
            }
            Command::Theme(theme) => self.set_theme(theme),
            Command::IncreaseText => self.font_size = (self.font_size + 1.0).min(28.0),
            Command::DecreaseText => self.font_size = (self.font_size - 1.0).max(10.0),
            Command::SaveDiagnostic => {
                let tab = &mut self.tabs[self.active];
                if tab.diagnostic_export.is_some() {
                    self.error = Some("This tab's diagnostic report is still being saved.".into());
                } else {
                    let folder = self
                        .config_path
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new("."));
                    let path = folder.join("neo-ssh-diagnostic.txt");
                    let path = if path.is_absolute() {
                        path
                    } else {
                        std::env::current_dir().unwrap_or_default().join(path)
                    };
                    tab.diagnostic_path = Some(path.to_string_lossy().into_owned());
                }
            }
            Command::InstallPublicKey => {
                if self.tabs[self.active].connected {
                    self.tabs[self.active].key_install =
                        Some(crate::key_install_dialog::Dialog::default());
                } else {
                    self.error =
                        Some("Connect to the account before installing its public key.".into());
                }
            }
            Command::Shortcuts => self.help = true,
        }
    }

    fn open_remote_files(&mut self) {
        if self.tabs[self.active].open_files().is_none() {
            self.error = Some("Connect to a server before opening remote files.".into());
        }
    }

    /// Ctrl+Shift+I (Ctrl+P in ssh4/ssh4gui): capture the clipboard image and review its upload
    /// in Remote files (NEO-SSH keeps the review-before-upload step).
    fn upload_clipboard_image(&mut self) {
        self.open_remote_files();
        if let Some(browser) = self.tabs[self.active].files.as_mut() {
            if let Err(error) = browser.capture_clipboard_image() {
                self.error = Some(error);
            }
        }
    }

    fn snippet_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut editor) = self.snippets.take() else {
            return;
        };
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            return;
        }
        let save_shortcut =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::S));
        let mut open = true;
        egui::Window::new("Command snippets").open(&mut open).default_width(620.0).show(ctx, |ui| {
            let mut snippets = match self.config.snippets() {
                Ok(snippets) => snippets,
                Err(error) => { ui.label(error); return; }
            };
            ui.horizontal_wrapped(|ui| {
                if ui.button("New snippet").clicked() { editor = SnippetEditor::default(); }
                for (index, snippet) in snippets.iter().enumerate() {
                    if ui.selectable_label(editor.selected == Some(index), &snippet.name).clicked() {
                        editor.selected = Some(index);
                        editor.draft = snippet.clone();
                        editor.execute = false;
                        editor.status.clear();
                        editor.focus_initial = true;
                    }
                }
            });
            ui.separator();
            ui.label("Name");
            let name = ui.add(egui::TextEdit::singleline(&mut editor.draft.name).hint_text("Service logs").desired_width(f32::INFINITY));
            if editor.focus_initial {
                name.request_focus();
                editor.focus_initial = false;
            }
            ui.label("Command");
            ui.add(egui::TextEdit::multiline(&mut editor.draft.command).code_editor().lock_focus(false).desired_rows(7).desired_width(f32::INFINITY));
            ui.horizontal(|ui| {
                if ui.button("Save snippet").on_hover_text("Ctrl/Cmd+S").clicked() || save_shortcut {
                    if let Some(index) = editor.selected { snippets[index] = editor.draft.clone(); }
                    else { snippets.push(editor.draft.clone()); }
                    match self.save_snippets(&snippets) {
                        Ok(()) => { editor.selected = Some(editor.selected.unwrap_or(snippets.len() - 1)); editor.status = "Saved".into(); }
                        Err(error) => editor.status = error,
                    }
                }
                if ui.add_enabled(editor.selected.is_some(), egui::Button::new("Delete snippet")).clicked() {
                    if let Some(index) = editor.selected {
                        snippets.remove(index);
                        match self.save_snippets(&snippets) {
                            Ok(()) => editor = SnippetEditor { status: "Deleted".into(), ..Default::default() },
                            Err(error) => editor.status = error,
                        }
                    }
                }
            });
            ui.separator();
            let tab = &mut self.tabs[self.active];
            ui.label(format!("Destination: {}", tab.title()));
            ui.checkbox(&mut editor.execute, "Press Enter after sending");
            ui.label("Review the command and destination before sending. Embedded newlines may execute commands immediately.");
            if ui.add_enabled(tab.connected && !tab.search_open && !editor.draft.command.is_empty(), egui::Button::new("Send reviewed command")).clicked() {
                let mut bytes = tab.terminal.paste(&editor.draft.command);
                if editor.execute { bytes.push(b'\r'); }
                if let Some(session) = &tab.session {
                    editor.status = match session.send(bytes) {
                        Ok(()) => "Sent to the terminal".into(),
                        Err(error) => error,
                    };
                }
            }
            if !editor.status.is_empty() { ui.label(&editor.status); }
        });
        if open {
            self.snippets = Some(editor);
        }
    }

    fn new_tab(&mut self, mut draft: Draft) {
        draft.global_keepalive = self
            .config
            .get("keep_alive")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false);
        self.tabs.push(Tab::new(draft));
        self.active = self.tabs.len() - 1;
    }
    fn connection_form(&mut self, ui: &mut egui::Ui) {
        let mut generate_key = false;
        let mut key_message: Option<String> = None;
        let key_status = self.key_status.clone();
        let returning = self.tabs[self.active].terminal.received > 0;
        ui.add_space(if returning {
            4.0
        } else if ui.ctx().screen_rect().height() < 600.0 {
            12.0
        } else {
            40.0
        });
        ui.horizontal(|ui| {
            ui.add_space((ui.available_width() - 470.0).max(0.0) / 2.0);
            ui.vertical(|ui| {
                ui.set_max_width(470.0);
                if !returning {
                ui.label(
                    RichText::new("A workspace for your servers.")
                        .size(28.0)
                        .strong()
                        .color(INK),
                );
                ui.label(RichText::new("Open a secure connection and get to work.").color(MUTED));
                ui.add_space(20.0);
                }
                let draft = &mut self.tabs[self.active].draft;
                field(
                    ui,
                    "Host",
                    &mut draft.target,
                    "user@server.example.com:22",
                    false,
                );
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(220.0);
                        field(
                            ui,
                            "Username override",
                            &mut draft.user,
                            "Local username by default",
                            false,
                        );
                    });
                    ui.vertical(|ui| {
                        ui.set_width(220.0);
                        field(ui, "Password", &mut draft.password, "Never saved", true);
                    });
                });
                field(
                    ui,
                    "Private key",
                    &mut draft.key_path,
                    "Default key discovered automatically",
                    false,
                );
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Browse…").on_hover_text("Choose a private key file").clicked() {
                        let mut dialog = rfd::FileDialog::new().set_title("Choose private key");
                        if let Some(folder) = Config::path()
                            .ok()
                            .and_then(|path| path.parent().map(|home| home.join(".ssh")))
                            .filter(|folder| folder.is_dir())
                        {
                            dialog = dialog.set_directory(folder);
                        }
                        if let Some(path) = dialog.pick_file() {
                            draft.key_path = path.to_string_lossy().into_owned();
                        }
                    }
                    if ui.button("Generate SSH key").clicked() {
                        generate_key = true;
                    }
                    let copy_public = ui
                        .button("Copy public key")
                        .on_hover_text("Copy the public key of the private key this connection uses");
                    let copy_command = ui
                        .button("Copy install command")
                        .on_hover_text("Copy a shell command that adds this key to ~/.ssh/authorized_keys on a server");
                    if copy_public.clicked() || copy_command.clicked() {
                        // Resolved on click only: it reads ~/.ssh/config.
                        let copied = form_key_path(draft).and_then(|key| {
                            let public = neo_ssh::keygen::public_key_for(&key)?;
                            let text = if copy_command.clicked() {
                                neo_ssh::keygen::authorized_keys_command(&public)?
                            } else {
                                public
                            };
                            Ok((key, text))
                        });
                        match copied {
                            Ok((key, text)) => {
                                ui.output_mut(|output| output.copied_text = text);
                                key_message = Some(format!("Copied from {}", key.display()));
                            }
                            Err(error) => key_message = Some(error),
                        }
                    }
                });
                if !key_status.is_empty() {
                    ui.label(RichText::new(&key_status).size(12.0).color(MUTED));
                }
                egui::CollapsingHeader::new("Connection options")
                    .default_open(self.qa_screenshot.is_some() && std::env::var_os("NEO_SSH_QA_OPTIONS").is_some())
                    .show(ui, |ui| {
                    field(ui, "Saved name", &mut draft.name, "Production", false);
                    field(
                        ui,
                        "tmux session",
                        &mut draft.tmux,
                        "Optional, e.g. deploy",
                        false,
                    );
                    ui.checkbox(&mut draft.strict, "Require a known host key");
                    field(ui, "Jump hosts", &mut draft.options.jump, "bastion,internal-hop (or none)", false);
                    field(ui, "Remote directory on reconnect", &mut draft.remote_directory, "Optional absolute POSIX path", false);
                    ui.label("Remembered from shell directory reports. Clear to use the login directory. Existing tmux panes keep their directory.");
                    ui.checkbox(&mut draft.options.use_agent, "Use SSH agent when available");
                    field(
                        ui,
                        "Authentication methods",
                        &mut draft.options.preferred_auth,
                        "agent,key,keyboard-interactive,password",
                        false,
                    );
                    field(
                        ui,
                        "Terminal type",
                        &mut draft.options.term,
                        "xterm-256color",
                        false,
                    );
                    field(
                        ui,
                        "Remote environment",
                        &mut draft.options.remote_env,
                        "LANG=C.UTF-8; EDITOR=vim",
                        false,
                    );
                    ui.horizontal(|ui| {
                        ui.label("Connect timeout (seconds)");
                        ui.add(
                            egui::DragValue::new(&mut draft.options.connect_timeout_secs)
                                .range(0..=300),
                        );
                    });
                    ui.label("SOCKS5 listeners (one [bind:]port per line)");
                    let mut dynamic = draft.options.dynamic_forwards.join("\n");
                    if ui
                        .add(
                            egui::TextEdit::multiline(&mut dynamic)
                                .desired_rows(2)
                                .desired_width(f32::INFINITY),
                        )
                        .changed()
                    {
                        draft.options.dynamic_forwards = dynamic
                            .lines()
                            .map(str::trim)
                            .filter(|line| !line.is_empty())
                            .map(str::to_owned)
                            .collect();
                    }
                    let mut forwards = draft.options.local_forwards.join("\n");
                    ui.label("Local forwards (one [bind:]port:host:port per line)");
                    if ui
                        .add(
                            egui::TextEdit::multiline(&mut forwards)
                                .desired_rows(2)
                                .desired_width(f32::INFINITY),
                        )
                        .changed()
                    {
                        draft.options.local_forwards = forwards
                            .lines()
                            .map(str::trim)
                            .filter(|line| !line.is_empty())
                            .map(str::to_owned)
                            .collect();
                    }
                    let mut remote = draft.options.remote_forwards.join("\n");
                    ui.label("Remote forwards (one [bind:]port:host:port per line)");
                    if ui
                        .add(
                            egui::TextEdit::multiline(&mut remote)
                                .desired_rows(2)
                                .desired_width(f32::INFINITY),
                        )
                        .changed()
                    {
                        draft.options.remote_forwards = remote
                            .lines()
                            .map(str::trim)
                            .filter(|line| !line.is_empty())
                            .map(str::to_owned)
                            .collect();
                    }
                    ui.horizontal(|ui| {
                        ui.label("Keepalive interval (seconds)");
                        ui.add(
                            egui::DragValue::new(&mut draft.options.keep_alive_secs)
                                .range(0..=3600),
                        );
                    });
                    ui.horizontal(|ui| {
                        let mut delay = draft
                            .options
                            .extensions
                            .get("reconnect_delay_secs")
                            .and_then(toml::Value::as_integer)
                            .unwrap_or(0)
                            .clamp(0, 300);
                        ui.label("Retry a dropped session after (seconds; 0 disables)");
                        if ui
                            .add(egui::DragValue::new(&mut delay).range(0..=300))
                            .changed()
                        {
                            draft
                                .options
                                .extensions
                                .insert("reconnect_delay_secs".into(), toml::Value::Integer(delay));
                        }
                    });
                    ui.label(
                        RichText::new("Zero uses the imported or global default.")
                            .small()
                            .color(MUTED),
                    );
                });
                ui.add_space(12.0);
                ui.add_space(20.0);
                ui.label(
                    RichText::new("Host fingerprints are verified before authentication.")
                        .size(12.0)
                        .color(MUTED),
                );
            });
        });
        if generate_key && self.tabs[self.active].keygen.is_none() {
            self.tabs[self.active].keygen = Some(crate::keygen_dialog::Dialog::default());
        }
        if let Some(message) = key_message {
            self.key_status = message;
        }
    }
    fn connection_actions(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add_space((ui.available_width() - 470.0).max(0.0) / 2.0);
            if ui
                .add_sized(
                    [220.0, 42.0],
                    egui::Button::new(
                        RichText::new(if self.tabs[self.active].terminal.received > 0 {
                            "Reconnect"
                        } else {
                            "Connect"
                        })
                        .strong()
                        .color(BACKGROUND),
                    )
                    .fill(ACCENT),
                )
                .clicked()
            {
                if let Err(error) = self.tabs[self.active].connect() {
                    self.error = Some(error);
                }
            }
            if ui.button("Save connection").clicked() {
                if let Err(error) = self.save_profile() {
                    self.error = Some(error);
                }
            }
        });
    }
    fn save_profile(&mut self) -> Result<(), String> {
        let resolved = self.tabs[self.active].resolve_connection()?;
        let draft = &self.tabs[self.active].draft;
        let target: Target = draft.target.parse()?;
        let mut name = if draft.name.trim().is_empty() {
            target.host.clone()
        } else {
            draft.name.trim().into()
        };
        let existing = self.config.profiles()?;
        if existing.contains_key(&name) && draft.original_name != name {
            let base = name.clone();
            let mut suffix = 2;
            while existing.contains_key(&name) {
                name = format!("{base} ({suffix})");
                suffix += 1;
            }
        }
        let mut profile = draft.options.clone();
        profile.host = target.host;
        profile.port = resolved.target.port;
        profile.user = resolved.target.user.unwrap_or_default();
        profile.key_path = draft.key_path.clone();
        profile.tmux_session = draft.tmux.clone();
        profile.strict_host_key = draft.strict;
        let original = draft.original_name.clone();
        if !original.is_empty() && original != name {
            self.config
                .rename_profile(&self.config_path, &original, &name, &profile)?;
            for tab in &mut self.tabs {
                if tab.draft.original_name == original {
                    tab.draft.original_name = name.clone();
                    if tab.draft.name == original {
                        tab.draft.name = name.clone();
                    }
                }
            }
        } else {
            self.config
                .save_profile(&self.config_path, &name, &profile)?;
        }
        self.tabs[self.active].draft.options = profile;
        self.tabs[self.active].draft.saved_profile = true;
        self.tabs[self.active].draft.original_name = name.clone();
        self.tabs[self.active].draft.name = name;
        self.tabs[self.active].detail = "Connection saved".into();
        Ok(())
    }
    fn delete_profile_dialog(&mut self, ctx: &egui::Context) {
        let Some(name) = self.delete_profile.clone() else {
            return;
        };
        if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.delete_profile = None;
            return;
        }
        let mut open = true;
        let mut confirm = false;
        let mut cancel = false;
        egui::Window::new("Delete saved connection")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(format!("Delete saved connection '{name}'?"));
                ui.label("Open tabs and remote sessions will remain open.");
                ui.horizontal(|ui| {
                    cancel = ui.button("Cancel").clicked();
                    confirm = ui.button("Delete saved connection").clicked();
                });
            });
        if confirm {
            match self.config.delete_profile(&self.config_path, &name) {
                Ok(()) => {
                    for tab in &mut self.tabs {
                        if tab.draft.original_name == name {
                            tab.draft.original_name.clear();
                            tab.draft.saved_profile = false;
                        }
                    }
                    self.delete_profile = None;
                }
                Err(error) => {
                    self.delete_profile = None;
                    self.error = Some(error);
                }
            }
        } else if cancel || !open {
            self.delete_profile = None;
        }
    }
    fn dialogs(&mut self, ctx: &egui::Context) {
        let tab = &mut self.tabs[self.active];
        let mut decision = None;
        if let Some((key, _)) = &tab.trust {
            egui::Window::new("Verify this server")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    let (heading, advice) = key.trust_warning();
                    if key.other_types_known {
                        ui.colored_label(ui.visuals().warn_fg_color, heading);
                    } else {
                        ui.label(heading);
                    }
                    ui.label(advice);
                    ui.monospace(&key.algorithm);
                    ui.monospace(&key.fingerprint);
                    ui.horizontal(|ui| {
                        if ui.button("Trust and connect").clicked() {
                            decision = Some(true);
                        }
                        if ui.button("Cancel").clicked() {
                            decision = Some(false);
                        }
                    });
                });
        }
        if let Some(value) = decision {
            if let Some((_, answer)) = tab.trust.take() {
                let _ = answer.send(value);
            }
        }
        let mut challenge_action = None;
        if let Some(challenge) = &mut tab.challenge {
            egui::Window::new("Authentication")
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(&challenge.name);
                    ui.label(&challenge.instruction);
                    for (index, (prompt, echo)) in challenge.prompts.iter().enumerate() {
                        field(ui, prompt, &mut challenge.answers[index], "", !echo);
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Continue").clicked() {
                            challenge_action = Some(true);
                        }
                        if ui.button("Cancel").clicked() {
                            challenge_action = Some(false);
                        }
                    });
                });
        }
        if let Some(accept) = challenge_action {
            if let Some(mut challenge) = tab.challenge.take() {
                if accept {
                    if let Err(mut error) =
                        challenge.reply.send(std::mem::take(&mut challenge.answers))
                    {
                        error.0.zeroize();
                    }
                }
            }
        }
        let mut paste_action = None;
        if let Some(review) = &self.paste {
            egui::Window::new("Review multiline paste")
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("This text contains line breaks. Review it before sending.");
                    for (index, _) in &review.recipients {
                        if let Some(tab) = self.tabs.get(*index) {
                            ui.label(format!("Destination: {}", tab.title()));
                        }
                    }
                    egui::ScrollArea::vertical()
                        .max_height(240.0)
                        .show(ui, |ui| {
                            // Shows CR-only line breaks and escapes hidden characters
                            // without changing the submitted payload.
                            ui.monospace(terminal::paste_preview(&review.text));
                        });
                    ui.horizontal(|ui| {
                        if ui.button("Send paste").clicked() {
                            paste_action = Some(PasteMode::Whole);
                        }
                        if ui
                            .button("Send line by line")
                            .on_hover_text(
                                "Send each line followed by Enter, without bracketed paste (ssh4).",
                            )
                            .clicked()
                        {
                            paste_action = Some(PasteMode::Lines);
                        }
                        if ui.button("Cancel").clicked() {
                            paste_action = Some(PasteMode::Cancel);
                        }
                    });
                });
        }
        if let Some(mode) = paste_action {
            if let Some(review) = self.paste.take() {
                match mode {
                    PasteMode::Whole => deliver_review(&mut self.tabs, review),
                    PasteMode::Lines => deliver_review_lines(&mut self.tabs, review),
                    PasteMode::Cancel => {}
                }
            }
        }
        if self.palette {
            let snippets = self.config.snippets().unwrap_or_default();
            let commands: Vec<(String, Command)> = palette_commands(&snippets)
                .into_iter()
                .filter(|(title, _)| palette_matches(title, &self.palette_query))
                .collect();
            self.palette_selected = self.palette_selected.min(commands.len().saturating_sub(1));
            let (up, down, enter) = ctx.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                )
            });
            if up {
                self.palette_selected = self.palette_selected.saturating_sub(1);
            }
            if down && self.palette_selected + 1 < commands.len() {
                self.palette_selected += 1;
            }
            let mut action = if enter {
                commands
                    .get(self.palette_selected)
                    .map(|(_, command)| command.clone())
            } else {
                None
            };
            let mut close = false;
            egui::Window::new("Commands")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_TOP, [0.0, 72.0])
                .show(ctx, |ui| {
                    let query = ui.add(
                        egui::TextEdit::singleline(&mut self.palette_query)
                            .hint_text("Type to filter commands, themes and snippets")
                            .desired_width(360.0),
                    );
                    if std::mem::take(&mut self.palette_focus) {
                        query.request_focus();
                    }
                    if query.changed() {
                        self.palette_selected = 0;
                    }
                    egui::ScrollArea::vertical()
                        .max_height(380.0)
                        .show(ui, |ui| {
                            for (index, (title, command)) in commands.iter().enumerate() {
                                let selected = index == self.palette_selected;
                                let row = ui.add_sized(
                                    [360.0, 28.0],
                                    egui::SelectableLabel::new(selected, title.as_str()),
                                );
                                if selected && (up || down) {
                                    row.scroll_to_me(None);
                                }
                                if row.clicked() {
                                    action = Some(command.clone());
                                }
                            }
                            if commands.is_empty() {
                                ui.label("No matching commands.");
                            }
                        });
                    ui.horizontal(|ui| {
                        ui.small("Up/Down select, Enter runs, Esc closes");
                        if ui.button("Close").clicked() {
                            close = true;
                        }
                    });
                });
            if close {
                self.palette = false;
            }
            if let Some(command) = action {
                self.palette = false;
                self.run_command(command);
            }
        }
        if self.help {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                self.help = false;
            }
            let mut open = self.help;
            egui::Window::new("Keyboard and mouse shortcuts")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    egui::Grid::new("shortcut_help")
                        .num_columns(2)
                        .spacing([24.0, 6.0])
                        .striped(true)
                        .show(ui, |ui| {
                            for (keys, action) in SHORTCUTS {
                                ui.monospace(*keys);
                                ui.label(*action);
                                ui.end_row();
                            }
                        });
                    if ui.button("Close").clicked() {
                        self.help = false;
                    }
                });
            self.help &= open;
        }
        if let Some(error) = self.error.clone() {
            egui::Window::new("Unable to complete action")
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(error);
                    if ui.button("OK").clicked() {
                        self.error = None;
                    }
                });
        }
    }
}

enum UserInput {
    Text(Zeroizing<String>),
    Paste(Zeroizing<String>),
    Key(Key, Modifiers),
}
impl UserInput {
    fn encode(&self, terminal: &Terminal) -> Vec<u8> {
        match self {
            Self::Text(text) => text.as_bytes().to_vec(),
            Self::Paste(text) => terminal.paste(text),
            Self::Key(key, modifiers) => {
                input::encode_key(*key, *modifiers, terminal.screen().application_cursor())
            }
        }
    }
}
struct PasteReview {
    text: String,
    recipients: Vec<(usize, u64)>,
}
fn accepts_sync(tab: &Tab) -> bool {
    tab.connected && tab.trust.is_none() && tab.challenge.is_none()
}
fn input_recipients(tabs: &[Tab], source: usize, sync: bool) -> Vec<(usize, u64)> {
    tabs.iter()
        .enumerate()
        .filter(|(index, tab)| (*index == source || sync) && accepts_sync(tab))
        .map(|(index, tab)| (index, tab.generation))
        .collect()
}
fn broadcast_input(tabs: &mut [Tab], source: usize, input: &UserInput) {
    for (index, tab) in tabs.iter_mut().enumerate() {
        if index != source && accepts_sync(tab) {
            tab.send(input.encode(&tab.terminal));
        }
    }
}
fn deliver_review(tabs: &mut [Tab], review: PasteReview) {
    for (index, generation) in review.recipients {
        if let Some(tab) = tabs
            .get_mut(index)
            .filter(|tab| accepts_sync(tab) && tab.generation == generation)
        {
            tab.send(tab.terminal.paste(&review.text));
        }
    }
}

fn home_directory() -> Result<PathBuf, String> {
    Ok(Config::path()
        .map_err(|e| e.to_string())?
        .parent()
        .ok_or("Home directory unavailable.")?
        .to_path_buf())
}

/// Resolve the form the way a connection does (profile, typed overrides and
/// `~/.ssh/config`); also returns the ssh_config warnings to record.
fn resolve_draft(draft: &Draft) -> Result<(neo_ssh::session::Connect, Vec<String>), String> {
    let home = home_directory()?;
    let imports = neo_ssh::ssh_config::SshConfig::load(&home.join(".ssh/config"))?;
    let local_user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_default();
    let connect = resolve_form(draft, &imports, &home, &local_user)?;
    Ok((connect, imports.warnings().to_vec()))
}

/// An unsaved form resolves without a profile, so its port, user and timeouts
/// keep falling back to ssh_config, but the forward lists it shows (typed or
/// from `--gui -L/-R/-D`) apply either way, as they would once saved.
fn resolve_form(
    draft: &Draft,
    imports: &neo_ssh::ssh_config::SshConfig,
    home: &std::path::Path,
    local_user: &str,
) -> Result<neo_ssh::session::Connect, String> {
    let mut profile = draft.options.clone();
    profile.strict_host_key = draft.strict;
    let profile = draft.saved_profile.then_some(&profile);
    let mut connect = neo_ssh::connection::resolve(
        profile,
        neo_ssh::connection::Overrides {
            target: Some(draft.target.clone()),
            user: (!draft.user.trim().is_empty()).then(|| draft.user.trim().into()),
            key: (!draft.key_path.trim().is_empty()).then(|| PathBuf::from(draft.key_path.trim())),
            tmux: (!draft.tmux.trim().is_empty()).then(|| draft.tmux.trim().into()),
            no_tmux: draft.tmux.trim().is_empty(),
            jump: (!draft.options.jump.trim().is_empty()).then(|| draft.options.jump.clone()),
        },
        imports,
        home,
        local_user,
        draft.global_keepalive,
    )?;
    if !draft.saved_profile {
        neo_ssh::connection::apply_forwards(&mut connect, &draft.options)?;
    }
    Ok(connect)
}

/// The private key a connection from this form would use: the typed path,
/// the profile key, ssh_config `IdentityFile`, then `id_ed25519`/`id_rsa`, with
/// `~` and `%` tokens expanded. Without a usable target, the typed path (with
/// `~` expanded) or the first default key.
fn form_key_path(draft: &Draft) -> Result<PathBuf, String> {
    if !draft.target.trim().is_empty() {
        if let Ok((connect, _)) = resolve_draft(draft) {
            return connect
                .key_path
                .ok_or_else(|| "No private key found for this connection.".into());
        }
    }
    let home = home_directory()?;
    let typed = draft.key_path.trim();
    if !typed.is_empty() {
        return neo_ssh::ssh_config::expand(typed, "", "", "", 22, &home).map(PathBuf::from);
    }
    ["id_ed25519", "id_rsa"]
        .iter()
        .map(|name| home.join(".ssh").join(name))
        .find(|path| path.is_file())
        .ok_or_else(|| "No private key found: choose one with Browse… or generate one.".into())
}

/// Header chips from ssh4gui: bracketed paste / mouse reporting / scroll offset
/// and the configured port forwards, each with an explanatory tooltip.
fn drawing_glyph(text: &str) -> bool {
    text.chars().next().is_some_and(|c| {
        matches!(c,
            '\u{2500}'..='\u{259f}' // box drawing and block elements (half blocks)
            | '\u{2800}'..='\u{28ff}' // braille patterns (graphs)
            | '\u{1fb00}'..='\u{1fbff}') // legacy computing (sextants)
    })
}

fn status_chips(tab: &Tab) -> Vec<(String, String)> {
    let mut chips = Vec::new();
    if !tab.connected {
        return chips;
    }
    let modes = tab.terminal.modes;
    if modes.bracketed_paste {
        chips.push((
            "PASTE".into(),
            "The remote application enabled bracketed paste.".into(),
        ));
    }
    if modes.click || modes.drag || modes.motion {
        chips.push((
            "MOUSE".into(),
            "The remote application receives mouse input. Hold Shift to select text locally."
                .into(),
        ));
    }
    if tab.scroll > 0 {
        chips.push((
            format!("SCROLL +{}", tab.scroll),
            "Viewing scrollback. Use Back to live output to return.".into(),
        ));
    }
    if !tab.background_edits.is_empty() {
        let now = std::time::Instant::now();
        let failed = tab
            .background_edits
            .iter()
            .any(|edit| edit.status(now).is_error());
        let details: Vec<String> = tab
            .background_edits
            .iter()
            .map(|edit| format!("{}: {}", edit.name(), edit.status(now).label()))
            .collect();
        chips.push((
            if failed {
                format!("EDITING {} (ERROR)", tab.background_edits.len())
            } else {
                format!("EDITING {}", tab.background_edits.len())
            },
            format!(
                "Remote edits syncing in the background. Open Remote files to manage them.\n{}",
                details.join("\n")
            ),
        ));
    }
    let options = &tab.draft.options;
    for (prefix, forwards, hint) in [
        ("L", &options.local_forwards, "Local forward"),
        ("R", &options.remote_forwards, "Remote forward"),
        ("D", &options.dynamic_forwards, "SOCKS (dynamic) forward"),
    ] {
        for forward in forwards.iter().filter(|forward| !forward.trim().is_empty()) {
            chips.push((format!("{prefix} {}", forward.trim()), hint.into()));
        }
    }
    chips
}

/// ssh4gui's command entry: type a command, press Enter (or Send) to type it
/// into the terminal followed by Enter. Honors synchronized input.
/// Send a command typed in the one-line bar, followed by Enter. A paste can
/// put line breaks or escape sequences into the single-line field; such text
/// is returned for the paste review instead of running unseen.
fn submit_command_line(tab: &mut Tab, text: String) -> Option<String> {
    if text.chars().any(char::is_control) {
        return Some(text);
    }
    tab.send_user(UserInput::Text(Zeroizing::new(text)));
    tab.send_user(UserInput::Key(Key::Enter, Modifiers::default()));
    None
}

/// The one-line command entry; returns text that needs the paste review.
fn command_line_bar(ui: &mut egui::Ui, tab: &mut Tab, modal: bool) -> Option<String> {
    let mut review = None;
    ui.horizontal(|ui| {
        let ready = tab.connected && !modal;
        let edit = ui.add_enabled(
            ready,
            egui::TextEdit::singleline(&mut tab.command_line)
                .hint_text("Type a command and press Enter to send it")
                .desired_width((ui.available_width() - 70.0).max(80.0)),
        );
        let submitted = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let clicked = ui
            .add_enabled(
                ready && !tab.command_line.is_empty(),
                egui::Button::new("Send"),
            )
            .clicked();
        if ready && (submitted || clicked) && !tab.command_line.is_empty() {
            let text = std::mem::take(&mut tab.command_line);
            review = submit_command_line(tab, text);
            edit.request_focus();
        }
    });
    review
}

fn deliver_review_lines(tabs: &mut [Tab], review: PasteReview) {
    let bytes = paste_lines(&review.text);
    for (index, generation) in review.recipients {
        if let Some(tab) = tabs
            .get_mut(index)
            .filter(|tab| accepts_sync(tab) && tab.generation == generation)
        {
            tab.send(bytes.clone());
        }
    }
}

#[derive(Clone, Copy)]
enum PasteMode {
    Whole,
    Lines,
    Cancel,
}

/// ssh4's "send line by line": every line (CRLF, CR or LF separated) is typed
/// followed by Enter, without bracketed-paste markers. A trailing line break
/// does not add an extra empty Enter.
fn paste_lines(text: &str) -> Vec<u8> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let body = normalized.strip_suffix('\n').unwrap_or(&normalized);
    let mut bytes = Vec::with_capacity(body.len() + 8);
    for line in body.split('\n') {
        bytes.extend_from_slice(line.as_bytes());
        bytes.push(b'\r');
    }
    bytes
}

/// Shortcut overview shown by the Shortcuts button / palette (ssh4 help overlay).
/// Application shortcuts, taken before the terminal sees the keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AppShortcut {
    NewTab,
    Palette,
    Find,
    FocusMode,
    LargerText,
    SmallerText,
    ResetText,
}

/// Like other terminal emulators, app commands use Ctrl+Shift (Cmd+Shift on
/// macOS), so plain Ctrl letters reach the remote: Ctrl+N/P history, Ctrl+K
/// kill line, Ctrl+F forward. Text size follows Windows Terminal's
/// Ctrl+= / Ctrl+- / Ctrl+0.
const APP_SHORTCUTS: &[(egui::Modifiers, egui::Key, AppShortcut)] = {
    use egui::{Key, Modifiers};
    const COMMAND_SHIFT: Modifiers = Modifiers::COMMAND.plus(Modifiers::SHIFT);
    &[
        (COMMAND_SHIFT, Key::T, AppShortcut::NewTab),
        (COMMAND_SHIFT, Key::P, AppShortcut::Palette),
        (COMMAND_SHIFT, Key::F, AppShortcut::Find),
        (Modifiers::NONE, Key::F11, AppShortcut::FocusMode),
        (Modifiers::COMMAND, Key::Equals, AppShortcut::LargerText),
        (Modifiers::COMMAND, Key::Plus, AppShortcut::LargerText),
        (COMMAND_SHIFT, Key::Plus, AppShortcut::LargerText),
        (Modifiers::COMMAND, Key::Minus, AppShortcut::SmallerText),
        (Modifiers::COMMAND, Key::Num0, AppShortcut::ResetText),
    ]
};

/// Takes the application shortcuts pressed this frame, in order. Modifiers must
/// match exactly: egui's `consume_key` also accepts extra Shift or Alt, which
/// would take Ctrl+_ (undo) as Ctrl+- and AltGr chords that type characters.
fn take_app_shortcuts(ctx: &egui::Context) -> Vec<AppShortcut> {
    let mut taken = Vec::new();
    ctx.input_mut(|input| {
        input.events.retain(|event| {
            let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            else {
                return true;
            };
            let found = APP_SHORTCUTS
                .iter()
                .find(|(pattern, wanted, _)| wanted == key && modifiers.matches_exact(*pattern));
            if let Some((_, _, shortcut)) = found {
                taken.push(*shortcut);
            }
            found.is_none()
        });
    });
    taken
}

const SHORTCUTS: &[(&str, &str)] = &[
    (
        "Ctrl/Cmd+Shift+P",
        "Command palette (type to filter, Enter to run)",
    ),
    ("Ctrl/Cmd+Shift+T", "New connection tab"),
    ("Ctrl/Cmd+Shift+F", "Find in output and scrollback"),
    ("F11", "Focus mode (hide sidebar and header)"),
    (
        "Ctrl/Cmd+= / Ctrl/Cmd+- / Ctrl/Cmd+0",
        "Larger, smaller or saved terminal text size",
    ),
    (
        "Ctrl/Cmd+Shift+I",
        "Upload clipboard image (opens Remote files)",
    ),
    (
        "Ctrl+C",
        "Copy selection, or send interrupt with no selection",
    ),
    ("Ctrl+Shift+C / Ctrl+Insert", "Copy selection only"),
    (
        "Ctrl+V / Shift+Insert",
        "Paste (multiline text is reviewed first)",
    ),
    ("Ctrl+Shift+V", "Paste immediately without review"),
    ("Esc", "Cancel selection or close the open dialog"),
    (
        "Drag",
        "Select text (Shift+drag when the remote app uses the mouse)",
    ),
    (
        "Right-click",
        "Paste, or Copy / Paste / Search online with a selection",
    ),
    (
        "Ctrl+click",
        "Open a link, or locate an absolute path in Remote files",
    ),
    ("Alt+hover", "Show when the hovered row last changed"),
    ("Middle-click tab", "Close the tab"),
    ("Drop files", "Queue uploads to the connected server"),
];

#[derive(Clone, Debug, PartialEq)]
enum Command {
    NewConnection,
    CloseTab,
    ToggleFocus,
    ToggleSync,
    FindInTerminal,
    FindTabs,
    RemoteFiles,
    UploadClipboardImage,
    Snippets,
    RunSnippet(usize, String, String),
    Preferences,
    Theme(&'static str),
    IncreaseText,
    DecreaseText,
    SaveDiagnostic,
    InstallPublicKey,
    Shortcuts,
}

fn palette_commands(snippets: &[Snippet]) -> Vec<(String, Command)> {
    let mut commands: Vec<(String, Command)> = [
        ("New connection", Command::NewConnection),
        ("Close tab", Command::CloseTab),
        ("Toggle focus mode", Command::ToggleFocus),
        ("Toggle synchronized input", Command::ToggleSync),
        ("Find in terminal", Command::FindInTerminal),
        ("Search open tabs", Command::FindTabs),
        ("Open remote files", Command::RemoteFiles),
        ("Upload clipboard image", Command::UploadClipboardImage),
        ("Command snippets", Command::Snippets),
        ("Preferences", Command::Preferences),
        ("Increase terminal text", Command::IncreaseText),
        ("Decrease terminal text", Command::DecreaseText),
        ("Save diagnostic report", Command::SaveDiagnostic),
        ("Install public key", Command::InstallPublicKey),
        ("Keyboard shortcuts", Command::Shortcuts),
    ]
    .into_iter()
    .map(|(title, command)| (title.to_owned(), command))
    .collect();
    commands.extend(
        neo_ssh::preferences::THEMES
            .iter()
            .map(|theme| (format!("Theme: {theme}"), Command::Theme(theme))),
    );
    commands.extend(snippets.iter().enumerate().map(|(index, snippet)| {
        (
            format!("Snippet: {}", snippet.name),
            Command::RunSnippet(index, snippet.name.clone(), snippet.command.clone()),
        )
    }));
    commands
}

/// Every whitespace-separated word of the query must appear in the title (case-insensitive).
fn palette_matches(title: &str, query: &str) -> bool {
    let title = title.to_lowercase();
    query
        .to_lowercase()
        .split_whitespace()
        .all(|word| title.contains(word))
}

fn tab_color(value: &str) -> Color32 {
    match value {
        "mint" => ACCENT,
        "blue" => Color32::from_rgb(137, 191, 255),
        "amber" => Color32::from_rgb(244, 196, 114),
        "pink" => Color32::from_rgb(241, 158, 205),
        _ => INK,
    }
}

enum CloseTabs {
    One(usize),
    Others(usize),
    Right(usize),
}

fn tab_matches(tab: &Tab, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    [
        tab.title(),
        &tab.draft.target,
        &tab.draft.user,
        &tab.draft.original_name,
    ]
    .iter()
    .any(|text| text.to_lowercase().contains(&query))
}

fn close_tabs(tabs: &mut Vec<Tab>, active: &mut usize, action: CloseTabs) {
    let old_active = *active;
    let mut next_active = None;
    let mut before_active = 0;
    let mut old_index = 0;
    let mut retained = 0;
    tabs.retain(|_| {
        let keep = match action {
            CloseTabs::One(index) => old_index != index,
            CloseTabs::Others(index) => old_index == index,
            CloseTabs::Right(index) => old_index <= index,
        };
        if keep {
            if old_index == old_active {
                next_active = Some(retained);
            }
            if old_index < old_active {
                before_active += 1;
            }
            retained += 1;
        }
        old_index += 1;
        keep
    });
    if tabs.is_empty() {
        tabs.push(Tab::new(Draft::default()));
    }
    *active = next_active.unwrap_or(before_active.min(tabs.len() - 1));
}

fn snapshot_tabs(tabs: &[Tab], active: usize) -> Snapshot {
    let mut snapshot = Snapshot::default();
    let mut chosen = false;
    for (index, tab) in tabs.iter().enumerate() {
        if tab.draft.target.trim().is_empty() {
            continue;
        }
        if !chosen {
            snapshot.active = snapshot.tabs.len();
            chosen = index >= active;
        }
        let mut connection = tab.draft.options.clone();
        if let Ok(target) = tab.draft.target.parse::<Target>() {
            connection.host = target.host;
            if target.port != 22 || connection.port == 0 {
                connection.port = target.port;
            }
        }
        connection.user = tab.draft.user.clone();
        connection.key_path = tab.draft.key_path.clone();
        connection.tmux_session = tab.draft.tmux.clone();
        connection.strict_host_key = tab.draft.strict;
        snapshot.tabs.push(SavedTab {
            tab_color: tab.color.clone(),
            title: tab.draft.name.clone(),
            profile_name: tab.draft.original_name.clone(),
            target: tab.draft.target.clone(),
            remote_directory: tab.draft.remote_directory.clone(),
            connection,
        });
    }
    snapshot
}

fn field(ui: &mut egui::Ui, label: &str, text: &mut String, hint: &str, secret: bool) {
    ui.label(RichText::new(label).size(12.0).color(MUTED));
    ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .password(secret)
            .desired_width(f32::INFINITY),
    );
}
fn format_target(profile: &Profile) -> String {
    Target {
        host: profile.host.clone(),
        user: None,
        port: profile.port,
    }
    .to_string()
}

fn search_bar(ui: &mut egui::Ui, tab: &mut Tab, modal: bool) {
    if !tab.search_open {
        return;
    }
    ui.add_enabled_ui(!modal, |ui| {
        ui.horizontal_wrapped(|ui| {
            let edit = ui.add(
                egui::TextEdit::singleline(&mut tab.search_query)
                    .hint_text("Find in output and scrollback")
                    .desired_width(240.0),
            );
            if std::mem::take(&mut tab.search_focus) {
                edit.request_focus();
            }
            let submitted = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if edit.changed() {
                tab.invalidate_search();
            }
            if ui.checkbox(&mut tab.search_case, "Match case").changed() {
                tab.invalidate_search();
            }
            if (submitted || ui.button("Search / refresh").clicked())
                && !tab.search_query.is_empty()
            {
                tab.start_search();
            }
            if tab.search_job.is_some() {
                ui.spinner();
            }
            if let Some(results) = &mut tab.search_results {
                let count = results.matches.len();
                ui.label(if count == 0 {
                    "No matches".to_owned()
                } else {
                    format!(
                        "{} / {}{}",
                        results.active + 1,
                        count,
                        if results.truncated { "+" } else { "" }
                    )
                });
                if ui
                    .add_enabled(count > 0, egui::Button::new("Previous"))
                    .clicked()
                {
                    results.navigate(false);
                    tab.scroll = results.screen.scrollback();
                    tab.selection = None;
                }
                if ui
                    .add_enabled(count > 0, egui::Button::new("Next"))
                    .clicked()
                {
                    results.navigate(true);
                    tab.scroll = results.screen.scrollback();
                    tab.selection = None;
                }
                ui.label(
                    RichText::new("Snapshot · session continues")
                        .small()
                        .color(MUTED),
                );
            }
            if ui.button("Close").clicked()
                || ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
            {
                tab.close_search();
            }
        });
    });
}

fn copy_selection(ui: &mut egui::Ui, tab: &mut Tab) -> bool {
    if let Some(text) = tab.copied.take() {
        ui.output_mut(|output| output.copied_text = text);
        tab.selection = None;
        true
    } else {
        false
    }
}

fn paste_text(tab: &mut Tab, text: String, force: bool) -> Option<String> {
    if terminal::needs_paste_confirmation(&text) && !force {
        Some(text)
    } else {
        tab.send_user(UserInput::Paste(Zeroizing::new(text)));
        None
    }
}

/// Window, taskbar and Alt+Tab icon (assets/make_icon.py); without it eframe
/// shows its own logo. The executable's file icon is embedded by build.rs.
fn app_icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/neo-ssh.png")).unwrap_or_default()
}

fn terminal_view(ui: &mut egui::Ui, tab: &mut Tab, font_size: f32, modal: bool) -> Option<String> {
    let rgb = |[r, g, b]: [u8; 3]| Color32::from_rgb(r, g, b);
    let foreground = rgb(tab.terminal.colors.foreground);
    let background = rgb(tab.terminal.colors.background);
    let cursor_color = rgb(tab.terminal.colors.cursor);
    if tab.selection.is_none() {
        tab.selecting = false;
    }
    if modal {
        tab.selecting = false;
        tab.link_press = None;
        let bytes = tab.mouse.release_all(tab.terminal.modes);
        tab.send(bytes);
    }
    let font = FontId::monospace(font_size);
    let cell = ui.fonts(|fonts| Vec2::new(fonts.glyph_width(&font, 'M'), fonts.row_height(&font)));
    let (rect, _) = ui.allocate_exact_size(
        ui.available_size().max(Vec2::splat(1.0)),
        egui::Sense::hover(),
    );
    // A fixed id keeps keyboard focus when rows above the terminal appear or
    // disappear (Remote files, last error, retry countdown), as they do on connect.
    let response = ui.interact(
        rect,
        egui::Id::new("neo-ssh-terminal"),
        egui::Sense::click_and_drag(),
    );
    if !modal && std::mem::take(&mut tab.focus_terminal) {
        response.request_focus();
    }
    let size = (
        ((rect.width() / cell.x).floor() as u16).max(1),
        ((rect.height() / cell.y).floor() as u16).max(1),
    );
    if size != tab.size {
        tab.size = size;
        tab.terminal.resize(size.1, size.0);
        if let Some(session) = &tab.session {
            session.resize(size.0, size.1);
        }
    }
    let (display_rows, display_columns) = tab.display_screen().size();
    if let Some(results) = &mut tab.search_results {
        results.viewport(size.1, size.0);
    }
    let origin = tab
        .search_results
        .as_ref()
        .map(|results| results.origin)
        .unwrap_or((0, 0));
    let size = (size.0.min(display_columns), size.1.min(display_rows));
    if response.clicked() || response.drag_started() {
        response.request_focus();
    }
    let position = |point: egui::Pos2| {
        (
            origin.0 + (((point.y - rect.top()) / cell.y).floor().max(0.0) as u16).min(size.1 - 1),
            origin.1 + (((point.x - rect.left()) / cell.x).floor().max(0.0) as u16).min(size.0 - 1),
        )
    };
    if !modal {
        let mut link_gesture = tab.link_press.is_some();
        let modes = tab.terminal.modes;
        let remote_mouse = tab.connected
            && !tab.search_open
            && tab.scroll == 0
            && (modes.click || modes.drag || modes.motion);
        for event in ui.input(|input| input.events.clone()) {
            match event {
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers,
                } => {
                    if button == egui::PointerButton::Primary {
                        if pressed {
                            tab.selecting = false;
                        }
                        if pressed
                            && modifiers.ctrl
                            && !modifiers.shift
                            && !modifiers.alt
                            && rect.contains(pos)
                            && !tab.mouse.captured()
                        {
                            if let Some(link) =
                                neo_ssh::links::at(tab.display_screen(), position(pos))
                            {
                                tab.link_press = Some((link, pos, false));
                                link_gesture = true;
                                response.request_focus();
                                continue;
                            }
                        } else if !pressed {
                            if let Some((link, start, dragged)) = tab.link_press.take() {
                                link_gesture = true;
                                if !dragged
                                    && start.distance(pos) <= 6.0
                                    && modifiers.ctrl
                                    && !modifiers.shift
                                    && !modifiers.alt
                                    && rect.contains(pos)
                                    && neo_ssh::links::at(tab.display_screen(), position(pos))
                                        .as_ref()
                                        == Some(&link)
                                {
                                    match link.destination {
                                        neo_ssh::links::Destination::Web(url) => {
                                            ui.ctx().open_url(egui::OpenUrl::new_tab(url))
                                        }
                                        neo_ssh::links::Destination::RemotePath(path) => {
                                            if tab.connected {
                                                tab.pending_remote_path = Some(path);
                                            }
                                            break;
                                        }
                                    }
                                }
                                continue;
                            }
                        }
                    }
                    let button = match button {
                        egui::PointerButton::Primary => 0,
                        egui::PointerButton::Middle => 1,
                        egui::PointerButton::Secondary => 2,
                        _ => continue,
                    };
                    if (pressed && remote_mouse && rect.contains(pos))
                        || (!pressed && tab.mouse.captured())
                    {
                        let (row, column) = position(pos);
                        let action = if pressed {
                            MouseAction::Press(button)
                        } else {
                            MouseAction::Release(button)
                        };
                        let bytes =
                            tab.mouse
                                .event(modes, action, column, row, gui_modifiers(modifiers));
                        tab.send(bytes);
                        if pressed {
                            response.request_focus();
                        }
                    }
                }
                egui::Event::PointerMoved(pos) if tab.link_press.is_some() => {
                    if let Some((_, start, dragged)) = &mut tab.link_press {
                        *dragged |= start.distance(pos) > 6.0;
                    }
                }
                egui::Event::PointerMoved(pos)
                    if remote_mouse
                        && !tab.selecting
                        && (rect.contains(pos) || tab.mouse.captured()) =>
                {
                    let (row, column) = position(pos);
                    let bytes = tab.mouse.event(
                        modes,
                        MouseAction::Move(None),
                        column,
                        row,
                        gui_modifiers(ui.input(|i| i.modifiers)),
                    );
                    tab.send(bytes);
                }
                egui::Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                } if response.hovered() => {
                    let lines = match unit {
                        egui::MouseWheelUnit::Point => delta.y / cell.y,
                        egui::MouseWheelUnit::Line => delta.y,
                        egui::MouseWheelUnit::Page => delta.y * f32::from(size.1),
                    };
                    tab.wheel += lines;
                    let steps = tab.wheel.trunc().clamp(-100.0, 100.0) as i32;
                    tab.wheel -= steps as f32;
                    if remote_mouse && !modifiers.shift {
                        if let Some(point) = ui.input(|i| i.pointer.hover_pos()) {
                            let (row, column) = position(point);
                            for _ in 0..steps.unsigned_abs() {
                                let action = if steps > 0 {
                                    MouseAction::WheelUp
                                } else {
                                    MouseAction::WheelDown
                                };
                                tab.send(input::encode_mouse(
                                    modes,
                                    action,
                                    column,
                                    row,
                                    gui_modifiers(modifiers),
                                ));
                            }
                        }
                    } else {
                        tab.scroll = if steps > 0 {
                            tab.scroll.saturating_add(steps as usize)
                        } else {
                            tab.scroll.saturating_sub(steps.unsigned_abs() as usize)
                        };
                        tab.scroll_to(tab.scroll);
                    }
                }
                egui::Event::WindowFocused(false) => {
                    tab.selecting = false;
                    tab.link_press = None;
                    let bytes = tab.mouse.release_all(modes);
                    tab.send(bytes);
                }
                _ => {}
            }
        }
        let local_selection = !link_gesture && (!remote_mouse || ui.input(|i| i.modifiers.shift));
        if response.drag_started() && local_selection && !tab.mouse.captured() {
            if let Some(point) = ui.input(|input| input.pointer.press_origin()) {
                let start = position(point);
                tab.selection = Some((start, start));
                tab.selecting = true;
                tab.copied = None;
            }
        }
        if (response.dragged() || response.drag_stopped()) && tab.selecting && !tab.mouse.captured()
        {
            if let (Some((start, _)), Some(point)) =
                (tab.selection, response.interact_pointer_pos())
            {
                tab.selection = Some((start, position(point)));
            }
        }
        if response.drag_stopped() && tab.selecting {
            tab.selecting = false;
            tab.copied = selection_text(tab);
            if let Some(text) = &tab.copied {
                ui.output_mut(|output| output.copied_text = text.clone());
            }
        }
    }
    if ui.input(|i| i.pointer.is_moving()) {
        tab.pointer_hidden = false;
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, background);
    let screen = tab.display_screen();
    let hover_link = if !modal && response.hovered() {
        ui.input(|i| i.pointer.hover_pos())
            .and_then(|pos| neo_ssh::links::at(screen, position(pos)))
    } else {
        None
    };
    if tab.pointer_hidden && !modal && response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::None);
    }
    // Alt+hover: when did this row last change? (ssh4)
    if !modal
        && hover_link.is_none()
        && tab.scroll == 0
        && tab.search_results.is_none()
        && response.hovered()
        && ui.input(|i| i.modifiers.alt && !i.modifiers.ctrl && !i.modifiers.shift)
    {
        if let Some(changed) = ui
            .input(|i| i.pointer.hover_pos())
            .and_then(|pos| tab.row_activity.changed(position(pos).0))
        {
            response.clone().on_hover_text(format!(
                "Row updated {}",
                neo_ssh::row_activity::describe_age(changed.elapsed())
            ));
        }
    }
    if let Some(link) = &hover_link {
        response.clone().on_hover_text(format!(
            "{}\nCtrl-click to {}",
            link.text(),
            match link.destination {
                neo_ssh::links::Destination::Web(_) => "open in your browser",
                neo_ssh::links::Destination::RemotePath(_) =>
                    "locate on this connection in Remote files",
            }
        ));
        if ui.input(|i| i.modifiers.ctrl && !i.modifiers.shift && !i.modifiers.alt) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
    let selected = selection_bounds(tab);
    let bracket_pair = if modal {
        None
    } else {
        let hovered = if response.hovered() {
            ui.input(|i| i.pointer.hover_pos())
                .and_then(|pos| neo_ssh::brackets::at(screen, position(pos)))
        } else {
            None
        };
        hovered.or_else(|| {
            if tab.scroll == 0 && !tab.search_open && !screen.hide_cursor() {
                neo_ssh::brackets::near_cursor(screen)
            } else {
                None
            }
        })
    };
    // Complete all backgrounds before drawing glyphs: a wide character spans
    // its continuation cell, whose background must never paint over it.
    for foreground_pass in [false, true] {
        for row in origin.0..origin.0 + size.1 {
            for column in origin.1..origin.1 + size.0 {
                let Some(value) = screen.cell(row, column) else {
                    continue;
                };
                let area = egui::Rect::from_min_size(
                    rect.min
                        + Vec2::new(
                            (column - origin.1) as f32 * cell.x,
                            (row - origin.0) as f32 * cell.y,
                        ),
                    cell,
                );
                let mut foreground = color(value.fgcolor(), foreground, &tab.terminal.colors);
                let mut background = color(value.bgcolor(), background, &tab.terminal.colors);
                if value.inverse() {
                    std::mem::swap(&mut foreground, &mut background);
                }
                let remote_background = background;
                if bracket_pair.is_some_and(|(a, b)| (row, column) == a || (row, column) == b) {
                    background = BRACKET;
                }
                if let Some(active) = tab
                    .search_results
                    .as_ref()
                    .and_then(|results| results.highlight(row, column))
                {
                    background = if active {
                        Color32::from_rgb(140, 94, 24)
                    } else {
                        Color32::from_rgb(74, 62, 32)
                    };
                }
                if let Some((a, b)) = selected {
                    if (row, column) >= a && (row, column) <= b {
                        background = Color32::from_rgb(39, 74, 91);
                    }
                }
                if !foreground_pass {
                    painter.rect_filled(area, 0.0, background);
                } else if !value.is_wide_continuation()
                    && (!value.contents().is_empty() || value.underline())
                {
                    // Contrast guard (from ssh4gui) runs only for painted
                    // glyphs, and leaves pictures drawn with block, braille or
                    // box characters alone unless NEO-SSH highlighted them.
                    if background != remote_background || !drawing_glyph(&value.contents()) {
                        let [r, g, b] = neo_ssh::preferences::readable_foreground(
                            [foreground.r(), foreground.g(), foreground.b()],
                            [background.r(), background.g(), background.b()],
                        );
                        foreground = Color32::from_rgb(r, g, b);
                    }
                    let mut job = egui::text::LayoutJob::default();
                    job.append(
                        &value.contents(),
                        0.0,
                        egui::TextFormat {
                            font_id: font.clone(),
                            color: foreground,
                            italics: value.italic(),
                            ..Default::default()
                        },
                    );
                    let galley = ui.fonts(|fonts| fonts.layout_job(job));
                    painter.galley(area.left_top(), galley.clone(), foreground);
                    if value.bold() {
                        // Synthetic weight preserves the terminal's fixed cell advance.
                        painter.galley(area.left_top() + Vec2::new(0.45, 0.0), galley, foreground);
                    }
                    if value.underline()
                        || hover_link
                            .as_ref()
                            .is_some_and(|link| link.contains((row, column)))
                    {
                        let width = cell.x * if value.is_wide() { 2.0 } else { 1.0 };
                        let start = egui::pos2(area.left(), area.bottom() - 1.0);
                        painter.line_segment(
                            [start, start + Vec2::new(width, 0.0)],
                            egui::Stroke::new(1.0, foreground),
                        );
                    }
                }
            }
        }
    }
    if !screen.hide_cursor() && tab.scroll == 0 && !tab.search_open {
        let (row, column) = screen.cursor_position();
        let area = egui::Rect::from_min_size(
            rect.min + Vec2::new(column as f32 * cell.x, row as f32 * cell.y),
            cell,
        );
        painter.rect_stroke(area, 0.0, egui::Stroke::new(1.0, cursor_color));
    }
    let cursor = screen.cursor_position();
    let ime_cursor = egui::Rect::from_min_size(
        rect.min
            + Vec2::new(
                cursor.1.min(size.0 - 1) as f32 * cell.x,
                cursor.0.min(size.1 - 1) as f32 * cell.y,
            ),
        cell,
    );
    let accepts_text = response.has_focus()
        && ui.input(|input| input.focused)
        && !modal
        && tab.connected
        && !tab.search_open;
    ui.memory_mut(|memory| {
        memory.set_focus_lock_filter(
            response.id,
            egui::EventFilter {
                tab: accepts_text,
                horizontal_arrows: accepts_text,
                vertical_arrows: accepts_text,
                escape: accepts_text,
            },
        );
    });
    if accepts_text {
        ui.output_mut(|output| {
            output.ime = Some(egui::output::IMEOutput {
                rect,
                cursor_rect: ime_cursor,
            });
        });
    } else {
        tab.ime_cancelled = true;
        tab.ime_active = false;
        tab.ime_preedit.zeroize();
    }
    let mut pending = None;
    let modes = tab.terminal.modes;
    let local_menu = !modal
        && (tab.search_open
            || !tab.connected
            || tab.scroll > 0
            || !(modes.click || modes.drag || modes.motion)
            || ui.input(|i| i.modifiers.shift));
    let mut request_clipboard = false;
    if local_menu {
        if tab.copied.is_some() {
            response.context_menu(|ui| {
                if ui.button("Copy").clicked() {
                    copy_selection(ui, tab);
                    ui.close_menu();
                    response.request_focus();
                }
                if ui
                    .add_enabled(
                        tab.connected && !tab.search_open,
                        egui::Button::new("Paste"),
                    )
                    .clicked()
                {
                    request_clipboard = true;
                    ui.close_menu();
                    response.request_focus();
                }
                if ui.button("Search online").clicked() {
                    if let Some(text) = &tab.copied {
                        let mut url = url::Url::parse("https://www.google.com/search").unwrap();
                        url.query_pairs_mut().append_pair("q", text);
                        ui.ctx().open_url(egui::OpenUrl::new_tab(url.to_string()));
                    }
                    ui.close_menu();
                    response.request_focus();
                }
            });
        } else if response.secondary_clicked() && tab.connected && !tab.search_open {
            request_clipboard = true;
            response.request_focus();
        }
    }
    if request_clipboard {
        match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get_text()) {
            Ok(text) => pending = paste_text(tab, text, false),
            Err(error) => tab.detail = format!("Cannot read clipboard text: {error}"),
        }
    }
    if response.has_focus()
        && !modal
        && tab.search_open
        && ui.input(|i| {
            i.events
                .iter()
                .any(|event| matches!(event, egui::Event::Copy))
        })
    {
        copy_selection(ui, tab);
    }
    if accepts_text {
        let events = ui.input(|input| input.events.clone());
        let composing = tab.ime_active
            || events
                .iter()
                .any(|event| matches!(event, egui::Event::Ime(event) if !matches!(event, egui::ImeEvent::Disabled)));
        // The local egui-winit patch emits the shortcut press before Copy/Paste.
        // Its modifiers remain valid even if Shift was released before this frame.
        let mut clipboard_modifiers = ui.input(|input| input.modifiers);
        // Windows Ctrl+Insert / Shift+Insert arrive as an Insert key followed by
        // Copy/Paste. Like ssh4gui: Ctrl+Insert only copies (never interrupts),
        // Shift+Insert pastes with the normal multiline review.
        let mut insert_chord = false;
        for event in events {
            match event {
                egui::Event::Ime(event) => match event {
                    egui::ImeEvent::Enabled => {
                        tab.ime_cancelled = false;
                        tab.ime_active = true;
                        tab.ime_preedit.zeroize();
                    }
                    egui::ImeEvent::Preedit(text) => {
                        tab.ime_cancelled = false;
                        tab.ime_active = true;
                        tab.ime_preedit.zeroize();
                        if text.len() <= 4096 && !text.chars().any(char::is_control) {
                            *tab.ime_preedit = text;
                        }
                    }
                    egui::ImeEvent::Commit(text) => {
                        tab.ime_active = false;
                        tab.ime_preedit.zeroize();
                        if tab.ime_cancelled {
                            continue;
                        }
                        if terminal::needs_paste_confirmation(&text) {
                            pending = Some(text);
                        } else {
                            tab.send_user(UserInput::Text(Zeroizing::new(text)));
                        }
                    }
                    egui::ImeEvent::Disabled => {
                        tab.ime_cancelled = false;
                        tab.ime_active = false;
                        tab.ime_preedit.zeroize();
                    }
                },
                egui::Event::Text(text) if !tab.ime_active => {
                    tab.send_user(UserInput::Text(Zeroizing::new(text)))
                }
                egui::Event::Paste(text) => {
                    let force = clipboard_modifiers.shift && !insert_chord;
                    insert_chord = false;
                    if let Some(text) = paste_text(tab, text, force) {
                        pending = Some(text);
                    }
                }
                egui::Event::Copy => {
                    let copy_only = clipboard_modifiers.shift || insert_chord;
                    insert_chord = false;
                    if !copy_selection(ui, tab) && !copy_only {
                        tab.send_user(UserInput::Key(
                            Key::Character('c'),
                            Modifiers {
                                control: true,
                                ..Default::default()
                            },
                        ));
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    clipboard_modifiers = modifiers;
                    if composing {
                        continue;
                    }
                    if key == egui::Key::Insert
                        && (modifiers.ctrl || modifiers.shift)
                        && !modifiers.alt
                        && cfg!(windows)
                    {
                        insert_chord = true;
                        continue;
                    }
                    if modifiers.command && modifiers.shift && !modifiers.alt && key == egui::Key::I
                    {
                        tab.clipboard_image_request = true;
                        continue;
                    }
                    if modifiers.command && modifiers.shift && key == egui::Key::C {
                        copy_selection(ui, tab);
                        continue;
                    }
                    // Copy and Paste events follow these presses and act on them.
                    if modifiers.command && matches!(key, egui::Key::C | egui::Key::V) {
                        continue;
                    }
                    if key == egui::Key::Escape && tab.selection.is_some() {
                        tab.selection = None;
                        tab.copied = None;
                        continue;
                    }
                    if let Some(key) = gui_key(key, modifiers.ctrl, modifiers.alt) {
                        tab.send_user(UserInput::Key(
                            key,
                            Modifiers {
                                shift: modifiers.shift,
                                alt: modifiers.alt,
                                control: modifiers.ctrl,
                            },
                        ));
                    }
                }
                _ => {}
            }
        }
    }
    if accepts_text && !tab.ime_preedit.is_empty() {
        let galley =
            ui.fonts(|fonts| fonts.layout_no_wrap(tab.ime_preedit.to_string(), font, foreground));
        let width = galley.size().x.min(rect.width());
        let left = ime_cursor.left().min(rect.right() - width);
        let area =
            egui::Rect::from_min_size(egui::pos2(left, ime_cursor.top()), Vec2::new(width, cell.y));
        painter.rect_filled(area, 0.0, background);
        painter.galley(area.min, galley, foreground);
        painter.line_segment(
            [area.left_bottom(), area.right_bottom()],
            egui::Stroke::new(1.0, ACCENT),
        );
    }
    pending
}

fn gui_modifiers(value: egui::Modifiers) -> Modifiers {
    Modifiers {
        shift: value.shift,
        alt: value.alt,
        control: value.ctrl,
    }
}

fn selection_bounds(tab: &Tab) -> Option<((u16, u16), (u16, u16))> {
    let (a, b) = tab.selection?;
    let (mut a, mut b) = if a <= b { (a, b) } else { (b, a) };
    if tab
        .display_screen()
        .cell(a.0, a.1)
        .is_some_and(|cell| cell.is_wide_continuation())
    {
        a.1 = a.1.saturating_sub(1);
    }
    if tab
        .display_screen()
        .cell(b.0, b.1)
        .is_some_and(|cell| cell.is_wide())
    {
        b.1 = b.1.saturating_add(1).min(tab.display_screen().size().1 - 1);
    }
    Some((a, b))
}

fn selection_text(tab: &Tab) -> Option<String> {
    let (a, b) = selection_bounds(tab)?;
    let mut result = String::new();
    for row in a.0..=b.0 {
        let start = if row == a.0 { a.1 } else { 0 };
        let end = if row == b.0 {
            b.1
        } else {
            tab.display_screen().size().1 - 1
        };
        let mut line = String::new();
        for col in start..=end {
            if let Some(cell) = tab.display_screen().cell(row, col) {
                if !cell.is_wide_continuation() {
                    let text = cell.contents();
                    line.push_str(if text.is_empty() { " " } else { &text });
                }
            }
        }
        if row < b.0 && tab.display_screen().row_wrapped(row) {
            result.push_str(&line);
        } else {
            result.push_str(line.trim_end());
            if row < b.0 {
                result.push('\n');
            }
        }
    }
    Some(result)
}

/// The terminal key for a key press. Letters and digits with Ctrl or Alt become
/// characters (text events carry plain typing). With Ctrl, the punctuation
/// that has control codes does too: Ctrl+[ (Escape), Ctrl+\ (quit signal),
/// Ctrl+], Ctrl+Space (NUL) and Ctrl+_ or Ctrl+/ (undo). egui sends no text
/// while Ctrl is held, so these are never typed twice.
pub fn gui_key(key: egui::Key, ctrl: bool, alt: bool) -> Option<Key> {
    if ctrl {
        let punctuation = match key {
            egui::Key::OpenBracket => Some('['),
            egui::Key::Backslash => Some('\\'),
            egui::Key::CloseBracket => Some(']'),
            egui::Key::Space => Some(' '),
            egui::Key::Minus => Some('-'),
            egui::Key::Slash => Some('/'),
            _ => None,
        };
        if let Some(character) = punctuation {
            return Some(Key::Character(character));
        }
    }
    let modified_text = ctrl || alt;
    Some(match key {
        egui::Key::Enter => Key::Enter,
        egui::Key::Tab => Key::Tab,
        egui::Key::Backspace => Key::Backspace,
        egui::Key::Escape => Key::Escape,
        egui::Key::ArrowUp => Key::Up,
        egui::Key::ArrowDown => Key::Down,
        egui::Key::ArrowLeft => Key::Left,
        egui::Key::ArrowRight => Key::Right,
        egui::Key::Home => Key::Home,
        egui::Key::End => Key::End,
        egui::Key::Insert => Key::Insert,
        egui::Key::Delete => Key::Delete,
        egui::Key::PageUp => Key::PageUp,
        egui::Key::PageDown => Key::PageDown,
        egui::Key::F1 => Key::Function(1),
        egui::Key::F2 => Key::Function(2),
        egui::Key::F3 => Key::Function(3),
        egui::Key::F4 => Key::Function(4),
        egui::Key::F5 => Key::Function(5),
        egui::Key::F6 => Key::Function(6),
        egui::Key::F7 => Key::Function(7),
        egui::Key::F8 => Key::Function(8),
        egui::Key::F9 => Key::Function(9),
        egui::Key::F10 => Key::Function(10),
        egui::Key::F11 => Key::Function(11),
        egui::Key::F12 => Key::Function(12),
        _ if modified_text => {
            let name = key.name();
            if name.len() == 1 {
                Key::Character(name.chars().next()?.to_ascii_lowercase())
            } else {
                return None;
            }
        }
        _ => return None,
    })
}

fn color(color: vt100::Color, default: Color32, colors: &neo_ssh::preferences::Colors) -> Color32 {
    match color {
        vt100::Color::Default => default,
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
        vt100::Color::Idx(index) => {
            let [r, g, b] = colors.indexed(index);
            Color32::from_rgb(r, g, b)
        }
    }
}

fn dismiss_palette(ctx: &egui::Context, open: &mut bool) {
    if *open && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        *open = false;
    }
}

#[cfg(test)]
mod interaction_tests {
    #[test]
    fn right_click_copy_menu_uses_frozen_selection_without_terminal_input() {
        let ctx = egui::Context::default();
        let mut tab = Tab::new(Draft::default());
        tab.connected = true;
        frame(&ctx, &mut tab, vec![], egui::Modifiers::NONE, false);
        tab.selection = Some(((0, 0), (0, 2)));
        tab.copied = Some("frozen menu text".into());
        let pos = egui::pos2(40.0, 40.0);
        for pressed in [true, false] {
            frame(
                &ctx,
                &mut tab,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Secondary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                egui::Modifiers::NONE,
                false,
            );
        }
        let output = frame(&ctx, &mut tab, vec![], egui::Modifiers::NONE, false);
        let copy_pos = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == "Copy" => {
                    Some(text.pos + egui::vec2(4.0, 4.0))
                }
                _ => None,
            })
            .expect("Selection context menu did not render Copy");
        tab.terminal.feed(b"replacement output");
        frame(
            &ctx,
            &mut tab,
            vec![
                egui::Event::PointerMoved(copy_pos),
                egui::Event::PointerButton {
                    pos: copy_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            egui::Modifiers::NONE,
            false,
        );
        let output = frame(
            &ctx,
            &mut tab,
            vec![egui::Event::PointerButton {
                pos: copy_pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
            egui::Modifiers::NONE,
            false,
        );
        assert_eq!(output.platform_output.copied_text, "frozen menu text");
        assert!(tab.copied.is_none());
        assert!(tab.selection.is_none());
        assert!(tab.sent_input.is_empty());
        frame(
            &ctx,
            &mut tab,
            vec![
                egui::Event::Ime(egui::ImeEvent::Disabled),
                egui::Event::Text("after-menu".into()),
                egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            egui::Modifiers::NONE,
            false,
        );
        assert_eq!(tab.sent_input, b"after-menu\r");
    }
    #[test]
    fn search_online_menu_keeps_selection_and_restores_keyboard_input() {
        let ctx = egui::Context::default();
        let mut tab = Tab::new(Draft::default());
        tab.connected = true;
        frame(&ctx, &mut tab, vec![], egui::Modifiers::NONE, false);
        tab.selection = Some(((0, 0), (0, 2)));
        tab.copied = Some("frozen menu text".into());
        let pos = egui::pos2(40.0, 40.0);
        for pressed in [true, false] {
            frame(
                &ctx,
                &mut tab,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Secondary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                egui::Modifiers::NONE,
                false,
            );
        }
        let output = frame(&ctx, &mut tab, vec![], egui::Modifiers::NONE, false);
        let copy_pos = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == "Search online" => {
                    Some(text.pos + egui::vec2(4.0, 4.0))
                }
                _ => None,
            })
            .expect("Selection context menu did not render Copy");
        tab.terminal.feed(b"replacement output");
        frame(
            &ctx,
            &mut tab,
            vec![
                egui::Event::PointerMoved(copy_pos),
                egui::Event::PointerButton {
                    pos: copy_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            egui::Modifiers::NONE,
            false,
        );
        let output = frame(
            &ctx,
            &mut tab,
            vec![egui::Event::PointerButton {
                pos: copy_pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
            egui::Modifiers::NONE,
            false,
        );
        assert_eq!(
            output.platform_output.open_url.unwrap().url,
            "https://www.google.com/search?q=frozen+menu+text"
        );
        assert!(tab.copied.is_some());
        assert!(tab.selection.is_some());
        assert!(tab.sent_input.is_empty());
        frame(
            &ctx,
            &mut tab,
            vec![
                egui::Event::Ime(egui::ImeEvent::Disabled),
                egui::Event::Text("after-menu".into()),
                egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            egui::Modifiers::NONE,
            false,
        );
        assert_eq!(tab.sent_input, b"after-menu\r");
    }
    #[test]
    fn copy_uses_frozen_text_clears_selection_and_shift_copy_never_interrupts() {
        let ctx = egui::Context::default();
        let mut tab = Tab::new(Draft::default());
        tab.connected = true;
        frame(&ctx, &mut tab, vec![], egui::Modifiers::NONE, false);
        let pos = egui::pos2(40.0, 40.0);
        for pressed in [true, false] {
            frame(
                &ctx,
                &mut tab,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                egui::Modifiers::NONE,
                false,
            );
        }
        tab.selection = Some(((0, 0), (0, 3)));
        tab.copied = Some("  frozen 界\ntext".into());
        tab.terminal.feed(b"new output");
        let output = frame(
            &ctx,
            &mut tab,
            vec![egui::Event::Copy],
            egui::Modifiers::CTRL,
            false,
        );
        assert_eq!(output.platform_output.copied_text, "  frozen 界\ntext");
        assert!(tab.selection.is_none());
        assert!(tab.copied.is_none());
        assert!(tab.sent_input.is_empty());
        let shift = egui::Modifiers {
            ctrl: true,
            command: true,
            shift: true,
            ..Default::default()
        };
        frame(
            &ctx,
            &mut tab,
            vec![
                egui::Event::Copy,
                egui::Event::Key {
                    key: egui::Key::C,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: shift,
                },
            ],
            shift,
            false,
        );
        assert!(tab.sent_input.is_empty());
        frame(
            &ctx,
            &mut tab,
            vec![egui::Event::Copy],
            egui::Modifiers::CTRL,
            false,
        );
        assert_eq!(tab.sent_input, [3]);
    }

    #[test]
    fn clipboard_shortcut_uses_press_modifiers_after_shift_release() {
        let ctx = egui::Context::default();
        let mut tab = Tab::new(Draft::default());
        tab.connected = true;
        frame(&ctx, &mut tab, vec![], egui::Modifiers::NONE, false);
        let pos = egui::pos2(40.0, 40.0);
        for pressed in [true, false] {
            frame(
                &ctx,
                &mut tab,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                egui::Modifiers::NONE,
                false,
            );
        }
        let shift = egui::Modifiers {
            ctrl: true,
            command: true,
            shift: true,
            ..Default::default()
        };
        let shortcut = |key, modifiers| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        frame(
            &ctx,
            &mut tab,
            vec![
                shortcut(egui::Key::V, shift),
                egui::Event::Paste("first\nsecond".into()),
            ],
            egui::Modifiers::NONE,
            false,
        );
        assert_eq!(tab.sent_input, b"first\nsecond");
        tab.sent_input.clear();
        frame(
            &ctx,
            &mut tab,
            vec![shortcut(egui::Key::C, shift), egui::Event::Copy],
            egui::Modifiers::NONE,
            false,
        );
        assert!(tab.sent_input.is_empty(), "Copy-only sent an interrupt");
        frame(
            &ctx,
            &mut tab,
            vec![
                shortcut(egui::Key::C, egui::Modifiers::COMMAND),
                egui::Event::Copy,
            ],
            shift,
            false,
        );
        assert_eq!(tab.sent_input, [3], "Later Shift press suppressed Ctrl+C");
        tab.sent_input.clear();
        frame(
            &ctx,
            &mut tab,
            vec![
                shortcut(egui::Key::V, egui::Modifiers::COMMAND),
                egui::Event::Paste("review\nrequired".into()),
            ],
            shift,
            false,
        );
        assert!(
            tab.sent_input.is_empty(),
            "Later Shift press bypassed paste review"
        );
    }

    #[test]
    fn paste_review_preserves_crlf_and_force_paste_wraps_exactly_once() {
        let mut tab = Tab::new(Draft::default());
        tab.terminal.feed(b"\x1b[?2004h");
        for text in ["a\nb", "a\r\nb", "a\rb", "trailing\n"] {
            assert_eq!(
                paste_text(&mut tab, text.into(), false).as_deref(),
                Some(text)
            );
            assert!(tab.sent_input.is_empty());
        }
        let text = "  界\r\n\t end ";
        assert!(paste_text(&mut tab, text.into(), true).is_none());
        assert_eq!(tab.sent_input, tab.terminal.paste(text));
        tab.sent_input.clear();
        assert!(paste_text(&mut tab, String::new(), false).is_none());
        assert!(tab.sent_input.is_empty());
    }
    #[test]
    fn sync_reencodes_keys_and_pastes_but_never_broadcasts_protocol_replies() {
        use super::*;
        let mut tabs = (0..3)
            .map(|_| {
                let mut tab = Tab::new(Draft::default());
                tab.connected = true;
                tab
            })
            .collect::<Vec<_>>();
        tabs[2].connected = false;
        tabs[1].terminal.feed(b"\x1b[?1h\x1b[?2004h");
        tabs[0].capture_input = true;
        tabs[0].send(b"protocol reply".to_vec());
        assert!(tabs[0].user_input.is_empty());
        tabs[0].sent_input.clear();
        tabs[0].send_user(UserInput::Key(Key::Up, Modifiers::default()));
        tabs[0].send_user(UserInput::Paste(Zeroizing::new("hello".into())));
        for input in std::mem::take(&mut tabs[0].user_input) {
            broadcast_input(&mut tabs, 0, &input);
        }
        assert_eq!(tabs[0].sent_input, b"\x1b[Ahello");
        assert_eq!(tabs[1].sent_input, b"\x1bOA\x1b[200~hello\x1b[201~");
        assert!(tabs[2].sent_input.is_empty());
        assert!(tabs[1].user_input.is_empty());
    }

    #[test]
    fn paste_review_does_not_expand_to_new_connections_or_reconnected_sessions() {
        use super::*;
        let mut tabs = (0..4)
            .map(|_| {
                let mut tab = Tab::new(Draft::default());
                tab.connected = true;
                tab
            })
            .collect::<Vec<_>>();
        tabs[2].connected = false;
        let review = PasteReview {
            text: "hello\nworld".into(),
            recipients: input_recipients(&tabs, 0, true),
        };
        tabs[1].generation += 1;
        tabs[2].connected = true;
        tabs[3].connected = false;
        deliver_review(&mut tabs, review);
        assert_eq!(tabs[0].sent_input, tabs[0].terminal.paste("hello\nworld"));
        for tab in &tabs[1..] {
            assert!(tab.sent_input.is_empty());
        }
        assert_eq!(input_recipients(&tabs, 0, false), vec![(0, 0)]);
    }
    #[test]
    fn tab_colors_survive_workspace_round_trip_and_unknown_colors_remain_readable() {
        use super::*;
        let mut tab = Tab::new(Draft {
            target: "example.test".into(),
            ..Default::default()
        });
        tab.color = "amber".into();
        let snapshot = snapshot_tabs(&[tab], 0);
        let mut config = Config::parse("").unwrap();
        snapshot.store(&mut config).unwrap();
        let restored = Snapshot::load(&config).unwrap();
        assert_eq!(restored.tabs[0].tab_color, "amber");
        assert_eq!(tab_color("unknown-future-color"), INK);
        assert_eq!(tab_color(""), INK);
    }
    #[test]
    fn closing_tabs_preserves_connection_identity_and_never_leaves_an_empty_workspace() {
        use super::*;
        let make = || {
            (0..4)
                .map(|index| {
                    Tab::new(Draft {
                        target: format!("host-{index}"),
                        ..Default::default()
                    })
                })
                .collect::<Vec<_>>()
        };
        let mut tabs = make();
        let mut active = 2;
        close_tabs(&mut tabs, &mut active, CloseTabs::One(0));
        assert_eq!(active, 1);
        assert_eq!(tabs[active].draft.target, "host-2");
        let snapshot = snapshot_tabs(&tabs, active);
        assert_eq!(snapshot.tabs[snapshot.active].target, "host-2");
        close_tabs(&mut tabs, &mut active, CloseTabs::Right(0));
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[active].draft.target, "host-1");
        close_tabs(&mut tabs, &mut active, CloseTabs::One(0));
        assert_eq!(tabs.len(), 1);
        assert_eq!(active, 0);
        assert!(tabs[0].draft.target.is_empty());
        let mut tabs = make();
        active = 3;
        close_tabs(&mut tabs, &mut active, CloseTabs::Others(1));
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs[active].draft.target, "host-1");
        let mut tabs = make();
        active = 1;
        close_tabs(&mut tabs, &mut active, CloseTabs::One(1));
        assert_eq!(tabs[active].draft.target, "host-2");
    }

    #[test]
    fn tab_search_matches_labels_targets_users_and_profile_names() {
        use super::*;
        let tab = Tab::new(Draft {
            name: "Production".into(),
            target: "edge.example:2222".into(),
            user: "Ops".into(),
            original_name: "Saved route".into(),
            ..Default::default()
        });
        for query in ["prod", " EXAMPLE ", "ops", "route", ""] {
            assert!(tab_matches(&tab, query));
        }
        assert!(!tab_matches(&tab, "missing"));
    }
    use super::*;
    #[test]
    fn palette_escape_is_consumed_only_when_palette_is_open() {
        for initially_open in [true, false] {
            let ctx = egui::Context::default();
            let mut open = initially_open;
            let _ = ctx.run(
                egui::RawInput {
                    events: vec![egui::Event::Key {
                        key: egui::Key::Escape,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                    ..Default::default()
                },
                |ctx| {
                    dismiss_palette(ctx, &mut open);
                    assert!(!open);
                    assert_eq!(
                        ctx.input(|i| i.key_pressed(egui::Key::Escape)),
                        !initially_open
                    );
                },
            );
        }
    }
    #[test]
    fn manual_disconnect_returns_to_form_without_losing_identity_or_previous_output() {
        let mut tab = Tab::new(Draft {
            target: "alice@example:2222".into(),
            original_name: "work".into(),
            name: "work".into(),
            tmux: "daily".into(),
            password: Zeroizing::new("synthetic-secret".into()),
            ..Default::default()
        });
        tab.terminal.feed(b"previous output");
        tab.connected = true;
        tab.review_disconnected_output = true;
        tab.reconnect_at = Some(std::time::Instant::now());
        tab.last_error = Some("previous failure".into());
        tab.disconnect();
        assert!(tab.connection_form_visible());
        assert!(!tab.connected);
        assert!(tab.reconnect_at.is_none());
        assert!(tab.draft.password.is_empty());
        assert_eq!(tab.draft.target, "alice@example:2222");
        assert_eq!(tab.draft.original_name, "work");
        assert_eq!(tab.draft.tmux, "daily");
        assert_eq!(tab.last_error.as_deref(), Some("previous failure"));
        assert_eq!(tab.terminal.screen().contents(), "previous output");
        tab.review_disconnected_output = true;
        assert!(!tab.connection_form_visible());
        assert_eq!(tab.display_screen().contents(), "previous output");
        tab.review_disconnected_output = false;
        assert!(tab.connection_form_visible());
    }

    #[test]
    fn dropped_session_retry_is_cancellable_single_use_and_keeps_screen_and_error() {
        let mut tab = Tab::new(Draft::default());
        tab.draft
            .options
            .extensions
            .insert("reconnect_delay_secs".into(), toml::Value::Integer(5));
        tab.terminal.feed(b"previous output");
        tab.connected = true;
        let now = std::time::Instant::now();
        tab.review_disconnected_output = true;
        tab.failed("transport ended".into(), now);
        assert!(tab.connection_form_visible());
        assert!(!tab.retry_due(now + Duration::from_secs(4)));
        assert!(tab.retry_due(now + Duration::from_secs(5)));
        assert!(!tab.retry_due(now + Duration::from_secs(6)));
        tab.record("Connecting".into());
        assert_eq!(tab.last_error.as_deref(), Some("transport ended"));
        assert_eq!(tab.terminal.screen().contents(), "previous output");
        tab.failed("authentication rejected".into(), now);
        assert!(
            tab.reconnect_at.is_none(),
            "Failed authentication must not create a retry loop"
        );
        tab.connected = true;
        tab.failed("transport ended".into(), now);
        tab.reconnect_at = None;
        assert!(!tab.retry_due(now + Duration::from_secs(10)));
        for index in 0..250 {
            tab.record(format!("phase {index}"));
        }
        assert_eq!(tab.timeline.len(), 200);
        assert_eq!(tab.timeline.back().unwrap().1, "phase 249");
    }
    #[test]
    fn workspace_omits_blank_tabs_maps_active_index_and_never_stores_password() {
        let mut first = Draft {
            target: "alice@server:2222".into(),
            name: "Work".into(),
            original_name: "production".into(),
            tmux: "daily".into(),
            remote_directory: "/work/a ' ; $(literal)".into(),
            ..Default::default()
        };
        first.password = Zeroizing::new("fixture-secret".into());
        first.options.remote_env = "LANG=en_US.UTF-8".into();
        let tabs = vec![
            Tab::new(Draft::default()),
            Tab::new(first),
            Tab::new(Draft::default()),
            Tab::new(Draft {
                target: "second".into(),
                ..Default::default()
            }),
        ];
        let snapshot = snapshot_tabs(&tabs, 2);
        assert_eq!(snapshot.active, 1);
        assert_eq!(snapshot.tabs.len(), 2);
        assert_eq!(snapshot.tabs[0].profile_name, "production");
        assert_eq!(snapshot.tabs[0].connection.tmux_session, "daily");
        assert_eq!(snapshot.tabs[0].remote_directory, "/work/a ' ; $(literal)");
        assert_eq!(snapshot.tabs[0].connection.port, 2222);
        let mut config = Config::default();
        snapshot.store(&mut config).unwrap();
        assert!(!config.encode().unwrap().contains("fixture-secret"));
        assert_eq!(
            Snapshot::load(&config).unwrap().tabs[0].remote_directory,
            "/work/a ' ; $(literal)"
        );
        assert_eq!(
            Snapshot::load(&config).unwrap().tabs[0].target,
            "alice@server:2222"
        );
        assert_eq!(snapshot_tabs(&tabs, 1).active, 0);
    }

    #[test]
    fn search_snapshot_and_copy_stay_stable_while_live_output_continues() {
        let mut tab = Tab::new(Draft::default());
        tab.terminal.feed(b"needle");
        tab.search_open = true;
        tab.search_query = "needle".into();
        tab.start_search();
        tab.terminal.feed(b"\r\x1b[2Kchanged");
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while tab.search_results.is_none() {
            assert!(std::time::Instant::now() < deadline);
            tab.poll(std::time::Instant::now() + Duration::from_secs(1));
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(tab.search_results.as_ref().unwrap().matches.len(), 1);
        tab.selection = Some(((0, 0), (0, 5)));
        assert_eq!(selection_text(&tab).as_deref(), Some("needle"));
        assert!(tab.terminal.screen().contents().starts_with("changed"));
        tab.close_search();
        assert!(tab.display_screen().contents().starts_with("changed"));
        assert_eq!(tab.scroll, 0);
    }

    #[test]
    fn selection_copies_whole_wide_cells_and_preserves_logical_lines() {
        let mut tab = Tab::new(Draft::default());
        tab.size = (4, 3);
        tab.terminal = Terminal::new(3, 4, 10);
        tab.terminal.feed("界abCD\r\nEF".as_bytes());
        tab.selection = Some(((0, 1), (0, 1)));
        assert_eq!(selection_text(&tab).as_deref(), Some("界"));
        tab.selection = Some(((2, 1), (0, 1)));
        assert_eq!(selection_text(&tab).as_deref(), Some("界abCD\nEF"));
    }

    #[test]
    fn wide_glyphs_are_drawn_after_backgrounds_and_keep_text_attributes() {
        let ctx = egui::Context::default();
        let mut tab = Tab::new(Draft::default());
        tab.terminal
            .feed("\x1b[?25l\x1b[1;3;4m界\x1b[0mX".as_bytes());
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(640.0, 480.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    terminal_view(ui, &mut tab, 14.0, false);
                });
            },
        );
        let mut text_seen = false;
        let mut wide_glyphs = 0;
        let mut underlines = 0;
        for shape in output.shapes {
            match shape.shape {
                egui::Shape::Text(text) => {
                    text_seen = true;
                    if text.galley.job.text == "界" {
                        wide_glyphs += 1;
                        assert!(text.galley.job.sections[0].format.italics);
                    }
                }
                egui::Shape::Rect(_) => assert!(!text_seen, "Cell background overwrote a glyph"),
                egui::Shape::LineSegment { .. } => underlines += 1,
                _ => {}
            }
        }
        assert_eq!(
            wide_glyphs, 2,
            "Bold should add weight without a continuation glyph"
        );
        assert_eq!(underlines, 1);
    }

    fn frame(
        ctx: &egui::Context,
        tab: &mut Tab,
        events: Vec<egui::Event>,
        modifiers: egui::Modifiers,
        modal: bool,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(640.0, 480.0),
                )),
                events,
                modifiers,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    terminal_view(ui, tab, 14.0, modal);
                });
            },
        )
    }

    #[test]
    fn terminal_navigation_keys_keep_focus_and_reach_remote() {
        let ctx = egui::Context::default();
        let mut tab = Tab::new(Draft::default());
        tab.connected = true;
        frame(&ctx, &mut tab, vec![], egui::Modifiers::NONE, false);
        let pos = egui::pos2(40.0, 40.0);
        for pressed in [true, false] {
            frame(
                &ctx,
                &mut tab,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                egui::Modifiers::NONE,
                false,
            );
        }
        frame(&ctx, &mut tab, vec![], egui::Modifiers::NONE, false);
        for key in [
            egui::Key::Escape,
            egui::Key::Tab,
            egui::Key::ArrowUp,
            egui::Key::ArrowDown,
            egui::Key::ArrowLeft,
            egui::Key::ArrowRight,
        ] {
            frame(
                &ctx,
                &mut tab,
                vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                egui::Modifiers::NONE,
                false,
            );
            assert!(
                ctx.memory(|memory| memory.focused().is_some()),
                "Lost focus on {key:?}"
            );
        }
        frame(
            &ctx,
            &mut tab,
            vec![egui::Event::Text(":wq".into())],
            egui::Modifiers::NONE,
            false,
        );
        assert_eq!(tab.sent_input, b"\x1b\t\x1b[A\x1b[B\x1b[D\x1b[C:wq");
    }

    #[test]
    fn drag_selection_starts_at_press_origin_before_drag_threshold() {
        let ctx = egui::Context::default();
        let mut tab = Tab::new(Draft::default());
        tab.terminal.feed(b"first");
        frame(&ctx, &mut tab, vec![], egui::Modifiers::NONE, false);
        let start = egui::pos2(9.0, 14.0);
        frame(
            &ctx,
            &mut tab,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            egui::Modifiers::NONE,
            false,
        );
        for x in [20.0, 30.0, 43.0] {
            frame(
                &ctx,
                &mut tab,
                vec![egui::Event::PointerMoved(egui::pos2(x, 14.0))],
                egui::Modifiers::NONE,
                false,
            );
        }
        frame(
            &ctx,
            &mut tab,
            vec![egui::Event::PointerButton {
                pos: egui::pos2(43.0, 14.0),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
            egui::Modifiers::NONE,
            false,
        );
        assert_eq!(tab.copied.as_deref(), Some("first"));
    }

    #[test]
    fn shift_selection_finishes_locally_when_shift_and_mouse_release_together() {
        let ctx = egui::Context::default();
        let mut tab = Tab::new(Draft::default());
        tab.connected = true;
        tab.terminal.feed(b"first\x1b[?1003h\x1b[?1006h");
        frame(&ctx, &mut tab, vec![], egui::Modifiers::NONE, false);
        let start = egui::pos2(9.0, 14.0);
        frame(
            &ctx,
            &mut tab,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::SHIFT,
                },
            ],
            egui::Modifiers::SHIFT,
            false,
        );
        for x in [20.0, 30.0] {
            frame(
                &ctx,
                &mut tab,
                vec![egui::Event::PointerMoved(egui::pos2(x, 14.0))],
                egui::Modifiers::SHIFT,
                false,
            );
        }
        let output = frame(
            &ctx,
            &mut tab,
            vec![
                egui::Event::PointerMoved(egui::pos2(43.0, 14.0)),
                egui::Event::PointerButton {
                    pos: egui::pos2(43.0, 14.0),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::SHIFT,
                },
            ],
            egui::Modifiers::NONE,
            false,
        );
        assert_eq!(tab.copied.as_deref(), Some("first"));
        assert_eq!(output.platform_output.copied_text, "first");
        assert!(tab.sent_input.is_empty(), "Local drag leaked mouse input");
        assert!(!tab.selecting);
        frame(
            &ctx,
            &mut tab,
            vec![egui::Event::PointerMoved(start)],
            egui::Modifiers::NONE,
            false,
        );
        assert!(
            !tab.sent_input.is_empty(),
            "Remote hover did not resume after local drag"
        );
    }

    #[test]
    fn drag_selection_includes_final_motion_batched_with_release() {
        let ctx = egui::Context::default();
        let mut tab = Tab::new(Draft::default());
        tab.terminal.feed(b"first");
        frame(&ctx, &mut tab, vec![], egui::Modifiers::NONE, false);
        let start = egui::pos2(9.0, 14.0);
        frame(
            &ctx,
            &mut tab,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            egui::Modifiers::NONE,
            false,
        );
        for x in [20.0, 30.0] {
            frame(
                &ctx,
                &mut tab,
                vec![egui::Event::PointerMoved(egui::pos2(x, 14.0))],
                egui::Modifiers::NONE,
                false,
            );
        }
        frame(
            &ctx,
            &mut tab,
            vec![
                egui::Event::PointerMoved(egui::pos2(43.0, 14.0)),
                egui::Event::PointerButton {
                    pos: egui::pos2(43.0, 14.0),
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            egui::Modifiers::NONE,
            false,
        );
        assert_eq!(tab.copied.as_deref(), Some("first"));
    }

    #[test]
    fn disconnected_output_review_never_captures_input_for_sync() {
        let ctx = egui::Context::default();
        let mut tab = Tab::new(Draft::default());
        tab.terminal.feed(b"retained output");
        tab.review_disconnected_output = true;
        tab.capture_input = true;
        frame(&ctx, &mut tab, vec![], egui::Modifiers::NONE, false);
        let pos = egui::pos2(400.0, 300.0);
        for pressed in [true, false] {
            frame(
                &ctx,
                &mut tab,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                egui::Modifiers::NONE,
                false,
            );
        }
        assert!(ctx.memory(|memory| memory.focused().is_some()));
        let before = tab.terminal.screen().contents();
        for event in [
            egui::Event::Text("must remain local".into()),
            egui::Event::Paste("single line".into()),
            egui::Event::Paste("multiple\nlines".into()),
            egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::Ime(egui::ImeEvent::Enabled),
            egui::Event::Ime(egui::ImeEvent::Preedit("draft".into())),
            egui::Event::Ime(egui::ImeEvent::Commit("commit".into())),
            egui::Event::Ime(egui::ImeEvent::Disabled),
        ] {
            let output = frame(&ctx, &mut tab, vec![event], egui::Modifiers::NONE, false);
            assert!(output.platform_output.ime.is_none());
            assert!(tab.sent_input.is_empty());
            assert!(
                tab.user_input.is_empty(),
                "Disconnected input would reach synced tabs"
            );
        }
        assert_eq!(tab.terminal.screen().contents(), before);
        assert!(tab.ime_preedit.is_empty());
        // Positive control: the same focused renderer sends and captures connected input.
        tab.connected = true;
        frame(
            &ctx,
            &mut tab,
            vec![egui::Event::Text("connected".into())],
            egui::Modifiers::NONE,
            false,
        );
        assert_eq!(tab.sent_input, b"connected");
        assert_eq!(tab.user_input.len(), 1);
    }

    #[test]
    fn explicit_hyperlink_click_opens_destination_instead_of_label() {
        let ctx = egui::Context::default();
        let mut tab = Tab::new(Draft::default());
        tab.terminal.feed(
            b"\x1b]8;;https://destination.example/path\x1b\\https://label.example\x1b]8;;\x1b\\",
        );
        frame(&ctx, &mut tab, vec![], egui::Modifiers::NONE, false);
        let pos = egui::pos2(40.0, 14.0);
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::CTRL,
        };
        frame(
            &ctx,
            &mut tab,
            vec![egui::Event::PointerMoved(pos), button(true)],
            egui::Modifiers::CTRL,
            false,
        );
        let output = frame(
            &ctx,
            &mut tab,
            vec![button(false)],
            egui::Modifiers::CTRL,
            false,
        );
        assert_eq!(
            output.platform_output.open_url.unwrap().url,
            "https://destination.example/path"
        );
    }

    #[test]
    fn ime_preedit_stays_local_commit_is_once_and_modal_discards_late_commit() {
        let ctx = egui::Context::default();
        let mut tab = Tab::new(Draft::default());
        tab.connected = true;
        frame(&ctx, &mut tab, vec![], egui::Modifiers::NONE, false);
        let pos = egui::pos2(40.0, 14.0);
        for pressed in [true, false] {
            frame(
                &ctx,
                &mut tab,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                egui::Modifiers::NONE,
                false,
            );
        }
        let ime = |event| egui::Event::Ime(event);
        let output = frame(
            &ctx,
            &mut tab,
            vec![
                ime(egui::ImeEvent::Enabled),
                ime(egui::ImeEvent::Preedit("にほん".into())),
                egui::Event::Text("n".into()),
            ],
            egui::Modifiers::NONE,
            false,
        );
        assert!(output.platform_output.ime.is_some());
        assert_eq!(&*tab.ime_preedit, "にほん");
        assert!(tab.sent_input.is_empty());
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "にほん")));
        frame(
            &ctx,
            &mut tab,
            vec![
                ime(egui::ImeEvent::Commit("日本".into())),
                ime(egui::ImeEvent::Disabled),
                egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            egui::Modifiers::NONE,
            false,
        );
        assert_eq!(tab.sent_input, "日本".as_bytes());
        assert!(tab.ime_preedit.is_empty());
        assert!(tab.terminal.screen().contents().is_empty());
        frame(
            &ctx,
            &mut tab,
            vec![
                ime(egui::ImeEvent::Enabled),
                ime(egui::ImeEvent::Preedit("draft".into())),
            ],
            egui::Modifiers::NONE,
            false,
        );
        let output = frame(&ctx, &mut tab, vec![], egui::Modifiers::NONE, true);
        assert!(output.platform_output.ime.is_none());
        assert!(tab.ime_preedit.is_empty());
        frame(
            &ctx,
            &mut tab,
            vec![
                ime(egui::ImeEvent::Commit("discard".into())),
                ime(egui::ImeEvent::Disabled),
            ],
            egui::Modifiers::NONE,
            false,
        );
        assert_eq!(tab.sent_input, "日本".as_bytes());
    }

    #[test]
    fn ctrl_click_links_open_once_without_remote_capture_and_cancel_on_drag_or_modal() {
        let ctx = egui::Context::default();
        let mut tab = Tab::new(Draft::default());
        tab.connected = true;
        tab.terminal
            .feed(b"https://example.org/a\x1b[?1002h\x1b[?1006h");
        frame(&ctx, &mut tab, vec![], egui::Modifiers::NONE, false);
        let pos = egui::pos2(40.0, 14.0);
        let press = |pressed, modifiers| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        };
        frame(
            &ctx,
            &mut tab,
            vec![
                egui::Event::PointerMoved(pos),
                press(true, egui::Modifiers::CTRL),
            ],
            egui::Modifiers::CTRL,
            false,
        );
        assert!(tab.link_press.is_some());
        assert!(!tab.mouse.captured());
        let output = frame(
            &ctx,
            &mut tab,
            vec![press(false, egui::Modifiers::CTRL)],
            egui::Modifiers::CTRL,
            false,
        );
        assert_eq!(
            output.platform_output.open_url.unwrap().url,
            "https://example.org/a"
        );
        assert!(tab.selection.is_none());
        assert!(frame(&ctx, &mut tab, vec![], egui::Modifiers::CTRL, false)
            .platform_output
            .open_url
            .is_none());

        frame(
            &ctx,
            &mut tab,
            vec![press(true, egui::Modifiers::CTRL)],
            egui::Modifiers::CTRL,
            false,
        );
        frame(
            &ctx,
            &mut tab,
            vec![egui::Event::PointerMoved(pos + Vec2::new(30.0, 0.0))],
            egui::Modifiers::CTRL,
            false,
        );
        assert!(frame(
            &ctx,
            &mut tab,
            vec![press(false, egui::Modifiers::CTRL)],
            egui::Modifiers::CTRL,
            false
        )
        .platform_output
        .open_url
        .is_none());
        frame(
            &ctx,
            &mut tab,
            vec![press(true, egui::Modifiers::CTRL)],
            egui::Modifiers::CTRL,
            false,
        );
        frame(&ctx, &mut tab, vec![], egui::Modifiers::CTRL, true);
        assert!(tab.link_press.is_none());
        assert!(frame(
            &ctx,
            &mut tab,
            vec![press(false, egui::Modifiers::CTRL)],
            egui::Modifiers::CTRL,
            false
        )
        .platform_output
        .open_url
        .is_none());
        frame(
            &ctx,
            &mut tab,
            vec![press(true, egui::Modifiers::NONE)],
            egui::Modifiers::NONE,
            false,
        );
        assert!(
            tab.mouse.captured(),
            "Unmodified click should reach the remote terminal"
        );
    }

    #[test]
    fn ctrl_click_remote_path_requests_file_browser_without_browser_or_remote_input() {
        let ctx = egui::Context::default();
        let mut tab = Tab::new(Draft::default());
        tab.connected = true;
        tab.terminal.feed(b"/tmp/fixture.txt\x1b[?1002h\x1b[?1006h");
        frame(&ctx, &mut tab, vec![], egui::Modifiers::NONE, false);
        let pos = egui::pos2(40.0, 14.0);
        for pressed in [true, false] {
            let output = frame(
                &ctx,
                &mut tab,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::CTRL,
                    },
                ],
                egui::Modifiers::CTRL,
                false,
            );
            assert!(output.platform_output.open_url.is_none());
        }
        assert_eq!(tab.pending_remote_path.as_deref(), Some("/tmp/fixture.txt"));
        assert!(tab.sent_input.is_empty());
        assert!(!tab.mouse.captured());
    }

    #[test]
    fn gui_mouse_capture_survives_shift_release_and_is_cleared_by_modal() {
        let ctx = egui::Context::default();
        let mut tab = Tab::new(Draft::default());
        tab.connected = true;
        tab.terminal.feed(b"\x1b[?1002h\x1b[?1006h");
        frame(&ctx, &mut tab, vec![], egui::Modifiers::NONE, false);
        let pos = egui::pos2(40.0, 40.0);
        let press = |pressed, modifiers| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        };
        frame(
            &ctx,
            &mut tab,
            vec![
                egui::Event::PointerMoved(pos),
                press(true, egui::Modifiers::NONE),
            ],
            egui::Modifiers::NONE,
            false,
        );
        assert!(tab.mouse.captured());
        assert!(tab.selection.is_none());
        frame(
            &ctx,
            &mut tab,
            vec![press(false, egui::Modifiers::SHIFT)],
            egui::Modifiers::SHIFT,
            false,
        );
        assert!(!tab.mouse.captured());
        frame(
            &ctx,
            &mut tab,
            vec![press(true, egui::Modifiers::NONE)],
            egui::Modifiers::NONE,
            false,
        );
        assert!(tab.mouse.captured());
        frame(&ctx, &mut tab, vec![], egui::Modifiers::NONE, true);
        assert!(!tab.mouse.captured());
    }

    #[test]
    fn bracket_highlights_use_two_cells_and_yield_to_selection_and_modals() {
        let ctx = egui::Context::default();
        let mut tab = Tab::new(Draft::default());
        tab.terminal.feed(b"(abc)");
        let count = |output: egui::FullOutput| {
            output
                .shapes
                .iter()
                .filter(
                    |shape| matches!(&shape.shape, egui::Shape::Rect(rect) if rect.fill == BRACKET),
                )
                .count()
        };
        assert_eq!(
            count(frame(&ctx, &mut tab, vec![], egui::Modifiers::NONE, false)),
            2
        );
        tab.selection = Some(((0, 0), (0, 4)));
        assert_eq!(
            count(frame(&ctx, &mut tab, vec![], egui::Modifiers::NONE, false)),
            0
        );
        tab.selection = None;
        assert_eq!(
            count(frame(&ctx, &mut tab, vec![], egui::Modifiers::NONE, true)),
            0
        );
    }

    #[test]
    fn app_icon_decodes_at_full_resolution() {
        let icon = app_icon();
        assert_eq!((icon.width, icon.height), (256, 256));
        assert_eq!(icon.rgba.len(), 256 * 256 * 4);
        // Transparent corners, opaque centre.
        assert_eq!(icon.rgba[3], 0);
        assert_eq!(icon.rgba[(128 * 256 + 128) * 4 + 3], 255);
    }

    #[test]
    fn terminal_focus_moves_to_a_newly_connected_session_without_a_click() {
        let ctx = egui::Context::default();
        let mut tab = Tab::new(Draft::default());
        tab.connected = true;
        tab.focus_terminal = true;
        // A dialog still open when the session connects keeps the request pending.
        frame(&ctx, &mut tab, vec![], egui::Modifiers::NONE, true);
        assert!(tab.focus_terminal);
        assert!(ctx.memory(|memory| memory.focused().is_none()));
        frame(&ctx, &mut tab, vec![], egui::Modifiers::NONE, false);
        assert!(!tab.focus_terminal);
        frame(
            &ctx,
            &mut tab,
            vec![egui::Event::Text("ls".into())],
            egui::Modifiers::NONE,
            false,
        );
        assert_eq!(tab.sent_input, b"ls");
    }

    #[test]
    fn terminal_focus_survives_status_rows_appearing_above_the_terminal() {
        let ctx = egui::Context::default();
        let mut tab = focused_tab(&ctx);
        // Connecting adds the Remote files button above the terminal.
        for _ in 0..2 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(640.0, 480.0),
                    )),
                    events: vec![egui::Event::Text("pwd".into())],
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let _ = ui.button("Remote files");
                        terminal_view(ui, &mut tab, 14.0, false);
                    });
                },
            );
        }
        assert_eq!(tab.sent_input, b"pwdpwd");
    }

    fn focused_tab(ctx: &egui::Context) -> Tab {
        let mut tab = Tab::new(Draft::default());
        tab.connected = true;
        frame(ctx, &mut tab, vec![], egui::Modifiers::NONE, false);
        let pos = egui::pos2(40.0, 40.0);
        for pressed in [true, false] {
            frame(
                ctx,
                &mut tab,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                egui::Modifiers::NONE,
                false,
            );
        }
        tab
    }

    fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    const CTRL: egui::Modifiers = egui::Modifiers {
        alt: false,
        ctrl: true,
        shift: false,
        mac_cmd: false,
        command: true,
    };
    const CTRL_SHIFT: egui::Modifiers = CTRL.plus(egui::Modifiers::SHIFT);

    #[test]
    fn ctrl_shift_i_requests_clipboard_image_upload_without_reaching_the_remote() {
        let ctx = egui::Context::default();
        let mut tab = focused_tab(&ctx);
        frame(
            &ctx,
            &mut tab,
            vec![key(egui::Key::I, CTRL_SHIFT)],
            CTRL_SHIFT,
            false,
        );
        assert!(tab.clipboard_image_request);
        assert!(tab.sent_input.is_empty());
    }

    #[test]
    fn ctrl_letters_and_punctuation_reach_the_remote() {
        let ctx = egui::Context::default();
        let mut tab = focused_tab(&ctx);
        let shift = egui::Modifiers::SHIFT;
        frame(
            &ctx,
            &mut tab,
            vec![
                // Readline/nano/emacs keys that used to be app shortcuts.
                key(egui::Key::N, CTRL),
                key(egui::Key::K, CTRL),
                key(egui::Key::F, CTRL),
                key(egui::Key::P, CTRL),
                // Ctrl+X and Shift+Delete arrive as a key, then Cut.
                key(egui::Key::X, CTRL),
                egui::Event::Cut,
                key(egui::Key::Delete, shift),
                egui::Event::Cut,
                key(egui::Key::OpenBracket, CTRL),
                key(egui::Key::Backslash, CTRL),
                key(egui::Key::CloseBracket, CTRL),
                key(egui::Key::Space, CTRL),
                key(egui::Key::Minus, CTRL_SHIFT),
                key(egui::Key::Slash, CTRL),
            ],
            CTRL,
            false,
        );
        assert_eq!(
            tab.sent_input,
            [
                b"\x0e\x0b\x06\x10\x18\x1b[3;2~".as_slice(),
                b"\x1b\x1c\x1d\x00\x1f\x1f"
            ]
            .concat()
        );
        assert!(!tab.clipboard_image_request);
    }

    fn take_shortcuts(events: Vec<egui::Event>) -> (Vec<AppShortcut>, usize) {
        let ctx = egui::Context::default();
        let (mut taken, mut left) = (Vec::new(), 0);
        let _ = ctx.run(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ctx| {
                taken = take_app_shortcuts(ctx);
                left = ctx.input(|input| input.events.len());
            },
        );
        (taken, left)
    }

    #[test]
    fn app_shortcuts_use_ctrl_shift_and_leave_ctrl_letters_to_the_terminal() {
        // Plain Ctrl letters, Ctrl+_ and AltGr-style chords stay with the terminal.
        let (taken, left) = take_shortcuts(vec![
            key(egui::Key::N, CTRL),
            key(egui::Key::K, CTRL),
            key(egui::Key::F, CTRL),
            key(egui::Key::P, CTRL),
            key(egui::Key::T, CTRL),
            key(egui::Key::Minus, CTRL_SHIFT),
            key(egui::Key::T, CTRL_SHIFT.plus(egui::Modifiers::ALT)),
        ]);
        assert_eq!((taken, left), (vec![], 7));
        let (taken, left) = take_shortcuts(vec![
            key(egui::Key::T, CTRL_SHIFT),
            key(egui::Key::P, CTRL_SHIFT),
            key(egui::Key::F, CTRL_SHIFT),
            key(egui::Key::F11, egui::Modifiers::NONE),
            key(egui::Key::Equals, CTRL),
            key(egui::Key::Plus, CTRL_SHIFT),
            key(egui::Key::Minus, CTRL),
            key(egui::Key::Num0, CTRL),
        ]);
        assert_eq!(
            taken,
            [
                AppShortcut::NewTab,
                AppShortcut::Palette,
                AppShortcut::Find,
                AppShortcut::FocusMode,
                AppShortcut::LargerText,
                AppShortcut::LargerText,
                AppShortcut::SmallerText,
                AppShortcut::ResetText,
            ]
        );
        assert_eq!(left, 0);
    }

    #[test]
    fn shortcut_help_lists_each_chord_once() {
        let mut seen = std::collections::HashSet::new();
        for (keys, _) in SHORTCUTS {
            for chord in keys.split(" / ") {
                assert!(seen.insert(chord), "{chord} is listed twice");
            }
        }
        for chord in ["Ctrl/Cmd+Shift+T", "Ctrl/Cmd+Shift+P", "Ctrl/Cmd+Shift+F"] {
            assert!(seen.contains(chord), "{chord} is missing");
        }
    }

    #[cfg(windows)]
    #[test]
    fn insert_chords_copy_without_interrupt_and_paste_with_review() {
        let ctx = egui::Context::default();
        let mut tab = focused_tab(&ctx);
        let ctrl = egui::Modifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        };
        frame(
            &ctx,
            &mut tab,
            vec![key(egui::Key::Insert, ctrl), egui::Event::Copy],
            ctrl,
            false,
        );
        assert!(tab.sent_input.is_empty(), "Ctrl+Insert sent Insert or ^C");
        let mut pending = None;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(640.0, 480.0),
                )),
                events: vec![
                    key(egui::Key::Insert, egui::Modifiers::SHIFT),
                    egui::Event::Paste("two\nlines".into()),
                ],
                modifiers: egui::Modifiers::SHIFT,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    pending = terminal_view(ui, &mut tab, 14.0, false);
                });
            },
        );
        assert_eq!(pending.as_deref(), Some("two\nlines"));
        assert!(tab.sent_input.is_empty(), "Shift+Insert skipped review");
        frame(
            &ctx,
            &mut tab,
            vec![
                key(egui::Key::Insert, egui::Modifiers::SHIFT),
                egui::Event::Paste("one line".into()),
            ],
            egui::Modifiers::SHIFT,
            false,
        );
        assert_eq!(tab.sent_input, b"one line");
    }

    #[test]
    fn typing_hides_the_pointer_until_it_moves() {
        let ctx = egui::Context::default();
        let mut tab = focused_tab(&ctx);
        frame(
            &ctx,
            &mut tab,
            vec![egui::Event::Text("ls".into())],
            egui::Modifiers::NONE,
            false,
        );
        assert!(tab.pointer_hidden);
        frame(
            &ctx,
            &mut tab,
            vec![egui::Event::PointerMoved(egui::pos2(60.0, 60.0))],
            egui::Modifiers::NONE,
            false,
        );
        assert!(!tab.pointer_hidden);
    }

    #[test]
    fn contrast_guard_skips_picture_glyphs_only() {
        for glyph in ["▀", "▄", "█", "─", "╋", "⣿", "\u{1fb00}"] {
            assert!(drawing_glyph(glyph), "{glyph}");
        }
        for text in ["a", "", "€", "→", "界"] {
            assert!(!drawing_glyph(text), "{text}");
        }
    }

    #[test]
    fn command_bar_runs_one_line_and_reviews_pasted_line_breaks() {
        let mut tab = Tab::new(Draft::default());
        tab.connected = true;
        assert_eq!(submit_command_line(&mut tab, "ls -la".into()), None);
        assert_eq!(tab.sent_input, b"ls -la\r");
        tab.sent_input.clear();
        for text in [
            "echo hi\nrm -rf ~/build",
            "a\rb",
            "x\x1b[201~y",
            "tab\there",
        ] {
            assert_eq!(
                submit_command_line(&mut tab, text.into()).as_deref(),
                Some(text)
            );
        }
        assert!(tab.sent_input.is_empty());
    }

    #[test]
    fn line_by_line_paste_sends_each_line_with_enter_and_no_brackets() {
        assert_eq!(paste_lines("a\r\nb\rc\n"), b"a\rb\rc\r");
        assert_eq!(paste_lines("single"), b"single\r");
        assert_eq!(paste_lines("x\n\ny"), b"x\r\ry\r");
        let mut tabs = vec![Tab::new(Draft::default())];
        tabs[0].connected = true;
        tabs[0].terminal.feed(b"\x1b[?2004h");
        let recipients = input_recipients(&tabs, 0, false);
        deliver_review_lines(
            &mut tabs,
            PasteReview {
                text: "echo 1\necho 2\n".into(),
                recipients,
            },
        );
        assert_eq!(tabs[0].sent_input, b"echo 1\recho 2\r");
    }

    #[test]
    fn palette_filters_commands_themes_and_snippets() {
        let snippets = vec![Snippet {
            name: "Disk usage".into(),
            command: "df -h".into(),
            extensions: Default::default(),
        }];
        let commands = palette_commands(&snippets);
        let matching = |query: &str| -> Vec<String> {
            commands
                .iter()
                .filter(|(title, _)| palette_matches(title, query))
                .map(|(title, _)| title.clone())
                .collect()
        };
        assert_eq!(matching("green"), ["Theme: Green Phosphor"]);
        assert_eq!(matching("snip disk"), ["Snippet: Disk usage"]);
        assert_eq!(matching("CLIPBOARD"), ["Upload clipboard image"]);
        assert_eq!(matching("").len(), commands.len());
        assert!(matching("no such command").is_empty());
        assert!(commands
            .iter()
            .any(|(_, command)| *command == Command::Theme("Paper")));
    }

    #[test]
    fn status_chips_show_modes_scroll_and_forwards_only_when_connected() {
        let mut tab = Tab::new(Draft::default());
        tab.draft.options.local_forwards = vec!["8080:localhost:80".into()];
        tab.draft.options.dynamic_forwards = vec![" ".into(), "1080".into()];
        assert!(status_chips(&tab).is_empty());
        tab.connected = true;
        tab.terminal.feed(b"\x1b[?2004h\x1b[?1000h");
        let chips: Vec<String> = status_chips(&tab)
            .into_iter()
            .map(|(chip, _)| chip)
            .collect();
        assert_eq!(chips, ["PASTE", "MOUSE", "L 8080:localhost:80", "D 1080"]);
    }

    #[test]
    fn form_forwards_apply_whether_or_not_the_connection_is_saved() {
        let imports = neo_ssh::ssh_config::SshConfig::parse(
            "Host dev\nPort 2200\nLocalForward 7000 db:5432\nDynamicForward 1079\n",
        )
        .unwrap();
        let mut draft = Draft {
            target: "dev".into(),
            options: neo_ssh::config::Profile {
                local_forwards: vec!["8080:localhost:80".into()],
                remote_forwards: vec!["9000:localhost:90".into()],
                dynamic_forwards: vec!["1080".into()],
                ..Default::default()
            },
            ..Default::default()
        };
        let counts = |draft: &Draft| {
            let connect =
                resolve_form(draft, &imports, std::path::Path::new("/home/test"), "u").unwrap();
            (
                connect.local_forwards.len(),
                connect.remote_forwards.len(),
                connect.dynamic_forwards.len(),
                connect.local_forwards[0].port,
                connect.dynamic_forwards[0].port,
                connect.target.port,
            )
        };
        // Unsaved: the form's lists replace the imported ones, as when saved;
        // other settings still come from ssh_config.
        assert_eq!(counts(&draft), (1, 1, 1, 8080, 1080, 2200));
        draft.saved_profile = true;
        let (local, remote, dynamic, first_local, first_dynamic, _) = counts(&draft);
        assert_eq!(
            (local, remote, dynamic, first_local, first_dynamic),
            (1, 1, 1, 8080, 1080)
        );
        // Empty form lists keep the imported forwards.
        draft.saved_profile = false;
        draft.options.local_forwards.clear();
        draft.options.dynamic_forwards.clear();
        assert_eq!(counts(&draft), (1, 1, 1, 7000, 1079, 2200));
    }

    #[test]
    fn alt_hover_reads_row_activity_recorded_by_poll() {
        let mut tab = Tab::new(Draft::default());
        tab.terminal.feed(b"first\r\nsecond");
        tab.rows_dirty = true;
        tab.poll(std::time::Instant::now() + Duration::from_secs(1));
        assert!(!tab.rows_dirty);
        assert!(tab.row_activity.changed(0).is_some());
        assert!(tab.row_activity.changed(1).is_some());
    }
}
