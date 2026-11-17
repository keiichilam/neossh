# NEO-SSH detailed reference

Detailed behavior, test-fixture and verification reference. It was written
for the ssh5 `fresh` rebuild (v0.1.146), which is the code base NEO-SSH starts
from; see the top-level README for the consolidated feature list and
FEATURES.md for where each feature came from. A local `vendor/egui-winit-0.28.1` patch
preserves clipboard shortcut modifiers; its provenance, change and licenses
are retained with that dependency.
The GUI loads installed CJK and symbol fallback fonts from standard system
locations. Linux candidates include Noto CJK, WenQuanYi, Symbola and Unifont;
those optional fonts are not bundled with the application.

The complete scope is [the development requirements](requirements/01-product-requirements.md).
Historical implementation and verification notes are in
[progress](history/ssh5-fresh-PROGRESS.md).

OpenSSH import supports `Match all`, `final`, `host`, `originalhost`, `user`, and
`localuser`, including combined conditions, comma-separated patterns and
negation. Host patterns ignore ASCII case; usernames retain case. Matching uses
the user selected by the CLI/profile, or the earlier imported `User` and local
user fallback. Each block keeps the match result from its entry point, and
conditional includes retain that scope. `Match final` requests a second pass
using the resolved hostname; values chosen on the first pass retain precedence,
and forwarding entries are not replayed. Unsupported conditions (including
`exec`, `canonical`, and `localnetwork`) skip their block with a warning.
Import does not execute config commands. Semantics follow the
[OpenSSH configuration manual](https://man.openbsd.org/ssh_config#Match).
`tests/ssh_config_fixture.py` compares generated configs with installed OpenSSH
using `ssh -G`; it opens no connections and runs as part of `tests/validate.ps1`.

## Install on Unix

Run `sh install.sh` to build the locked release and install into
`~/.local/bin`. Use `--install-dir DIRECTORY` for another destination, or
`--binary PATH` to install a trusted prebuilt binary matching the manifest.
The installer checks the staged executable, sets executable permissions and
atomically replaces an existing file. Failed validation preserves the old
installation. Directory and symbolic-link destinations are rejected.
Add the destination to your shell PATH if needed; shell settings are not edited.
Linux installation is exercised by `tests/linux_smoke.py`, including replacement,
failed installs, literal paths, permissions and cleanup. macOS runtime validation
and application bundles remain pending.

## Cross-platform verification

macOS target checking from Windows uses
`powershell -File tests/check_macos.ps1 -ZigDirectory PATH_TO_ZIG -Clippy`.
Install the Rust targets first with
`rustup target add aarch64-apple-darwin x86_64-apple-darwin`.
The default target is Apple Silicon; add `-Target x86_64-apple-darwin` for Intel.
Omit `-Clippy` for `cargo check` only. The script checks all app/test/example
targets, restores compiler environment variables, and saves results under
`.qa/macos-cross-*`. Both architectures pass strict Clippy at v0.1.111.
These checks do not link a macOS executable or run native tests; a macOS SDK
and host are still needed for that acceptance.

Linux portability can be checked from Windows with
`powershell -File tests/check_linux.ps1 -ZigDirectory PATH_TO_ZIG`.
This checks all Linux app, test and example targets using an isolated C
cross-compiler and restores the caller's compiler environment afterward.
It requires the Rust `x86_64-unknown-linux-gnu` target and Zig; verification used
[Zig 0.16.0 from the official release index](https://ziglang.org/download/index.json)
with its published SHA-256. Results and logs go under `.qa/linux-cross-*`.
Add `-Build` to link the Linux executable, or `-BuildTests` to link its library
and application test binaries. Add `-Release` for optimized builds, and pass
`--release` to the runtime runner to use the matching app and clipboard helper.
The runtime results record the selected build profile. `python tests/linux_smoke.py HOST USER KEY`
uploads the Linux executable to an authorized host, verifies startup and key
generation, and cleans its temporary directory. Append Linux test-binary paths
to execute those tests too. Results are retained under `.qa/linux-runtime-*`.
GUI rendering and interactive Linux SSH acceptance remain separate checks.
`--gui-auth` checks native password success/rejection/cancellation and
single/multiple keyboard-interactive prompts, cancellation and subsequent
terminal input. It installs Paramiko 5.0.0 into the owned temporary test
directory and runs synthetic loopback servers there; it does not change host
users, passwords or PAM settings. This mode needs pip and package-download access.
`--gui-key-install` verifies typed-path public-key review, cancellation,
installation with preserved entries and permissions, and subsequent GUI login
with the installed key. It uses an isolated authorization file and forced-command
home under the test directory; the host's normal authorization is unchanged.
`--gui-workspace` exercises native two-tab save/restart, reconnect confirmation,
active-tab input routing and distinct per-tab TERM/environment settings through
an isolated SSH server and Xvfb display. `--gui-tmux-fallback` instead launches
an isolated interactive Bash shell with a saved tmux profile and verifies usable
GUI terminal input after missing-command and failing-command startup errors,
including explicit-disconnect process cleanup.
That run then starts real tmux on a separate test socket, checks pane identity
and shell state across GUI reconnect, native pane resizing, and Ctrl+B,D detach
to a usable outer shell. It also checks reviewed and forced paste in real Vim,
exact Unicode/whitespace saved contents, Vim resize and exit, less paging, and
htop's sort menu and exit. Mouse-enabled Vim checks include cursor placement,
remote drag selection, wheel scrolling, and Shift-local selection with exact
clipboard contents. It stops the owned tmux server and clipboard helper
during cleanup.
The connected session header displays the configured `tmux:<name>`; startup
failures remain visible in the terminal output.
The workspace run also verifies persisted connection
options, private config permissions and cleanup.
The same run checks synchronized terminal input, disabling synchronization,
filter isolation, closing other/right/final tabs and surviving-session input.
It also selects an amber tab color and verifies save/restart restoration, then
toggles F11 focus mode while checking active-tab input and server-reported PTY
dimensions before, during and after the expanded terminal view.
`--gui-keygen` checks the native key dialog: cancellation, passphrase mismatch,
protected generation, OpenSSH public-key comparison, private permissions,
existing-file preservation and retry. It then selects the generated key, saves
it in the fixture profile and verifies an authenticated SSH session with input.
`--gui-unlock` checks encrypted-key cancellation, wrong-passphrase failure and
successful retry, encrypted-source preservation, terminal input and diagnostic
logging without passphrases or terminal text.
`--gui-forward` enters and saves local, remote and SOCKS listeners through
Connection options, verifies local bind-conflict reporting and recovery, and
transfers binary data through every tunnel, including SOCKS IPv4 and hostname
requests. It repeats the transfers after reconnect and checks release of all
three listeners on disconnect and normal exit.
It also verifies remote/SOCKS bind failures, rollback of earlier listeners,
SOCKS authentication/destination rejection replies and successful recovery.
`--gui-jump` saves a two-hop route in the native GUI, verifies rejection of a
changed second-hop key, then checks input, route history, reconnect and cleanup
using two isolated jump servers with distinct host keys.
It also forces rekeying on both hops during a roughly 1 MB terminal-output burst,
verifies new key exchanges in both server logs, waits for complete delivery to
the app and checks subsequent terminal input.
The second hop uses an encrypted key: cancellation and a wrong passphrase must
not reach the target, correct retry must connect, and reconnect must prompt again.
The fixture also checks source-key preservation and passphrase exclusion from
saved settings and logs.
After reconnect it opens the native SFTP browser through both hops, cancels a
reviewed download, confirms a binary download, checks exact contents and private
permissions, then verifies that the terminal still accepts input.

`python tests/render_smoke.py` captures the native renderer directly,
without relying on Windows desktop screenshot services. It uses isolated
synthetic profiles and makes no SSH connections. It captures normal/minimum
window layouts, a long host list and preferences. Inspect the PNGs under
`.qa/render-*`; a successful capture alone is not a visual-quality check.

`python tests/connected_render_fixture.py` additionally connects the GUI
to an isolated loopback SSH fixture and captures normal, minimum-size and
alternate-screen terminal output. It verifies cursor replies, PTY sizing and
server shutdown; inspect the images under `.qa/connected-render-*`.
It also sends about 8 MB of styled output and records connected GUI frame CPU
times, requiring a maximum below 100 ms and a 95th percentile below 50 ms on
the test machine. This checks output processing, not native input latency.
The opt-in `NEO_SSH_QA_CONNECT=1` hook requires `NEO_SSH_QA_SCREENSHOT` and an explicit
GUI target/profile. These render checks do not replace keyboard, mouse,
clipboard or file-dialog interaction acceptance.

`python tests/multitab_load_fixture.py` restores four isolated loopback
connections, verifies complete output and ongoing progress in each tab, records
frame times, and checks that GUI exit closes all connections. Its screenshot-only
`NEO_SSH_QA_RESTORE_CONNECT=1` hook permits explicitly configured workspace reconnect
and still respects the reconnect-confirmation setting. Results are retained under
`.qa/multitab-load-*`.
Add `--flood` to keep all servers sending continuously and verify that GUI exit
closes their connections during active output. Connection-reset messages from
the fixture servers are expected for that cancellation case.

## Install on Windows

From the repository root, run `./install.ps1`. It builds with the lockfile,
verifies and stages the executable, and installs to `%LOCALAPPDATA%\neo-ssh`.
The installer adds that directory to user PATH; open a new terminal afterward.
If an existing terminal host retains the old PATH, restart that host first.
Explicit default-install acceptance can be repeated with
`powershell -File tests/windows_install_acceptance.ps1` after a release
build. This installs for the current user and retains the installation; it checks
reinstallation and command/help resolution in a fresh PowerShell using saved
Windows PATH values. The routine validation runner uses an isolated installation.
Run `neo-ssh` for the desktop or `neo-ssh user@host` for the CLI.

Use `-InstallDir <directory> -NoPath` for a portable installation without PATH
changes. `-SkipBuild` installs an existing release build only when its version
matches the manifest. Reinstallation replaces the executable atomically; close
a running installed app before upgrading. Older ssh4 / ssh4gui installations (`%LOCALAPPDATA%\ssh4`)
are separate. `tests/install_smoke.ps1` tests installation in a QA directory.

`build.rs` embeds `assets/neo-ssh.ico` and version details (FileDescription
"NEO-SSH") in the Windows executable with the SDK resource compiler; a missing
compiler is a build warning, not an error. The desktop window and taskbar use
`assets/neo-ssh.png`. Both are generated by `python assets/make_icon.py`
(standard library only); edit that script rather than the images.

GUI launch detaches a console owned only by the application; launches from an
existing terminal retain that terminal. CLI help and version output remain available.

For native Windows CLI regression testing, run
`python tests/cli_clipboard_fixture.py` in a dedicated terminal to check
text paste, multiline reviews, bracketed/forced paste and unavailable-clipboard
recovery. It uses a noninteractive Windows station with a fixture desktop and
clipboard, leaving the interactive user's clipboard untouched. Ctrl+V reports
unavailable text locally and keeps the SSH session connected.

For native CLI image-upload acceptance, build the QA helper with
`cargo build --manifest-path Cargo.toml --release --locked --example clipboard_fixture`,
then run `python tests/cli_image_fixture.py` in a dedicated terminal.
The isolated clipboard and loopback SCP receiver check PNG pixels, review
cancellation, rejection, path copy/insertion, transfer cancellation and local
staging cleanup. This complements the real-host SCP file tests.

Run
`python tests/cli_console_fixture.py` from a **dedicated terminal** after
building the release executable. It uses a generated key and a loopback SSH
server, injects console key/mouse events, resizes that console, and verifies
remote bytes, output-only logging and mode restoration. It records results in
`.qa/cli-console-*/results.json`. This separate test needs real console handles;
do not pipe its input/output through the general validation runner. Clipboard,
password prompts and real tmux/TUI acceptance remain separate checks.
A remote shell-channel close ends the session even if its SSH transport stays
open. The CLI exits successfully for reported status 0 and reports an error
for nonzero or missing status.

`python tests/cli_auth_fixture.py` adds native password and
keyboard-interactive acceptance in a dedicated terminal. Synthetic loopback
credentials cover Unicode/Backspace, rejection, Escape/Ctrl+C cancellation,
echoed and secret prompts, console restoration and raw-log exclusion. No real
credentials are used. Evidence is saved in `.qa/cli-auth-*/results.json`.

For an explicitly authorized Unix test host with tmux and Vim installed, run
`python tests/live_cli_fixture.py --host HOST --user USER --key KEY_PATH`
in a dedicated terminal. It uses existing trusted known_hosts entries, an
isolated profile and a unique temporary tmux session. It verifies Vim keyboard
interaction, tmux process persistence across disconnect, profile reattachment,
Ctrl+C and the `--no-tmux` override, then removes its test session. Results go to
`.qa/live-cli-*/results.json`. Live alternate-screen resizing remains a manual
check because this ConPTY does not retain the requested viewport dimensions.

For real-host file acceptance, build `live_files_probe` with
`cargo build --manifest-path Cargo.toml --example live_files_probe`.
Run `target/debug/examples/live_files_probe.exe TARGET KEY KNOWN_HOSTS LOCAL_EVIDENCE_DIR`
only against an authorized Unix host. It requires existing trusted host keys,
creates a random `/tmp/ssh4-qa-*` directory, and checks SFTP/SCP empty and binary
round trips, literal paths, refusal to overwrite, listing, chmod, rename and
cleanup. It removes its remote directory and retains local fixtures and a
`results.toml` report. This exercises the shared SSH/file engine; GUI transfer
interaction remains a separate acceptance check.

`live_tmux_probe` uses the same four arguments as `live_files_probe` and needs
tmux on the authorized host. It creates one random tmux session, checks three
real PTY sizes through the terminal engine, and verifies that bracketed
multiline paste preserves literal text and waits for Enter. A second client
attaches to the same session; both must receive shared output, and the second
must remain usable after the first disconnects. It removes its
test session and writes `results.toml`. Native window-resize and clipboard
integration still require separate UI acceptance.

`live_directory_probe` takes the same four arguments. It checks literal working
directory restoration and OSC 7 reports on a Unix host with Python 3 and tmux,
including a missing directory and new/existing tmux sessions. It creates and
removes one unique temporary directory tree and tmux session and retains a
`results.toml` report. Native workspace save/reconnect interaction remains separate.

## NEO-SSH additions

Features ported from ssh4 and ssh4gui on top of the rebuild (see FEATURES.md):

- **Configuration** — `~/.neo-ssh.toml`; on first run `~/.ssh4.toml` is imported
  read-only. `auto_tmux = true` without a name becomes `tmux_session = "ssh4"`;
  a global `reconnect_countdown_secs` becomes each profile's `reconnect_delay_secs`;
  an ssh4gui `host = "user@host:port"` is split up, and `port = 0` becomes the
  ssh_config `Port` for that host, else the port in `host`, else 22.
- **Themes** — Midnight, Forest, Amber, Slate, Amber Phosphor, Green Phosphor and
  Paper, each with its own 16-colour palette. A contrast guard swaps unreadable
  text (for example ink on a highlight) to near-black or near-white; pictures
  drawn with block, braille or box characters keep their colours unless a
  selection, search or bracket highlight is behind them. OSC 4
  palette queries are answered from the active theme.
- **Scrollback length** — Preferences sets `scrollback_lines` (100–50000,
  default 10000); a profile's own `scrollback_lines` overrides it, and the CLI
  accepts `--scrollback-lines`. The on/off switch still disables scrollback.
- **Command palette** — Ctrl/Cmd+Shift+P opens a filter box; type words, use Up/Down,
  Enter runs and Escape closes. Entries include new/close tab, focus, sync input,
  find, search tabs, remote files, clipboard image, snippets, preferences, text
  size, diagnostics, key install, shortcuts, `Theme: …` and `Snippet: …`
  (snippets open their review window). **Shortcuts** lists all shortcuts.
- **Paste review** — **Send line by line** types each line followed by Enter,
  without bracketed paste, honouring sync input. The review (and the CLI's
  multiline prompt) shows control and bidirectional-formatting characters
  escaped, such as `\u{1b}`, so they cannot hide or reorder the text. Inside
  bracketed paste, ESC and ^C are dropped, so pasted text cannot end the paste
  early and run the rest as typed commands.
- **Keyboard** — App commands use Ctrl+Shift (Cmd+Shift on macOS): Ctrl+Shift+P
  palette, Ctrl+Shift+T new connection, Ctrl+Shift+F find, Ctrl+Shift+I
  clipboard image (opens its upload review in Remote files). Plain Ctrl keys
  reach the remote, including Ctrl+N/P/K/F, Ctrl+X, Ctrl+_ (Ctrl+Shift+-) and
  Ctrl+/ (undo), Ctrl+[ ] \ and Ctrl+Space. Ctrl+= / Ctrl+- / Ctrl+0 make the
  terminal text larger, smaller or the saved size (egui's whole-window
  keyboard zoom is off; Preferences sets the interface zoom). On Windows,
  Ctrl+Insert only copies (never interrupts) and Shift+Insert pastes with the
  normal multiline review. Ctrl+V pastes; Vim's block selection is also on Ctrl+Q.
  Earlier ssh4/ssh4gui chords moved: Ctrl+K→Ctrl+Shift+P, Ctrl+N→Ctrl+Shift+T,
  Ctrl+F→Ctrl+Shift+F, Ctrl+P→Ctrl+Shift+I.
- **Terminal view** — the pointer hides while typing and returns on movement;
  Alt+hover shows when the row under the pointer last changed (live screen
  only). Header chips show PASTE / MOUSE / SCROLL +N, forwards, and background
  edits. **Command line** toggles a one-line entry that sends text plus Enter;
  pasted text with line breaks or control characters goes to the paste review.
  **Resume tmux** replaces Reconnect when the tab has a tmux session.
- **Tabs and hosts** — middle-click closes a tab; Enter in **Find open tabs**
  switches to the first match; the host filter matches names, users and hosts.
- **Keys** — **Browse…** picks a private key; **Copy public key** copies the
  public half of an OpenSSH private key (or the `.pub` file of a PEM key);
  **Copy install command** copies a one-line command that runs its own `sh`,
  ends an unterminated last line, and appends the key to
  `~/.ssh/authorized_keys` unless it is already there. An entry for the same
  key with another comment or with options (`restrict`, `command=`) counts as
  present, so its restrictions are never lifted by a bare copy; **Install key**
  follows the same rule. Commented-out lines do not count. Both use the key the
  connection would use: the typed path (with `~` and `%` tokens), the profile,
  ssh_config `IdentityFile`, then `id_ed25519`/`id_rsa`.
- **Local pane** — select a local entry to rename, copy, move or delete it, or
  create a file or folder. Existing entries are never replaced, only empty
  folders can be deleted, copies are staged and removed on failure or cancel,
  and moves across volumes delete the source only after a complete copy, and
  only files that still hold the bytes copied: a file changed meanwhile is kept
  and the move stops with an error naming it. Read-only files move like they
  do within a volume.
- **Remote edit** — see FEATURES.md "Remote edit".
- **CLI** — `--once` uploads the clipboard image as
  `DIR/clip_YYYYmmdd_HHMMSS.png` (UTC) and exits; DIR is `--remote-dir`, the
  profile's `remote_dir`, or `/tmp`. `--remote-dir` also pre-fills the
  Ctrl+Shift+I prompt. `--save-profile NAME` saves the resolved options (never passwords);
  forwards added with `-L`/`-R`/`-D` are saved together with the imported
  forwards of that kind that were active, because a profile's forward list
  replaces the imported one;
  `--timeout SECONDS` (1–300) overrides the connect timeout. Aliases: `-u`,
  `-k`/`--key`/`--identity-file`, `--local`/`--remote`/`--dynamic`,
  `--tmux-session NAME`. On Windows the console uses UTF-8 (code page 65001)
  and restores the original code pages on exit. After each session a summary
  line reports duration and bytes sent/received.

## Usage

Use **Preferences** in the sidebar to save interface zoom, terminal text size,
terminal theme, keep-alive and scrollback choices. Display changes apply after
saving; keep-alive and scrollback changes take effect on new connections.
Preferences receives keyboard focus when opened and disables workspace controls
until closed. Press Escape to discard its unsaved changes.

Upload reviews offer **Use SCP instead of SFTP**. **Upload with SCP…** also works
without a remote directory listing. SCP uploads require a POSIX host with `scp`,
`mktemp` and `ln -T`; existing destinations are never replaced. Uploads and
downloads automatically use SCP if the server explicitly refuses the SFTP
subsystem. Transfer reviews explain this and progress shows when fallback is
active. File errors, transport errors and timeouts do not trigger a second
transfer. Directory browsing and file actions fall back to shell commands after
the same explicit subsystem refusal. The shell backend needs POSIX tools and
GNU or BSD `stat`; rename requires GNU `mv` with `-n -T`. Listings preserve spaces
and line breaks in UTF-8 names. Creation and rename preserve existing targets;
folder deletion accepts only empty directories. Cancellation stops waiting, but
a submitted remote change may already have completed, so refresh before retrying.
Shell listing and file-action requests time out after 30 seconds; cancellation
and timeout have distinct messages. Both close the request's exec channel.

**Rename/move…** accepts a new name, a path relative to the displayed remote
folder (such as `../archive/report.txt`), or an absolute remote path. Include the
destination entry name; the destination folder must already exist. Review the
displayed source and destination before confirming. Existing targets are preserved.

**Download with SCP...** works without a remote listing: enter the remote
folder, the filename, and a new local destination, then confirm the transfer.
Listed files also offer **Use SCP instead of SFTP** in the download review.
Downloads require a POSIX shell with `scp` and support one regular file at a
time. Progress and cancellation are available; incomplete local staging is
removed and existing destinations are preserved. A successful SCP exit status
is required before publishing the local file.

Jump routes are available through **Connection options → Jump hosts**, saved
profiles, OpenSSH `ProxyJump`, or CLI `-J edge,ops@inner:2222`. Up to eight hops
are supported, including imported aliases and nested ProxyJump settings. Each hop
uses its own imported username, identity and authentication policy, with separate
host-key verification and password prompts. Destination passwords are not reused
for hops. Use `-J none` or enter `none` in the GUI to override an imported route.
Routing failures stop the connection; the app does not fall back to direct access.

To install a public key, connect to the destination account, open **Commands →
Install public key**, choose its `.pub` file or type its path and click
**Load public key**, review the account and fingerprint,
then select **Install on this account**. This supports POSIX hosts with standard
shell tools and updates `~/.ssh/authorized_keys` while preserving existing entries.
Reconnect with the corresponding private key to verify access. Native review,
cancellation, installation and subsequent login were verified against an isolated
authorization file; see [release acceptance](tests/RELEASE_ACCEPTANCE.md).

```powershell
cargo run --manifest-path Cargo.toml --locked
cargo run --manifest-path Cargo.toml --locked -- user@host
cargo run --manifest-path Cargo.toml --locked -- --gui user@host
cargo test --manifest-path Cargo.toml --locked --all-targets
```

The development binary is `target/debug/neo-ssh.exe`. GUI connections
support ordered password/key/agent/keyboard-interactive authentication, host trust prompts,
saved profiles, tabs, terminal input/output, scrollback, selection, paste review,
and named tmux startup. OpenSSH aliases and advanced profile options use the
same resolver in the GUI and CLI. The CLI shares the terminal engine and SSH worker.

Host keys: a changed or revoked key is rejected. An unknown host prompts, or
is accepted under ssh_config `StrictHostKeyChecking accept-new`. A host
already known only by keys of other types always prompts with a warning
(accept-new never applies to it), and accepting records the new type beside
the old. **Require a known host key** (a profile's `strict_host_key`) rejects
unknown keys on the destination and on every ProxyJump hop.

Use **Find open tabs** to filter by title, host, username, or saved profile.
Filtering leaves the active terminal open. Right-click a tab for **Close other
tabs**, **Close tabs to the right**, or **Tab color**. Colors are restored with
the workspace. Closing a different tab preserves the active connection; closing
the last tab opens a blank connection form.

Enable **Sync input** in the status bar to send terminal keys and pastes to
every connected tab. A banner lists the destinations; sync starts off each
launch. Keys and bracketed pastes use each terminal's own modes. Mouse events,
protocol replies, authentication prompts, snippets and file commands remain
specific to their connection. Multiline paste review lists its destinations;
new connections and sessions reconnected during review do not receive that paste.

Terminal **Ctrl+C** copies the frozen selection and clears its highlight; with
no selection it sends an interrupt. **Ctrl+Shift+C** only copies. Right-click a
selection for **Copy / Paste / Search online**; right-click without a selection
pastes clipboard text, reviewing multiline content first. **Ctrl+Shift+V** skips
that review. Shift overrides remote mouse tracking for local selection/menu use.

In the CLI, **Ctrl+Shift+I** captures a clipboard image as PNG, asks for the
remote file path, and confirms an SCP upload on the active connection. Press
Ctrl+Shift+I again to cancel capture or an active transfer. CLI Ctrl+Shift
chords (and Ctrl+V paste) are taken on Windows consoles, which report Shift
with Ctrl; other terminals send the plain Ctrl key to the remote, so paste
there with the terminal's own paste (reviewed the same way) and upload images
with `--once`. Successful uploads copy the
remote path to the clipboard; an optional prompt enables inserting its
POSIX-quoted path into the terminal without pressing Enter. Existing remote
files are preserved. SCP requires the POSIX tools described above.

Use **Find / Ctrl+Shift+F** to search a snapshot of output and retained scrollback.
Search supports case matching and previous/next highlights across wrapped
lines. The session continues receiving output; **Search / refresh** captures
new output, and **Back to live output** restores terminal input.

**Command snippets** in the sidebar opens the saved-command editor. Create,
edit, or delete snippets there, then review the command and active destination
before sending. The optional Enter checkbox submits the command after pasting.
The name field receives focus when opening or selecting a snippet. Ctrl/Cmd+S
saves the draft; Escape closes the editor and discards unsaved changes.
Tab moves from the command field to the Save and Delete controls.

Right-click a saved connection in **Your hosts** and choose **Delete saved
connection…** to review its removal. Confirm to remove it from the saved
configuration, or use Cancel/Escape to keep it. Open tabs and remote sessions
remain open. A failed configuration write preserves the saved connection.
To rename a saved connection, open it, edit **Saved name** under **Connection
options**, and save. The old entry is replaced only after the write succeeds.
An occupied name receives a numeric suffix to preserve the other connection.

After disconnecting, the tab returns to its prefilled connection form. Use
**Reconnect** to retry, or **Review previous output** to inspect retained output
and **Edit connection** to return to the form. Scrollback is cleared only when
the next connection succeeds; the last connection error stays visible meanwhile.

**Workspace settings** controls restoring tabs and reconnecting on launch.
Changed tab settings save automatically, excluding passwords. By default,
restored tabs remain disconnected. Enable reconnect and its confirmation
preference there; **Save workspace now** also retries a failed autosave.
When the remote shell emits OSC 7 directory reports, tabs remember that path for
reconnection. **Remote directory on reconnect** in connection options lets you
edit or clear it. Restoration sends a quoted `cd` command to a POSIX shell before
tmux startup, if configured; existing tmux panes retain their own directory.
Shells without directory reports require entering the path manually.

Advanced connection settings can schedule one retry after a session drops.
The countdown has a cancel button. **Connection history** shows recent phases,
and the last error remains visible until a connection succeeds.

Connection setup has a cancellable wait and tries up to 16 DNS addresses within
the configured timeout. Cancelling also interrupts SSH transport reads and writes,
including a server that stalls before sending its SSH banner. OS DNS/TCP calls
already in progress may finish in the background; at most eight setup workers
can remain active.

Typed input and pastes share an ordered 8 MiB queue (up to 1 MiB per submission).
Large pastes send in small steps so a full remote receive window does not prevent
reading terminal output. If the queue reaches its limit, the app reports that the
new input was not sent; wait before retrying it.

Configure local forwards in advanced connection settings or with
`-L 8080:service.internal:80`. Omitting the bind address listens on loopback.
Repeated OpenSSH `LocalForward` directives are also imported. In the GUI, a
nonempty forward list in the connection settings (or from `--gui -L/-R/-D`)
replaces the imported forwards of that kind, whether or not the connection is
saved.

Use `-D 1080` for a loopback SOCKS5 proxy, or configure SOCKS listeners in the
advanced connection settings. It supports TCP CONNECT with remote DNS,
IPv4 and IPv6 destinations. OpenSSH `DynamicForward` is imported too.

Use `-R 8080:localhost:3000` to make the SSH server's loopback port 8080
reach local port 3000. Remote forwards also have an editor in advanced settings;
repeated OpenSSH `RemoteForward` directives are imported. Port 0 requests a
server-assigned port, reported in the connection timeline. Server denial stops
connection startup with an error. The server's forwarding policy controls
which bind addresses are allowed.

To test without editing your normal configuration, pass
`--config .qa/ui.toml`. Host verification still uses your normal
`~/.ssh/known_hosts`; the loopback fixture below uses an isolated trust file.
Existing OpenSSH hashed entries are recognized, including changed and revoked
keys. The fixture requires `ssh-keygen` on PATH to verify this compatibility.

**Remote files** on a connected tab opens the SFTP directory browser. Enter a
path, refresh, go up, or double-click a folder; sizes and permissions appear
alongside filenames. Cancel or close the window to stop a pending listing.
Select an entry to rename or delete it, or choose **New file** / **New folder**. New files are empty, use private
permissions (0600), and never replace an existing entry. Review the exact
path before confirming. Deletion supports files and empty folders; successful
actions refresh the listing. **Permissions** edits octal modes for a selected file
or folder after path review, including special bits; changes are not recursive.
**Download** offers a native destination picker,
path review, progress and cancellation. Choose a new local filename: downloads
never replace an existing file, and partial files are cleaned up. **Upload** lets you choose a local source, review
its remote filename, then watch progress or cancel. Uploads use private (0600)
remote staging files and publish only after writes and close succeed. Existing
remote destinations are preserved. Cancellation requests staging cleanup;
connection loss or server refusal can leave a temporary file, identified in the
error when the transfer window remains open. Overwrite workflows remain pending.
Servers that explicitly refuse SFTP use the shell/SCP fallback described above.

## Verification

The Linux runtime fixture accepts `--gui` to capture the native GUI on the
explicitly authorized test host using its installed Xvfb. It uploads the
cross-built executable and any supplied test binaries, uses an isolated home
and display, retrieves a PNG and logs, and removes its remote test directory.
This checks headless Linux rendering and clean exit; native interactive input
and desktop integration remain separate acceptance requirements.
Add `--gui-input` to exercise Preferences with native keyboard events through
the host's X11/XTest libraries. This implies `--gui` and additionally requires
`xwininfo` and Python 3.11+. It checks Save, reload after application restart,
and Escape cancellation against the isolated configuration. The keyboard test
uses [X.Org's XTest API](https://www.x.org/releases/X11R7.5/doc/man/man3/XTestFakeKeyEvent.3.html)
on its own display; it does not send input to an existing user desktop.
The same mode checks snippet creation, editing, cancellation, deletion and
restart persistence. Use `--gui-profile` for the separate saved-profile
workflow: create, edit and rename after restart, repeated saves without duplicates,
password exclusion, context menu, Cancel/Escape/close, confirmed removal and
restart, with an unrelated profile preserved. It uses native pointer and
keyboard events on the isolated display and retains captures for review.
Use `--gui-files` for native SFTP listing and New file review/cancellation/
creation checks, plus folder creation, double-click navigation, Up, rename and
reviewed permission changes, deletion cancellation/confirmation and nonempty-folder
protection, plus reviewed downloads with exact-content comparison, cancellation,
existing-destination preservation and recovery after rejection. Reviewed binary
uploads verify exact contents, private permissions, staging cleanup and
review cancellation followed by a successful retry, and preservation of an
existing remote file when a conflicting upload is rejected. An owned SFTP
worker pause after at least 1 MiB of verified payload also checks native
Cancel transfer and remote partial-file cleanup. Downloads similarly verify
cancellation after 1 MiB, local staging cleanup and preservation of the source.
Use `--gui-files-close` to close the owning tab during a partial upload and
verify staging cleanup. Use `--gui-files-disconnect` to check explicit
Disconnect during a partial upload. Both actions allow up to five seconds of
background file cleanup before disconnecting a busy session. Use
`--gui-files-exit` to send a normal window-close request during a partial
upload and verify cleanup before the application exits. Application shutdown
cancels the remaining tabs and waits up to five seconds total for pending
cleanup, including sessions retired by earlier tab closures or Disconnect.
An unresponsive or lost connection can still prevent remote staging removal.
`--gui-files-exit-stalled` holds the owned SFTP worker paused until process
exit, checks the shutdown time limit, and records the remaining staging file
separately from successful cleanup. The fixture then resumes and stops its
owned processes and removes its temporary directory.
Use `--gui-files-download-close` or `--gui-files-download-exit` to check local
staging cleanup after closing a tab or the application during a partial
download. These cases verify that the cancelled destination stays absent and
both the remote source and an existing local file are preserved.
Use `--gui-drop` to deliver native X11 URI-list drops and verify queue disposal
on window close, review cancellation, exact selected-file upload and collision
preservation/cleanup. Its generated source window exists only on the owned display.
Use `--gui-clipboard` for native PNG preview, review cancellation, exact pixel
and alpha transfer, destination preservation and temporary-file cleanup after
cancellation, success and collision failure. The `-Build` Linux cross-build
also links its clipboard helper, which owns an image only on the isolated display.
Use `--gui-clipboard-fallback` for the same checks with the SFTP subsystem
disabled and automatic SCP fallback, leaving the SCP option unchecked.
Use `--gui-local` for native local-folder navigation, selection clearing and
reviewed queued uploads. The fixture clears a two-file queue, queues it again,
cancels the first review and uploads the remaining file. It checks that the
cancelled file stays local, sources survive and uploads preserve exact bytes.
Use `--gui-scp` for the native Upload with SCP and Download with SCP controls.
The isolated server runs real SCP commands for a binary round trip, checking
exact contents, private upload permissions, source preservation and staging
cleanup. It then attempts conflicting downloads and uploads with different
replacement bytes and verifies that both existing destinations survive.
It also cancels upload/download reviews without starting server commands and
verifies a successful download to a new destination after collision errors.
Finally it pauses the owned SCP receiver after payload arrives and checks
Cancel transfer, remote staging cleanup and preservation of existing files.
It also pauses an SCP sender after partial download data arrives, then checks
cancellation, local staging removal and preservation of the remote source.
This case retains review, confirmation, completion and refusal captures.
Use `--gui-fallback` to disable the isolated server's SFTP subsystem and
verify shell directory listing plus automatic SCP upload/download with the
SCP checkbox unchecked. This focused binary round trip checks exact contents,
private upload permissions, source preservation and staging cleanup; server
logs and command records distinguish fallback from explicit SCP selection.
The fixture also uses a remote filename with a space and apostrophe, copies
the completed path through the clipboard, and inserts its shell-quoted form.
It verifies exact terminal input and requires an explicit Enter for submission.
Transfer
reviews scroll when needed; keyboard-focused confirmation and cancellation
buttons are brought into view. Files
created after navigation are checked at their exact filesystem destinations.
This uses the isolated server with the host's
`/usr/lib/openssh/sftp-server`, starts in an owned temporary directory, verifies
empty-file contents and 0600 permissions, and cleans the owned processes/files.
Use `--gui-workspace` to save workspace settings through the native controls,
restart without a profile argument, keep restored tabs disconnected, then restart
and confirm reconnection. It checks persisted settings, permissions and input.
The `--gui-workspace` case edits strict/agent options, auth order, TERM,
environment, timeout and keepalive, then checks profile/workspace persistence
and remote TERM/environment across restart. It also edits the tmux name through
Connection options, verifies profile and workspace persistence, and records the
restored startup command. Real tmux behavior is tested by `--gui-tmux-fallback`.
Use `--gui-reconnect` to end an owned remote session, verify the configured
countdown and automatic reconnection, cancel a subsequent retry, reconnect
manually, and ensure explicit Disconnect does not trigger another retry.
Use `--gui-responsiveness` to run the release-matrix output commands through an
owned Bash session, wait for GUI receive counters to catch up, measure idle CPU,
interrupt a foreground command, interact during continuous output and close its
tab. CPU is reported in cores over five-second windows. Build with `-Build -Release`
and run with `--release --gui-responsiveness` for optimized measurements. Without
`--release` the runner uses debug binaries. Xvfb uses software rendering in both
cases, so retain that measurement context. The evidence includes per-thread CPU
samples. For a controlled software-renderer comparison, add
`--software-renderer-threads 1`; this sets `LP_NUM_THREADS` only for the owned
test GUI and records the override. The default leaves driver settings unchanged.
Use `--gui-text-clipboard` for raw-PTY byte checks of empty, whitespace and
Unicode paste, LF/CRLF/CR-only/trailing-newline review and cancellation,
confirmed and forced paste, selection Escape cancellation, remote Ctrl+C,
and copy-only behavior. It uses the clipboard helper on the owned display.
Use `--gui-links` to verify native Ctrl-click URL and OSC 8 destinations,
remote-path opening, bracket highlights, and selection-menu Search online and
Paste. An owned `BROWSER` handler records URL arguments without opening an
external site; the clipboard helper supplies a distinct paste value. Screenshots
capture the bracket pair and remote file-browser destination for review.
Use `--gui-session` to test Connect, terminal input, manual Disconnect,
retained-output review, and Reconnect with native input. This mode requires
the host's `/usr/sbin/sshd`; it starts a separate unprivileged loopback server
with temporary generated keys and a forced fixture command. It leaves the
normal SSH service and authorized keys unchanged, checks worker shutdown,
and removes the owned server, display and temporary files after testing.
It also sends a saved snippet with Enter disabled, verifies submission waits
for a manual Enter, then reloads it and verifies sending with Enter enabled.
Terminal search captures cover matching and missing queries, Next/Previous
wraparound, case-sensitive and insensitive refresh, local-only query input,
Close-button and Escape dismissal, and successful terminal input afterward.
Clipboard checks drag-select an echoed word, verify exact context-menu Copy
text using the native clipboard helper, copy with Ctrl+C, and verify
exact server input after both Ctrl+V and right-click paste. Multiline checks
verify that review waits for confirmation, Cancel sends nothing, confirmed
lines arrive exactly, and right-click paste also opens review. Shortcut checks
exercise Ctrl+Shift+V without review and Ctrl+Shift+C without a selection,
verifying the latter does not interrupt the remote worker. The session worker
also enables bracketed paste in raw PTY mode and compares the exact received
bytes for forced and reviewed paste, then checks input after disabling the mode.
Unicode clipboard checks cover a wide CJK character, a combining accent, emoji,
leading spaces and reverse selection across a wide character's second cell. A
140-character wrapped-line check replaces the remote screen after selection,
scrolls known history using the native wheel, then verifies copying still pastes
the original logical line without a newline.
That check also shrinks/restores the native window, observes both PTY size
changes in the remote worker, and verifies selection survives the resize.

On Windows, run `./tests/validate.ps1` from the repository root for format,
compile, strict Clippy, all-target tests, loopback SSH fixtures, release build,
and isolated installation checks. Logs and `results.json` are saved beneath
`.qa/validation-<id>/`. The SSH fixtures require the Python dependencies
below. `-SkipSsh` and `-SkipInstaller` are available for focused runs; skipped
checks are recorded and do not count as release acceptance. Native interactive
GUI/CLI and real-host acceptance remain separate requirements.
The SSH stage explicitly builds the debug application used for key generation
before running fixtures, so this check does not depend on a previous local build.

```powershell
python -m pip install --target .qa/deps paramiko
cargo build --manifest-path Cargo.toml --locked --bin neo-ssh --example session_probe
python tests/ssh_fixture.py
```

This fixture binds only to loopback, creates ephemeral host keys and trust
files, and never executes shell commands. Its scenarios cover password,
RSA/Ed25519 file keys, rejected-key fallback, MFA, mock Windows-agent signing,
host trust, environment, shell input, CPR ordering, resize, local forwarding,
zero-window forwarding responsiveness, bidirectional half-close, SOCKS5 proxying,
two simultaneous remote forwards, server rejection, stalled agent signing/listing
cancellation, hashed known-host matching/rejection, SFTP browsing/actions/permissions/errors,
download/upload contents, cancellation, cleanup and close failures, leading-zero key-exchange secrets, and
worker shutdown.
It does not establish real-host, installed-agent UI, tmux, or complete SFTP compatibility.

`python tests/pageant_live_fixture.py <path-to-pageant.exe>` separately
checks real Pageant 0.84 authentication through automatic discovery, WM_COPYDATA
and its OpenSSH-compatible named pipe. Build `session_probe` first. The fixture
verifies the official executable's SHA-256, refuses an existing Pageant process,
loads only a generated RSA key, and uses a loopback SSH server. It stops its own
agent and removes the generated private key on completion or failure. Results
are saved beneath `.qa/pageant-live-<id>/`. This does not test encrypted-key
dialogs or the Windows OpenSSH agent service.

For a reproducible native layout capture, set `NEO_SSH_QA_SCREENSHOT` to a PNG
path before launching with an isolated config. The app captures its own egui
viewport and closes after saving. This is a developer QA hook.
With that hook enabled, `NEO_SSH_QA_SIZE=760x480` selects a bounded test window size
and `NEO_SSH_QA_OPTIONS=1` opens advanced connection options for layout inspection.

Hover over an HTTP(S) URL in terminal output to underline it and preview the
destination. Ctrl-click opens it in your browser; dragging does not open links.
Detection follows soft-wrapped terminal lines and wide-character cell positions.
Explicit OSC 8 links also support HTTP(S) destinations, including labels that
differ from the destination. Hover shows the actual destination before opening.
Links stay attached to their cells through scrolling and screen snapshots.

Ctrl-click an absolute POSIX file path to locate it in **Remote files** on the
current connection. Its containing folder opens and the entry is selected and
brought into view. Quoted paths can contain spaces; explicit OSC 8 `file://`
links can carry encoded names. File links always refer to the current remote
connection, including when the URI contains a hostname. This action does not
download, execute or open a local file. Windows drive paths are not supported
by this remote POSIX path workflow.

The terminal supports IME composition: the candidate window anchors to the cursor,
preedit text appears locally with an underline, and committed text is sent once.
Leaving the terminal, opening a modal, or switching tabs clears local composition.

Matching parentheses, square brackets and braces are highlighted at the cursor
or under the pointer. Matching uses displayed text, including nested pairs and
wide-character cells; it does not parse programming-language quotes or comments.

Drop local files onto the app while connected to queue uploads for the active
connection. Open the desired remote folder, choose **Review next upload**, then
confirm each filename and upload. Up to 64 files can wait; folder recursion is
not supported. After a successful upload, **Copy remote path** copies its name,
and **Insert quoted path** pastes a POSIX-quoted path without pressing Enter.

In **Remote files**, choose **Clipboard image** to capture an image as PNG.
Review its preview and remote filename, then start the upload. Capture and PNG
encoding run in the background; temporary PNGs are cleaned up when discarded or
finished. Upload review offers SFTP or SCP, with the same subsystem-refusal
fallback as other uploads.

**Browse local files** opens local folder navigation within the remote file tools.
Double-click folders, select files with checkboxes, and choose **Queue selected
uploads**. The queue keeps the same destination-review step used for dropped files.

Choose **Generate SSH key** in the connection form to create an Ed25519 keypair,
review its save location, copy the public key, and use the private key for that
connection. Passphrase protection is enabled by default in the GUI; private-file access is
restricted for both protected and unprotected keys.
Existing keys are preserved. The same generator is available from the CLI:

```powershell
target/debug/neo-ssh.exe --generate-key C:/Users/<you>/.ssh/id_ed25519_new
```

The parent folder must exist. Add `--protect-key` to the CLI command to enter and
confirm a passphrase interactively. Without it, the CLI generates an unencrypted
key. Connected-account public-key installation is available from the Commands menu.

Existing encrypted OpenSSH private keys prompt for a passphrase in GUI or CLI.
Passphrases are not saved. A wrong passphrase ends that connection attempt;
reconnect to retry. Keys exceeding the supported bcrypt work limit can be unlocked
through an SSH agent instead.

Rust panics record a small diagnostic beside the selected config, using a
`.panic.log` extension. Records contain time, version and source location, and
omit panic text and terminal/user data. The log resets before exceeding 64 KiB.

Use **Save diagnostic report** in the command palette to export terminal state,
traffic counters, visible text and the latest 64 KiB of remote output. Reports
exclude local input buffers and connection configuration, but remote output may
contain echoed input. Saving runs in the background and preserves existing files;
choose a new filename for each report.
The in-app form accepts an absolute destination directly and offers an optional
system chooser. Cancel, Escape or closing the form creates no report.
Use `--gui-diagnostics` in the Linux runtime fixture to verify logging, export,
cancellation, existing-file preservation and recovery to a new destination.

The sidebar's **Diagnostics → Debug logging** switch writes connection-state
changes, prompt waits, traffic counters and terminal dimensions beside the selected
config (`.debug.log`). It excludes terminal text and connection configuration.
Logging starts off each launch, runs in a bounded background worker and resets
the file before it exceeds 1 MiB. Turning it off drains and flushes queued records.

Native CLI reconnect acceptance can be run in a dedicated Windows console with
`python tests/cli_reconnect_fixture.py`. It owns two loopback SSH sessions,
accepts the first reconnect prompt, declines the second and verifies restored
console modes, continued terminal input and output-only logging. Successful remote
shell exit honors reconnect settings; explicit local quit and --no-reconnect exit
without that prompt. Windows input and output modes are restored on teardown.
