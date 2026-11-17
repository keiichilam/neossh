# Rebuild Status

Last updated: 2026-09-06

## Completed Foundation

The entries below record implementation history. They do not establish that
every acceptance scenario has passed on the current revision. See the
[current source inventory](00-current-state.md) for module ownership and the
[validation checklist](04-validation-and-release.md) for required evidence.

The new source tree is an independent rebuild based on this redevelopment
packet. It does not use `backup.doc` as an implementation source.

- Thin GUI-versus-CLI launch dispatch.
- Strict parsing for the documented host forms.
- Compatible TOML configuration shell at `~/.ssh4.toml`, including profiles,
  snippets, zoom, terminal text size, theme, keep-alive, and debug log.
- New native desktop workspace with a dark, focused layout:
  - saved-host rail and host search;
  - connection forms and locally persisted profiles;
  - multi-tab workspace;
  - snippets and preferences;
  - command palette (`Ctrl/Cmd+K`) and new-session shortcut (`Ctrl/Cmd+N`);
  - an ANSI-aware `vt100` screen surface backed by a real SSH worker.
- Basic SSH core:
  - password or private-key authentication in the GUI, without persisting the
    password;
  - default `id_ed25519` / `id_rsa` discovery when no key is specified;
  - one non-blocking SSH worker per GUI tab, including resize and shutdown;
  - a CLI interactive shell with raw mode, key forwarding, and resize events.
- Terminal interaction baseline:
  - direct keyboard focus on the terminal surface, including control and
    navigation keys;
  - expanded key map: F1–F12, Shift+Tab, modified arrows/home/end/etc.;
  - split-sequence detection for bracketed-paste mode;
  - bracket-wrapped paste when requested by the remote;
  - explicit review before a multiline GUI paste is sent;
  - right-click pastes the clipboard when nothing is selected (multiline
    pastes still confirm first); with a selection, Copy / Paste / Search online menu.
- Terminal protocol scanner (M4):
  - DEC private-mode scanning for 1000 / 1002 / 1003 / 1006 and bracketed paste;
  - independent mode flags with Any > Button > Click priority (partial resets);
  - DA / DSR / CPR and OSC color query replies in stream order;
  - CPR answered after prior clean bytes hit the parser (in-chunk cursor motion);
  - CPR response stripping; unknown CSI/OSC passthrough; incomplete carry;
  - SGR and legacy mouse report encoding with modifiers and wheel buttons;
  - GUI edge-detects press/release (no drag-threshold), de-dupes drag/motion by
    cell, multi-tick wheel reports, and Shift forces local selection;
  - CLI shares the same scanner + shadow parser for query replies.
- CLI parity (M5):
  - password prompt when no key is available, and key-auth failure fallback;
  - mouse capture with remote DEC mouse forwarding when the host enables it;
  - `Ctrl+V` clipboard paste with multiline confirmation;
  - `Ctrl+P` clipboard image upload over SCP (temp file cleaned up);
  - optional `--log PATH` raw remote-byte append;
  - reconnect prompt after disconnect (disable with `--no-reconnect`);
  - raw mode + mouse capture teardown on exit.
- Terminal search baseline:
  - `Ctrl/Cmd+F` terminal search with previous/next navigation;
  - match highlights use terminal-cell coordinates and display width rather
    than shaped-text offsets.
- Terminal selection baseline:
  - drag selection is mapped to the same terminal-cell grid as painting;
  - releasing a selection freezes normalized text until an explicit Copy or
    Search online action, so later output, scrolling, or resize cannot retarget it;
  - Shift forces local selection when remote mouse tracking is active.
- Bracket matching (pure grid logic + GUI paint at the live cursor).
- Cell renderer baseline (M8):
  - paints default, indexed, and RGB cell colors plus inverse video;
  - paints cursor, selection, search highlights, link underlines, and
    bracket pairs on the same cell grid;
  - scrollback view via mouse wheel (Shift+wheel when remote mouse is on)
    and Shift+PageUp/PageDown; `SCROLL +N` chip while viewing history.
- Transfers (M9):
  - clipboard image → temp PNG → SCP with cleanup guards (CLI + GUI palette);
  - drag-and-drop file staging and confirmation dialog;
  - success copies remote path(s) to clipboard and can type them into the shell.
- Responsiveness safeguards:
  - SSH receive batches, UI event processing, and PTY resize messages are
    bounded or coalesced to protect interactive responsiveness under output
    load.
- Remote-file safety foundation:
  - POSIX path quoting, constrained remote-path joining, permission validation,
    and command builders are covered by unit tests before any remote file UI
    invokes them;
  - `ls -la` parsing and parent-path helpers for the shell file backend.
- File tools UI (M10):
  - dual-pane remote + local browser window (Files button / command palette);
  - list, navigate, create file/folder, rename, chmod, delete with confirmation;
  - upload selected local file / download selected remote file via SCP;
  - remote ops run on worker threads so the GUI does not block.
- Observability and polish (M11):
  - the command palette can write a diagnostic state dump containing terminal
    dimensions and a bounded ring of recent remote output, never user input;
  - panic hook writes `ssh4-panic.log` under the system temp directory;
  - optional debug log (`debug_log` in config) appends to `ssh4-debug.log`;
  - session byte counters (↓/↑) in the workspace header;
  - theme picker (Midnight / Amber / Forest / Slate) persisted to config;
  - focus mode (F11 / settings / palette) hides top bar and host rail;
  - sync input broadcasts terminal keys to all connected tabs.
- Packaging (M12):
  - `cargo build --release` succeeds;
  - `install.ps1` installs to `%LOCALAPPDATA%\ssh4` and updates user PATH;
  - README documents build, install, GUI/CLI usage, and config.

- SSH hardening (M13):
  - known_hosts verification during KEX (vendored ssh-rs thread-local host-key hook);
  - policies: Ask default (GUI fingerprint modal + CLI prompt), AcceptNew/Strict available;
  - known_hosts writes are staged at KEX check and committed only after signature proof;
  - one-shot accept is fingerprint-bound and cleared after successful connect (not sticky);
  - same-algorithm key changes only count as CHANGED; new algorithms are Unknown;
  - accept-changed replaces same-type known_hosts lines (no dual-trust append);
  - structured `HOST_KEY_UNKNOWN|…` / `HOST_KEY_CHANGED|…` errors survive KexError wrapping;
  - all connects (shell, exec, SCP upload/download) share `connect_session` peer binding;
  - known_hosts file lock for concurrent writers;
  - connection timeline diagnostics on failure (+Nms step labels);
  - keep-alive via SSH_MSG_IGNORE every 30s (GUI worker + CLI loop) when enabled;
  - Ed25519 key generation + copy public key / install command from GUI;
  - clipboard/drag-drop SCP runs on background threads;
  - disconnect and remote drop return tabs to prefilled connection forms;
  - SSH agent support: Windows `\\.\pipe\openssh-ssh-agent`, Pageant
    (`WM_COPYDATA` + user-SID mapping ACL + `SendMessageTimeout`), and
    `SSH_AUTH_SOCK`; agent identities tried before file key / password;
    profile `use_agent` opt-out; OpenSSH + Pageant keys merged when sock unset;
    sign prefers the backend that listed each blob (per-thread routes, cleared
    each list); RSA agent auth coordinates USERAUTH algorithm with a single
    matching flag (`rsa-sha2-256` → `512` → classic `ssh-rsa` per identity);
    agent sign failures advance the auth ladder instead of aborting; short list
    timeout (5s) vs long sign timeout (120s) so Pageant/askpass confirmations
    can complete while discovery stays snappy; WaitNamedPipe before pipe open;
    orphan helper threads counted after caller timeout;
  - keyboard-interactive auth: multi-prompt CLI inline + non-blocking GUI dialog;
    answers never logged.
  - Note: hashed OpenSSH known_hosts (`|1|…`) entries are skipped (not matched).

- OpenSSH config import (M14):
  - parse `~/.ssh/config` for Host / HostName / User / Port / IdentityFile / ProxyJump;
  - GUI host rail lists imported aliases under “FROM ~/.SSH/CONFIG”;
  - CLI resolves aliases against config when target matches a Host pattern;
  - explicit ssh4 profile / form / CLI flags override imported values.
- Jump hosts (M14):
  - ProxyJump via bastion `direct-tcpip` then nested SSH (`-J` / config ProxyJump);
  - multi-hop `-J a,b,c` (comma-separated) chains type-erased tunnels;
  - connection timeline labels hop tunnel steps.
- Port forwards (M14):
  - local `-L [bind:]local:host:port` (repeatable);
  - remote `-R [bind:]port:host:hostport` via `tcpip-forward` / `forwarded-tcpip`;
  - concurrent `-R` relays on one session: accept-path and channel-path demux
    park live-relay traffic; pending buffer for local write backpressure;
    CLOSE on local-connect failure and relay teardown;
  - dynamic SOCKS5 `-D [bind:]port` (CONNECT, no-auth);
  - dedicated sessions; guards stop listeners when the shell drops.
  - Note: half-close (`CHANNEL_EOF`) is still best-effort; IPv6 forward specs
    and session-shared `-L`/`-D` are not claimed.
- Reconnect polish (M14):
  - disconnected GUI tabs keep identity and show error + Reconnect;
  - Disconnect still returns a prefilled connection form.

- SFTP file backend (M15):
  - vendored `open_sftp` subsystem channel;
  - SFTP v3 client: list, mkdir, create, remove, rmdir, rename, chmod, upload, download;
  - file tools prefer SFTP and fall back to shell commands / SCP on failure;
  - SFTP works over ProxyJump hop chains (same tunnel path as shell).

- tmux session integration (M16):
  - optional named attach/create after opening the target interactive shell;
  - GUI control under Advanced SSH options plus a `tmux:<name>` workspace
    indicator;
  - profile and workspace-restore persistence, including reconnect reuse;
  - CLI `--tmux`, `--tmux=<name>`, and `--no-tmux` profile override;
  - validated session names prevent shell metacharacters and tmux target
    delimiters from entering startup input;
  - plain-command startup stays compatible with simple non-POSIX login shells;
  - missing/failing tmux leaves the normal remote shell interactive;
  - jump hops and one-shot transfer, exec, forwarding, and key-install sessions
    do not auto-launch tmux.

## Future Extensions

- Roadmap polish only: hashed known_hosts (`|1|…`), IPv6 forward/SOCKS forms,
  optional half-close polish, session-multiplexed `-L`/`-D`.

## Outstanding Verification

Existing SSH capabilities remain required parity. Record current command
results and applicable GUI/CLI/SSH manual checks before claiming a milestone
or release complete. The historical unit-test count below is not current
release evidence. Live tmux scenarios listed below still require verification.

## Verification

```powershell
cargo fmt --check
cargo check --locked
cargo test --locked --all-targets
cargo build --release --locked
cargo run -- --help
.\install.ps1
```

Historical result: formatting/checks pass; 166 unit tests pass (2026-07-30).
These runtime/build checks were not rerun for the 2026-09-06 documentation update.

Live follow-up: exercise create, detach, reconnect, multi-client attach, resize,
mouse, and missing-tmux fallback against a real SSH host.
