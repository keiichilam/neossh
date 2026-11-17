# Implementation Plan

Last updated: 2026-09-06

This plan is ordered to protect terminal correctness and keep each milestone
testable.

M0-M16 describe the original build sequence and retain acceptance criteria for
regression checks. Much of this work is already implemented. Start from the
[current source inventory](00-current-state.md) and
[rebuild status](05-rebuild-status.md), then verify the relevant acceptance
criteria. Do not recreate modules or defer existing features merely because
they appear later in this sequence. Existing SSH capabilities, file tools,
themes, and command-palette behavior are required parity.

## M0 Baseline

Tasks:

- Run `cargo fmt --check`.
- Run `cargo check --locked`.
- Run `cargo test --locked --all-targets`.
- Record warnings and failures.
- Capture screenshots of current GUI flows.
- Record vendored dependency local modifications.

Acceptance:

- Baseline result is documented.
- Dirty worktree state is understood.

## M1 Skeleton

Tasks:

- Create module layout.
- Keep `main.rs` thin.
- Add placeholder modules and compile.
- Keep Cargo dependencies pinned.

Acceptance:

- `cargo check` passes.

## M2 Config And Auth

Tasks:

- Implement `Config`, `Profile`, and `Snippet`.
- Implement config path resolution.
- Implement load/save and default values.
- Implement host parsing.
- Implement username fallback.
- Implement key discovery.
- Implement key generation and public key reading.

Tests:

- Host parse variants.
- Invalid port behavior.
- Config load/save roundtrip.
- Profile name uniqueness.
- Snippet persistence.
- Default key discovery order.
- Key generation refuses overwrite.

## M3 SSH Core

Tasks:

- Implement `ConnParams`.
- Implement connect helper.
- Implement shell open helper.
- Implement friendly error messages.
- Preserve password zeroing or introduce a secret wrapper.

Manual acceptance:

- Password auth connects to a real host.
- Key auth connects to a real host.
- Auth failure returns a useful error.

## M4 Terminal Protocol Core

Tasks:

- Implement key mapping.
- Implement paste payload builder.
- Implement mouse encoding.
- Implement remote output scanner.
- Implement query replies.
- Implement bracketed paste and mouse mode scanners.
- Implement selection extraction.
- Implement bracket matching.

Tests:

- Ctrl letters.
- Alt prefix.
- Shift+Tab.
- Navigation keys.
- Function keys.
- Bracketed paste.
- SGR mouse.
- Split escape sequences.
- OSC BEL and ST.
- CPR ordering.
- CPR response stripping.
- Unknown escape passthrough.
- Wide-char selection.

## M5 CLI Parity

Tasks:

- Implement CLI args.
- Resolve profile and host params.
- Open raw terminal mode.
- Forward keyboard and mouse events.
- Process output scanner events.
- Maintain shadow parser.
- Implement paste and clipboard image upload.
- Implement reconnect prompt.
- Implement raw log append.
- Guarantee cleanup.

Manual acceptance:

- `ssh4 user@host` connects.
- `ssh4 -P profile` connects.
- `Ctrl+C` reaches remote.
- `Ctrl+D` reaches remote.
- `Ctrl+V` pastes.
- `Ctrl+P` uploads image when present.
- Raw mode restores after disconnect.

## M6 GUI Shell

Tasks:

- Implement eframe startup.
- Install fonts and theme.
- Implement app state.
- Implement dock.
- Implement tab bar.
- Implement connection form.
- Implement profile/snippet UI.
- Implement command palette.
- Implement modal ownership rules.

Acceptance:

- GUI opens with no args.
- New tab and close tab work.
- Profiles and snippets persist.
- Modal input does not reach terminal.

## M7 GUI SSH Sessions

Tasks:

- Implement `Session`.
- Implement GUI SSH worker.
- Implement channels.
- Implement output batching.
- Implement heartbeat.
- Implement resize messages.
- Implement disconnect-to-form transition.
- Implement session drop stop flag.

Acceptance:

- GUI connects to a real SSH shell.
- Output appears.
- Input reaches shell.
- Resize updates PTY.
- Closing a tab stops the worker.

## M8 GUI Renderer

Tasks:

- Integrate `vt100::Parser`.
- Paint cells from `vt100::Screen`.
- Paint cursor.
- Paint selection.
- Paint search highlights.
- Paint links.
- Paint bracket matching.
- Implement scrollback.
- Implement pointer-to-cell hit testing.
- Implement local selection and remote mouse forwarding.

Acceptance:

- Shell output is cell-aligned.
- Cursor is aligned to text.
- Wide characters do not drift.
- Selection copies expected text.
- TUI mouse mode works.

## M9 Transfers

Tasks:

- Implement clipboard image upload.
- Implement drag-and-drop confirmation.
- Implement dropped-file staging.
- Implement SCP upload.
- Implement cleanup guards.
- Integrate upload status.

Acceptance:

- Upload success copies remote path.
- Upload failure reports error.
- Temp files are removed on failure.

## M10 File Tools

Tasks:

- Implement remote listing.
- Implement remote upload/download.
- Implement create file/folder.
- Implement rename/move.
- Implement chmod.
- Implement delete confirmation.
- Implement local file pane.

Tests:

- Shell quoting.
- `ls -la` parsing.
- Chmod validation.
- Remote path joining.

## M11 Polish And Observability

Tasks:

- Implement debug log toggle.
- Implement panic logger.
- Implement state dump.
- Implement line timestamp hover.
- Implement status counters.
- Implement sync input.
- Implement focus mode.
- Implement theme switching.

Acceptance:

- State dump captures current terminal state and recent remote output.
- User input is not included in state dump.
- Theme persists.
- Sync input only broadcasts terminal input.

## M12 Packaging

Tasks:

- Verify `cargo build --release`.
- Verify `install.ps1`.
- Update README.
- Update redevelopment packet.
- Record manual test results.

Acceptance:

- Release binary runs.
- Installer places binary where expected.
- Docs match behavior.

## M13 SSH Client Hardening

Tasks:

- Add known-hosts verification.
- Add first-connect and changed-host-key UX.
- Add SSH agent support.
- Add keyboard-interactive authentication.
- Add connection timeline diagnostics.

Acceptance:

- Unknown hosts are handled intentionally.
- Changed host keys are not silently accepted.
- Agent auth works on at least one supported platform.
- Keyboard-interactive auth works for a test server.
- GUI and CLI expose useful connection failure details.

## M14 SSH Workflow Expansion

Tasks:

- Import `~/.ssh/config` host aliases.
- Add advanced per-profile SSH options.
- Add session restore.
- Improve reconnect behavior.
- Add local port forwarding.
- Add jump host support.

Acceptance:

- Imported aliases can be selected in GUI and CLI.
- Explicit `ssh4` profile fields override imported config.
- Reconnect preserves tab identity and visible error state.
- Local forwarding works for a test TCP service.
- Jump host connection works through at least one hop.

## M15 SFTP File Backend

Tasks:

- Select and audit an SFTP implementation.
- Add SFTP list/upload/download/rename/delete/chmod.
- Keep shell/SCP backend as fallback.
- Prefer SFTP in file tools when available.

Acceptance:

- File tools work without remote shell command quoting for SFTP servers.
- Shell/SCP fallback remains available.
- Existing remote file tests still pass.

## M16 tmux Session Integration

Tasks:

- Add an optional tmux session name to connection parameters and profiles.
- Persist the setting through GUI drafts and workspace restore.
- Add GUI enablement, session-name entry, and a configured-session indicator.
- Add CLI `--tmux[=<name>]` and `--no-tmux` controls.
- Validate session names before constructing remote terminal input.
- Send `tmux new-session -A -s <name>` after shell/forward setup.
- Keep jump, transfer, exec, and key-install sessions tmux-free.
- Document remote tmux installation and fallback behavior.

Tests:

- Valid and invalid session names.
- Exact startup command bytes.
- CLI default, named, option-first, and disabled parsing.
- Profile and workspace round trips.
- GUI draft-to-connection round trip.

Manual acceptance:

- GUI and CLI create a missing named session.
- A second connection attaches to the existing session.
- Disconnect/reconnect preserves work running inside tmux.
- `--no-tmux` overrides a tmux-enabled profile.
- A host without tmux remains at a usable login shell.
