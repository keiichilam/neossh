# NEO-SSH feature consolidation

NEO-SSH merges three Rust SSH clients:

| Source | Folder | What it was |
|---|---|---|
| **ssh4** | `../ssh4` | First egui + crossterm client (June 2026): tabs, profiles, snippets, dual-pane file manager, phosphor themes. |
| **ssh4gui** ("SSHGUI") | `../ssh4gui` | Successor (July 2026): agents, known_hosts, SFTP, ProxyJump, -L/-R/-D, workspace restore, ssh_config import. |
| **ssh5** | `../ssh5` | ssh4gui with UI polish (`src/`) plus **`fresh/`**, an independent rebuild (v0.1.146) with 149 tests and acceptance fixtures. |

**Base:** `ssh5/fresh` was the most complete and best-verified implementation, so
it is the NEO-SSH code base. The ssh5 `src/` app only differs from ssh4gui in UI
copy and layout, which the rebuild already has. Every feature of ssh4 and ssh4gui
was then checked against the rebuild; the missing ones were ported (marked
**ported**) or deliberately left out (see [Not carried over](#not-carried-over)).

Legend: ✓ present · ~ partial · — absent.

## Connection and authentication

| Feature | ssh4 | ssh4gui | ssh5 | NEO-SSH |
|---|:-:|:-:|:-:|---|
| Password, key and keyboard-interactive auth | ~ | ✓ | ✓ | ✓ (ordered auth preference per profile) |
| Agents: Windows OpenSSH pipe, Pageant, `SSH_AUTH_SOCK` | — | ✓ | ✓ | ✓ (on Windows an unreachable `SSH_AUTH_SOCK`, such as a Git Bash socket, falls back to the pipe and Pageant) |
| Encrypted keys with passphrase prompt / retry | — | ~ | ✓ | ✓ |
| known_hosts Ask / AcceptNew / Strict, `@revoked` | — | ✓ | ✓ | ✓ (changed keys rejected; a known host's new key type always asks, with a warning; "Require a known host key" covers every jump hop) |
| ProxyJump / `-J` multi-hop | — | ✓ | ✓ | ✓ up to 8 hops, per-hop auth and host keys |
| Local / remote / SOCKS5 forwards (`-L -R -D`) | — | ✓ | ✓ | ✓ plus header chips (**ported** from ssh4gui) |
| `~/.ssh/config` import incl. `Match`, `Include` | — | ~ | ✓ | ✓ |
| Keep-alive | ✓ | ✓ | ✓ | ✓ global and per profile |
| Connect timeout | ✓ `--timeout` | ✓ | ✓ profile | ✓ profile + `--timeout` (**ported**) |
| Auto-reconnect / retry countdown | ✓ | ✓ | ✓ | ✓; ssh4gui `reconnect_countdown_secs` migrated |
| tmux attach-or-create | ✓ | ✓ | ✓ | ✓; "Resume tmux" button (**ported**), ssh4 `auto_tmux` migrated |
| Connection timeline diagnostics | — | ✓ | ✓ | ✓ |
| Workspace restore (tabs, colours, options) | — | ✓ | ✓ | ✓ |

## Keys

| Feature | ssh4 | ssh4gui | ssh5 | NEO-SSH |
|---|:-:|:-:|:-:|---|
| Generate Ed25519 key (optionally protected) | ✓ | ✓ | ✓ | ✓ GUI and `--generate-key` |
| Install public key on an account | ✓ | ✓ | ✓ | ✓ reviewed install over the connection |
| Browse… for the private key | ✓ | — | — | ✓ **ported** |
| Copy public key of an existing key | ✓ | ✓ | ~ | ✓ **ported** (uses `.pub` or the key's public half) |
| Copy `authorized_keys` install command | — | ✓ | — | ✓ **ported** (one line, own `sh`, never joins lines or duplicates a key) |

## Terminal

| Feature | ssh4 | ssh4gui | ssh5 | NEO-SSH |
|---|:-:|:-:|:-:|---|
| VT100/xterm rendering, scrollback, alternate screen | ✓ | ✓ | ✓ | ✓ |
| DA / DSR / CPR / OSC 10/11/12 replies | ✓ | ✓ | ✓ | ✓ |
| OSC 4 palette queries | — | ✓ | — | ✓ **ported** |
| DEC mouse (SGR, UTF-8), bracketed paste, 1007 | ✓ | ✓ | ✓ | ✓ |
| Selection, copy, right-click menu, Search online | ✓ | ✓ | ✓ | ✓ |
| Multiline paste review | ✓ | ✓ | ✓ | ✓ plus **Send line by line** (**ported** from ssh4) |
| Ctrl+Insert copies only / Shift+Insert reviewed paste | — | ✓ | ~ | ✓ **ported** fix |
| App shortcuts on Ctrl+Shift so Ctrl+N/P/K/F/X/_ reach the remote | — | — | — | ✓ new (Ctrl+Shift+P/T/F/I; Ctrl+=/-/0 text size) |
| Find in scrollback (case, next/prev) | ✓ | ✓ | ✓ | ✓ |
| Links (URLs, OSC 8, remote paths) | ✓ | ✓ | ✓ | ✓ |
| Bracket match | ✓ | ✓ | ✓ | ✓ |
| Readable-text contrast guard | — | ✓ | — | ✓ **ported** (block/braille/box pictures left alone) |
| Hide mouse pointer while typing | ✓ | — | — | ✓ **ported** |
| Alt+hover "row last changed" | ✓ | — | — | ✓ **ported** (relative time) |
| Scrollback length setting (global / per profile) | ✓ | — | on/off | ✓ **ported** `scrollback_lines` + Preferences + `--scrollback-lines` |
| Sync input across tabs | ✓ | ✓ | ✓ | ✓ |
| One-line command entry under the terminal | — | ✓ | — | ✓ **ported** (header toggle "Command line") |
| Mode chips (PASTE / MOUSE / SCROLL+N) | — | ✓ | — | ✓ **ported** |
| CJK / symbol fallback fonts | — | — | ✓ | ✓ |

## Workspace UI

| Feature | ssh4 | ssh4gui | ssh5 | NEO-SSH |
|---|:-:|:-:|:-:|---|
| Tabs, colours, close others / right | ✓ | ~ | ✓ | ✓ |
| Middle-click closes a tab | — | ✓ | — | ✓ **ported** |
| Tab search | ✓ modal | — | ✓ filter | ✓ filter; Enter switches to first match (**ported**) |
| Saved hosts with filter | ✓ | ✓ | ✓ | ✓ filter also matches user (**ported**) |
| Command palette | ✓ filterable | ~ | ~ fixed list | ✓ **ported** filterable, keyboard driven, themes and snippets |
| Shortcut help overlay | ✓ | — | — | ✓ **ported** ("Shortcuts" button / palette) |
| Focus mode (F11) | ✓ | ✓ | ✓ | ✓ |
| Snippets | ✓ one-click | ✓ | ✓ reviewed | ✓ reviewed; runnable from the palette |
| Themes | Amber Phosphor, Green Phosphor, Paper | Midnight, Amber, Forest, Slate | Midnight, Forest, Amber | **all seven** (Slate, Phosphors, Paper **ported**) with per-theme ANSI palettes |
| Preferences dialog (zoom, font, theme, keep-alive, scrollback) | ~ | ✓ | ✓ | ✓ |
| Debug log / panic log / diagnostic report | ~ | ✓ | ✓ | ✓ |
| Windows exe version details (VERSIONINFO) | ✓ | ✓ | ✓ | ✓ **ported** (`build.rs`) |
| Application icon (exe, window, taskbar) | — | — | — | ✓ NEO-SSH (`assets/make_icon.py`) |

## Files and transfers

| Feature | ssh4 | ssh4gui | ssh5 | NEO-SSH |
|---|:-:|:-:|:-:|---|
| Remote browser (SFTP; shell fallback) | exec+SCP | ✓ | ✓ | ✓ |
| Remote new / rename / chmod / delete | ✓ | ✓ | ✓ | ✓ (empty folders only) |
| Upload / download with review, progress, cancel | ~ | ✓ | ✓ | ✓ never overwrites |
| SCP fallback | ✓ | ✓ any error | ✓ on refusal | ✓ on explicit SFTP refusal |
| Drag-and-drop upload | ✓ | ✓ | ✓ | ✓ reviewed queue |
| Clipboard image upload (GUI) | ✓ Ctrl+P | ✓ Ctrl+P | ~ button | ✓ Ctrl+Shift+I / palette (**ported**; moved off Ctrl+P, which is shell previous-history), reviewed |
| Remote **Edit** (open in local editor, re-upload on save) | ✓ | — | — | ✓ **ported** with atomic replace and conflict detection — see [Remote edit](#remote-edit) |
| Local pane: browse and select for upload | ✓ | ✓ | ✓ | ✓ |
| Local pane: new file/folder, rename, copy to, move to, delete | ✓ | — | — | ✓ **ported** (no overwrite, staged copies, cancellable) |

## CLI

| Feature | ssh4 | ssh4gui | ssh5 | NEO-SSH |
|---|:-:|:-:|:-:|---|
| Raw-mode session, mouse, paste review, reconnect prompt | ✓ | ✓ | ✓ | ✓ |
| `-P`, `-l`, `-i`, `--log`, `--no-reconnect`, `--gui` | ✓ | ✓ | ✓ | ✓ |
| `--tmux[=NAME]`, `--no-tmux` | ~ | ✓ | ✓ | ✓ plus `--tmux-session NAME` alias (**ported**) |
| `-J`, `-L`, `-R`, `-D` | — | ✓ | ✓ | ✓ plus `--local/--remote/--dynamic` aliases (**ported**) |
| `-u`, `-k/--key`, `--identity-file` aliases | ✓ | ✓ | — | ✓ **ported** |
| Ctrl+P clipboard image upload | ✓ | ✓ | ✓ | ✓ on Ctrl+Shift+I (Windows console) with `--remote-dir` default; `--once` elsewhere |
| `--once` (upload clipboard image and exit) | ✓ | — | — | ✓ **ported** |
| `--remote-dir`, `--save-profile`, `--timeout`, `--scrollback-lines` | ✓ | — | — | ✓ **ported** |
| UTF-8 console code page on Windows | ✓ | — | — | ✓ **ported** (restored on exit) |
| Disconnect summary (duration, bytes) | ✓ | — | — | ✓ **ported** |
| `--generate-key [--protect-key]` | — | — | ✓ | ✓ |

## Configuration

- File: `~/.neo-ssh.toml` (`%USERPROFILE%\.neo-ssh.toml` on Windows).
- First run imports `~/.ssh4.toml` read-only (shared by ssh4, ssh4gui and ssh5);
  the legacy file is never modified. Translations: ssh4 `auto_tmux = true` with no
  session name → `tmux_session = "ssh4"`; ssh4gui `reconnect_countdown_secs` →
  per-profile `reconnect_delay_secs`; ssh4gui `host = "user@host:port"` is split
  into user, host and port, and `port = 0` becomes the ssh_config `Port` for that
  host, else the port in `host`, else 22. Older array-style profiles, ssh4
  `scrollback_lines` and `remote_dir`, and theme names from all three apps are read.
- Unknown keys are preserved, so the file stays usable by the older apps.

## Not carried over

These older behaviors conflict with deliberate safety decisions in the rebuild
and were intentionally **not** ported:

| Older behavior | From | NEO-SSH behavior |
|---|---|---|
| Accept a changed host key ("dangerous") | ssh4gui | Changed keys are rejected; fix `known_hosts` explicitly. |
| `--password` on the command line | ssh4 | Secrets are never accepted as arguments. |
| Recursive remote delete (`rm -rf`) | ssh4, ssh4gui | Only files and empty folders. |
| Overwriting files on upload/download/copy | ssh4, ssh4gui | Existing destinations are preserved. |
| SCP fallback after any SFTP error | ssh4gui | Fallback only when the server refuses SFTP. |
| Auto-typing uploaded paths into the shell | ssh4gui | "Insert quoted path" button, no Enter. |
| One-click snippet send | ssh4 | Snippets are reviewed before sending. |
| `debug_log = true` enables logging at startup | ssh4gui | Debug logging is switched on per launch. |
| Recursive folder upload / drop folders | ssh4 | Regular files only (folders are refused with a message). |
| CLI local mouse selection (Enter copies) | ssh4 | Mouse goes to the remote; use the terminal's own selection. |
| Status-bar Idle / Heartbeat chips | ssh4 | Rx/Tx counters and the connection timeline. |
| ECDSA / security-key default key discovery | ssh4 | Default keys are `id_ed25519` and `id_rsa` (transport limits). |

## Remote edit

ssh4's **Edit** downloaded a remote file, opened it in the system editor and
re-uploaded it on every save. NEO-SSH keeps the workflow but makes it safe:

- **Review first.** Edit… states that each save will replace the remote file.
- **Private copy.** The file is downloaded into a per-edit temporary folder
  (owner-only permissions on Unix, a current-user ACL on Windows that files
  created inside, such as an editor's saves, inherit).
- **Safe opener.** Windows uses `ShellExecuteW` (no `cmd /C start` quoting
  pitfalls); macOS `open`, Linux `xdg-open`, with the path as one argument.
  File types whose default action runs code (scripts, executables, installers,
  macro-enabled Office files, disk images, web pages and SVG that a browser
  would run, …) are never opened by default (Windows uses their **edit** verb); on
  Unix, extensionless native executables and desktop launchers are refused too.
  Windows copies carry the Mark of the Web, so Office opens them protected.
- **Debounced sync.** A save is uploaded only after the file is unchanged across
  two polls about 2 s apart, so half-written saves are not sent.
- **Atomic replace.** Upload to a hidden temporary file in the same remote
  folder, re-check the remote size/mtime, copy the original owner, group and
  permission bits, then rename over the original (`posix-rename@openssh.com`,
  or a quoted `mv -f -T` shell step; the SCP-only path stages through
  `mktemp -d`). When the owner or group cannot be kept (only root may give a
  file away) or the file has hard links, the shell step writes the contents
  into the original file instead, which keeps its owner, group, ACLs and links.
- **Conflict detection.** If the server copy changed (or was deleted or replaced
  by a link or folder), syncing stops and the edits stay in the local copy.
- **Background sync.** Closing Remote files offers **Close, keep syncing**;
  the tab header shows `EDITING n`, and reopening the window shows the edits.
  Edits continue after an automatic reconnect; Disconnect stops them. Copies
  with unsynced edits (or missing mid-save) are always kept.

This is the only operation that replaces an existing remote file; ordinary
uploads still refuse to overwrite.

Limits: change detection uses size and whole-second mtime; the shell/SCP paths
need GNU `mv -T` and `stat`. A rename keeps owner, group and mode but not ACLs,
xattrs or SELinux labels; SFTP cannot see hard links, so a hard-linked file is
written in place only on the shell/SCP paths. The in-place fallback is not
atomic.
