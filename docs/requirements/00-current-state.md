# Current State

Last updated: 2026-09-06

This document records the repository state a redevelopment should preserve.

## Product

`ssh4` is a Rust SSH client with:

- GUI mode by default, built with `eframe` / `egui`.
- CLI mode when a host or saved profile is supplied.
- Shared SSH, config, transfer, terminal input/output, and selection logic.
- Vendored SSH and terminal parser dependencies with local modifications.

## Source Layout

```text
src/
  main.rs
  cli.rs
  auth.rs
  config.rs
  known_hosts.rs
  logutil.rs
  ssh_agent.rs
  ssh_client.rs
  ssh_config.rs
  sftp.rs
  transfer.rs
  remote_fs.rs
  workspace.rs
  terminal/
    mod.rs
    input.rs
    output.rs
    mouse.rs
    selection.rs
    brackets.rs
  gui/
    mod.rs
    app.rs
    files.rs
    links.rs
    search.rs
    session.rs
    theme.rs
vendor/
  ssh-rs-0.5.0/
  vt100-0.15.2/
```

## Current Capabilities

- Host parsing for `host`, `host:port`, `user@host`, and `user@host:port`.
- Config persistence in `~/.ssh4.toml`.
- Profiles, snippets, UI zoom, terminal font size, theme, keep-alive, and
  optional named tmux auto-attach.
- Password, key, agent, and keyboard-interactive auth through vendored `ssh-rs`.
- Known-host verification with Ask, AcceptNew, and Strict policies.
- OpenSSH config import, multi-hop ProxyJump, and local/remote/dynamic forwarding.
- Workspace restore, reconnect controls, and advanced per-profile SSH options.
- GUI tabs, saved profiles, snippets, focus mode, sync input, command palette,
  theme picker, file tools, diagnostic state dump, terminal search, tab search.
- CLI raw terminal mode with reconnect prompt, optional raw output logging,
  and `--tmux[=<name>]` / `--no-tmux`.
- GUI and CLI can attach to or create a named tmux session after login;
  profiles and workspace restore preserve the session name.
- Terminal key mapping, bracketed paste, mouse forwarding, CPR/DSR/DA/OSC query
  handling, CPR response stripping, split escape buffering.
- Clipboard image upload over SCP.
- GUI drag-and-drop upload.
- Remote file operations prefer SFTP, with SSH exec/SCP fallback.
- Local file pane in the GUI file tools window.

## Dependency Notes

Keep these exact concerns in scope before replacing dependencies:

- `ssh-rs` is vendored at `vendor/ssh-rs-0.5.0`.
- Local SSH modifications include PTY resize, keepalive, resumable packet reads,
  rekey timeout hardening, EOF handling, and `TCP_NODELAY`, plus host-key proof
  hooks, agent/keyboard-interactive auth, forwarding channels, and SFTP support.
- `vt100` is vendored at `vendor/vt100-0.15.2`.
- Local vt100 modification: visible row calculation saturates under deep
  scrollback instead of underflowing.

## Build And Test

```powershell
cargo fmt --check
cargo check --locked
cargo test --locked --all-targets
cargo build --release --locked
```

Historical baseline recorded on 2026-07-30 (not rerun for this documentation update):

- `cargo test`: 166 passed.
- `cargo check`: passes with existing dead-code warnings.

## Known Sensitive Areas

- Terminal renderer cell alignment and cursor placement.
- Split escape sequences across SSH reads.
- CPR query ordering.
- Windows console UTF-8 and raw-mode cleanup.
- SSH worker shutdown on tab close.
- Temp file cleanup for upload workflows.
- Remote shell quoting in `remote_fs.rs`.
- Live tmux startup and reconnect behavior requires a real SSH host with tmux
  installed; unit tests cover name validation, command construction, CLI
  parsing, and persistence.
