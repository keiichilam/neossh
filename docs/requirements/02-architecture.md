# Architecture

Last updated: 2026-09-06

## High-Level Shape

```text
User
  |
  | keyboard, mouse, clipboard, files
  v
ssh4 GUI or CLI
  |
  | shared auth/config/terminal/transfer/SSH modules
  v
SSH server
```

The application has two interfaces and one shared core:

- `cli.rs`: raw terminal client.
- `gui/`: native egui application.
- Shared modules: `auth`, `config`, `ssh_client`, `known_hosts`, `ssh_agent`,
  `ssh_config`, `sftp`, `transfer`, `remote_fs`, `workspace`, `logutil`, and
  `terminal/*`.

## Runtime Dispatch

`main.rs` should stay thin:

1. Parse CLI arguments.
2. Decide GUI vs CLI.
3. Call `gui::run()` or `cli::run()`.

The dispatch rule:

- No target/profile: GUI.
- Target/profile: CLI.
- `--gui`: GUI even with target/profile.

## Module Ownership

### `auth.rs`

Owns:

- Host parsing.
- Username fallback.
- Default SSH key discovery.
- Ed25519 key generation.
- Public key reading.
- GUI key-install helpers.

### `config.rs`

Owns:

- `Config`.
- `Profile`.
- `Snippet`.
- Config path.
- Load/save.
- Profile name disambiguation.
- Per-profile tmux session persistence.

### `ssh_client.rs`

Owns:

- `ConnParams`.
- Session connection helper.
- Shell open helper.
- Friendly SSH error mapping.
- Password zeroing behavior.
- tmux session-name validation and startup-command construction.

### `terminal/input.rs`

Owns keyboard and paste translation:

- Crossterm key to bytes.
- egui key to bytes.
- Paste payload wrapping.
- Multiline paste policy.

### `terminal/output.rs`

Owns remote output scanning:

- Query interception.
- Query replies.
- CPR event generation.
- CPR response stripping.
- Mouse mode scanning.
- Bracketed paste scanning.
- Incomplete escape carry.

### `terminal/mouse.rs`

Owns mouse reporting:

- Mouse report mode model.
- Encoding model.
- SGR/default/UTF-8 mouse byte generation.
- Modifier encoding.

### `terminal/selection.rs`

Owns:

- Cell-position model.
- Selection normalization.
- Text extraction.
- Scrollback-aware extraction.

### `terminal/brackets.rs`

Owns:

- Bracket matching over a grid text source.
- Unit-testable matching logic.

### `transfer.rs`

Owns:

- Clipboard image extraction.
- PNG temp file creation.
- SCP upload.
- Dropped-file staging.
- Cleanup guards.
- Remote path generation.

### `remote_fs.rs`

Owns:

- POSIX shell quoting.
- `ls -la` parsing.
- Remote file command construction.
- Public key install command.

Execution belongs to `ssh_client.rs` (exec/SCP), `sftp.rs` (SFTP v3), and
`gui/files.rs` (background file jobs and fallback selection).

### Other Shared Modules

- `known_hosts.rs`: host-key policy, matching, and trust-file persistence.
- `ssh_agent.rs`: OpenSSH/Pageant discovery and signing.
- `ssh_config.rs`: OpenSSH alias parsing and resolution.
- `sftp.rs`: SFTP v3 file operations over the SSH subsystem channel.
- `workspace.rs`: workspace snapshots, restore policy, forward drafts, remote
  environment parsing, and reconnect countdown state.
- `logutil.rs`: diagnostic logging and panic hook.

### `gui/app.rs`

Owns:

- eframe app state.
- Tab list.
- Modal routing.
- Top-level dock/tab/terminal composition.
- Global shortcuts.
- Transfer result handling.
- Connection forms, tab actions, command palette, and diagnostic report writing.
- Terminal rendering, cell metrics, selection/search/link painting, keyboard
  routing, and pointer/mouse routing.

### `gui/session.rs`

Owns:

- `Session`.
- GUI SSH worker.
- UI-to-worker and worker-to-UI messages.
- Parser state.
- State dump data.

### `gui/theme.rs`

Owns:

- Theme tokens.
- Font installation.
- Built-in terminal themes.
- Fixed chrome palette.

### Other GUI Modules

- `files.rs`: file tools window.
- `links.rs`: terminal link/path detection and actions.
- `search.rs`: terminal search state.

### Possible Future GUI Extraction

Dedicated renderer, tabs, dialogs, chrome, and dump modules are possible future
boundaries, not existing files. Their behavior currently resides largely in
`gui/app.rs`; any extraction must preserve the shared cell-coordinate and input
routing invariants below.

## GUI Threading

Each active GUI session has one SSH worker.

```text
egui thread
  | WorkerCommand::Data / Resize / Stop
  v
SSH worker thread
  | WorkerEvent::Connected / Data / AuthPrompt / Disconnected
  v
egui thread
```

Rules:

- UI thread never blocks on network I/O.
- Worker never mutates GUI state directly.
- Worker may request repaint through cloned `egui::Context`.
- Dropping a session sends `WorkerCommand::Stop` and drops its channel handles.
- Resize messages are coalesced.
- Large writes are chunked.
- Output is batched.

## Terminal Protocol Pipeline

Remote bytes flow through:

1. Raw SSH read.
2. Diagnostic capture ring.
3. `terminal::output` scanner.
4. Consume scanner events in order: feed each `Clean` event to `vt100::Parser`,
   append static `Reply` bytes, and compute each cursor-position reply from the
   parser state at that event.
5. Send the accumulated replies in order. Never answer CPR before processing
   preceding clean bytes, or after processing clean bytes that follow that CPR.
6. GUI renderer paints `vt100::Screen`.

The protocol scanner must be independent of GUI and SSH.

## CLI Terminal Pipeline

CLI output flow:

1. Read remote bytes.
2. Append raw output to the log if enabled.
3. Consume scanner events in stream order, writing each clean event to stdout
   and feeding it to the shadow `vt100::Parser` before processing the next event.
4. At each query event, append its reply using the parser state at that point.
5. Send accumulated replies in order and update terminal mode state.

## tmux Startup Flow

tmux is an optional post-login action shared by GUI and CLI:

1. Resolve the requested session name from CLI overrides, a GUI draft, or a
   saved profile.
2. Validate and normalize the name to ASCII letters, numbers, hyphens, and
   underscores (maximum 64 bytes).
3. Open the interactive SSH shell and establish requested port forwards.
4. Send `tmux new-session -A -s <name>` as terminal input.
5. Continue through the normal terminal pipeline.

The startup input is a plain command rather than a POSIX-shell conditional so
simple non-POSIX login shells can accept it. A missing or failing tmux command
returns control to the login shell. `ConnParams::tmux_session` belongs only to
the target interactive shell; jump hops, SFTP/SCP/exec connections, port-forward
sessions, and key-install helpers do not launch tmux.

GUI reconnect uses the existing `ConnParams`, so it sends the same command and
reattaches to the named remote session. Workspace snapshots store the name but
never store passwords.

## Rendering Invariants

- Cell metrics are the source of truth for cursor, text, selection, hit
  testing, search highlights, and links.
- Do not let shaped text-run advances define terminal columns.
- Wide cells occupy two columns.
- Wide continuation cells are not painted separately.
- Link/path token positions use display width, not byte length.

## Security Invariants

- Never log passwords or private keys.
- Do not persist passwords by default.
- Confirm remote delete.
- Quote every remote path passed through shell commands.
- Remove temp files on every upload path.
