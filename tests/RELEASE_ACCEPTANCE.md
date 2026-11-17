# Release acceptance — fresh v0.1.146

Development is not yet accepted as complete. This is the current work queue;
PROGRESS.md retains historical evidence and limitations for each version.
Source requirements are [product requirements](../../docs/redevelopment/01-product-requirements.md)
and the [release matrix](../../docs/redevelopment/04-validation-and-release.md).
Implementation, automated regression and native interaction are distinct claims.

## Verified baseline

| Area | Evidence (paths relative to fresh/) |
| --- | --- |
| Full automated suite | `.qa/validation-b258de8078c149f88ca2ed210e4727cb/results.json`: all 12 gates, 149 app tests, one backend test, 48 OpenSSH comparisons and 101 SSH scenarios; v0.1.146 rerun, no skipped gates |
| Windows CLI keyboard, mouse, resize, logging, exit | `.qa/cli-console-1fb4e28826d8486cb42b444455e26dd6/results.json`: five cases including final output/query draining |
| Windows CLI password and keyboard-interactive | `.qa/cli-auth-8b64f7463d6d4996a87a43d8c0fed724/results.json`: six success/failure/cancellation cases and console restoration |
| Windows CLI clipboard | `.qa/cli-clipboard-a8d06c80a02b4f6eb4bdff68e8ae53d6/results.json`: Unicode/whitespace, mixed newline bytes, normal/bracketed/forced paste, review cancel/confirm, empty-clipboard recovery, output-only logs and console restoration; `.qa/cli-image-20caf1fa234349f7bdf764e0b502b110/results.json`: exact PNG pixels/bytes, review cancellation, SCP rejection, quoted insertion, completed-path copy, active cancellation and local staging cleanup |
| Live CLI Vim and tmux | `.qa/live-cli-29b51e670bfd43c3b2b29d1a3c7bef64/results.json`: Vim entry/exit, tmux process persistence/reattachment and profile override; live tmux resize explicitly unverified |
| GUI profiles, terminal, paste, search and files | PROGRESS.md v0.1.39–0.1.88 records native cases, including Unicode/wrapped selection, bracketed paste, SFTP/SCP actions, cancellation and shutdown cleanup |
| Native diagnostic security | `.qa/linux-runtime-c54ab7ca7d45473787ffcd6245911364/results.json`: logging/export, cancellation, output-only capture and private destination |
| Native clipboard and drops | `.qa/linux-runtime-5560f76b0c7046f8955cafe54a94fe5a/results.json`, `.qa/linux-runtime-03e2d56884e446fc88aa37c9852360a6/results.json`, `.qa/linux-runtime-990714a5788a451799fe4f9fba77abbc/results.json`: image/drop success, cancellation, collision preservation and cleanup, including automatic SCP fallback |
| Workspace and tab controls | `.qa/linux-runtime-de0ea356c79a4c1095739e6fb9faf7bb/results.json`: two-tab restore, active identity, TERM/environment, sync input, tab filter and close-other/right/last |
| Key generation and unlock | `.qa/linux-runtime-05801156f9884d6aab43f2d2a5b80c13/results.json`, `.qa/linux-runtime-d061d732119f49ee9c2826a3f8ec5c0e/results.json`: native generation, use-key authentication, cancellation, wrong/correct passphrase and metadata secret exclusion |
| Native forwarding | `.qa/linux-runtime-cbef0d9f496448eb88de66c42e1461fc/results.json`: local/remote/SOCKS relay, saved controls, failures, rollback, reconnect and listener cleanup |
| Native jump routing | `.qa/linux-runtime-08cd014b6abc42fa8cd95983b7266a8a/results.json`: two hops, changed-key rejection, encrypted-hop prompts, rekey/full output, reconnect and reviewed SFTP download; connection-history screenshot recorded with v0.1.103 |
| Packaging | Windows isolated installer in validation results; native Linux installer/startup and failure preservation in the v0.1.106 runtime result above; default Windows installation/reinstallation, saved user PATH and fresh PowerShell command/help resolution pass in `.qa/windows-install-bd86fecdc2054217b6d144c78e7a2170/results.json` at v0.1.108 |

Native GUI authentication and key management are now verified: password success,
rejection and cancellation, single/multiple keyboard-interactive prompts,
keyboard-interactive cancellation, exact post-authentication terminal input and
profile/app-log secret exclusion pass at v0.1.123 in
`.qa/linux-runtime-176dd455cd2646cd9f73de0c550e7e4d/results.json`.
Public-key copy passes at v0.1.122 in
`.qa/linux-runtime-66f9ab96eb814517bee188a9814ca1f7/results.json`; reviewed
installation/cancellation, preserved authorization and subsequent GUI login pass
at v0.1.121 in `.qa/linux-runtime-20534311eefc4f63973894dfd366c700/results.json`.

## Verified tmux and real TUI acceptance

Native tmux name editing, profile/workspace save, both restore choices and the
restored startup command pass at v0.1.132 in
`.qa/linux-runtime-746dc4e2189149c6b64fab12e16a278b/results.json`.
Reviewed screenshots show `daily` in the GUI field and `tmux:daily` after restore.
This fixture records the startup command; actual tmux behavior is established by
the separate native real-tmux runs below.
Native Vim/tmux mouse placement, visual drag, wheel scrolling and Shift-local
selection with exact clipboard text pass in
`.qa/linux-runtime-5e1ae95d482046389db82218a360a8f0/results.json` at v0.1.119.
This fixed local selection completion when Shift and mouse release share a
frame. The native run verifies the original file remains unchanged and all
owned processes stop; the unit regression also verifies remote hover resumes.
Reviewed and forced Vim paste with exact Unicode/whitespace save checks,
Vim resize/restore and exit, less paging/exit and htop sort-menu/exit pass in
`.qa/linux-runtime-d29c51f9893a4c30bb729a91aaf14908/results.json` at v0.1.117.
This exposed and fixed Escape removing terminal focus. The new regression
verifies Escape, Tab, arrow keys and subsequent text reach the remote session.
Existing engine and live CLI tmux tests do not establish every native case.
Missing/failing-tmux usable-shell behavior passes through the native GUI in
`.qa/linux-runtime-0af34d635c7e4dc79dc20684bd4ad1fa/results.json`: actual Bash
status 127 with tmux absent from PATH, status 42 from a failing tmux function,
subsequent shell commands, explicit disconnect and process cleanup.
The previously missing configured `tmux:<name>` header indicator is implemented
at v0.1.114 and visually verified in both native fallback screenshots from
`.qa/linux-runtime-53ff7acb6855496f83e155a6019864d5/results.json`.
Native real-tmux attach/reconnect, retained pane PID and shell variable,
pane resize/restore and Ctrl+B,D detach to a usable outer shell pass in
`.qa/linux-runtime-50622325229b4170bbb39ada1336b6f5/results.json` at v0.1.115.
Pane dimensions changed from 104x24 to 131x28 and back. The owned tmux server
uses a separate socket and is stopped during cleanup.

## Verified GUI terminal and controls

Advanced profile controls pass at v0.1.133 in
`.qa/linux-runtime-8cb11bf2056745f7bdde190610dd09dd/results.json`: native strict
and agent toggles, key/agent order, TERM, changed environment, timeout and
keepalive edits; profile/workspace persistence and remote TERM/environment
before and after restart. See [PROFILE_ACCEPTANCE.md](PROFILE_ACCEPTANCE.md)
for the setting-by-setting evidence and its limits.
Clipboard requirements are reconciled
row by row in [CLIPBOARD_ACCEPTANCE.md](CLIPBOARD_ACCEPTANCE.md).
Frozen selection across remote replacement output, native history scrolling
and window shrink/restore passes at v0.1.131 in
`.qa/linux-runtime-61f54dc2e5104d4093643d92cd9e0ffb/results.json`.
Reviewed screenshots show HISTORY 14/15 and Back to live output after the
wheel gesture; the final paste equals the original 140-character logical line.
The same run rechecks session input/reconnect, clipboard shortcuts, search,
snippets, exact bracketed paste, Unicode and owned-process cleanup.
Raw-PTY GUI checks pass at v0.1.130 in
`.qa/linux-runtime-46dd9bd28c344a84a5597e4ab68f0543/results.json`: exact
LF/CRLF/CR-only/trailing-newline review/cancel/confirm, empty/whitespace/Unicode
paste, forced mixed newlines, local selection Escape, remote Ctrl+C byte and
no-selection copy-only. This fixed Ctrl+V CRLF normalization in the GUI backend
and invisible CR-only breaks in the preview. Payload bytes remain unchanged.
Native Ctrl-click URL, OSC 8 destination, remote-path opening with the clicked
file selected, bracket-pair highlighting, exact selection Search online and
clipboard-based context Paste pass at v0.1.127 in
`.qa/linux-runtime-730dab74f0224bce9217633be3f07203/results.json`.
The isolated URL handler records exact arguments without external browsing.
This exposed and fixed menu focus restoration and idle IME-disabled events
swallowing Enter; native Paste followed immediately by Enter now submits
exactly once. Unit regressions cover menu input restoration, idle IME shutdown
and continued suppression during real composition.
Search Next/Previous wraparound (14 -> 1 -> 14), case-sensitive no-match and
case-insensitive refresh (14 matches), Close-button/Escape dismissal and
local-only query input pass at v0.1.124 in
`.qa/linux-runtime-62dbaa9eda754a11b9d42b21b74bdc9b/results.json`.
Reviewed context-menu screenshots show Copy/Paste/Search online; Copy yields
exactly `first`, clears the highlight and sends no remote input. The same
run rechecks reviewed/forced bracketed paste, Unicode selection, frozen
wrapped selection after output/resize, reconnect and owned-process cleanup.
Native tab-color selection/save/restart and F11 focus/input/PTY resize pass
in `.qa/linux-runtime-fcdeaa7d147741dc94321b4caac7ead8/results.json`;
reviewed screenshots show amber before/after restart, hidden/restored panels,
and terminal dimensions of 104x25, 131x29 and 104x25 respectively.

## Verified responsiveness

The release-matrix output commands, complete GUI drain, real Ctrl+C interrupt
and recovery, focus-mode interaction during continuous output, busy-tab closure
and owned-process cleanup pass on optimized v0.1.139 in
`.qa/linux-runtime-f780153b2c0e48d4b7f52bbb5615fdd7/results.json`.
The 780,000-byte and 2,123,770-byte bursts drain in 1.51 and 2.52 seconds after
remote completion; closing the busy tab stops its shell and command in 0.45 seconds.
Reviewed screenshots show the restored panels while output continues.
Five-second idle samples after verified output drain are 0.17-0.18 CPU cores,
versus 0.74-0.83 before the last two fixes; blank-form idle is 0.06 cores.
Software-rendering workers account for 0.14-0.15 cores and other threads about
0.03. This is periodic rendering overhead under Xvfb/llvmpipe, with no continued
output backlog or full-core idle loop. It is not a hardware-GPU benchmark or a
zero-CPU claim. Historical debug measurements and driver comparisons are retained
in PROGRESS.md v0.1.134-0.1.138.

SSH arrivals wake the UI directly, unchanged traffic snapshots are suppressed,
and idle fallback follows the two-second workspace autosave interval. Jobs and
reconnect countdowns retain one-second polling; output drain retains 33 ms fallback.
Native automatic retry/cancel/manual retry and explicit-disconnect suppression
pass at v0.1.139 in `.qa/linux-runtime-1b0ca8db977b4dc68568b1228a1cdfd9/results.json`.
Native workspace persistence, restore choices and tab interactions also pass in
`.qa/linux-runtime-4b4c833bdc804fe1a844f4008d31b545/results.json` on the same version.
Existing single/four-tab load, frame-budget and shutdown fixtures complement these
native checks. Further renderer optimization is additional work, not an unbounded
extension of the finite release-performance checklist.

## Verified connection phases

SSH-008 is resolved at v0.1.144. [PHASE_ACCEPTANCE.md](PHASE_ACCEPTANCE.md) records
actual-operation notifications, ordered/failure integration checks, 32 library
unit tests and inspected native history with explicit request-sent semantics.

## Verified native CLI reconnect

The mode audit found normal remote exit bypassing the reconnect preference and
Windows output mode surviving process exit. Both are fixed at v0.1.145.
`.qa/cli-reconnect-48685294b76b4263b4afd2315ade9936/results.json` verifies two
sessions, prompt acceptance/decline, terminal input, prompt input-mode restoration,
final complete console restoration and output-only logging/cleanup. Existing
five console cases pass in `.qa/cli-console-6bcf076a569b4dc18f931c5ebac68344/results.json`;
six auth cases pass in `.qa/cli-auth-750c19c8c81542dfb901b06f3a9a55e8/results.json`.
NFR and required-mode rows are reconciled in REQUIREMENTS_AUDIT.md; NFR-004
remains incomplete for native Mac verification.

## Required acceptance still to finish

These groups are a finite queue, not a claim that every case within a group is
untested. Reuse the cited passing cases and add evidence only for the gaps.

1. **Packaging and portability:** native macOS linking/runtime verification.
   README examples and build prerequisites are audited in
   [README_ACCEPTANCE.md](README_ACCEPTANCE.md); the missing standalone fixture
   app-build prerequisite is fixed, and current full validation/default Windows
   installation pass. Apple Silicon and Intel all-target checks plus
   strict Clippy pass at v0.1.146 in
   `.qa/macos-cross-f823ae9b869e480292a79dea333c67d7/results.json` and
   `.qa/macos-cross-295578a9959c4d688f52f23f7781657c/results.json`.
   These are cross-compilation checks without a macOS SDK/host; they establish
   target-code compatibility, not a linked or runtime-tested macOS release.
   Default Windows installation and fresh-shell PATH resolution now pass.
   This shell was launched with persisted machine/user PATH; existing terminal
   hosts can retain their previous environment until restarted. Interactive
   console teardown remains covered by the native CLI fixtures.
## Completed final release audit

FR/NFR/SSH requirements, modes and every release-matrix group are reconciled in
[REQUIREMENTS_AUDIT.md](REQUIREMENTS_AUDIT.md) and
[RELEASE_MATRIX_AUDIT.md](RELEASE_MATRIX_AUDIT.md). All 12 current-version gates,
native Linux release startup/GUI/installer smoke and default Windows installation
pass. [Release notes](../RELEASE_NOTES.md) contain the exact v0.1.146 results and
explicit limitations. README and security audits are complete. Native Mac
linking/runtime remains the outstanding acceptance item.

## Additional coverage, not new release requirements

The Windows agent matrix permits OpenSSH agent **and/or Pageant**. Real Pageant
authentication is recorded at v0.1.33; testing the disabled OpenSSH service is
additional coverage, not a prerequisite implied by that matrix alone.

Native routed uploads/forwarding and every feature combination on every platform
would add confidence. Required routing, transfers and forwarding still need their
own acceptance; the matrix does not require exhaustive cross-products. Keep
Linux/macOS viability in scope without silently inventing an unlimited OS/UI matrix.

Older PROGRESS.md entries describe what was pending at that time. Later evidence
supersedes those historical gaps only for the cases actually exercised.
