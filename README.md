# NEO-SSH

A native SSH workspace for Windows and Linux, with a desktop GUI (egui) and a
terminal CLI (crossterm). It consolidates the features of three earlier clients —
**ssh4**, **ssh4gui** (SSHGUI) and **ssh5** — into one code base. See
[FEATURES.md](FEATURES.md) for the full feature matrix and where each feature
came from, and [docs/REFERENCE.md](docs/REFERENCE.md) for detailed behavior and
the acceptance-test fixtures.

## Highlights

- **Connections** — password, key, keyboard-interactive and agent auth (Windows
  OpenSSH agent, Pageant, `SSH_AUTH_SOCK`); encrypted keys; known_hosts
  verification; ProxyJump up to 8 hops; local, remote and SOCKS5 forwards;
  `~/.ssh/config` import (including `Match`); keep-alive; retry; tmux
  attach-or-create; workspace restore.
- **Terminal** — xterm-compatible rendering with scrollback, search, links,
  bracket match, mouse reporting, bracketed paste, multiline paste review
  (send whole or line by line), synchronized input across tabs, seven themes,
  Alt+hover row timestamps, an optional command line, and status chips.
- **Files** — SFTP browser with SCP/shell fallback, reviewed uploads and
  downloads that never overwrite, drag-and-drop and clipboard-image upload,
  chmod/rename/delete, a local pane with new/rename/copy/move/delete, and
  **remote Edit**: open a server file in your local editor and sync every save
  back, with conflict detection.
- **Keys** — generate Ed25519 keys, install a public key on the connected
  account, browse for a key file, copy a public key or an `authorized_keys`
  install command.
- **CLI** — the same SSH engine in a raw-mode terminal, plus one-shot clipboard
  image upload (`--once`), profile saving and the ssh4/ssh4gui flag aliases.

## Install

Windows (builds a release binary and installs to `%LOCALAPPDATA%\neo-ssh`,
adding it to your user `PATH`):

```powershell
.\install.ps1
```

Linux (installs to `~/.local/bin`):

```sh
sh install.sh
```

Requires a Rust toolchain (`cargo`). See [docs/REFERENCE.md](docs/REFERENCE.md)
for portable installs and installer options.

## Usage

```powershell
neo-ssh                              # desktop workspace
neo-ssh user@host                    # CLI session
neo-ssh user@host:2222
neo-ssh -P prod                      # CLI from a saved profile
neo-ssh --gui -P prod                # open the profile in the GUI
neo-ssh -l deploy -i ~\.ssh\id_ed25519 server.example.com
neo-ssh -J bastion,ops@inner:2222 user@host
neo-ssh -L 8080:localhost:80 -R 9000:127.0.0.1:3000 -D 1080 user@host
neo-ssh user@host --tmux             # attach/create tmux session "neo-ssh"
neo-ssh user@host --tmux=deploy      # (or --tmux-session deploy)
neo-ssh -P prod --no-tmux
neo-ssh user@host --log session.log --no-reconnect
neo-ssh user@host --timeout 10 --scrollback-lines 20000
neo-ssh user@host --save-profile prod
neo-ssh user@host --once --remote-dir /tmp   # upload clipboard image, print path, exit
neo-ssh --generate-key ~\.ssh\id_neo --protect-key
```

Older flag spellings still work: `-u` (`-l`), `-k`/`--key`/`--identity-file`
(`-i`), `--local`/`--remote`/`--dynamic` (`-L`/`-R`/`-D`), `--tmux-session NAME`.
Passwords are never accepted on the command line.

### GUI shortcuts

| Shortcut | Action |
|---|---|
| `Ctrl/Cmd+Shift+P` | Command palette — type to filter commands, themes and snippets |
| `Ctrl/Cmd+Shift+T` | New connection |
| `Ctrl/Cmd+Shift+F` | Find in output and scrollback |
| `F11` | Focus mode |
| `Ctrl/Cmd+=` / `Ctrl/Cmd+-` / `Ctrl/Cmd+0` | Larger, smaller or saved terminal text size |
| `Ctrl/Cmd+Shift+I` | Upload a clipboard image (reviewed in Remote files) |
| `Ctrl+C` | Copy selection, or interrupt when nothing is selected |
| `Ctrl+Shift+C` / `Ctrl+Insert` | Copy only |
| `Ctrl+V` / `Shift+Insert` | Paste (multiline text is reviewed) |
| `Ctrl+Shift+V` | Paste without review |
| `Alt`+hover | When did this row last change? |
| Middle-click a tab | Close it |
| Right-click | Paste, or Copy / Paste / Search online |
| Ctrl+click | Open a link, or locate a remote path in Remote files |

The **Shortcuts** button in the header shows this list in the app. App
commands use Ctrl+Shift, like other terminal emulators, so the plain Ctrl
keys reach the remote: Ctrl+N/P (history), Ctrl+K (kill line), Ctrl+F, Ctrl+X
(nano, emacs), Ctrl+_ (undo), Ctrl+[ ] \ and Ctrl+Space. Ctrl+V pastes, so use
Ctrl+Q for Vim's block selection.

### CLI shortcuts

| Shortcut | Action |
|---|---|
| `Ctrl+V` | Paste (multiline confirms; Windows) |
| `Ctrl+Shift+V` | Paste immediately (Windows) |
| `Ctrl+Shift+I` | Upload a clipboard image over SCP (defaults to `--remote-dir`; Windows) |
| `Ctrl+Shift+Q` | Quit (Windows) |

Ctrl+Shift chords need a console that reports Shift with Ctrl, as the Windows
console does; other terminals send them as the plain Ctrl key, which then
reaches the remote. Elsewhere, paste with the terminal's own paste shortcut,
which the CLI reviews the same way, and Ctrl+V stays with the remote (quoted
insert, Vim block selection). Use `--once` to upload a clipboard image there.

### Remote edit

In **Remote files**, select a file and choose **Edit…**. After you confirm,
NEO-SSH downloads it to a private temporary folder and opens it in your default
editor. Each save is uploaded once it is stable and replaces the server file
atomically (temporary file + rename, keeping its owner, group and permissions).
A file whose owner cannot be kept, or that has hard links, is written in place
instead. If the file changed on the server since it was opened, syncing stops
and your edits stay in the local copy. Closing the window offers **Close, keep
syncing**; background edits show as `EDITING n` in the tab header and continue
after an automatic reconnect. Scripts, executables and other file types that
can run code are never opened by default.

### Codex and other alternate-screen apps

Programs that use the alternate screen (Codex, vim, less) keep their output out
of the normal scrollback. To keep a Codex transcript in scrollback, run
`codex --no-alt-screen`, or set `[tui] alternate_screen = false` in
`~/.codex/config.toml` on the server.

## Configuration

Settings live in `~/.neo-ssh.toml` (`%USERPROFILE%\.neo-ssh.toml`): profiles,
snippets, interface zoom, terminal font size, theme (`Midnight`, `Forest`,
`Amber`, `Slate`, `Amber Phosphor`, `Green Phosphor`, `Paper`), keep-alive,
scrollback (`terminal_scrollback`, `scrollback_lines`) and workspace restore.

On first run NEO-SSH imports `~/.ssh4.toml` from ssh4/ssh4gui/ssh5 without
modifying it; the first save writes `~/.neo-ssh.toml`. Passwords are never saved.

```toml
theme = "Green Phosphor"
scrollback_lines = 20000

[profiles.prod]
host = "prod.example.com"
user = "deploy"
port = 22
key_path = "C:/Users/me/.ssh/id_ed25519"
tmux_session = "work"
jump = "bastion"
local_forwards = ["8080:localhost:80"]
scrollback_lines = 50000

[[snippets]]
name = "disk usage"
command = "df -h"
```

## Build and test

```powershell
cargo build --release          # target/release/neo-ssh(.exe)
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
powershell -File tests/validate.ps1   # full gate set incl. SSH fixtures and installer
```

## Layout

| Path | Contents |
|---|---|
| `src/` | Library `neo_ssh` (SSH, terminal, files, config) and binary `neo-ssh` (GUI + CLI) |
| `vendor/` | Patched `ssh-rs` 0.5.0, `vt100` 0.15.2, `egui-winit` 0.28.1 |
| `tests/` | Python/PowerShell acceptance fixtures and audits |
| `docs/REFERENCE.md` | Detailed behavior and verification reference |
| `docs/requirements/` | Product requirements and architecture packet |
| `docs/history/` | ssh5 rebuild progress log and release notes |

## License

MIT — see [LICENSE](LICENSE).
