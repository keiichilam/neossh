# Validation And Release

Last updated: 2026-09-06

## Required Local Checks

Run before considering a redevelopment milestone complete:

```powershell
cargo fmt --check
cargo check --locked
cargo test --locked --all-targets
```

Expected baseline:

- `cargo test` passes.
- Known dead-code warnings may exist in `src/terminal/brackets.rs`.

For changes to either vendored crate, also run its tests directly:

```powershell
cargo test --manifest-path vendor/ssh-rs-0.5.0/Cargo.toml
cargo test --manifest-path vendor/vt100-0.15.2/Cargo.toml
```

These checks replace historical references to the absent migration-check script.
Record command, date, revision, result, and any skipped or environment-dependent
scenarios. Ignored tests are not executed by the default test command; a green
local run does not replace required live checks.

## Unit Test Coverage Checklist

Auth and config:

- Host parsing.
- Username fallback.
- Default key discovery.
- Key generation.
- Public key reading.
- Config defaults.
- Config load/save.
- Profile deletion.
- Snippet persistence.
- tmux profile and workspace persistence.
- tmux session-name validation and startup command construction.
- CLI tmux flag parsing and target-position disambiguation.

Terminal:

- Key mappings.
- Bracketed paste.
- Mouse encoding.
- Output scanner.
- Split escape sequences.
- Query replies.
- CPR ordering.
- CPR stripping.
- UTF-8 preservation.
- Selection normalization.
- Wide-char selection.
- Bracket matching.

Transfer and remote files:

- Remote path joining.
- Temp cleanup.
- Staging copies.
- Shell quoting.
- `ls -la` parsing.
- Chmod validation.
- Public key install command generation.

GUI pure logic:

- URL encoding.
- Token display-width columns.
- Dialog action routing.
- Search state where practical.

## Manual CLI Test Matrix

Run against a known test host:

```powershell
cargo run -- user@host
cargo run -- user@host:2222
cargo run -- -P profile
cargo run -- user@host --log session.log
cargo run -- user@host --tmux
cargo run -- user@host --tmux=validation
cargo run -- -P tmux-profile --no-tmux
```

Verify:

- Password auth.
- Key auth.
- MOTD renders.
- Shell prompt accepts input.
- `Ctrl+C` interrupts remote command.
- `Ctrl+D` sends EOF.
- `Ctrl+V` pastes.
- Window resize updates remote `$COLUMNS` / `$LINES`.
- Optional raw log appends and preserves ANSI bytes.
- Raw mode is restored after exit.

### tmux CLI test case

Use a Unix-like test host with tmux installed:

- Run with `--tmux=validation`; confirm session `validation` is created.
- Start a long-running command, disconnect without stopping it, reconnect with
  the same option, and confirm the command is still running.
- Attach from a second ssh4 process and confirm both clients reach the same
  session.
- Verify mouse, resize, alternate-screen applications, and bracketed paste from
  inside tmux.
- Save `tmux_session = "validation"` in a profile and verify plain `-P` attaches.
- Repeat with `--no-tmux` and verify the normal login shell is used.
- Test a host without tmux; confirm its command-not-found error is visible and
  the login shell remains usable.

## Required SSH Regression Matrix

Use this matrix when changing existing SSH capabilities or verifying rebuild
parity. These checks cover required behavior, not an optional improvement backlog.

Known hosts:

- First connect to unknown host prompts or records intentionally.
- Second connect verifies silently.
- Changed host key is blocked or requires explicit confirmation.
- Strict mode rejects unknown hosts.

Agent auth:

- Connect with agent identity and no key path.
- Profile can disable agent use.
- Password fallback still works when agent auth fails.
- Windows: OpenSSH agent and/or Pageant keys work when the agent is running;
  `SSH_AUTH_SOCK` overrides automatic discovery.

Keyboard-interactive:

- Single prompt succeeds.
- Multiple prompts succeed.
- Canceling prompts aborts cleanly.

OpenSSH config:

- `Host` alias resolves `HostName`, `User`, `Port`, and `IdentityFile`.
- Explicit `ssh4` profile values override imported values.
- Unsupported config fields are ignored safely.

Forwarding:

- Local forwarding reaches a remote TCP service.
- Remote forwarding reaches the configured local service.
- Dynamic SOCKS5 CONNECT reaches the requested destination.
- Closing the session closes the forward.
- Port bind failure is shown clearly.

Jump hosts:

- One-hop jump connects.
- Multi-hop jump connects and verifies each hop's host key.
- Failed intermediate hop reports the failing phase.

SFTP:

- List, upload, download, rename, chmod, and delete work through SFTP.
- Shell/SCP fallback still works when SFTP is unavailable.

## Manual GUI Test Matrix

Launch:

```powershell
cargo run
```

Verify:

- GUI opens with no args.
- Connection form connects.
- Saved profile connects.
- Profile auto-save does not duplicate existing matching profiles.
- New tab, close tab, close others, close right.
- Tab color menu.
- Tab search.
- Focus mode.
- Sync input.
- Terminal search.
- Ctrl-click URL.
- Ctrl-click path.
- Clipboard image upload.
- Drag-and-drop upload.
- File tools list remote directory.
- File tools upload/download.
- File tools create/rename/chmod/delete with confirmation.
- Theme switching persists.
- State dump creates a report.
- Disconnect returns tab to prefilled form.
- Advanced SSH options can enable tmux and edit a valid session name.
- The workspace header shows the configured `tmux:<name>` indicator.
- Disconnecting and reconnecting the tab reattaches to the same tmux session.
- Saving/restoring the profile or workspace preserves the tmux session name.

### Copy and paste test case

Use terminal text containing leading spaces, Unicode, and at least two lines.
Keep a local text editor open to inspect clipboard contents.

- Drag over text, then press `Ctrl/Cmd+C`; the exact normalized selection is
  copied and the highlight clears.
- Drag over text, then cause remote output, scroll history, and resize the
  terminal before pressing `Ctrl/Cmd+C`; the originally selected text is copied.
- With no selection, run a long command and press `Ctrl/Cmd+C`; the remote
  command receives the interrupt instead of clipboard text being produced.
- `Ctrl/Cmd+Shift+C` copies when a selection exists and does nothing when none
  exists. `Esc` cancels a selection without sending Escape remotely.
- Right-click with no selection: a single-line clipboard value pastes directly;
  multiline LF, CRLF, CR-only, and trailing-newline values open confirmation.
- Cancel the multiline dialog and verify no bytes reach the remote. Repeat and
  confirm; verify the complete text is sent exactly once.
- Right-click with a selection and verify the Copy / Paste / Search online menu.
  Copy and Search use the frozen selection; Paste uses clipboard text.
- `Ctrl/Cmd+V` follows the same single-line/multiline policy as right-click.
  `Ctrl/Cmd+Shift+V` sends multiline text immediately without a dialog.
- In an editor that enables bracketed paste (for example `vim`), verify both
  normal and forced paste are wrapped as one bracketed-paste payload and the
  pasted lines are inserted rather than executed.
- Paste text containing leading/trailing spaces, tabs, Unicode, and an empty
  clipboard; content is preserved byte-for-byte and an empty paste sends nothing.

## TUI Compatibility Tests

On a remote host, test:

```sh
vim
tmux
less
htop
python3 - <<'PY'
print('\033[6n', end='', flush=True)
PY
```

Verify:

- Alternate screen renders.
- Mouse forwarding works when enabled.
- Shift forces local selection over mouse forwarding.
- CPR replies do not corrupt display.
- Bracketed paste works in editors.
- Auto-attach reaches the configured session and detach returns to the login
  shell without terminating ssh4.

## Performance Checks

Run:

```sh
yes "abcdefghijklmnopqrstuvwxyz 0123456789" | head -n 20000
find /usr -maxdepth 4 2>/dev/null
```

Verify:

- GUI remains responsive.
- Output continues painting incrementally.
- CPU does not spin after output stops.
- Closing the tab stops work.

## Security Release Checklist

- No password in logs.
- No private key in logs.
- Diagnostic dump captures output only, not user input.
- Temp clipboard image removed after upload failure.
- Dropped-file staging directory removed after upload failure.
- Remote delete always confirms.
- Remote file shell commands quote all paths.
- Config file permissions are owner-only on Unix.
- CLI warns or avoids unsafe password exposure.

## Packaging Checklist

```powershell
cargo build --release --locked
.\install.ps1
```

Verify:

- `target/release/ssh4.exe` starts.
- Installed `ssh4` starts from a new shell.
- GUI launch does not leave a stuck console window in expected packaging mode.
- README examples are valid.
- Redevelopment packet reflects current behavior.
- tmux examples and remote-host requirements are documented.

## Release Notes Template

```markdown
# ssh4 Release YYYY-MM-DD

## Verification

- cargo fmt --check: pass/fail
- cargo check --locked: pass/fail
- cargo test --locked --all-targets: pass/fail, N tests
- GUI smoke test: pass/fail
- CLI smoke test: pass/fail

## Changed

- ...

## Known Issues

- ...
```
