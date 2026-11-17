# Product Requirements

Last updated: 2026-09-06

## Goal

Rebuild `ssh4` as a Windows-first, cross-platform-capable SSH client that works
well as both a native GUI terminal and a command-line SSH client.

## Users

- Developers and operators who SSH into Unix-like hosts.
- Windows users who want a Rust-native GUI SSH client.
- Users who need tabs, saved profiles, snippets, search, transfer workflows,
  and terminal fidelity.
- Maintainers extending terminal behavior or transfer features.

## Required Modes

### GUI Mode

Launched by default when no host or profile is supplied.

Must provide:

- Connection form.
- Saved profiles.
- Multiple tabs.
- Terminal renderer.
- Local selection.
- Search across visible terminal and scrollback.
- Paste dialog for multiline paste.
- Clipboard image upload.
- Drag-and-drop upload.
- File tools window.
- Snippets.
- Command palette.
- Focus mode.
- Sync input.
- Theme picker.
- Debug logging and state dump.
- Optional named tmux attach/create on connect.

### CLI Mode

Launched when a host or profile is supplied unless `--gui` is used.

Must provide:

- Raw-mode terminal loop.
- Key forwarding.
- Mouse forwarding.
- Clipboard text paste.
- Clipboard image upload.
- Optional raw output logging.
- Reconnect prompt unless disabled.
- Optional named tmux attach/create with profile inheritance and CLI override.
- Clean raw-mode and mouse-capture teardown.

## Functional Requirements

### FR-001 Launch

- `ssh4` opens GUI mode.
- `ssh4 --gui` opens GUI mode.
- `ssh4 user@host` opens CLI mode.
- `ssh4 -P profile` opens CLI mode using a saved profile.

### FR-002 Host Parsing

Support:

- `host`
- `host:port`
- `user@host`
- `user@host:port`

Default port: `22`.

### FR-003 Authentication

Support:

- Username from host string.
- Username override.
- Password auth.
- Key auth.
- Default key discovery.
- GUI password field.
- CLI password prompt where appropriate.
- Ed25519 key generation and public key installation from the GUI.

Do not log credentials. Do not persist passwords by default.

### FR-004 Configuration

Persist to `~/.ssh4.toml`:

- Profiles.
- Snippets.
- UI zoom.
- Terminal theme.
- Keep-alive preference.
- Display preferences.
- Per-profile tmux session name.

Existing config files must remain compatible.

### FR-005 Terminal Protocol

Shared terminal protocol logic must support:

- VT key byte mapping.
- Bracketed paste.
- DEC mouse mode scanning.
- SGR mouse encoding.
- DA, DSR, CPR, and OSC color query handling.
- CPR response stripping.
- Split escape buffering.
- Unknown escape passthrough.

This logic must be unit-testable without SSH.

### FR-006 GUI Terminal Rendering

Must render:

- ANSI colors.
- Default, indexed, and RGB colors.
- Cursor.
- Selection.
- Search highlights.
- Link highlights.
- Bracket matching.
- Scrollback.
- Wide characters.

Text, cursor, selection, hit testing, search highlights, and links must share
the same terminal-cell coordinate system.

### FR-007 Transfers

Must support:

- Clipboard image upload as PNG over SCP.
- Drag-and-drop upload.
- Remote path copied to clipboard.
- Optional remote path typed into active shell.
- Cleanup on success and failure.

### FR-008 File Tools

Must support:

- Remote listing.
- Remote upload.
- Remote download.
- Create file.
- Create folder.
- Rename/move.
- Chmod.
- Delete with confirmation.
- Local browsing and selected-file upload.

Remote shell commands must quote paths safely.

### FR-009 Observability

Must provide:

- GUI status counters.
- Worker heartbeat information.
- Panic log.
- Debug log toggle.
- Diagnostic state dump containing terminal state and recent remote output.

Diagnostic dumps must not record user input.

### FR-010 tmux Session Integration

Must support:

- Optional attach-or-create after the interactive SSH shell opens.
- `tmux new-session -A -s <name>` semantics.
- GUI enablement and session-name entry in advanced connection options.
- Profile and workspace-restore persistence.
- CLI `--tmux`, `--tmux=<name>`, and `--no-tmux` overrides.
- Automatic reuse of the configured name after reconnect.
- Conservative session-name validation before constructing remote input.

If tmux is missing or fails to start, the normal remote login shell must remain
interactive. Jump-host and one-shot file/key-install connections must not start
tmux.

## Nonfunctional Requirements

### NFR-001 Responsiveness

- GUI thread must never block on network I/O.
- SSH output must be batched.
- Per-frame processing limits must prevent UI starvation.
- Transfers must run on worker threads.

### NFR-002 Reliability

- Closing a tab stops its SSH worker.
- CLI raw mode is restored on exit.
- Temp upload files and staging directories are removed.
- Disconnects return GUI tabs to prefilled connection forms.

### NFR-003 Security

- No local shell-out to `ssh` for core SSH behavior.
- No password or key material in logs.
- Confirm destructive remote actions.
- Use owner-only config permissions on Unix where possible.
- Treat command construction in remote file tools as high risk.

### NFR-004 Portability

- Windows is the first-class target.
- Linux and macOS should remain viable.
- Use platform-specific font fallbacks.
- Use `USERPROFILE` or `HOME` for config path resolution.

## Definition Of Parity

Redevelopment reaches parity when:

- `cargo fmt --check`, `cargo check --locked`, and `cargo test --locked --all-targets` pass.
- GUI opens with no args.
- CLI connects with host and profile args.
- GUI connects, renders, accepts input, resizes, searches, pastes, and closes.
- CLI connects, forwards keys, handles mouse mode, pastes, uploads, and exits.
- Profiles and snippets persist.
- tmux settings persist and reconnects reuse the configured session name.
- Clipboard image upload works.
- GUI drag-and-drop upload works.
- File tools basic operations work.
- Terminal protocol regression tests pass.
- Existing SSH capabilities SSH-001 through SSH-011 below remain available;
  run their applicable manual checks as well as focused regression tests.

## Required SSH Capabilities

The implemented behavior in SSH-001 through SSH-011 is part of current parity.
Preserve it during refactoring or rebuilding, including host verification on
shell, transfer, exec, forwarding, and jump connections. Implementation history
is recorded in [rebuild status](05-rebuild-status.md); passing unit tests alone
does not establish live interoperability. Explicit future extensions are listed
at the end of this document.

### SSH-001 Known Hosts Verification

Preserve the existing OpenSSH-style `known_hosts` checking and trust policies.

Required behavior:

- Verify host keys on every connection.
- Prompt on first connect.
- Warn and block or require explicit confirmation when a host key changes.
- Support a strict mode that fails unknown or changed hosts.

Known limitation: hashed `|1|...` entries are currently skipped. Preserve
verification for supported entries; hashed-entry support is a future extension.

### SSH-002 SSH Agent Support

Preserve support for local SSH agents.

Targets:

- Windows OpenSSH agent.
- Pageant.
- Unix `SSH_AUTH_SOCK`.

Required behavior:

- Try agent identities before password fallback when configured.
- Surface which identity succeeded without exposing private material.
- Let profiles opt out of agent use.

### SSH-003 Keyboard-Interactive Auth

Support keyboard-interactive authentication in addition to key and password
auth.

Required behavior:

- GUI displays prompts without blocking the UI thread.
- CLI prompts inline.
- Multi-prompt flows are supported.
- Prompt answers are never logged.

### SSH-004 OpenSSH Config Import

Read `~/.ssh/config` for host aliases.

Useful fields:

- `HostName`
- `User`
- `Port`
- `IdentityFile`
- `ProxyJump`
- `ProxyCommand` as future scope

Required behavior:

- Imported host aliases appear in the GUI profile/host picker.
- Explicit `ssh4` profile fields override imported SSH config values.

### SSH-005 SFTP Backend

Preserve SFTP for file tools and keep shell/SCP as fallback.

Required behavior:

- List directories.
- Upload and download files.
- Rename/move.
- Delete.
- Chmod where supported.

Why: SFTP avoids shell command quoting risks and is a better long-term backend
for remote file tools.

### SSH-006 Port Forwarding

Preserve all implemented forwarding modes:

- Local forwarding: `-L [bind:]local_port:host:remote_port`.
- Remote forwarding: `-R [bind:]port:host:hostport`.
- Dynamic SOCKS5 forwarding: `-D [bind:]port`.

Required behavior:

- Forwarding lifecycle is tied to the session.
- GUI shows active forwards and errors.
- CLI supports OpenSSH-like flags where practical.

### SSH-007 Jump Hosts

Support `ProxyJump`-style connections.

Required behavior:

- One or more jump hosts.
- Profile-level jump host config.
- Imported `~/.ssh/config` jump host entries.
- Clear connection timeline across each hop.

### SSH-008 Connection Timeline Diagnostics

Record user-visible connection phases:

- Config resolution.
- DNS / TCP connect.
- Key exchange.
- Host key verification.
- Auth method attempts.
- PTY request.
- Shell open.

Why: SSH failures are otherwise hard to diagnose, especially in GUI mode.

### SSH-009 Session Restore

Optionally restore previous GUI tabs on launch.

Required behavior:

- Restore tab list and profile identity.
- Do not restore passwords.
- Ask before reconnecting if configured.
- Preserve previous remote directory where available.

### SSH-010 Better Reconnect

Preserve reconnect after disconnect.

Required behavior:

- Show reconnect button and optional countdown.
- Preserve tab identity.
- Preserve scrollback until reconnect succeeds.
- Keep last error visible.

### SSH-011 Advanced Profile Options

Preserve optional per-profile SSH settings:

- Keepalive interval.
- Preferred auth order.
- Agent on/off.
- Terminal type (`TERM`).
- Remote environment variables.
- Connect timeout.
- Strict host key mode.

Existing profile schema compatibility must be preserved.

## Future Extensions

These additions do not reduce the required existing behavior above:

- Hashed OpenSSH known-host entry matching.
- IPv6 forwarding/SOCKS address forms.
- Complete half-close handling for forwarded connections.
- Session-shared local and dynamic forwarding.
- `ProxyCommand` support.
