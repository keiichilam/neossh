# Fresh implementation progress

Started: 2026-09-06. This tracks the fresh project only; historical root-app
completion claims do not apply to it.

Current release acceptance queue: [tests/RELEASE_ACCEPTANCE.md](tests/RELEASE_ACCEPTANCE.md).
Historical pending notes below must be read alongside later evidence.

## Full delivery scope

| Requirement | Fresh implementation status |
| --- | --- |
| FR-001 GUI/CLI launch | GUI/CLI launch and host/profile dispatch verified; see tests/REQUIREMENTS_AUDIT.md FR-001 and native CLI/GUI evidence in tests/RELEASE_ACCEPTANCE.md. |
| FR-002 host parsing | All required forms and default port verified by source and target regression tests; see tests/REQUIREMENTS_AUDIT.md FR-002. |
| FR-003 auth and key management | Username resolution, auth/key discovery, native GUI/CLI prompts, key generation/copy/install and credential handling reconciled in tests/REQUIREMENTS_AUDIT.md FR-003 and tests/SECURITY_REVIEW.md. |
| FR-004 config compatibility | Configuration path/schema compatibility, profiles/snippets, display preferences and tmux persistence reconciled in tests/REQUIREMENTS_AUDIT.md FR-004; native multiline snippet CRUD and restarts verified. |
| FR-005 terminal protocol | Shared protocol encoders/scanner and native terminal interaction reconciled in tests/REQUIREMENTS_AUDIT.md FR-005; current regression and real TUI evidence pass. |
| FR-006 renderer, selection, search | Grid rendering, colors, cell coordinates, wide text, frozen selection, history/search/link/bracket interactions reconciled in tests/REQUIREMENTS_AUDIT.md FR-006. |
| FR-007 clipboard/drop transfers | Native PNG/SCP and drop upload, destination copy/quoted insertion, cancellation/collision/staging cleanup reconciled in tests/REQUIREMENTS_AUDIT.md FR-007 and tests/SECURITY_REVIEW.md. |
| FR-008 remote/local file tools | Required remote/local actions, reviewed transfers, SFTP/SCP fallback and cancellation/cleanup reconciled in tests/REQUIREMENTS_AUDIT.md FR-008; no claim of every platform/feature cross-product. |
| FR-009 diagnostics and heartbeat | Counters/changed heartbeat snapshots, panic/metadata logs and output-only state export reconciled in tests/REQUIREMENTS_AUDIT.md FR-009 and tests/SECURITY_REVIEW.md. |
| FR-010 tmux integration | Named startup, CLI overrides, GUI settings/workspace restore, real tmux reconnect/resize/detach and failure fallback reconciled in tests/REQUIREMENTS_AUDIT.md FR-010. |
| SSH-001 host verification | Required behavior and applicable native/automated evidence reconciled in tests/REQUIREMENTS_AUDIT.md SSH-001. Platform/cross-product limits remain explicit; SSH-008 timeline detail is tracked separately. |
| SSH-002 agents | Required behavior and applicable native/automated evidence reconciled in tests/REQUIREMENTS_AUDIT.md SSH-002. Platform/cross-product limits remain explicit; SSH-008 timeline detail is tracked separately. |
| SSH-003 keyboard-interactive | Required behavior and applicable native/automated evidence reconciled in tests/REQUIREMENTS_AUDIT.md SSH-003. Platform/cross-product limits remain explicit; SSH-008 timeline detail is tracked separately. |
| SSH-004 OpenSSH import | Required behavior and applicable native/automated evidence reconciled in tests/REQUIREMENTS_AUDIT.md SSH-004. Platform/cross-product limits remain explicit; SSH-008 timeline detail is tracked separately. |
| SSH-005 SFTP/fallback | Required behavior and applicable native/automated evidence reconciled in tests/REQUIREMENTS_AUDIT.md SSH-005. Platform/cross-product limits remain explicit; SSH-008 timeline detail is tracked separately. |
| SSH-006 forwarding | Required behavior and applicable native/automated evidence reconciled in tests/REQUIREMENTS_AUDIT.md SSH-006. Platform/cross-product limits remain explicit; SSH-008 timeline detail is tracked separately. |
| SSH-007 jump chains | Required behavior and applicable native/automated evidence reconciled in tests/REQUIREMENTS_AUDIT.md SSH-007. Platform/cross-product limits remain explicit; SSH-008 timeline detail is tracked separately. |
| SSH-008 connection timeline | The v0.1.143 gap is resolved at v0.1.144 with scoped library notifications, configuration/verifier records, ordered/failure SSH assertions and native history inspection. See tests/PHASE_ACCEPTANCE.md. |
| SSH-009 workspace restore | Required behavior and applicable native/automated evidence reconciled in tests/REQUIREMENTS_AUDIT.md SSH-009. Platform/cross-product limits remain explicit; SSH-008 timeline detail is tracked separately. |
| SSH-010 reconnect | Required behavior and applicable native/automated evidence reconciled in tests/REQUIREMENTS_AUDIT.md SSH-010. Platform/cross-product limits remain explicit; SSH-008 timeline detail is tracked separately. |
| SSH-011 advanced profiles | Required behavior and applicable native/automated evidence reconciled in tests/REQUIREMENTS_AUDIT.md SSH-011. Platform/cross-product limits remain explicit; SSH-008 timeline detail is tracked separately. |
| NFR-001 responsiveness | Worker/thread/queue boundaries and native single/four-tab/output/idle/interrupt/closure evidence reconciled in tests/REQUIREMENTS_AUDIT.md and tests/RELEASE_ACCEPTANCE.md. |
| NFR-002 reliability | Worker cancellation, temporary-file cleanup and disconnected forms reconciled. Native CLI prompt and full input/output mode restoration defects fixed and verified at v0.1.145; see tests/REQUIREMENTS_AUDIT.md. |
| NFR-003 security | Specified security checklist reconciled against current source and native evidence at v0.1.141 in tests/SECURITY_REVIEW.md; metadata/crash/input exclusion, credential persistence, private config, quoting, delete confirmation and clipboard/drop failure cleanup covered with explicit remote-output/platform limitations. |
| NFR-004 portability | Windows/Linux execution and platform font/config paths verified; Mac cross-target checks exist but native linking/runtime remains incomplete. See tests/REQUIREMENTS_AUDIT.md. |
| Packaging and live GUI/CLI/SSH acceptance | Windows installer and isolated install/reinstall/GUI smoke implemented and verified; Unix installer with native Linux install/replacement/failure-preservation checks verified; default Windows user-PATH install/reinstall and fresh-shell version/help resolution verified at v0.1.108; remaining native/live acceptance and macOS viability pending |

## Evidence

- 2026-09-10, fresh v0.1.146: final release matrix and parity audit completed;
  release-candidate notes added. All 12 automated gates pass (149 app tests,
  one backend test, 48 config comparisons, 101 SSH scenarios). Optimized Linux
  link and native startup/GUI/keygen/installer checks pass; both Mac targets
  pass strict all-target Clippy. Default Windows v0.1.146 installation and
  fresh-shell resolution pass. Direct VT100 and SSH doc checks pass.
  Exact evidence and dependency warning are recorded in RELEASE_NOTES.md and
  tests/RELEASE_MATRIX_AUDIT.md. No application behavior changed after v0.1.145.
  Native macOS linking/runtime is the remaining release acceptance item.

- 2026-09-10, fresh v0.1.145: mode/NFR audit and native CLI reconnect fixes.
  Added mode and NFR rows to REQUIREMENTS_AUDIT.md; Mac runtime stays incomplete.
  Existing CLI fixtures all disabled reconnect, so added cli_reconnect_fixture.py
  using an owned hidden console, generated key and two loopback SSH sessions.
  The first test profile used username instead of user; corrected the harness.
  The corrected test exposed successful remote exit bypassing reconnect, then
  exposed Windows output mode surviving process exit. command_line.rs now routes
  successful remote exit through the reconnect preference and Console guards
  capture/restore original Windows output mode as well as raw input state.
  Explicit local quit and --no-reconnect behavior remain covered by prior cases.
  Final native test passes in
  `.qa/cli-reconnect-48685294b76b4263b4afd2315ade9936/results.json`: accept/reconnect,
  input in both sessions, decline/exit, restored input at prompts, full console
  modes at exit, exact output-only log and server/keypair cleanup.
  Existing five native CLI cases and six auth cases pass respectively in
  `.qa/cli-console-6bcf076a569b4dc18f931c5ebac68344/results.json` and
  `.qa/cli-auth-750c19c8c81542dfb901b06f3a9a55e8/results.json`.
  Full suite after the reconnect fix, before output-mode restoration, passes in
  `.qa/validation-792d75a4c92440b5a65d30863d80c0da/results.json` (12 gates).
  Final seven-gate suite after both fixes passes in
  `.qa/validation-20c834f53b7046c2a697e8879c7bbc9c/results.json` (149 app tests,
  one backend test; SSH skipped because the guard-only follow-up does not alter
  transport). Final full release gates remain part of the pending final audit.
  Default installation/fresh-shell verification passes in
  `.qa/windows-install-f2c8346c86b94224a2d8cf06220c93f6/results.json`.
  Source and default Windows installation v0.1.145. Remaining: parity/release-matrix
  reconciliation, applicable final gates, release notes and native Mac execution.

- 2026-09-10, fresh v0.1.144: implement actual SSH startup phase reporting.
  Added a scoped thread-local observer to the shared vendored SSH library and
  notifications at version negotiation, initial key exchange, authentication,
  PTY and shell request boundaries. GUI configuration resolution and actual host
  verification now record separate events. Scoped callbacks restore on nesting
  and panic; the 32-test library suite includes a thread-isolation regression.
  PTY/shell want-reply is false, so the final event says request sent with no
  acknowledgement requested, not accepted. No input/environment/secret payloads
  enter phase enum events. Existing bounded delivery and cancellation remain.
  session_probe now asserts ordered successful phases and absence of terminal
  startup after selected failed trust/auth/hop setup. Full 12-gate validation
  passes with 149 app tests, one backend test, 48 OpenSSH comparisons and 101 SSH
  scenarios: `.qa/validation-a02a2223d8f844f4b1011038c2b29df1/results.json`.
  Library unit evidence: `.qa/phase-library-77ca53e6f4a8468a81c321273fc1fb30/results.json`.
  Linux optimized link: `.qa/linux-cross-c6659745a1014b1fbbd254bc1e20fdd3/results.json`.
  Native performance/history runs pass in
  `.qa/linux-runtime-5b53feb6c04f4ece817fda1251f95396/results.json` and
  `.qa/linux-runtime-9aa7169b2899444ea890cc6904c089bc/results.json`.
  Reviewed top/tail history captures show config/route and auth/PTY/shell events;
  middle entries are proved by source/integration ordering, not claimed visible
  in those captures. Output, interrupt/recovery, busy-tab closure and owned cleanup
  pass; first run idle CPU 0.17-0.18 cores and drains 1.61/2.42 seconds.
  Default install/reinstall/fresh-shell PATH/help passes in
  `.qa/windows-install-a154ca5f152740fdaa7f05e7604d707c/results.json`.
  Source/default installation v0.1.144. SSH-008 is resolved; final matrix/NFR/mode
  audit, release notes and native macOS verification remain. See PHASE_ACCEPTANCE.md
  for exact scope and the upstream host-dependent tests not run by cargo test --lib.

- 2026-09-10, fresh v0.1.143: SSH capability audit finds a remaining gap.
  Reconciled SSH-001 through SSH-011 in tests/REQUIREMENTS_AUDIT.md against
  current verifier/agent/resolver/session/timeline paths, baseline regressions
  and native evidence. Ten capability rows have applicable evidence. SSH-008
  is explicitly incomplete: broad negotiating/authenticating and opening-terminal
  messages do not separately expose all required config/key-exchange/PTY/shell
  phases. The library currently combines these operations, so the fix must add
  notifications at their actual boundaries and verify order/failure attribution,
  not relabel a combined call or claim success early. Release queue updated.
  Top-level SSH summaries now reference the audit and preserve the discovered gap.
  No runtime logic changed. Windows seven-gate validation (149 app tests, one
  backend test) passes in
  `.qa/validation-4dc41e808e9a4571a70e877018381971/results.json`; SSH explicitly
  skipped for this documentation-only increment, full v0.1.140 baseline retained.
  Default install/reinstall and fresh-shell PATH/help pass in
  `.qa/windows-install-97d1ac22ef0e45cfbd7bce8be0befebc/results.json`.
  Source and default Windows installation are v0.1.143. Goal remains active with
  concrete timeline implementation work, remaining audit sections/release notes
  and native macOS verification.

- 2026-09-10, fresh v0.1.142: functional requirement audit.
  Added tests/REQUIREMENTS_AUDIT.md with FR-001 through FR-010 mapped to current
  source, specific baseline regressions and applicable native evidence. All named
  functional behavior is reconciled; remaining NFR/SSH/mode/parity/release-matrix
  sections are explicitly pending. Updated the ten stale top-level FR summaries
  to point to the audit instead of historical pending notes. Inspected current
  target/dispatch/protocol/rendering paths and baseline test names; rechecked
  native multiline snippet CRUD/restart and current workspace result contents.
  No application behavior changed. Windows seven-gate validation (149 app tests,
  one backend test) passes in
  `.qa/validation-222dd576fde34a9c9cf0974a36ae5c5b/results.json`; SSH explicitly
  skipped for this documentation-only increment, full v0.1.140 baseline retained.
  Default install/reinstall and fresh-shell PATH/help pass in
  `.qa/windows-install-28f15eef28f94173b2318ea820698075/results.json`.
  Source and default Windows installation are v0.1.142. Full goal remains active:
  complete the remaining audit sections, final checks/release notes and native
  macOS verification; functional-row verification is not whole-release acceptance.

- 2026-09-10, fresh v0.1.141: security release-checklist reconciliation.
  Replaced stale current security claims with a requirement-by-requirement table
  against inspected source and authoritative native results. Historical notes are
  retained explicitly as history. Reviewed metadata/crash/output logging, argument
  exposure, credential stripping, Unix atomic permissions, subprocess boundaries,
  remote quoting and delete submission. Later native password/KBI, encrypted-key,
  two-hop and Windows clipboard failure evidence closes the older pending notes
  within the specified checklist. Remote-output echo and platform coverage limits
  remain explicit; this is not a certification or a full release-completion claim.
  All 13 cited result files report completion; source hashes/evidence checks are
  retained in `.qa/security-audit-04f92b144640472f87b1ddf9f26cadce/results.json`.
  No application behavior changed. Windows seven-gate validation (149 app tests,
  one backend test, no installer skip) passes in
  `.qa/validation-f1092f828f404add8ac2816510620f39/results.json`; SSH explicitly
  skipped for this documentation-only increment, full v0.1.140 baseline retained.
  Default install/reinstall and fresh-shell PATH/help pass in
  `.qa/windows-install-3bb1e022b71543a6bf3602ffb9d20390/results.json`.
  Source and default Windows installation are v0.1.141. Remaining work is native
  macOS execution plus full requirement/matrix reconciliation and release notes.

- 2026-09-10, fresh v0.1.140: README/build example audit.
  Fixed the standalone SSH fixture instructions to build both ssh4-fresh and
  session_probe; generated-key fixture cases require the app too. Cargo launch
  examples now use --locked. Removed a stale native key-install pending statement
  superseded by the isolated review/install/reconnect evidence. No app behavior
  changed. Added tests/README_ACCEPTANCE.md mapping README and historical packet
  command paths to current builds and native evidence without claiming placeholder
  hosts were contacted or help-only checks establish connections.
  `.qa/readme-audit-f8cdf47b8cbd41e49becf3b38c39a3f3/results.json` verifies ten
  documented argument forms with --help, owned-path Ed25519 generation/keypair
  removal, and existence of all 19 referenced QA scripts.
  Full automated validation passes: 12 gates, 149 app tests, one backend test,
  48 OpenSSH comparisons and 101 SSH scenarios with no skips in
  `.qa/validation-370a7146e6af437296eab4f93751358c/results.json`.
  Default install/reinstall and fresh-shell PATH/help pass in
  `.qa/windows-install-7fb086a6233443d887ff613f3c425b8f/results.json`.
  Source and default Windows installation are v0.1.140. Native macOS runtime
  verification and final requirement/security/release-note reconciliation remain.

- 2026-09-10, fresh v0.1.139: conditional idle repaint fallback.
  The idle GUI now schedules its fallback at the existing two-second workspace
  autosave cadence. Reconnect countdowns, search, file/key dialogs, diagnostic
  exports and debug-log startup/stop retain one-second polling. SSH arrivals still
  wake immediately; pending output retains the 33 ms drain fallback.
  Optimized Linux linking passes in
  `.qa/linux-cross-ba3a2d9226854f5dbb26f1c8ef6a4673/results.json`.
  Default-driver native performance passes in
  `.qa/linux-runtime-f780153b2c0e48d4b7f52bbb5615fdd7/results.json`:
  connected idle 0.17-0.18 CPU cores, blank form 0.06; renderer workers 0.14-0.15,
  other threads about 0.03. Exact output drain completes in 1.51/2.52 seconds;
  Ctrl+C recovery, focus-mode interaction and busy-tab closure/cleanup pass.
  Reviewed screenshot shows normal panels restored during continuous output.
  Native reconnect passes in
  `.qa/linux-runtime-1b0ca8db977b4dc68568b1228a1cdfd9/results.json`: automatic retry
  (6.22 seconds), cancellation, manual retry, new-session input and no retry after
  explicit Disconnect, with all owned cleanup confirmed.
  The finite performance acceptance group is now verified with explicit software-
  rendering limitations. Further optimization does not expand the release queue.
  Windows seven-gate validation passes (149 app tests and one backend test) in
  `.qa/validation-abb0a19d8a2c432baa8558a0d57a596b/results.json`; SSH explicitly
  skipped for the GUI-only increment, full v0.1.138 baseline retained.
  Default install/reinstall and fresh-shell PATH/help pass in
  `.qa/windows-install-65ae1e0590b344568b6fac43707f1b4d/results.json`.
  Native workspace regression also passes in
  `.qa/linux-runtime-4b4c833bdc804fe1a844f4008d31b545/results.json`: settings/profile
  and workspace persistence, both restore choices, two-tab TERM/environment/input,
  sync/filter/focus/color and tab cleanup; all owned processes stop.
  Source and default Windows installation are v0.1.139.

- 2026-09-10, fresh v0.1.138: stop redundant idle traffic notifications.
  Session traffic snapshots now emit only when received/sent counters change.
  The protocol keepalive schedule remains independent. This avoids repeated GUI
  wakeups from unchanged idle counters while retaining the one-second traffic
  update cadence during activity and a final changed snapshot after output stops.
  Optimized Linux linking passes in
  `.qa/linux-cross-c96991abbee449948ee5b0971fad7fb0/results.json`.
  Native default-driver performance passes in
  `.qa/linux-runtime-701a47afd5a44199a3e1e1a1e34c7025/results.json`:
  connected idle falls from 0.74-0.83 to 0.38-0.42 cores; software-rendering workers
  account for 0.33-0.37, all other threads 0.044-0.048. Blank-form idle is 0.128.
  Output bursts drain in 1.51/2.32 seconds with final GUI counters verified.
  Ctrl+C interruption/recovery, busy focus-mode interaction and tab closure pass;
  closing stops the owned shell/command in 0.45 seconds. All owned cleanup passes.
  Full automated baseline refreshed: all 12 gates, 149 app tests, one backend
  test, 48 OpenSSH comparisons and 101 SSH scenarios pass with no skips in
  `.qa/validation-73b58acea05645a097f815fa4d493774/results.json`.
  Default install/reinstall and fresh-shell PATH/help pass in
  `.qa/windows-install-8fbb1e0770c94583a93008d8ef521937/results.json`.
  Source and default Windows installation are v0.1.138. Remaining performance
  work: assess idle fallback repainting and background-job wakeups. Native macOS
  verification remains pending; available Mac host details requested this turn.

- 2026-09-10, fresh v0.1.137: attribute native idle CPU by thread.
  The responsiveness fixture now records per-thread CPU deltas alongside total
  process CPU. linux_smoke.py accepts an opt-in --software-renderer-threads limit
  for a controlled owned-GUI comparison and records that override. README explains
  the measurement. No application behavior or default driver settings changed.
  Optimized Linux linking passes in
  `.qa/linux-cross-f741d47674e94cd88fe36fc62bc6988e/results.json`.
  Default driver run `.qa/linux-runtime-d263c568bf334173a3e9a5246d5d6972/results.json`
  passes: connected idle 0.74-0.83 cores, 0.67-0.75 in eight llvmpipe workers,
  about 0.07 in other threads; output drains 1.51/2.52 seconds, closure 0.45 seconds.
  One-worker run `.qa/linux-runtime-c3cb04b99094417caea633f9fa9fcc81/results.json`
  passes: connected idle 0.45-0.49 cores, about 0.07 outside llvmpipe; drains slow
  to 3.62/6.43 seconds. Both verify interrupt/recovery, busy interaction/closure,
  native startup/key generation/installation and all owned cleanup.
  This identifies rendering as the dominant cost, not a default-setting fix.
  Rendering cadence/cost investigation remains in the acceptance queue.
  Windows seven-gate validation passes (149 app tests, one backend test) in
  `.qa/validation-bb8ebbefb05a4ed1b686a6077898c3b1/results.json`. SSH explicitly
  skipped for this diagnostic-only increment; full v0.1.135 baseline retained.
  Default install/reinstall and fresh-shell PATH/help pass in
  `.qa/windows-install-b76b9c8e9664431e86c79ee2b202bde4/results.json`.
  Source and default Windows installation are v0.1.137.

- 2026-09-10, fresh v0.1.136: optimized Linux performance measurement.
  Added check_linux.ps1 -Release and linux_smoke.py --release, selecting matching
  app/helper binaries and recording the build profile. README documents both.
  Application behavior is unchanged from v0.1.135.
  Optimized linking passes in
  `.qa/linux-cross-169130834f0b4e748f98942a3399eeb7/results.json`.
  Native release acceptance on <test-host> passes in
  `.qa/linux-runtime-4f30b9e6946c4842a93c4db066c09272/results.json`:
  780,000/2,123,770-byte outputs drain after remote completion in 1.61/2.42 seconds
  versus debug 8.56/20.76; Ctrl+C foreground interruption/recovery, focus-mode
  interaction and busy-tab cleanup pass, closure taking 0.45 seconds. Reviewed
  screenshots show the final shell prompt and expanded terminal during output.
  Connected idle samples are 0.73-0.81 CPU cores; blank form is 0.13, measured
  over five seconds with Xvfb/software GL. Idle CPU remains an acceptance concern;
  this behavioral pass is not a claim that performance work is complete.
  Startup, key generation, isolated Linux installation and all owned cleanup pass.
  Windows seven-gate validation (149 app tests, one backend test) passes in
  `.qa/validation-48313cf16d0548ee8a13bf0078a843c1/results.json`; SSH explicitly
  skipped for this tooling/docs-only increment, full v0.1.135 baseline retained.
  Default installation/reinstallation and fresh-shell PATH/help pass in
  `.qa/windows-install-77d789fae45043259846607d80aeab24/results.json`.
  Source and default Windows installation are v0.1.136.

- 2026-09-10, fresh v0.1.135: event-driven SSH GUI wakeups reduce idle repainting.
  Session delivery now invokes an optional framework-independent wakeup after
  enqueue, and after worker completion/output sender closure. The GUI registers
  its repaint callback, keeps prompt follow-up frames while draining output and
  uses a one-second fallback for countdowns/non-SSH jobs. A regression checks
  successful delivery wakes and failed delivery does not. CLI behavior is unchanged.
  The new --gui-responsiveness fixture exposed ~3.98-core connected idle and
  ~4.70-core blank-form idle in the v0.1.134 debug/software-GL baseline
  `.qa/linux-runtime-4528896f94bb4f38a9148e4fd34436bb/results.json`.
  Corrected final v0.1.135 measurement waits for GUI received counters and stable
  output before sampling:
  `.qa/linux-runtime-c26b0ef13d5c44859016db929b599340/results.json`.
  Connected idle is ~0.84 cores, after-output samples ~0.91/~0.84, and after closing
  the busy tab ~0.15 cores. Native Ctrl+C stops foreground sleep and the shell
  remains usable. Busy-tab closure stops its shell/yes child in 0.45 seconds;
  owned cleanup passes. Focus-mode/restore during output is visually verified in
  `.qa/linux-runtime-ad0c169ed08f4268a76923e3a2ad6523` captures. Earlier find samples
  were taken while output remained queued and are not idle evidence. The corrected
  fixture tees output into owned files to calculate expected PTY bytes, waits up
  to 60s for GUI drain, and measures five-second process CPU windows.
  Final debug drain times are 8.56s for 780,000 bytes and 20.76s for 2,123,770 bytes;
  optimized-build performance remains required before responsiveness acceptance.
  Linux app/helper linking passes in
  `.qa/linux-cross-3aba706320a64a38b1c2113e6ecd6ddd/results.json`.
  Full Windows validation passes all 12 gates, 149 app tests, one backend test,
  48 OpenSSH config comparisons and 101 SSH scenarios in
  `.qa/validation-4c6efa46f1a8476ba85758b640011730/results.json`.
  This supersedes the v0.1.119 full baseline. The initial non-escalated library
  run encountered the known filesystem/IPC restrictions; the authorized complete
  suite passes. Default install/reinstall and fresh-shell version/help/PATH pass
  in `.qa/windows-install-9840cd239c704c8b81921bd203a52e85/results.json`.
  Source and default installation are v0.1.135.

- 2026-09-10, fresh v0.1.133: native advanced profile controls pass in
  `.qa/linux-runtime-8cb11bf2056745f7bdde190610dd09dd/results.json`.
  The workspace fixture changes strict mode true to false, agent false to true,
  auth key to key,agent, TERM xterm-256color to vt220, environment tag before to
  first, timeout 12 to 13 and keepalive 17 to 18 through native input. It checks
  profile/workspace persistence and restoration alongside an unchanged second
  profile. Remote SSH records confirm vt220/first before and after restart.
  Existing tmux, tab color, focus/resize, sync, filtering, closure and owned
  cleanup checks pass. Settings are reconciled in tests/PROFILE_ACCEPTANCE.md.
  Initial attempts corrected fixture issues: an off-screen numeric edit and a
  stale expected TERM. The clear settings screenshot is retained in
  `.qa/linux-runtime-0da79e3dd739438bb607bc1d21037fba/workspace-advanced-edited.png`;
  the corresponding final-run capture is blank and excluded from visual claims.
  Final persistence assertions and other screenshots remain valid evidence.
  Linux linking passes in
  `.qa/linux-cross-4ed462e3bd5c4af0965ec8451d84818e/results.json`.
  Windows seven-gate validation passes with 148 app tests and one backend test in
  `.qa/validation-b7ef732be62f4b209d58f9a1b406f119/results.json` (SSH gates skipped;
  full-suite baseline remains v0.1.119). Default install/reinstall and fresh-shell
  version/help/PATH pass in
  `.qa/windows-install-4e2944b30ba64f3da97fc643269842f9/results.json`.
  No production behavior changed; source and default installation are v0.1.133.

- 2026-09-10, fresh v0.1.132: native tmux configuration/workspace persistence
  passes in `.qa/linux-runtime-746dc4e2189149c6b64fab12e16a278b/results.json`.
  The workspace fixture expands Connection options, enters daily in the tmux
  field and saves the profile through the GUI. It verifies distinct daily/empty
  values in the two-tab workspace, keeps disconnected on one restart, reconnects
  on the next, and verifies the first restored worker receives the daily tmux
  startup command. Reviewed screenshots show the edited field and restored
  tmux:daily header. This fixture records startup input; the earlier real-tmux
  runs establish attachment, state retention, pane resize and detach behavior.
  Existing workspace identity/options, tab color, sync input, filter, focus/PTY
  resize, tab-close and cleanup checks pass. The tmux/TUI group is now moved to
  verified acceptance; advanced controls and responsiveness remain separate.
  Linux linking passes in
  `.qa/linux-cross-433fecd3bf8c4b9f85ab57bf78403349/results.json`.
  Windows seven-gate validation passes with 148 app tests and one backend test in
  `.qa/validation-779c25ea33e940068868171c280041f5/results.json` (SSH gates skipped;
  full-suite baseline remains v0.1.119). Default install/reinstall and fresh-shell
  version/help/PATH pass in
  `.qa/windows-install-f00ecdfe84214ff4b6486934d0d1d315/results.json`.
  No production behavior changed; source and default installation are v0.1.132.

- 2026-09-10, fresh v0.1.131: native frozen-selection history scrolling passes in
  `.qa/linux-runtime-61f54dc2e5104d4093643d92cd9e0ffb/results.json`.
  Extended the existing wrapped-line fixture to generate 40 known history lines,
  replace the screen after selecting 140 characters, scroll upward with the
  native wheel, shrink/restore the window and then copy/paste. Reviewed captures
  show HISTORY 14/15 and Back to live output after scrolling. The remote input
  equals the original logical line exactly, with no inserted newline or
  replacement/history text. The full native session run also passes clipboard,
  search, snippets, reconnect, Unicode, bracketed-paste bytes, PTY resize and
  owned-process/display/root cleanup. The session harness timeout is now 300s
  to accommodate its expanded capture sequence. No production behavior changed.
  Linux linking passes in
  `.qa/linux-cross-bdbd7fb4c596461caefc5c39e424152f/results.json`.
  Windows seven-gate validation passes with 148 app tests and one backend test in
  `.qa/validation-4cb8781e73ca4cb4baaef8f80a22db5e/results.json` (SSH gates skipped;
  full-suite baseline remains v0.1.119). Default install/reinstall and fresh-shell
  version/help/PATH pass in
  `.qa/windows-install-9f850932ec314451955a2fe86fc0bb03/results.json`.
  Source and default installation are v0.1.131.

- 2026-09-10, fresh v0.1.130: raw-PTY GUI clipboard acceptance passes in
  `.qa/linux-runtime-46dd9bd28c344a84a5597e4ab68f0543/results.json`.
  New `--gui-text-clipboard` uses an owned native clipboard helper and raw SSH
  input recording. It verifies empty paste sends nothing, exact spaces/tabs/CJK/
  combining/emoji bytes, LF/CRLF/CR-only/trailing-newline right-click review and
  cancellation, Ctrl+V review/confirmation, forced mixed-newline paste, no
  duplicate delivery, local Escape selection cancellation, exact remote Ctrl+C
  byte afterward, copy-only without selection, and all owned cleanup.
  v0.1.128 exposed Ctrl+V changing CRLF to LF in
  `.qa/linux-runtime-9cb170888ba8490b8fa69921cc58f38a/results.json`.
  Removed the egui-winit clipboard normalization; v0.1.129 raw-byte checks passed
  in `.qa/linux-runtime-81cb3662d54646bb8af260b00948a835/results.json`.
  Screenshot review then found CR-only preview lines collapsed together. The
  preview now displays normalized line breaks while the original payload is
  retained; the final screenshot shows separate a/b lines and raw bytes still
  contain CR. Clipboard matrix reconciliation is in tests/CLIPBOARD_ACCEPTANCE.md;
  native frozen-selection history scrolling and a running-command GUI interrupt
  remain explicit checks rather than broad completion claims.
  Linux app/helper linking passes in
  `.qa/linux-cross-ff38270a11054f2a85dace28967a4cef/results.json`.
  Windows seven-gate validation passes with 148 app tests and one backend test in
  `.qa/validation-005ff1ae16f94e2d99ea0517f0fe0b1d/results.json` (SSH gates skipped;
  full-suite baseline remains v0.1.119). Default install/reinstall and fresh-shell
  version/help/PATH pass in
  `.qa/windows-install-f45b6acb5fac4c0badc8a27383bc1d70/results.json`.
  Source and default installation are v0.1.130.

- 2026-09-10, fresh v0.1.127: native terminal links and selection-menu acceptance
  passes in `.qa/linux-runtime-730dab74f0224bce9217633be3f07203/results.json`.
  The new `--gui-links` fixture verifies plain-click suppression, exact Ctrl-click
  URL and OSC 8 destination, and exact Search online query using an owned BROWSER
  handler that records arguments without external browsing. Context Paste uses
  `menu-paste` from the native clipboard rather than selected `frozen words`;
  immediate Enter submits exactly once. Reviewed screenshots show the matching
  bracket pair and Ctrl-click path opening its parent folder with seed.txt selected.
  The remote file is unchanged and app/server/worker/clipboard/display/root cleanup
  passes. This exposed two production issues: context actions did not restore
  terminal focus, and an idle IME-disabled event arriving alongside Enter caused
  the key to be treated as composition input and discarded. Both are fixed.
  Menu regressions reproduced the failures and now pass; the existing IME
  preedit/commit/cancellation test still passes. Temporary diagnostic tracing was
  removed. Failed native diagnostic evidence is retained at
  `.qa/linux-runtime-b10e89dcf3b2468380b21d2c7c4b120e/results.json` (pasted text,
  ignored Enter) and `.qa/linux-runtime-54d686e75a2945389c2b761230fd063f/results.json`
  (restored focus, same-frame IME notification). The initial XTest-library fixture
  error was corrected before these runs. Intermediate versions were 0.1.125-126.
  Linux app/helper linking passes in
  `.qa/linux-cross-b8a89d604d30410496a8a92258dd8daa/results.json`.
  Windows seven-gate validation passes with 148 app tests and one backend test in
  `.qa/validation-074dadb5eef94d0cbe814a6dec96ad93/results.json`; SSH fixture gates
  were not rerun, and the full-suite baseline remains v0.1.119.
  Default install/reinstall and fresh-shell version/help/PATH checks pass in
  `.qa/windows-install-176ab29d07184aa28ac600b785a91150/results.json`.
  Source and default installation are now v0.1.127.

- 2026-09-10, fresh v0.1.124: extended native terminal-control acceptance passes
  in `.qa/linux-runtime-62dbaa9eda754a11b9d42b21b74bdc9b/results.json`.
  The `--gui-session` fixture verifies selection-context Copy using the native
  clipboard helper: exact `first`, no remote input, and reviewed screenshots
  show the menu and cleared highlight. Search screenshots verify Next/Previous
  wraparound 14 -> 1 -> 14, uppercase no-match with Match case enabled, 14 matches
  after disabling it and refreshing, and Close-button return to live output.
  Existing assertions verify queries stay local and terminal input resumes after
  Escape; the full native run also passes paste review/cancel/confirm, exact
  bracketed-paste bytes, Unicode and frozen wrapped selection after output and
  resize, snippet sending, disconnect/reconnect, and owned-process/root cleanup.
  Linux app/helper linking passes in
  `.qa/linux-cross-84774412a2f04bce9bf594bbda6511fc/results.json`.
  Windows seven-gate validation passes with 147 app tests and one backend test
  in `.qa/validation-98200bff37854fc3bb3720744193ddd8/results.json`;
  SSH fixture gates were not rerun (full-suite baseline remains v0.1.119).
  Default installation/reinstallation and fresh-shell version/help/PATH checks
  pass in `.qa/windows-install-de627a8cbcde49ff9264596b4d327a33/results.json`.
  No production behavior changed; source and installed version are 0.1.124.

- 2026-09-10, fresh v0.1.123: native GUI authentication acceptance passes in
  `.qa/linux-runtime-176dd455cd2646cd9f73de0c550e7e4d/results.json`.
  The new `--gui-auth` fixture uses Paramiko 5.0.0 installed only under the owned
  temporary directory, with synthetic credentials and loopback listeners.
  Six cases cover password success/rejection/cancellation and single/multiple
  keyboard-interactive success plus cancellation. Credentials are not submitted
  before Continue; cancellation submits none. Password cancellation occurs
  before a socket is opened; interactive cancellation closes its negotiated
  transport. Successful sessions receive exactly `guiinput` plus carriage return,
  with no credential leakage into terminal input. Profiles and app logs exclude
  both synthetic secret values. Reviewed screenshots show masked secret fields,
  echoed account input, successful terminal output, and clean failure/cancellation
  forms. Every app/server thread is stopped and the remote directory is removed.
  Seven Windows gates pass with 147 app tests and one backend regression in
  `.qa/validation-99dedc050b2a4e80a4bde91fd696ca2e/results.json`.
  Linux app/helper linking: `.qa/linux-cross-333e115d476741f2b5c16bb8025bcbd7/results.json`.
  Default installation and fresh-shell version/help:
  `.qa/windows-install-04e949135a51413abfc482f3b79e256b/results.json`.
  Full automated SSH baseline remains v0.1.119. This closes the authentication
  group in RELEASE_ACCEPTANCE.md; remaining terminal controls, tmux settings,
  performance, portability and final audit remain in scope.

- 2026-09-10, fresh v0.1.122: native Copy public key acceptance passes for
  protected generation and the unprotected retry after an existing-file
  collision. The owned-display helper reads the clipboard after each GUI click
  and compares it exactly with that public-key file's line (without the final
  newline). The retry proves the previous key is not copied. Existing native
  generation/cancellation/mismatch, permissions, collision preservation,
  use-key persistence and generated-key login also pass. Screenshots reviewed:
  protected and retry dialogs with distinct fingerprints and Copy controls.
  Evidence: `.qa/linux-runtime-66f9ab96eb814517bee188a9814ca1f7/results.json`;
  all owned app/SSH/workers and the temporary directory are cleaned up.
  Linux build: `.qa/linux-cross-de3df79d902a4cd48ab288a6ad93595c/results.json`.
  Seven Windows gates pass with 147 app tests and one backend regression in
  `.qa/validation-bcae5dab229c4db9b6e9e582996cff9f/results.json`.
  SSH gates were skipped; the full baseline remains v0.1.119.
  Default Windows installation and fresh-shell version/help checks pass in
  `.qa/windows-install-b052714d2d2a4db7862d2e1bbe21cdfa/results.json`.

- 2026-09-10, fresh v0.1.121: native connected-account public-key installation
  acceptance passes through the typed-path flow. The new `--gui-key-install`
  fixture loads a generated public key, reviews its fingerprint, closes without
  modifying authorization, reopens and explicitly installs. The existing entry
  and comment are preserved; the new key appears exactly once, permissions are
  0700/0600 and staging/lock files are removed. After GUI disconnect, the private
  key path is saved and a new GUI connection authenticates and accepts input.
  The server's accepted fingerprint matches the reviewed key, distinct from the
  initial connection's identity. The SSH server, forced command home and
  authorization file are isolated under the owned test directory.
  Reviewed screenshots show fingerprint review, cancelled dialog, saved private
  key and the second connected session. Native evidence and complete cleanup:
  `.qa/linux-runtime-20534311eefc4f63973894dfd366c700/results.json`.
  Linux app/helper build: `.qa/linux-cross-7039a49d6080449baa58e8a19ceff4c4/results.json`.
  Seven Windows gates pass with 147 app tests and one backend regression:
  `.qa/validation-07561c0af69745abb893177ec94fdb45/results.json`.
  Default installation and fresh-shell checks:
  `.qa/windows-install-4e6acc71d32f482a9236417c49be5e35/results.json`.
  Full automated SSH baseline remains v0.1.119; public-key clipboard and GUI
  password/keyboard-interactive acceptance remain open.

- 2026-09-10, fresh v0.1.120: the public-key installation dialog now accepts
  a typed path and explicit Load action in addition to the system file chooser.
  Editing the path clears the reviewed key and previous result. Missing or
  invalid files clear any previously loaded key, so Install remains unavailable
  until a valid key is loaded and its fingerprint can be reviewed. A regression
  covers changing paths, load failure and replacement of an already loaded file
  with invalid contents. Native install/cancel/reconnect acceptance remains open.
  Seven Windows gates pass in
  `.qa/validation-4cee61d94033439081735d159cfac4d4/results.json`, including 147
  app tests, one backend test, strict Clippy, release and isolated installation.
  Linux app/helper linking passes in
  `.qa/linux-cross-8701f9c0ca1d49d48c9e25ab32693faf/results.json`.
  Default Windows install/reinstall and fresh-shell checks pass in
  `.qa/windows-install-278c336258b24d03befa262d19b81e28/results.json`.
  The immediately preceding full v0.1.119 validation is
  `.qa/validation-425fc06d87f54043b884780c77b7a93b/results.json`: all twelve
  gates, 146 app tests, one backend test, 48 OpenSSH comparisons and 101 SSH
  scenarios, with nothing skipped. This supersedes the older full-suite baseline.

- 2026-09-10, fresh v0.1.119: native Vim mouse acceptance exposed a local
  selection completion defect. With mouse forwarding enabled, releasing Shift
  in the mouse-up frame left a visible local selection without updating the
  clipboard. The renderer now retains local drag ownership through release,
  suppresses remote motion during that drag, and clears ownership on completion,
  focus loss, modal interruption and tab changes. A regression checks exact
  copied text, no remote input leakage and resumed remote hover afterward.
  The failing native case is retained in
  `.qa/linux-runtime-8a973b9c2cde4a7e896da4a9e8c26064/results.json` at v0.1.118.
  Native verification passes in
  `.qa/linux-runtime-5e1ae95d482046389db82218a360a8f0/results.json`: mouse places
  Vim's cursor at row 3, column 5; Shift-drag copies exactly `3 abcde` without
  moving Vim; a subsequent remote drag reaches row 5, column 11 in Visual mode;
  wheel scrolling advances the viewport to row 7. Reviewed screenshots show
  local highlight, Vim Visual selection and scrolled content. The file remains
  unchanged on exit. Paste/resize, less/htop, tmux reconnect/fallback/detach and
  owned-process cleanup also pass. Linux app/helper build passes in
  `.qa/linux-cross-c7aa04600eda4c56892b5b5cea88bea0/results.json`; seven Windows
  gates pass in `.qa/validation-addb12065f5e40148b2ac9cbad1b471a/results.json`
  with 146 app tests and one backend regression. SSH gates were explicitly
  skipped, so v0.1.107 remains the full-suite baseline.
  Default installation/reinstallation and fresh-shell version/help pass in
  `.qa/windows-install-9cdafc36d38e4846ab8fe0ea79084763/results.json`.

- 2026-09-10, fresh v0.1.117: real Vim acceptance exposed a terminal focus
  defect: egui consumed Escape by removing focus before the terminal could
  encode it. The terminal now reserves Escape, Tab and arrow keys while it
  accepts connected input. A regression reproduced the pre-fix Escape failure
  and verifies focus retention and exact remote bytes for all six keys plus
  subsequent text. Temporary input tracing was removed after diagnosis.
  Native acceptance passes in
  `.qa/linux-runtime-d29c51f9893a4c30bb729a91aaf14908/results.json`:
  both Vim paste modes save exact Unicode/whitespace contents and exit;
  native Vim resize/restore, less paging/exit and htop sort-menu/exit pass.
  Reviewed screenshots show paste review, resized Vim, the returned shell,
  a later less page and the htop sort menu. Tmux fallback, retained pane/state,
  reconnect, resize and detach still pass; all owned processes are stopped.
  Linux app/helper build passes in
  `.qa/linux-cross-0f6ff521d0a549088b322a384955c535/results.json`.
  Seven Windows gates pass in
  `.qa/validation-e20873554f914af4a55add7959c41fc6/results.json`:
  fmt/check/strict Clippy, 145 app tests, one backend test, release and isolated
  installer. SSH gates were explicitly skipped; v0.1.107 remains the full-suite
  baseline. Default installation/reinstallation and fresh-shell version/help
  pass in `.qa/windows-install-50a3bfdc17ef44e2a3fcce17b66a1981/results.json`.
  The v0.1.116 fixture added owned text clipboard input, reviewed/forced Vim
  paste with exact Unicode/whitespace file checks, resize, less paging and
  htop sort/exit. Failed diagnostic runs are retained in
  `.qa/linux-runtime-3956357218eb427d81ed825e86b87a1f/results.json` and earlier
  v0.1.116 directories; they are not passing TUI acceptance evidence.

- 2026-09-10, fresh v0.1.115: native GUI real-tmux acceptance passes. The
  fallback fixture now starts an actual tmux server on a separate socket under
  its owned directory. Native input sets a shell variable and writes a proof
  file from the pane. F11 changes real pane dimensions from 104x24 to 131x28,
  then restores them. GUI disconnect leaves the pane alive; reconnect reattaches
  the same PID and the retained variable produces the expected file contents.
  Ctrl+B,D detaches to the outer interactive Bash shell, where a subsequent GUI
  command succeeds while the tmux pane remains alive. Reviewed screenshots show
  the tmux status line, configured header, resized/reconnected pane and outer
  shell after detach. Missing/failing startup cases still pass.
  Evidence: `.qa/linux-runtime-50622325229b4170bbb39ada1336b6f5/results.json`.
  The owned tmux server, SSH/app/workers, display and directory are cleaned up;
  native Unix installer/startup/key-generation checks also pass.
  Linux build: `.qa/linux-cross-506cfd18fbbb489882f8c3132095ea4a/results.json`.
  Seven Windows gates pass, including 144 app tests and one backend test:
  `.qa/validation-e7b05f2233904ba0857d0310dd03ea17/results.json` (SSH suite skipped).
  Default installation/fresh-shell checks pass in
  `.qa/windows-install-35ef348afc5b48c8abb4d138e9b841fa/results.json`.
  No production behavior changed. Native tmux GUI configuration/workspace
  restore, mouse/paste and remaining TUI/release acceptance stay open.


- 2026-09-10, fresh v0.1.114: implemented the release matrix's missing
  configured tmux session indicator in the connected/output-review header.
  It renders tmux:<name> next to session controls only when a name is configured;
  its tooltip explains that startup errors appear in terminal output. This does
  not claim tmux is running when the remote command failed. Native screenshots
  show tmux:fixture after missing-command and failing-command startup errors,
  with normal input, explicit disconnect and full resource cleanup passing.
  Evidence: `.qa/linux-runtime-53ff7acb6855496f83e155a6019864d5/results.json`
  and reviewed tmux-missing-usable.png/tmux-failed-usable.png.
  Linux build: `.qa/linux-cross-a06a9ecc593440778a2a57453d0d978f/results.json`.
  Seven Windows gates pass (144 app tests, one backend test, release, installer):
  `.qa/validation-de1e5a1c31664d15b9989f8adb20efce/results.json`; SSH suite skipped
  for this display-only change. Default installation/fresh-shell checks pass in
  `.qa/windows-install-24d5d35e3a6440d18b85ccd5d9d89179/results.json`.
  GUI tmux configuration/restore, real TUIs and the remaining acceptance queue
  are still open; this closes the configured-header-indicator requirement only.


- 2026-09-10, fresh v0.1.113: missing/failing tmux fallback verified through
  the native GUI and real isolated SSH/PTY Bash sessions. A saved tmux profile
  sends its startup command; Bash returns 127 with tmux excluded from PATH and
  returns 42 from a deliberately failing tmux function on the second connection.
  In each case native GUI typing runs a subsequent shell builtin and creates
  the exact owned proof file. Screenshots show the error, successful command and
  ready prompt. Explicit disconnect stops the Bash PID in both cases. Worker
  tracking recognizes the owned rc-file command after Python execs Bash; the
  final run asserts each Bash process is tracked before disconnecting it.
  Native evidence: `.qa/linux-runtime-0af34d635c7e4dc79dc20684bd4ad1fa/results.json`.
  All owned app/server/worker/display/temp-directory cleanup and native Unix
  installer/startup/key-generation checks pass. The installed host tmux and
  normal SSH configuration were not modified. No production behavior changed.
  Linux build: `.qa/linux-cross-0291dd391e5d4155bf70258918e2063b/results.json`.
  Seven Windows gates pass with 144 app tests, one backend test and release in
  `.qa/validation-ff40f1ce8add450ea223725b5c493bb1/results.json`; SSH suite skipped.
  Default user installation/fresh-shell checks pass in
  `.qa/windows-install-5cd5acf24aea4c5591b80bb986535901/results.json`.
  GUI tmux options/header/restore and real TUI interaction remain separate
  acceptance work, alongside the other items in tests/RELEASE_ACCEPTANCE.md.


- 2026-09-10, fresh v0.1.112: native GUI tab-color and focus-mode acceptance
  passes. The context menu selects amber; workspace save contains that color
  and restart/reconnect preserves it. F11 hides the header/sidebar and enlarges
  the server-observed PTY from 104x25 to 131x29. Input remains on the first tab;
  toggling F11 off restores 104x25 and normal panels. Reviewed color, focused
  and restored screenshots corroborate the saved config and server records.
  Existing two-tab restore/confirmation, TERM/environment, sync input, filtering,
  close-other/right/final-tab and all process/display/directory cleanup pass.
  Native evidence: `.qa/linux-runtime-fcdeaa7d147741dc94321b4caac7ead8/results.json`.
  Linux app/helper build: `.qa/linux-cross-a1d5f27f7d714bc592ec6a9f449a0412/results.json`.
  Seven Windows gates pass with 144 app tests, one backend test and release:
  `.qa/validation-8d57c67c09624769ab14852ab48dc465/results.json` (SSH suite skipped).
  Default installation/fresh-shell checks pass in
  `.qa/windows-install-0bb31a4213b6452cbdb62952695b0b7d/results.json`.
  Native Unix installer/startup/key-generation checks also pass. No production
  behavior changed. Remaining GUI/TUI, responsiveness, macOS runtime and final
  release audit are tracked in tests/RELEASE_ACCEPTANCE.md.


- 2026-09-10, fresh v0.1.111: macOS target-code compatibility verified for
  Apple Silicon and Intel. Added tests/check_macos.ps1 using isolated Zig C
  wrappers and installed Rust Darwin targets. All app/test/example targets pass
  strict Clippy with warnings denied for aarch64-apple-darwin and
  x86_64-apple-darwin. The first Apple Silicon check found a Linux-only test's
  unused Arc import on macOS; moving that import into the Linux test fixes it.
  No runtime behavior changed. An initial sandboxed dependency-cache write was
  denied; the authorized rerun populated the cache and completed.
  Apple Silicon: `.qa/macos-cross-5f100551ffb44927b80dabbdabf4f7fc/results.json`.
  Intel: `.qa/macos-cross-3392f7f0ae854497a71d8f95a64f6019/results.json`.
  Both checks report Cargo's existing dependency future-compatibility notice for
  block 0.1.6; current strict Clippy passes. These checks neither link a macOS
  executable nor run macOS tests, and no macOS runtime acceptance is claimed.
  Linux all-target checking also passes in
  `.qa/linux-cross-31c681ef41e24fc284ad75864d3a729e/results.json`.
  Seven Windows gates pass (144 app tests, one backend test, release and isolated
  installer) in `.qa/validation-3b7bd16d88d14eb580a65b8ac299353d/results.json`;
  SSH integration explicitly skipped for this test/import-only change.
  Default user installation and fresh-shell checks pass in
  `.qa/windows-install-5369b59292e840b781e3a3433e4163fc/results.json`.
  GUI/TUI, responsiveness, native macOS and the final release audit remain open.


- 2026-09-10, fresh v0.1.110: native Windows CLI clipboard-image acceptance
  passes. The isolated station/desktop and real Ctrl+P input exercise blank-path
  cancellation, declined confirmation, SCP rejection, a successful 801-byte PNG,
  and active transfer cancellation while waiting for the header acknowledgement.
  Local PNG staging is removed after every outcome. The successful payload
  matches the captured PNG exactly and the independent Rust image decoder
  verifies all 32x16 RGBA pixels, including alpha. The destination contains a
  space and apostrophe; its quoted insertion reaches SSH without Enter and its
  unquoted path is copied to the owned clipboard. Terminal input works after
  success/cancellation, raw logs contain only remote output, console modes are
  restored and fixture server threads stop.
  Evidence: `.qa/cli-image-20caf1fa234349f7bdf764e0b502b110/results.json`.
  This loopback SCP receiver verifies the CLI/protocol boundary and local cleanup;
  actual remote filesystem publication/cleanup has separate real-host evidence.
  No production behavior changed. An exploratory v0.1.109 run also passed.
  Seven local gates pass in
  `.qa/validation-87bd7ccc4a07489282df323d272bcaa2/results.json` (144 app tests,
  one backend test, release and isolated installer; SSH suite explicitly skipped).
  Updated default installation and fresh-shell checks pass in
  `.qa/windows-install-a2e1c528884f46c690197f00605a6ce0/results.json`.
  The explicit Windows CLI text/image clipboard requirement now has native
  acceptance. GUI/TUI, responsiveness, platform viability and final audit remain.


- 2026-09-10, fresh v0.1.109: fixed CLI Ctrl+V terminating SSH when clipboard
  text is unavailable. The clipboard error now stays local and the connection
  continues accepting input. The native regression fails on v0.1.108 in
  `.qa/cli-clipboard-41ca44b0273044b8826f680af7b9c493/results.json` and passes
  on v0.1.109 in `.qa/cli-clipboard-a8d06c80a02b4f6eb4bdff68e8ae53d6/results.json`.
  Both bracketed-paste modes verify Unicode/emoji, leading/trailing whitespace,
  tabs and mixed CRLF/LF/CR bytes, multiline cancellation and confirmation,
  Ctrl+Shift+V bypass, input after an empty clipboard, output-only raw logging,
  exact remote bytes, console restoration and worker/desktop handle cleanup.
  The fixture uses a noninteractive station and native CreateProcess desktop
  selection; Python STARTUPINFO does not forward an added lpDesktop attribute.
  Earlier setup attempts did not share clipboard state with the child; those
  failures are not claimed as application paste failures. The initial guard
  refused the interactive clipboard's copied files without modifying them.
  Five existing native keyboard/mouse/resize/query/exit scenarios also pass in
  `.qa/cli-console-223712764acc4703b3f7286a5e0db17e/results.json`.
  Seven local gates pass, including 144 app tests, one backend test and release:
  `.qa/validation-bf4b997e66fd43b89a6efd4fc9e149d1/results.json` (SSH suite skipped;
  v0.1.107 remains the full integration baseline). Updated the default user
  installation and verified fresh-shell resolution/version/help again in
  `.qa/windows-install-2c6309c1566e42aeaa4872b2fe8d9065/results.json`.
  CLI image/path uploads, GUI/TUI and remaining release acceptance stay open.


- 2026-09-10, fresh v0.1.108: default Windows packaging acceptance passes.
  Added tests/windows_install_acceptance.ps1 as an explicit live installation
  check, separate from routine isolated installer validation. Installed the
  release into LOCALAPPDATA/ssh4-fresh, checked its hash, verified exactly one
  saved user-PATH entry and unchanged PATH after reinstall. A fresh hidden
  PowerShell process using persisted machine/user PATH resolved ssh4-fresh to
  that installation, returned version 0.1.108, printed help and continued to
  successful shell exit. The installation remains available for use.
  Evidence: `.qa/windows-install-bd86fecdc2054217b6d144c78e7a2170/results.json`.
  Existing terminal hosts can retain stale environment values until restarted;
  this does not claim Windows Explorer or every terminal host was restarted.
  Interactive raw-console teardown has separate native CLI evidence.
  All seven local gates pass (format, locked check, strict Clippy, 144 app tests,
  one backend test, release build, isolated installer) in
  `.qa/validation-b26a0468833d46128e19af8a2c83f15f/results.json`.
  SSH integration was explicitly skipped for this packaging-only change; the
  full 12-gate v0.1.107 run remains the latest SSH baseline. No production logic
  changed. GUI/TUI, CLI clipboard and remaining release acceptance stay open.


- 2026-09-10, fresh v0.1.107: reconciled the release acceptance queue with
  recorded evidence. Corrected stale timeline, advanced-profile and security
  summaries; distinguished explicit release gaps from optional cross-product
  coverage. No production behavior changed. All 12 automated gates pass: formatting, locked check, strict Clippy,
  144 app tests, one backend test, 48 OpenSSH comparisons, 101 SSH scenarios,
  release compilation and isolated install/reinstall/failure preservation.
  Evidence: `.qa/validation-29c18f5fe02a41a48690872dd08f528f/results.json`.
  The release remains under development.

- 2026-09-10, fresh v0.1.106: native SFTP browser/download through the encrypted
  two-hop route passes. After reconnect, the target starts an SFTP channel and
  the browser lists the target fixture file. Cancelling its download review
  creates no local file. Reopening and confirming saves the exact 65,792-byte
  binary payload with 0600 permissions, preserves the remote source and leaves
  no download staging file. Closing the browser permits acknowledged terminal
  input on the same target session. Encrypted-hop cancellation/failure/retry,
  fresh reconnect prompting, complete large-output delivery, rekey on both hops
  (32 and 31 exchanges in this run), trust preservation and all cleanup pass.
  Evidence: `.qa/linux-runtime-08cd014b6abc42fa8cd95983b7266a8a/results.json`
  and reviewed routed-file screenshots. One intermediate run clicked a stale
  button position after cancellation moved the window; correcting the fixture
  coordinates allowed the final run to pass without a production-code change.
  Linux build: `.qa/linux-cross-24e033bcf8d04e2b9d5850c5394c090f/results.json`.
  Windows format/check/strict Clippy, 144 app tests, one backend test, release
  and isolated installer pass in
  `.qa/validation-e6cc8fb1b8764efea6224ab5220c23c0/results.json`.
  Native Unix installer/startup/key-generation checks also pass in the final run.
  Routed native uploads/forwarding and broader platform acceptance remain open.


- 2026-09-10, fresh v0.1.105: native encrypted second-hop authentication passes.
  Hop2 uses its own encrypted client key while hop1 and the target retain their
  separate unprotected fixture identity. After changed-host-key rejection and
  restoration, cancelling the hop2 passphrase prompt creates no target session.
  An incorrect passphrase reports route step 2 and likewise creates no session.
  Correct retry connects, delivers the complete 1,050,743-byte rekey run (31
  additional key exchanges per hop) and accepts terminal input. Disconnect and
  reconnect show a fresh hop2 prompt before a new target session; entering the
  passphrase again succeeds. The encrypted source bytes remain unchanged. Neither
  entered passphrase occurs in profile/import settings, app/debug logs or either
  jump-server log. All owned process/display/temp-directory cleanup passes.
  Evidence: `.qa/linux-runtime-c354db800c2344318d20fef83331e137/results.json`
  and reviewed hop authentication screenshots.
  Linux build: `.qa/linux-cross-391eaf1523aa475293ef85c60f25ee1c/results.json`.
  Windows format/check/strict Clippy, 144 app tests, one backend test, release
  and isolated installer pass in
  `.qa/validation-afc91c0d61564cbfba26181573571801/results.json`.
  Native Unix installer/startup/key-generation checks also pass in the final run.
  No production logic fix was required. Routed native file/forwarding workflows,
  other authentication methods and broader platform acceptance remain open.


- 2026-09-10, fresh v0.1.104: native two-hop rekey acceptance passes. Both jump
  servers use a 128K rekey limit. A 2048-line output burst causes 31 additional
  SSH2_MSG_NEWKEYS exchanges on each hop, measured relative to the established
  connection baseline. The fixture waits for the application's received-byte
  counter to cover the entire burst; it reports 1,050,743 bytes. The reviewed
  screenshot shows REKEY_DONE and ACK afterrekey. Input after rekey, subsequent
  disconnect/reconnect input, changed-hop-key rejection and trust preservation
  all pass, as does cleanup of both jump servers and all other owned resources.
  Evidence: `.qa/linux-runtime-a054e23531d0402cbb4cdd207d17a12a/results.json`,
  its hop/debug logs and reviewed `jump-rekey.png`. An earlier screenshot was
  taken before output finished arriving; the final fixture requires complete
  application delivery instead of accepting server-write completion alone.
  Linux build: `.qa/linux-cross-00b9c483d8d64113a5015aee666a7580/results.json`.
  Windows format/check/strict Clippy, 144 app tests, one backend test, release
  and isolated installer pass in
  `.qa/validation-61264d16d4324bef9952fe5d92087370/results.json`.
  Native Unix installer/startup/key-generation checks also pass in the final run.
  No production logic fix was required. Hop prompts, routed native file/forwarding
  workflows and broader platform acceptance remain open.


- 2026-09-10, fresh v0.1.103: native two-hop SSH acceptance passes. The fixture
  starts two distinct jump sshd instances with separate host keys and strict
  verification, imports their aliases, enters hop1,hop2 through the native Jump
  hosts field and saves the profile. A mismatched known-host entry for hop2 is
  rejected without creating a target terminal or replacing the rejected entry.
  Restoring the correct trust entry permits connection and acknowledged input.
  Both hop logs show direct-tcpip tunnels to the expected next port. The native
  connection history shows route/authentication/tunnel steps. Disconnect stops
  the target worker; reconnect creates a fresh session accepting input. Normal
  close, both jump servers, target server/workers, display and temporary-directory
  cleanup pass. The original known-host bytes and saved route are preserved.
  Evidence: `.qa/linux-runtime-6874823bfd08450a9a726654512c9891/results.json`,
  hop logs, session events and reviewed native screenshots.
  Linux build: `.qa/linux-cross-3a448533cd364894865c019ee060e4f1/results.json`.
  Windows format/check/strict Clippy, 144 app tests, one backend test, release
  and isolated installer pass in
  `.qa/validation-52b8ec6c917f436c87918d148db51dad/results.json`.
  Native Unix installer/startup/key-generation checks also pass in the final run.
  No production logic fix was required. Hop prompts, multi-hop rekey, routed
  native file/forwarding workflows and broader platform acceptance remain open.


- 2026-09-10, fresh v0.1.102: native forwarding failure/recovery acceptance
  passes. Keeping each configured listener occupied in turn triggers local bind
  failure, server rejection of remote forwarding and SOCKS bind failure. No
  terminal session starts during those failures. Rebinding verifies rollback of
  the local listener after remote failure and both local/remote listeners after
  SOCKS setup failure. Releasing the final conflict allows normal connection.
  SOCKS rejects unsupported authentication with 05 FF and returns a failed
  CONNECT reply for a reserved but non-listening destination. The GUI shows the
  remote-listener rejection and SOCKS destination error. Subsequent exact binary
  round trips through local, remote, SOCKS IPv4 and SOCKS hostname routes pass,
  as do reconnect and release of all listeners on disconnect/normal exit.
  Evidence: `.qa/linux-runtime-cbef0d9f496448eb88de66c42e1461fc/results.json`
  and reviewed forwarding failure screenshots. All owned cleanup checks pass.
  Linux build: `.qa/linux-cross-76581bba9bb342daa583d1dc26912e9e/results.json`.
  Windows format/check/strict Clippy, 144 app tests, one backend test, release
  and isolated installer pass in
  `.qa/validation-9678f7efab824eeda4942e6000cac418/results.json`.
  Native Unix installer/startup/key-generation checks also pass in the final run.
  No production logic fix was required. Jump chains, remaining native workflows
  and other-platform acceptance are still pending.


- 2026-09-10, fresh v0.1.101: native forwarding acceptance now configures local,
  remote and SOCKS listeners together through Connection options. The saved
  profile contains all three exact specifications. After the existing local
  bind-conflict/retry check, each connection transfers 4096 exact binary bytes
  and a reversed response through the local tunnel, remote tunnel, SOCKS IPv4
  CONNECT and SOCKS domain CONNECT using localhost. SOCKS no-auth negotiation
  and success replies are checked. All four round trips repeat after reconnect.
  Disconnect and normal application exit release all three listening ports,
  verified by rebinding them. Owned app/server/workers/display/temp cleanup pass.
  Evidence: `.qa/linux-runtime-cc2c08e3f814437f997293cde6828f0a/results.json`
  and the reviewed native forwarding settings screenshot.
  Linux build: `.qa/linux-cross-923ad6c004a741deb3ad6fb25b1fd5de/results.json`.
  Windows format/check/strict Clippy, 144 app tests, one backend test, release
  and isolated installer pass in
  `.qa/validation-cd612a7c7aa047b7a993cdd491f3645d/results.json`.
  Native Unix installer/startup/key-generation checks also pass in the final run.
  No production logic fix was required. Native remote-forward denial, SOCKS
  failure displays, jump chains and other-platform acceptance remain separate.


- 2026-09-10, fresh v0.1.100: native local-forward acceptance passes. The fixture
  expands Connection options, enters a loopback local-forward specification and
  saves it through the GUI. A deliberately occupied listener produces a visible
  address-in-use error without opening a terminal session. After releasing that
  port, native retry connects and transfers 4096 binary bytes to the destination
  and a different 4096-byte response back. Disconnect stops the worker and allows
  rebinding the listening port. Reconnect repeats the exact binary round trip;
  normal window close again releases the port. Saved profile settings survive,
  and all app/server/worker/display/temp cleanup checks pass.
  Evidence: `.qa/linux-runtime-33f11f882f3f40e48b3f24ee8910e535/results.json`
  and reviewed forwarding screenshots, including the bind-error display.
  Linux build: `.qa/linux-cross-2d693c120b734dccae55612d4ebdea09/results.json`.
  Windows format/check/strict Clippy, 144 app tests, one backend test, release
  and isolated installer pass in
  `.qa/validation-7b4df7e8e735490caccea6a935edb03e/results.json`.
  Native Unix installer/startup/key-generation checks also pass in the final run.
  No production logic fix was needed. Native remote and SOCKS forwarding,
  jump chains and other-platform acceptance remain pending.


- 2026-09-10, fresh v0.1.99: native encrypted-key authentication acceptance
  passes. A generated encrypted Ed25519 key is trusted only by the owned fixture
  sshd. Cancel creates no session; a wrong passphrase shows the expected error
  and creates no session. Retry with the correct passphrase connects and terminal
  input is acknowledged. Encrypted key bytes and 0600 permissions are preserved.
  Neither passphrase is saved in configuration. With native diagnostic logging
  enabled across cancellation, failure and success, the log contains metadata
  states but neither passphrase, private-key material nor submitted terminal text.
  Logging is stopped through the UI. App/server/workers/display/temp cleanup pass.
  Evidence: `.qa/linux-runtime-d061d732119f49ee9c2826a3f8ec5c0e/results.json`
  and its reviewed authentication screenshots and `session.debug.log`.
  The earlier native unlock run also passed without logging; one intermediate
  logging run failed solely because the fixture expected JSON rather than the
  actual plain-text format. Correcting that assertion required no app change.
  Linux build: `.qa/linux-cross-b21bde1d60f1450d86f80337fbe43f64/results.json`.
  Windows format/check/strict Clippy, 144 app tests, one backend test, release
  and isolated installer pass in
  `.qa/validation-4b06d12c65c1498abaf17c3e534aa2c2/results.json`.
  Native Unix installer/startup/key-generation checks also pass in the final run.
  Other authentication methods, hop prompts and platform-specific dialogs remain
  separate acceptance work; this does not close the full security checklist.


- 2026-09-10, fresh v0.1.98: native Linux key-generation acceptance passes.
  The fixture closes the dialog before generation and verifies no files, rejects
  mismatched passphrases, generates a protected Ed25519 key, and independently
  compares its public key with OpenSSH. A wrong passphrase is rejected. Private
  files have mode 0600. A second dialog refuses an existing keypair without
  changing either file, then retries successfully with an unprotected key at a
  new path. No staging files remain. Use key for this connection updates the
  saved profile; that generated key authenticates to the isolated fixture server
  and terminal input is acknowledged. The synthetic passphrase is absent from
  saved configuration. All app/server/worker/display/temp-directory cleanup
  checks pass. The normal host authorized_keys is untouched.
  Evidence: `.qa/linux-runtime-05801156f9884d6aab43f2d2a5b80c13/results.json`
  and reviewed keygen screenshots. Linux build:
  `.qa/linux-cross-e40937caf1b8490db9828cc7a62aeb51/results.json`.
  Windows format/check/strict Clippy, 144 app tests, one backend test, release
  and isolated installer pass in
  `.qa/validation-ffa2151bb8c1481aa13ad10841480cd8/results.json`.
  Native Unix installer/startup/CLI key-generation checks also pass.
  The first exploratory attempt failed before application startup due to a
  fixture argument mismatch; the corrected fixture and final run pass. No
  production logic fix was required. The optional OS chooser, public-key copy,
  protected-key authentication prompt and other-platform UI remain unverified.


- 2026-09-10, fresh v0.1.97: native Linux tab-management acceptance passes.
  After two-tab workspace restoration, Sync input sends one submitted line to
  each connected worker exactly once; disabling it sends the next line only to
  the active worker. Native tab filtering shows the matching tab and does not
  leak text into SSH. Clear restores the tab strip. The first tab's context menu
  closes other tabs, stopping the second worker while fresh input succeeds on
  the retained session. Reopening the second profile and closing tabs to the
  right stops its new worker and preserves input on the first. Closing the final
  tab stops that worker and leaves a usable new-connection form, verified by the
  captured UI. App/server/workers/display and remote-directory cleanup pass.
  Evidence: `.qa/linux-runtime-de0ea356c79a4c1095739e6fb9faf7bb/results.json`,
  its `session-events.jsonl`, and reviewed tab screenshots. Two-tab restoration,
  native installer and key-generation checks also pass in this run.
  Linux build: `.qa/linux-cross-82505da9b56b48b496220478fd33c72b/results.json`.
  Windows format/check/strict Clippy, 144 app tests, one backend test, release
  and isolated installer checks pass:
  `.qa/validation-ba7d73f85adb48e19e7bb20d782ae549/results.json`.
  No production logic fix was required. Tab-color restoration, synchronized
  paste/disconnected-target handling and tab closure under continuous output
  remain separate acceptance tasks.


- 2026-09-10, fresh v0.1.96: native workspace acceptance now opens two distinct
  saved profiles through the sidebar, connects both, and saves with the second
  tab active. Restart without a profile argument keeps both disconnected when
  requested, then a second restart reconnects both after confirmation. Server
  records prove distinct `xterm-256color`/`vt100` terminal types and first/second
  environment values before and after restoration. Input initially reaches the
  restored active second tab; selecting the first tab routes subsequent input to
  its own worker. Snapshot checks cover order, profile identity, host/port,
  user/key, authentication/trust, TERM/environment, keepalive and timeout.
  Private configuration permissions, normal shutdown of original sessions and
  final app/server/worker/display/temp-directory cleanup pass.
  Evidence: `.qa/linux-runtime-92ce2c6732034842b59648c7b2997ec1/results.json`
  and its `session-events.jsonl` plus reviewed workspace screenshots.
  Linux v0.1.96 build passes in
  `.qa/linux-cross-44355da9de534a679730b80a5c862dcb/results.json`.
  Windows format/check/strict Clippy, 144 app tests, one backend test, release and
  isolated installer checks pass in
  `.qa/validation-46e85043d8f74fafa77536f5e8ed3058/results.json`.
  Native Linux Unix-installer/startup/key-generation checks also pass in this run.
  This extends acceptance coverage; no production logic change was needed.
  Workspace colors, forwarding/jumps and tmux restoration remain pending.


- 2026-09-09, fresh v0.1.95: added `install.sh` for locked release builds or
  trusted prebuilt binaries, defaulting to `~/.local/bin` with an explicit
  destination override. It validates a private staged copy against the manifest,
  applies 0755 permissions and renames it into place without editing shell PATH.
  Native Linux checks cover install/startup, replacement, spaces/apostrophes/dollar
  signs in paths, stale/failing/invalid/missing sources, build failure, directory
  and symlink refusal, default HOME destination and staging cleanup. Existing
  installation hashes remain unchanged on failures. Linux runtime/key-generation
  checks also pass and the owned remote directory is removed.
  Evidence: `.qa/linux-runtime-9cfb926e8e1849969931e5078618968d/results.json`;
  Linux build: `.qa/linux-cross-2c86eac607424a519dd0c373375bbdea/results.json`.
  Windows fmt/check/strict Clippy, 144 app tests, one clipboard backend test,
  release build and isolated installer checks pass in
  `.qa/validation-553f37af8aef43bfb54d0cdb599394fe/results.json`.
  The first sandboxed run failed on denied Git Bash IPC, Pageant mapping and
  temporary key publication; the authorized unsandboxed rerun passed unchanged.
  Native Unix source compilation and macOS installation remain unverified; this
  run used the Linux cross-built executable through `--binary`.


- 2026-09-09, fresh v0.1.94: native single-tab workspace save/restore passes.
  Through Workspace settings, the fixture enables restoration and reconnect,
  retains confirmation and saves. The persisted active tab, profile identity,
  user/key path, key-auth order and strict host verification are checked, along
  with no password field and 0600 config permissions. Normal app exit stops the
  first worker. Restart without a profile argument presents confirmation before
  connecting; Keep disconnected creates no session. A second restart and explicit
  Reconnect tabs starts a new worker and accepts terminal input. Settings survive
  both restarts; captures and owned process/display/directory cleanup pass.
  Application logic is unchanged; use `--gui-workspace`.
  Native: `.qa/linux-runtime-7ebfda7ec92649839497a0f138e4bbb2/results.json`.
  Linux build: `.qa/linux-cross-7ef3296231324178b9f12defe5dc4f61/results.json`.
  Windows seven-gate validation (`-SkipSsh`), including 144 app tests, one backend
  regression, strict Clippy, release and installer:
  `.qa/validation-c5c355a88aec43fc99c17aa51e441a99/results.json`.
  Multi-tab/advanced workspace restore, forwarding/jump/profile controls and
  broader protocol/platform/release acceptance remain pending. Full development
  is incomplete.

- 2026-09-09, fresh v0.1.93: native automatic reconnect and cancellation pass.
  The fixture configures a five-second delay and terminates only its owned remote
  session worker. A distinct worker starts automatically after 5.37 seconds. On
  the next remote closure, Cancel retry suppresses reconnection beyond the deadline;
  the original connection error remains visible. Manual Reconnect then starts a
  third worker and routes input to it. Explicit Disconnect stops that worker and
  does not retry during a further six-second observation. Captures were inspected,
  and owned process/display/directory cleanup passes. App logic is unchanged;
  use `--gui-reconnect`.
  Native: `.qa/linux-runtime-b6ede30911164b4488479e3b30676ccf/results.json`.
  Linux build: `.qa/linux-cross-5ce5f000a7424b10b1fc4ba1b654f5b7/results.json`.
  Windows seven-gate validation (`-SkipSsh`), including 144 app tests, one backend
  regression, strict Clippy, release and installer:
  `.qa/validation-2f7375dba6dc4c4fb5ab06220012feb9/results.json`.
  Advanced profiles/forwarding/jumps, workspace interactions, remaining protocol
  and platform/release acceptance remain pending. Full development is incomplete.

- 2026-09-09, fresh v0.1.92: native X11 file-drop acceptance passes.
  The fixture implements the XDND selection exchange against the owned app
  window, delivers two percent-encoded file URIs containing a space/apostrophe,
  and waits for drop acceptance. It verifies queued review without upload, queue
  disposal when Files closes, repeated drop, cancellation of the first review,
  and exact upload of the second file (33,024 bytes, 0600 permissions). A further
  drop of the same path with different source bytes is confirmed and rejected;
  the original remote bytes and changed local source survive, with no remote
  staging left. Captures show confirmation, success and collision failure/cleanup.
  The synthetic source window and owned app/server/display/directory are cleaned
  up. Application logic is unchanged; use `--gui-drop`.
  Native: `.qa/linux-runtime-990714a5788a451799fe4f9fba77abbc/results.json`.
  Linux build: `.qa/linux-cross-8dfac528768a490dab9b1853e67d0222/results.json`.
  Windows seven-gate validation (`-SkipSsh`), including 144 app tests, one backend
  regression, strict Clippy, release and installer:
  `.qa/validation-c77501ee5008460bb8cf97fe21477155/results.json`.
  This verifies XDND event delivery, not a particular desktop file manager or
  Windows/Wayland/macOS drag source. Further interruption, advanced connection
  and broader release acceptance remain pending. Full development is incomplete.

- 2026-09-09, fresh v0.1.91: native clipboard PNG workflows pass over SFTP
  and automatic SCP fallback. An owned test helper publishes a 32x16 RGBA image
  only on the isolated X display and decodes captured/uploaded PNGs to verify
  every color and alpha byte. The fixture checks preview, keyboard review Cancel,
  no premature remote file, 0600 local staging and remote permissions, successful
  upload, existing-destination preservation, and removal of local clipboard PNGs
  after cancellation, success and collision failure. Remote staging and the seed
  file are checked, and all owned helper/app/server/display/directory cleanup
  passes. The fallback server omits SFTP; command records and unchecked review
  controls confirm automatic SCP use. Captures were inspected. App logic is
  unchanged; -Build now also links the clipboard QA example.
  SFTP: `.qa/linux-runtime-5560f76b0c7046f8955cafe54a94fe5a/results.json`.
  Fallback: `.qa/linux-runtime-03e2d56884e446fc88aa37c9852360a6/results.json`.
  Linux build: `.qa/linux-cross-969ee19a1cb84d67be2015c7163f83b1/results.json`.
  Windows seven-gate validation (`-SkipSsh`), including 144 app tests, one backend
  regression, strict Clippy, release and installer:
  `.qa/validation-9422fea851a64c79bcb136cd88ac5fbe/results.json`.
  Use `--gui-clipboard` or `--gui-clipboard-fallback`. Native file-drop, additional
  clipboard interruption/failure paths, advanced connections and broader release
  acceptance remain pending. Full development is not complete.

- 2026-09-09, fresh v0.1.90: full twelve-gate validation passes after the
  diagnostics change, including the previously skipped SSH/configuration fixtures.
  All 144 application tests, one GUI-backend regression, 48 OpenSSH configuration
  parity cases and 101 isolated SSH cases pass. The SSH matrix covers auth/key
  prompts and cancellation, agent cancellation, trust rejection, forwarding,
  jumps/rekey, SFTP, SCP, shell fallback, malformed responses, cancellation,
  cleanup, destination preservation and recovery. Release and isolated installer
  checks also pass. No application change was needed for this verification.
  Evidence: `.qa/validation-a9af2e6a0b7c4aa9ad8c421508d4cfe3/results.json`.
  Native clipboard/drop, advanced controls, remaining platform/packaging and
  other explicit acceptance items above remain open; this is not full release
  completion.

- 2026-09-09, fresh v0.1.90: native diagnostics exposed and fixed an export
  usability gap. The original system-chooser-only action produced no dialog or
  saved file on the isolated Linux desktop. The command now opens an in-app form
  with an editable absolute destination, explicit content disclosure, Save/Cancel
  and an optional system chooser. Terminal keyboard routing is suppressed while
  the form is open. Saving still uses the existing background exporter and never
  replaces an existing file.
  Native tests verify logging from connection start through terminal output and
  stop, metadata without terminal text, Cancel/Escape/window-close without saving,
  report contents and decoded remote bytes, 0600 permissions, collision refusal,
  retry to a new path, no destination-field input reaching SSH, and staging/owned
  process/display/directory cleanup. Captures were inspected. Two label punctuation
  corrections followed this native run; final Linux and Windows builds include them.
  Native: `.qa/linux-runtime-c54ab7ca7d45473787ffcd6245911364/results.json`.
  Final Linux build: `.qa/linux-cross-ccb6403f759d4c6f9dbba30f3c189470/results.json`.
  Final Windows seven-gate validation (`-SkipSsh`), including 144 app tests, one
  backend regression, strict Clippy, release and isolated installer:
  `.qa/validation-232ad919a23f4fb2bd7b04a3cb4e8737/results.json`.
  Use `--gui-diagnostics`. Optional system chooser, native authentication logging,
  clipboard/drop and broader release acceptance remain pending. Full development
  is not complete.

- 2026-09-09, fresh v0.1.89: credential persistence/security regressions pass
  on Windows and native Linux. New tests verify mixed-case secret fields are
  removed from nested legacy profile/workspace structures and Config::set data,
  while unrelated extensions survive save/reload. Malformed-input errors do not
  echo source secrets or change the original file. Linux also verifies replacement
  of an existing 0644 config with a 0600 file and no staging leftovers.
  The source/evidence audit is in `tests/SECURITY_REVIEW.md`; native diagnostic,
  authentication logging and clipboard/drop security acceptance remain open.
  All current Linux library/application tests now pass (104 + 38), including the
  newer shutdown-cleanup regression. Application logic is unchanged.
  Linux runtime: `.qa/linux-runtime-ed48b2b1401240c388e1a005ddde2627/results.json`.
  Linux test build: `.qa/linux-cross-765bbf7a0794413ebc1236029c1c8e0e/results.json`.
  Linux app build: `.qa/linux-cross-36c6c13413504874b86846869c986a29/results.json`.
  Windows seven-gate validation (`-SkipSsh`), including 144 app tests, one
  backend regression, strict Clippy, release and isolated installer:
  `.qa/validation-a2701f6ad339478f989ac18f8bc9687d/results.json`.
  Full development and release acceptance are not complete.

- 2026-09-09, fresh v0.1.88: native multi-file queue cancellation passes.
  The local-browser fixture selects two files, clears their waiting queue without
  creating remote files, and queues them again. It cancels chosen.bin's review
  using the keyboard-focused Cancel button, confirms leave.bin remains queued,
  and uploads only that remaining file after explicit confirmation. Its 28 bytes
  match the local source and permissions are 0600. The cancelled file remains
  absent remotely, both local sources and the remote seed survive, and staging
  plus all owned process/display/directory cleanup pass. Queue-clearing,
  cancellation and completion captures were inspected. Application logic is
  unchanged; `--gui-local` now covers this queue workflow. The v0.1.87 evidence
  separately retains the selected binary upload acceptance result.
  Native: `.qa/linux-runtime-6de2a6ad5507423094f2d980160d2c5a/results.json`.
  Linux build: `.qa/linux-cross-1ab7f15003af4d7ca2a1f46d50a3a02f/results.json`.
  Windows seven-gate validation (`-SkipSsh`), including 142 app tests, one
  backend regression, strict Clippy, release and isolated installer:
  `.qa/validation-0986d5b79e5742269ade462d3104f585/results.json`.
  Native file chooser dialogs, clipboard-image/drop, queue disposal on window
  closure, advanced connections and broader release acceptance remain pending.
  Full development is not complete.

- 2026-09-09, fresh v0.1.87: native local-browser selected upload passes.
  The new `--gui-local` fixture reads an isolated child folder through the local
  path field, navigates to its parent and double-clicks back into the child.
  It selects then clears an unrelated file, selects only chosen.bin and queues
  it. No remote file appears before review and confirmation. The upload contains
  exactly 33,024 binary bytes with 0600 permissions; both local sources and the
  original remote seed survive, the unselected file stays local, and staging is
  removed. Navigation, selection, queue, confirmation and completion captures
  were inspected. All owned processes, display and directory were cleaned up.
  Application logic is unchanged.
  Native: `.qa/linux-runtime-5963f6e6bcfa4ee683530e06037104b7/results.json`.
  Linux build: `.qa/linux-cross-9600abec039f45a3b33f8e78619a794f/results.json`.
  Windows seven-gate validation (`-SkipSsh`), including 142 app tests, one
  backend regression, strict Clippy, release and isolated installer:
  `.qa/validation-bdc90f661fc04115891e189c1a9f479a/results.json`.
  Native file chooser dialogs, multi-file queue/cancellation, clipboard-image/
  drop, advanced connection controls and broader release acceptance remain
  pending. Full development is not complete.

- 2026-09-09, fresh v0.1.86: native completed-upload path actions pass.
  The fallback fixture now uploads and downloads a binary file whose name
  contains a space and apostrophe. Copy remote path survives clipboard paste
  byte-for-byte; Insert quoted path produces one POSIX shell argument matching
  the original path. Neither action submits before explicit Enter. The fixture
  closes Files before terminal keyboard input, as the open browser suppresses
  terminal keys. Captures and recorded server input were inspected. Automatic
  fallback, exact transfer bytes, private permissions and all owned cleanup pass.
  Application logic is unchanged. Initial fixture runs exposed a missing XTest
  punctuation mapping and attempted terminal input with Files still open; both
  fixture issues were corrected, and their owned resources were cleaned up.
  Native: `.qa/linux-runtime-2bef9997297a4680b97c79fcd135508f/results.json`.
  Linux build: `.qa/linux-cross-476457bb3cbc4ad0bffd073f1e8da700/results.json`.
  Windows seven-gate validation (`-SkipSsh`), including 142 app tests, one
  backend regression, strict Clippy, release and isolated installer:
  `.qa/validation-028ccd4311ab497084d01694c03e3a48/results.json`.
  Clipboard-image/drop, local file dialogs, advanced connection controls and
  broader release acceptance remain pending. Full development is not complete.

- 2026-09-09, fresh v0.1.85: native automatic SFTP-refusal fallback passes.
  An isolated server omits the SFTP subsystem; its log confirms four refused
  subsystem requests. The GUI lists the remote folder through the shell and
  uploads/downloads 65,792 exact binary bytes with the SCP option unchecked.
  Confirmation and completion captures were inspected. Recorded server commands
  confirm shell listing and SCP transfers, with no SFTP worker started. Private
  upload permissions, original sources, staging cleanup and owned process/display/
  directory cleanup all pass. Application logic is unchanged; the new fixture
  option is `--gui-fallback`.
  Native: `.qa/linux-runtime-494a4c92e56b4f37bd8834f979d9b576/results.json`.
  Linux build: `.qa/linux-cross-fa5ad6c048a44ccb8a35d707e62c35c5/results.json`.
  Windows seven-gate validation (`-SkipSsh`), including 142 app tests, one
  backend regression, strict Clippy, release and isolated installer:
  `.qa/validation-f8f4901c61a7450babe8b6d9c17f9bbb/results.json`.
  Clipboard/drop, local file dialogs, advanced connection controls, remaining
  shutdown/reconnect paths and broader release acceptance remain pending.
  The full development goal is not complete.

- 2026-09-09, fresh v0.1.84: native active SCP download cancellation passes.
  The fixture creates a separate 67,371,008-byte remote source and pauses its
  SCP sender after local staging contains at least 1 MiB. It verifies 1,409,024
  received bytes against the source prefix before cancelling. Local staging
  disappears, the partial destination is never published, and the full remote
  source, original upload, protected local destination and recovered download
  remain intact. The active-transfer and final cancellation captures were
  inspected; the GUI reports SCP download cancelled. All prior SCP acceptance
  cases and owned process/directory cleanup pass. Application logic is unchanged;
  use `--gui-scp`.
  Native: `.qa/linux-runtime-a2f5ca4eb9874718996d490fa1652902/results.json`.
  Linux build: `.qa/linux-cross-bb1d1b2e28994eb98dbe59bc13e97a9a/results.json`.
  Windows seven-gate validation (`-SkipSsh`), including 142 app tests, one
  backend regression, strict Clippy, release and isolated installer:
  `.qa/validation-441b325a25e0497d91fc5701f4c3acdb/results.json`.
  Native SFTP-refusal fallback, clipboard/drop, advanced connection controls,
  remaining shutdown/reconnect paths and broader release acceptance remain
  pending. The full development goal is not complete.

- 2026-09-09, fresh v0.1.83: native active SCP upload cancellation passes.
  The fixture uploads a 67,371,008-byte source and identifies the actual SCP
  receiver child by its command and owned staging path. It pauses that receiver
  after 1,048,576 verified payload bytes, clicks Cancel transfer and resumes it.
  The staging directory disappears, the partial destination is never published,
  the original upload and protected local destination survive, and the large
  source remains unchanged. The receiver is tracked separately for cleanup.
  Active-transfer and stopped-transfer captures were inspected. The GUI retains
  its conservative warning that remote state may need verification; this
  responsive-server fixture demonstrates cleanup, not a guarantee for every
  interrupted connection. All earlier SCP checks and owned cleanup pass.
  Application logic is unchanged; use `--gui-scp`.
  Native: `.qa/linux-runtime-068d38e4ecd54371b99fe70e5ceb88be/results.json`.
  Linux build: `.qa/linux-cross-e73b005dacc24be2b50a28273609662d/results.json`.
  Windows seven-gate validation (`-SkipSsh`), including 142 app tests, one
  backend regression, strict Clippy, release and isolated installer:
  `.qa/validation-59367b021f804e9ebe1b16ab9bc539a8/results.json`.
  Native active SCP download cancellation, SFTP-refusal fallback,
  clipboard/drop, advanced connection controls and broader release acceptance
  remain pending.

- 2026-09-09, fresh v0.1.82: native SCP review cancellation and recovery after
  destination collisions pass. After the prior upload/download collision
  checks, the fixture prepares a new download destination, focuses Cancel and
  activates it. No additional SCP download command, destination or local stage
  appears. Reopening and confirming the download produces the exact 65,792
  original bytes; the protected existing local file and remote source remain
  unchanged, and the GUI reports the saved download. Cancelling a subsequent
  upload review similarly starts no SCP command and leaves the remote
  directory and local source unchanged. Cancel-focus and recovery captures
  were inspected. All preceding SCP checks and owned process/directory cleanup
  pass. Application logic is unchanged; use `--gui-scp`.
  Native: `.qa/linux-runtime-d0e67d01be5b475da7f35e84265a5457/results.json`.
  Linux build: `.qa/linux-cross-582b1b53f44c4cfeb5a1ea1f91a35eab/results.json`.
  Windows seven-gate validation (`-SkipSsh`), including 142 app tests, one
  backend regression, strict Clippy, release and isolated installer:
  `.qa/validation-8a6a4d369a534c5282ec607111775729/results.json`.
  Native SFTP-refusal fallback, cancellation of active SCP transfers,
  clipboard/drop, advanced connection controls and broader release acceptance
  remain pending. Review cancellation does not establish active-transfer cleanup.

- 2026-09-09, fresh v0.1.81: native SCP destination-collision acceptance
  passes for download and upload. After the verified binary round trip, the
  fixture replaces the local destination with distinct sentinel bytes and
  submits another SCP download to that path. The UI reports that the local
  destination already exists; sentinel bytes survive and local staging is
  absent. It then changes the upload source and submits a conflicting upload
  to the existing remote file. An additional SCP command is recorded, the UI
  reports publication failure, original remote bytes and 0600 permissions
  survive, and remote staging is removed. Both refusal captures were inspected.
  All earlier round-trip assertions and owned process/directory cleanup pass.
  Application logic is unchanged; `--gui-scp` now includes these cases.
  Native: `.qa/linux-runtime-c209f1c437794413bdfb94bd6c7c80c0/results.json`.
  Linux build: `.qa/linux-cross-6593e28f5ac142d7a00c49d6c9b6ec5b/results.json`.
  Windows seven-gate validation (`-SkipSsh`), including 142 app tests, one
  backend regression, strict Clippy, release and isolated installer:
  `.qa/validation-7c284c5b293849038f2dbf85f55da306/results.json`.
  Native SFTP-refusal fallback, SCP cancellation/recovery, clipboard/drop,
  advanced connection controls and broader release acceptance remain pending.

- 2026-09-09, fresh v0.1.80: native explicit SCP upload/download round trip
  passes. The fixture opens Upload with SCP, confirms a 65,792-byte binary
  source, verifies exact remote contents and 0600 permissions, then opens
  Download with SCP and verifies an exact local copy. Both sources remain
  unchanged; remote SCP staging and local download staging are removed.
  The isolated forced-command server records execution of real SCP upload and
  download commands, so this is not an SFTP transfer mislabeled as SCP.
  Review/confirmation/completion captures were inspected; confirmation buttons
  are visible and the final UI reports the saved download. All owned processes
  and temporary files are removed. This focused variant uses `--gui-scp`.
  Capture timing was adjusted to allow the GUI to render focus/completion,
  and worker ownership checks recognize null-separated SCP arguments.
  Final native: `.qa/linux-runtime-3042391a9e8e475ca599eaf2a746360e/results.json`.
  Linux build: `.qa/linux-cross-b1992e1e96e34ee59f2167d0cec8666c/results.json`.
  Windows seven-gate validation (`-SkipSsh`), including 142 app tests, one
  backend regression, strict Clippy, release and isolated installer:
  `.qa/validation-bc6258a1105d4d81ad350619faca1b38/results.json`.
  Application logic is unchanged. Native SFTP-refusal fallback, SCP collision
  and cancellation UI, clipboard/drop, advanced connection controls and broader
  release acceptance remain pending.

- 2026-09-09, fresh v0.1.79: native partial-download cleanup passes for both
  tab closure and normal application exit. Each fixture pauses its owned SFTP
  worker after 1,048,576 verified download bytes, closes the tab or application,
  and verifies local staging removal, absence of the cancelled destination,
  preservation of the full remote source and preservation of an existing local
  file. Final captures confirm the blank replacement tab and closed application.
  All owned processes and temporary directories are removed. The variants
  stop after download acceptance and skip unrelated upload capture retrieval.
  Tab close: `.qa/linux-runtime-020040fa16a7413e9e291b6fc3403d8f/results.json`.
  App exit: `.qa/linux-runtime-0d0532912570499f9bb18114e94255bf/results.json`.
  An initial concurrent app-exit fixture hit X11 BadMatch during startup focus,
  before download acceptance (`.qa/linux-runtime-16079645a3da408ca0c4d8bdd7a42653`).
  Xlib terminated Python before finally; the directory was removed by the
  outer runner, and a subsequent ownership audit stopped its remaining app,
  SSH listener and Xvfb, confirming no survivors (`cleanup-audit.json`).
  The fixture now waits for a viewable window and installs a scoped X11 error
  handler so protocol errors can follow Python's normal cleanup path.
  The corrected app-exit rerun passes. Application logic is unchanged.
  Linux build: `.qa/linux-cross-18411a1baccc4e9d982347b74997cff5/results.json`.
  Windows seven-gate validation (`-SkipSsh`), including 142 app tests, one
  backend regression, strict Clippy, release and isolated installer:
  `.qa/validation-6edcd3944d6c48009c23451298d8dd9c/results.json`.
  Use `--gui-files-download-close` and `--gui-files-download-exit`.
  Native clipboard/drop, SCP, advanced connection controls and broader release
  acceptance remain pending; these SFTP checks do not establish those paths.

- 2026-09-09, fresh v0.1.78: native stalled-server shutdown acceptance passes.
  `--gui-files-exit-stalled` keeps the owned SFTP worker paused after
  1,048,576 verified upload bytes, sends a normal window-close request and
  does not resume the worker until the application exits. The process exits
  successfully in 5.144 seconds, within the fixture's 4.5–7 second window
  around the five-second cleanup grace period. Remote staging is explicitly
  confirmed present at exit; this is a bounded-shutdown pass, not a remote
  cleanup pass. The cancelled destination is never published and the earlier
  uploaded file remains intact. The fixture resumes and stops its workers,
  removes its owned directory and verifies cleanup. The closed-window capture
  was inspected. Preceding file checks also pass, including cancellation after
  1,081,344 download bytes. Application logic is unchanged from v0.1.77.
  Native: `.qa/linux-runtime-6dd94a83388c42549479d01760ddf923/results.json`.
  Linux build: `.qa/linux-cross-6c546308828f45a8945d6622c227a616/results.json`.
  Windows seven-gate validation (`-SkipSsh`), including 142 app tests, one
  backend regression, strict Clippy, release and isolated installer:
  `.qa/validation-d9a1f07799c1448594cf93e2adb94081/results.json`.
  Native download tab-close/shutdown, clipboard/drop, SCP and broader release
  acceptance remain pending. A stalled server can leave staging behind;
  bounded exit does not promise cleanup after transport loss or forced exit.

- 2026-09-09, fresh v0.1.77: fixed partial remote uploads left behind by
  normal whole-application exit. A native WM_DELETE_WINDOW request against
  v0.1.76 after 1 MiB of upload payload reproduced exit status zero with staging
  still present (`.qa/linux-runtime-8e6c85d26fb6460ca50b01ff3af18252`).
  Workspace teardown now cancels all remaining file tasks and waits once, for
  at most five seconds total, for registered background cleanup. Registration
  precedes thread creation, and completion guards also account for sessions
  retired by earlier tab closures or Disconnect. The shared timeout does not
  multiply by tab count. A regression checks multiple pending cleanups,
  timeout, partial completion and notification after the last completion.
  Native rerun verifies 1,048,576 partial-upload bytes, removes remote staging
  before successful process exit, never publishes the cancelled destination
  and preserves the existing upload. All preceding file checks and owned
  process/directory cleanup pass. Active-transfer and closed-window captures
  were inspected. Use `--gui-files-exit` for this variant.
  Native: `.qa/linux-runtime-08578e20c43543418cbc16a35e6ab459/results.json`.
  Linux build: `.qa/linux-cross-f1ee393111364405b133c6227a681758/results.json`.
  Windows seven-gate validation (`-SkipSsh`), including 142 app tests, one
  backend regression, strict Clippy, release and isolated installer:
  `.qa/validation-36c8dfcdccdf47dd885cf9129d959482/results.json`.
  The full SSH/config suite last passed at v0.1.75; Linux unit execution last
  passed at v0.1.76. Native download tab-close/shutdown, timeout with an
  unresponsive server, clipboard/drop and SCP acceptance remain pending.
  Abrupt process termination or a lost/unresponsive transport can still
  prevent remote staging removal.

- 2026-09-09, fresh v0.1.76: explicit Disconnect now cancels file handles
  before allowing the busy session its bounded background cleanup, matching
  tab closure. Native Linux acceptance disconnects during an upload after
  1,048,576 verified bytes, removes remote staging, never publishes the cancelled
  destination and preserves the existing uploaded file. The final capture shows
  the disconnected form with Reconnect and retained-output review available.
  All earlier native file checks and owned process/directory cleanup pass.
  Evidence: `.qa/linux-runtime-7d8ae948481b43878fe9da5bfbdc1fb9/results.json`.
  Linux build: `.qa/linux-cross-5bf0f4b97dcd43e681a0fba44723d082/results.json`.
  Windows validation passes seven gates (`-SkipSsh`), including 141 app tests,
  one backend regression, strict Clippy, release and isolated installer:
  `.qa/validation-b818aa40d41b403881ef1e55ee31a3ce/results.json`.
  Current Linux unit executables were also built and run on <test-host>:
  101 library and 38 application tests pass (139 total), together with CLI
  startup/key generation checks and temporary-directory removal.
  Test build: `.qa/linux-cross-b9a46d3bb5294cf497097a9bb5a807d8/results.json`;
  runtime: `.qa/linux-runtime-40ffc741b49942f586f8d13641fe8a11/results.json`.
  The full SSH/config suite last passed at v0.1.75. Use
  `--gui-files-disconnect` for this native variant. Whole-process exit and
  download tab-close acceptance remain pending; remote cleanup still requires
  a responsive connection within the five-second bound.
  Follow-up source review found that application exit does not wait for the
  background session cleanup threads. Tab/Disconnect acceptance therefore
  does not establish cleanup on whole-application exit; that path needs its
  own reproduction and shutdown lifecycle fix.

- 2026-09-09, fresh v0.1.75: fixed remote upload staging left behind when
  closing its tab. Native reproduction on the previous binary closed the tab
  after 1 MiB while its owned SFTP worker was paused; staging never disappeared
  after resume (`.qa/linux-runtime-babbbf32cbd647bca0ba23ade69e652b`).
  Tab teardown now drops file handles to request cancellation and retires busy
  sessions in the background for at most five seconds. It drains terminal
  events to prevent output backpressure from blocking cleanup, disconnecting
  when file work finishes, the worker exits, or the bound expires. Idle sessions
  still close immediately. A unit regression verifies prompt caller return,
  output draining, transport preservation during cleanup and closure afterward.
  Native rerun closes the tab, removes staging, preserves the existing upload
  and never publishes the cancelled destination. All earlier file checks and
  owned cleanup pass; the closed-tab capture was inspected.
  Evidence: `.qa/linux-runtime-152125bafdbe40ab98a66e901566d4f2/results.json`.
  Linux build: `.qa/linux-cross-6e41ad57f0bb4879af911735a58098d3/results.json`.
  Full validation (12 gates, no SSH skip): format/check, strict Clippy, 141 app
  tests, one backend regression, 48 OpenSSH config cases, 101 SSH fixture PASS
  records, Windows release and isolated installer:
  `.qa/validation-92683ed6d9a847098db4c9d603804d03/results.json`.
  Use `--gui-files-close` to reproduce the native tab-close variant; ordinary
  `--gui-files` retains explicit upload cancellation. Whole-process exit,
  explicit disconnect during transfers, download tab-close, clipboard/drop,
  native SCP and broader acceptance remain pending. Cleanup is bounded and
  cannot guarantee server-side removal after a lost/unresponsive connection.

- 2026-09-09, fresh v0.1.74: native partial-download cancellation passes.
  The fixture receives exactly 1,048,576 verified bytes of a 67,108,864-byte
  source into a local staging file and pauses its owned SFTP worker. Native
  progress shows the same count. Cancel transfer reports cancellation and
  removes local staging without publishing the requested destination. The
  complete remote source and unrelated existing local binary remain unchanged.
  Source data is restored before the remaining upload checks, which also pass,
  including cancellation after 1 MiB. Captures were inspected and all owned
  processes, display and temporary directory are cleaned up.
  Evidence: `.qa/linux-runtime-7390643984c543359142b5c4840e12ff/results.json`.
  Initial attempts selected a folder because a retained completion notice
  shifted rows; added review/start captures exposed the coordinate error and
  corrected native input produced the final evidence above.
  Linux build: `.qa/linux-cross-7fef919ca0c74cb7a7f5b03982eb0cc8/results.json`.
  Format/check, strict Clippy, 140 app tests, one backend regression, Windows
  release and isolated installer pass:
  `.qa/validation-657c9591f87344ffb5109a9320bd257b/results.json` (`-SkipSsh`).
  No application fix was required. Transfer cleanup on tab close, clipboard/drop,
  native SCP, remaining advanced workflows and platform acceptance remain.

- 2026-09-09, fresh v0.1.73: native cancellation after upload payload passes.
  The fixture now waits for at least 1 MiB in the remote staging file before
  pausing its verified SFTP worker. This run held exactly 1,048,576 bytes of
  the 67,371,008-byte source. Every staged byte matched the source prefix, and
  the native progress display independently showed 1,048,576 bytes transferred.
  Cancel and worker resume remove the partial file without publishing the
  destination or altering the prior upload. Progress and cancellation captures
  were inspected; all preceding file checks and owned cleanup pass.
  Evidence: `.qa/linux-runtime-fd935defcd9c443588ff32527d0a3375/results.json`.
  Linux build: `.qa/linux-cross-a38802fca31749c9a9b5b97cc8a08318/results.json`.
  Format/check, strict Clippy, 140 app tests, one backend regression, Windows
  release and isolated installer pass:
  `.qa/validation-cbd4e7acd75b47ef8c028bcb3fbbf7de/results.json` (`-SkipSsh`).
  No application fix was required. Active-download cancellation, tab-close
  cleanup, clipboard/drop, native SCP and broader acceptance remain pending.

- 2026-09-09, fresh v0.1.72: native cancellation after upload staging begins
  passes. A monitor waits for the owned remote staging file, verifies the
  current fixture SFTP worker identity and pauses it. The native GUI shows an
  active 67,371,008-byte upload (zero acknowledged payload bytes in this run).
  Cancel transfer enters the cleanup-wait state. Resuming the worker permits
  cleanup; the UI reports cancellation and temporary-file removal. Filesystem
  assertions prove no destination is published, staging disappears and the
  prior uploaded binary is unchanged. All three captures were inspected and
  all preceding file checks and owned cleanup pass.
  Evidence: `.qa/linux-runtime-03bd5801ca21490e83d4ef6a79664c5c/results.json`.
  An initial monitor selected an exited worker from an earlier operation;
  selecting the latest verified live fixture worker corrected the test.
  Linux build: `.qa/linux-cross-7bcdf8b23f754c8592d1ee3c0bd76252/results.json`.
  Format/check, strict Clippy, 140 app tests, one backend regression, Windows
  release and isolated installer pass:
  `.qa/validation-97072bc15dea4222a6c83929615a9fe1/results.json` (`-SkipSsh`).
  No application fix was required. Cancellation later in payload transfer,
  active-download cancellation, tab-close cleanup, clipboard/drop, SCP and
  broader acceptance remain outstanding.

- 2026-09-09, fresh v0.1.71: native SFTP upload collision protection passes.
  After a successful binary upload, the fixture changes its local source to
  different bytes and confirms an upload to the same remote filename. The
  captured UI reports SFTP failure and removal of the remote temporary file.
  Exact filesystem assertions prove the original 65,792 bytes and 0600 mode
  remain intact, the changed local source is untouched, and no staging entries
  remain. Reviewed source/destination and error captures were inspected. All
  earlier file checks and owned process/display/directory cleanup pass.
  Evidence: `.qa/linux-runtime-15bc3b4dba58408c9ae66bae2f98d285/results.json`.
  Linux build: `.qa/linux-cross-b5085d18b38644679d582eb2dfeb8a69/results.json`.
  Format/check, strict Clippy, 140 app tests, one backend regression, Windows
  release and isolated installer pass:
  `.qa/validation-4821a4cfd99f4bed84a2c6bbd1cfc60e/results.json` (`-SkipSsh`).
  No application fix was required. Active-transfer cancellation, clipboard/drop,
  native SCP, remaining advanced workflows and platform acceptance remain.

- 2026-09-09, fresh v0.1.70: native upload review cancellation and retry
  pass. Keyboard Cancel is scrolled into view; cancellation leaves exactly
  the original remote directory entries and preserves the local binary source.
  Reopening the review and confirming uploads the exact 65,792-byte payload
  with 0600 permissions. Cancellation, retry and completed-listing captures
  were inspected. All preceding native file checks and owned cleanup pass.
  Evidence: `.qa/linux-runtime-63dfb88db2ca45dd8a6d04237ccd4171/results.json`.
  The expanded file workflow exceeded its previous 240-second fixture budget;
  its owned directory was removed and a subsequent /proc audit found no owned
  processes. The file-mode timeout is now 420 seconds; other modes retain 240.
  The rerun above completed successfully within that budget.
  Linux build: `.qa/linux-cross-3a4f4d088d554300946b3f6cb900078e/results.json`.
  Format/check, strict Clippy, 140 app tests, one backend regression, Windows
  release and isolated installer pass:
  `.qa/validation-3cf4dddabe09490897544fa52975c8b6/results.json` (`-SkipSsh`).
  No application change was required. This verifies cancellation before
  upload confirmation; active-transfer cancellation and upload collision,
  clipboard/drop, SCP and broader acceptance remain outstanding.

- 2026-09-09, fresh v0.1.69: native reviewed binary SFTP upload passes.
  The fixture uploads 65,792 bytes containing every byte value, verifies exact
  remote contents and 0600 permissions, and checks unchanged local source and
  remote seed contents. No remote destination exists before confirmation.
  Exact destination-directory contents exclude leftover staging files. Native
  wheel scrolling reveals the upload fields, keyboard focus reveals Start
  upload, and inspected captures show the reviewed paths, completion notice,
  final size and permissions. All preceding file checks and owned cleanup pass.
  Evidence: `.qa/linux-runtime-d84b00464fc04b65a456eb3fa421dcb0/results.json`.
  Linux build: `.qa/linux-cross-96c2ef2e6ffe49f7a09c50920ce9d87c/results.json`.
  Format/check, strict Clippy, 140 app tests, one backend regression, Windows
  release and isolated installer pass:
  `.qa/validation-4b96dbd0949148b4b2f02679644832d9/results.json` (`-SkipSsh`).
  No application fix was required. Native upload collision/cancellation,
  active-transfer cancellation, clipboard/drop, SCP and platform work remain.

- 2026-09-09, fresh v0.1.68: native download cancellation, collision and
  recovery acceptance pass. Keyboard Cancel scrolls into view and creates no
  destination file; reopening the review and confirming downloads exact seed
  contents. The fixture then replaces its owned local download with distinct
  binary data and retries that destination. The captured UI reports that the
  local destination already exists; the original bytes remain unchanged.
  Refresh and a new destination recover successfully with exact seed contents,
  while preserving the existing file. All prior file checks and owned cleanup
  pass. Cancellation, rejection and recovery captures were inspected.
  Evidence: `.qa/linux-runtime-d2b82780d7804f77a8599c2f9c55f76e/results.json`.
  An earlier collision attempt missed the field because the review retained
  its scroll position; its preservation assertion alone did not prove rejection.
  Corrected input produced the inspected existing-destination error above.
  Linux build: `.qa/linux-cross-5d7a1b10c8fb432a9745ebe7911851f0/results.json`.
  Format/check, strict Clippy, 140 app tests, one backend regression, Windows
  release and isolated installer pass:
  `.qa/validation-27cd24f8b28c48549ef9e9d312c09062/results.json` (`-SkipSsh`).
  No application fix was required. Cancellation during an active transfer,
  upload, dialogs, SCP and broader platform acceptance remain outstanding.

- 2026-09-09, fresh v0.1.67: native reviewed SFTP download acceptance found
  and fixed transfer-review overflow. Entering a valid destination added path
  details that pushed Start download/Cancel outside the native window. Upload
  and download reviews now have bounded vertical scrolling; focused confirm
  and cancel buttons request visibility. Other action reviews retain their
  existing layout. Native captures show the focused Start download and Cancel
  fully visible after scrolling. Confirmation creates the requested local file
  with exactly the seed bytes, leaves the remote source unchanged, and creates
  nothing before confirmation. Existing file operations and all owned cleanup
  checks pass. Evidence:
  `.qa/linux-runtime-e9f0cdfa30a446bc9b1172324cd2ebfb/results.json`.
  The initial download fixture dropped punctuation because X11 requires key
  names such as `slash`; corrected events proved the transfer worked before
  the UI fix (`.qa/linux-runtime-db01a542af674f828a55b9a5a52969c1`).
  Intermediate scroll layouts were revised after native captures exposed
  constrained creation dialogs and failure to reveal keyboard focus.
  Linux build: `.qa/linux-cross-05b03edf4d814c2a9d023e31eda84dea/results.json`.
  Format/check, strict Clippy, 140 app tests, one backend regression, Windows
  release and isolated installer pass:
  `.qa/validation-1bc4dff84ebf45dc9995c64f060a2ee5/results.json` (`-SkipSsh`).
  Native upload, transfer cancellation/collision, file-dialog and broader
  platform acceptance remain; this small-file download does not prove them.

- 2026-09-09, fresh v0.1.66: native SFTP deletion acceptance passes.
  Cancel preserves the selected file, its empty contents and 0640 permissions.
  Confirm removes only that file; exact remaining directory entries and seed
  contents are checked. Confirming deletion of a nonempty folder displays
  `SFTP error 4: Failure` with refresh guidance and preserves its child file.
  File cancellation/deletion and folder review/error captures were inspected.
  The first folder attempt missed its differently positioned Delete button;
  that run's preservation assertion alone was insufficient. Corrected native
  coordinates produced the inspected server error in the final run.
  Evidence: `.qa/linux-runtime-bf0c2b7a641244b8b6b059cc73092732/results.json`.
  Existing SFTP acceptance and owned process/display/directory cleanup pass.
  Linux build: `.qa/linux-cross-6a75cae185d4452380972105af85c8f8/results.json`.
  Format/check, strict Clippy, 140 app tests, one backend regression, Windows
  release and isolated installer pass:
  `.qa/validation-61b1f49e084242f09d9a7f660aa8db53/results.json` (`-SkipSsh`).
  An earlier sandboxed validation denied Windows IPC/private-key operations;
  the authorized unsandboxed rerun passed. An earlier Linux build overlapped
  a binary upload and failed with file-in-use; the sequential rebuild passed.
  No application fix was needed. Native transfer, additional error recovery,
  advanced connection and platform acceptance remain in the full scope above.

- 2026-09-09, fresh v0.1.65: native SFTP rename and reviewed permissions pass.
  Renaming `created` to `renamed` removes the original path while preserving
  empty contents and 0600 permissions. Entering 0640 in the permissions review
  leaves the file at 0600 until confirmation; confirmation changes the selected
  file to 0640 while the sibling remains 0600 and the seed contents are unchanged.
  Review and refreshed-listing captures were inspected. Existing creation,
  cancellation, folder navigation and all owned fixture cleanup checks pass.
  Evidence: `.qa/linux-runtime-358f2f1a033a4fd5af593c8504c829a8/results.json`.
  An earlier fixture attempt left keyboard focus on the toolbar; explicitly
  clicking the octal field corrected the test. No application fix was needed.
  Linux build: `.qa/linux-cross-f9d7b87c5d3f40fc9f6ae9864eb7ed53/results.json`.
  Format/check, strict Clippy, 140 app tests, one backend regression, Windows
  release and isolated installer pass:
  `.qa/validation-0869c6f389d7447a810aa9d6c7498307/results.json` (`-SkipSsh`).
  Native deletion, transfer and additional error/cancellation acceptance remain.

- 2026-09-09, fresh v0.1.64: native SFTP folder creation and navigation pass.
  The fixture creates `folder`, opens its directory row with a native double
  click, and creates `inside`. Filesystem assertions prove the new file is
  `folder/inside`, not a file in the parent. Using Up and creating `sibling`
  produces the parent file, not `folder/sibling`. Exact parent/child directory
  contents are checked, and opened-folder, child, parent and final captures
  were inspected. Existing listing/create/cancel checks and owned cleanup pass.
  Evidence: `.qa/linux-runtime-353f5bee79a4415a909bb7a654fe8e4d/results.json`.
  Linux build: `.qa/linux-cross-ee37bff1de1f427a9df460793dbba59a/results.json`.
  Format/check, strict Clippy, 140 app tests, one backend regression, Windows
  release and isolated installer pass:
  `.qa/validation-af2c0cc408384125a18d2ccadd108123/results.json` (`-SkipSsh`).
  No application fix was needed. Native rename, chmod, deletion and transfer
  workflows remain acceptance work.

- 2026-09-09, fresh v0.1.63: added `--gui-files` native Linux SFTP acceptance.
  The isolated sshd routes subsystem requests to the host's sftp-server with
  an owned temporary starting directory; terminal requests retain the existing
  fixture worker. Cleanup tracks both worker types. The native browser lists
  the seed file, opens New file review, accepts keyboard cancellation and
  confirmation, and refreshes after creation. Before confirmation and after
  cancellation the directory contains only the seed file. Confirmation creates
  exactly an empty `created` file with 0600 permissions; the seed is unchanged.
  Review/cancel/created captures were inspected. App, sshd, SFTP workers,
  display and temporary directory cleanup pass.
  Evidence: `.qa/linux-runtime-f059f7f96f0545428ac9340c598bce20/results.json`.
  Earlier native layout probes guided the field coordinates; an initial passing
  run was tightened to verify the entire directory after cancellation and allow
  the final filename character to render before capture.
  Linux build: `.qa/linux-cross-f03be87b3191450c979426756e5b6af4/results.json`.
  Format/check, strict Clippy, 140 app tests, one backend regression, Windows
  release and isolated installer pass:
  `.qa/validation-8c367978af074fef9ae4efe275791e55/results.json` (`-SkipSsh`).
  No application fix was needed. Native folder/navigation, rename, chmod,
  deletion, transfer and clipboard-image workflows remain acceptance work.

- 2026-09-09, fresh v0.1.62: native Linux resize and retained selection pass.
  After selecting a wrapped logical line and receiving replacement output, the
  fixture resizes the native window from 1180x780 to 1000x700 and back. The
  remote worker observes PTY sizes 104x25 -> 83x20 -> 104x25, proving propagation
  through SSH. Copy/paste afterward still submits exactly the original 140
  characters in one input record. Smaller/restored captures were inspected;
  selection remains present and the layout fits both window sizes.
  Existing clipboard, Unicode, bracketed-paste, search, snippet and reconnect
  acceptance plus owned fixture cleanup pass.
  Evidence: `.qa/linux-runtime-f5dde58456264ac0b5464cb665b833b0/results.json`.
  Linux build: `.qa/linux-cross-0703b9e87a8d490291dea0593be5b598/results.json`.
  Format/check, strict Clippy, 140 app tests, one backend regression, Windows
  release and isolated installer pass:
  `.qa/validation-ea27db2b42614fd2ae15e563223b9d59/results.json` (`-SkipSsh`).
  No application fix was required for this acceptance increment. Remaining
  native file-transfer, extended search/link and platform work is tracked above.

- 2026-09-09, fresh v0.1.61: native wrapped-line selection and frozen-copy
  acceptance pass. A 140-character logical line wraps across 104 and 36 cells;
  after drag selection, the remote worker clears/replaces the screen. Ctrl+C
  still copies the original completed selection. Pasting and submitting yields
  exactly the original 140 characters in one server input record, with no added
  newline or replacement-screen text. The selected, replaced and pasted-screen
  captures were inspected. This verifies frozen copied text; the live screen
  continues to update while the selection highlight remains present.
  Existing Unicode/clipboard/bracketed-paste/search/snippet/reconnect checks
  and owned fixture cleanup pass.
  Evidence: `.qa/linux-runtime-a0e7f592f9964a5392402bc07a934394/results.json`.
  Linux build: `.qa/linux-cross-8ef33ecc8dbf4b629c7a7b2ba938f6d7/results.json`.
  Format/check, strict Clippy, 140 app tests, one backend regression, Windows
  release and isolated installer pass:
  `.qa/validation-5a9fa8950d6f4b85afa5eb3302c7b689/results.json` (`-SkipSsh`).
  This increment adds native acceptance coverage; no application fix was
  required. Selection after native resize and clipboard transfers remain
  separate acceptance work.

- 2026-09-09, fresh v0.1.60: fixed missing native Linux CJK and emoji glyphs
  by loading installed fallback fonts from standard system locations. The
  original startup only tried Windows CJK fonts; native captures showed boxes
  even though the clipboard preserved the text. Linux verification uses the
  host's WenQuanYi Zen Hei and Unifont Upper fonts; no fonts were installed or
  bundled. Noto CJK/Symbola and other platform paths are also candidates, but
  their rendering is not claimed as verified by this Linux run.
  Native selection/copy/paste preserves exactly `  A界é🙂Z`, including both
  leading spaces and the decomposed combining accent. A reverse drag across
  the CJK character's second cell copies exactly `界`. All three Unicode
  captures were inspected; the CJK character and emoji now render.
  Evidence: `.qa/linux-runtime-0b623ffffa02471289bcf3281e22b55b/results.json`.
  Existing clipboard, bracketed-paste, search, snippet, reconnect and owned
  fixture cleanup checks pass. Initial evidence
  `.qa/linux-runtime-6292d2aa89124f94b1b72c1389f23169/results.json` exposed the
  missing glyphs and two fixture issues: a drag endpoint on the preceding cell
  boundary and Windows-default decoding of UTF-8 error output. Both fixture
  issues were corrected without changing the expected clipboard contents.
  Linux build: `.qa/linux-cross-41890f7fef8e48f1bb22a972596303b0/results.json`.
  Format/check, strict Clippy, 140 app tests, one backend test, Windows release
  and isolated installer pass:
  `.qa/validation-db758854a10a4d5d9ddde8edb099cf72/results.json` (`-SkipSsh`).
  Native selection after output/resize, long wrapped lines and clipboard
  transfers remain separate acceptance work.

- 2026-09-09, fresh v0.1.59: native Linux bracketed-paste acceptance passes.
  The isolated SSH worker enables mode 2004 and switches its PTY to raw input.
  Both Ctrl+Shift+V and confirmed Ctrl+V deliver exactly
  `ESC[200~first\nACK firstESC[201~`, verified from raw byte logs with no
  duplicate markers or extra input. Ordinary paste still waits for review in
  bracketed mode. After disabling the mode and restoring canonical input,
  `afterbracketed` arrives as normal input. The review and final captures were
  inspected. Existing selection/clipboard/search/snippet/reconnect checks and
  owned app/server/worker/display/directory cleanup also pass.
  Evidence: `.qa/linux-runtime-da5d32701f1f47afad2e20197d3a2449/results.json`.
  Linux build: `.qa/linux-cross-8fd38e9bb1c04d019a25b58e6863db1b/results.json`.
  Format/check, strict Clippy, 140 application tests, one backend regression,
  Windows release and isolated installer pass:
  `.qa/validation-82c0f49cda164f768a2df27cd768a510/results.json` (`-SkipSsh`).
  This adds native acceptance coverage; no application fix was required.
  Unicode selection, clipboard transfers and tmux-specific native paste
  acceptance remain separate work.

- 2026-09-09, fresh v0.1.58: fixed clipboard shortcuts losing their Shift
  modifier when the key is released before the next GUI frame. Native Linux
  Ctrl+Shift+V incorrectly opened multiline review on v0.1.57:
  `.qa/linux-runtime-59f568d5e57a4706b6d0b02a28387afc/results.json`.
  The local egui-winit 0.28.1 patch now preserves the shortcut key-press event
  immediately before Copy/Paste, and the terminal uses those modifiers. This
  preserves exact press-time intent regardless of later modifier releases or
  presses. Backend provenance, patch notes and both upstream licenses are in
  `vendor/egui-winit-0.28.1`. Application regression failed before the fix and
  passes afterward; it also verifies copy-only does not interrupt and a later
  Shift press cannot suppress Ctrl+C or bypass ordinary paste review.
  The backend regression is now a required validation gate.
  Native Linux force-paste sends exactly `first\nACK first` without review;
  Ctrl+Shift+C with no selection leaves the same worker alive and accepting
  `aftercopy`. The force-paste capture was inspected. Selection, multiline
  cancellation/confirmation, search, snippets, reconnect and owned fixture
  cleanup pass:
  `.qa/linux-runtime-0a6a59e2848641a8931ba791639d5222/results.json`.
  Linux build: `.qa/linux-cross-51fbac7752cd492f8faec1d402cf5d61/results.json`.
  Full validation passes all 12 gates with no skips: 140 application Rust tests,
  one backend regression, 48 OpenSSH config comparisons, 101 SSH fixture PASS
  records, strict Clippy, Windows release and isolated installation:
  `.qa/validation-0463e224ab304d0699887b7677e1b1f3/results.json`.
  Native bracketed paste, Unicode selection and clipboard transfers remain
  separate acceptance work; full development is not complete.

- 2026-09-09, fresh v0.1.57: fixed selection dropping its final characters when
  the final pointer movement and release arrive in the same frame. Native
  selection copied `ACK fir` instead of `ACK first` on v0.1.56:
  `.qa/linux-runtime-7e7635a4cf7a4301b8b9bcf6b3dda104/results.json`.
  A focused pointer-event regression reproduced `fir` instead of `first`.
  Updating the endpoint on drag release fixes it; both start/end regressions
  pass, and the subsequent native multiline tests pass on v0.1.58 above.
  Local format/check/Clippy, 139 Rust tests, release and isolated installer
  passed in `.qa/validation-0db5fce8dfaf436e9999b617a5a2092f/results.json`
  (`-SkipSsh`). The native v0.1.57 run then exposed the modifier bug fixed above.

- 2026-09-09, fresh v0.1.56: native Linux multiline clipboard acceptance passes.
  Selecting two output rows copies `first\nACK first`; Ctrl+V opens a review
  showing both lines and the fixture destination. No input reaches the server
  before confirmation. Cancel sends neither line, and `aftercancel` is received
  normally with no pasted prefix. Confirming sends exactly the first line and
  leaves the final line awaiting Enter; its subsequent submission is exactly
  `ACK first`. Right-click with the same multiline clipboard opens review too.
  Review, confirmed-output and right-click-review captures were inspected.
  Search, snippet sends, disconnect/review/reconnect and full owned fixture
  cleanup pass. Evidence:
  `.qa/linux-runtime-166edd5b5e04480998aa655d25c4314a/results.json`.
  Linux build: `.qa/linux-cross-9ceb6145644b4561ae6b4bb556c6f83a/results.json`.
  Windows format/check, strict Clippy, 138 Rust tests, optimized release and
  isolated installer pass:
  `.qa/validation-3e5829cb4cab4228b962f159fb0bc88d/results.json` (`-SkipSsh`).
  This increment adds acceptance coverage; no further application bug was
  found. Native force-paste, bracketed mode, Unicode selection and clipboard
  transfers remain unverified. Initial dialog-layout probes were intentionally
  partial and are not counted as acceptance passes.

- 2026-09-09, fresh v0.1.55: fixed drag selection losing its first character.
  The native Linux clipboard fixture copied `irst` when dragging over `first`
  on v0.1.54 (`.qa/linux-runtime-d1e2e1cc6cdd44009307e1b87e62de79/results.json`).
  A focused egui pointer-event regression reproduced the same failure.
  Selection now uses the original button-press position when the drag threshold
  is crossed. The regression passes after the fix. Native Linux Ctrl+C clears
  the highlight, Ctrl+V pastes the complete word, and right-click paste submits
  the same complete word; exact server records verify both paths. Selection,
  cleared-selection and pasted-text captures were inspected. Search, snippets,
  disconnect/review/reconnect and owned process/display/directory cleanup pass.
  Evidence: `.qa/linux-runtime-dace88d9cca2456a9374d7cfc41c398f/results.json`.
  Linux build: `.qa/linux-cross-d230863499aa43c1a9b14cd6419c8d54/results.json`.
  Windows format/check, strict Clippy, 138 Rust tests, optimized release and
  isolated installation pass in
  `.qa/validation-9a5add83170443eea621afe453431f5c/results.json` (`-SkipSsh`).
  Unicode/multiline selection, paste review and clipboard transfers still
  require native acceptance; this is not a completed parity release.

- 2026-09-09, fresh v0.1.54: native Linux terminal search acceptance passes.
  Ctrl+F focuses the query; searching `first` shows two highlighted matches,
  replacing it with `absent` shows No matches with navigation disabled, and
  Escape closes search and returns to live output. All three captures were
  inspected. Neither query nor its Enter reaches the server. After closing,
  the fixture submits `aftersearch` and the server records it normally.
  Snippet sends, manual disconnect, output review, reconnect and full owned
  fixture cleanup also pass.
  Evidence: `.qa/linux-runtime-811582f5c9834bbf9ffbb51d5fe41d60/results.json`.
  Linux app build: `.qa/linux-cross-60bcaaa444b448b6a8ba2ae15c86bfe7/results.json`.
  Windows format/check, strict Clippy, 137 Rust tests, optimized release and
  isolated installer checks pass:
  `.qa/validation-269912fce6fd488ea6ee610760cb160c/results.json` (`-SkipSsh`).
  Native search navigation, case matching, long-scrollback and copy from a
  search snapshot remain separate acceptance cases.

- 2026-09-09, fresh v0.1.53: native Linux session acceptance now exercises
  reviewed saved-snippet sending. It creates/saves a single-line snippet,
  sends with Enter disabled, verifies the server has not received a submitted
  line, and captures its insertion at the terminal cursor. A manual Enter
  submits it once. Reopening/selecting the saved snippet and enabling Enter
  submits it once more. The server log contains exactly `first`, `snippet`,
  `snippet` in the first session and `second` after reconnect; no extra lines.
  The three snippet captures were inspected. Manual disconnect, output review,
  reconnect and explicit app/sshd/worker/display/directory cleanup also pass.
  Evidence: `.qa/linux-runtime-10bd4e1a4dce4960ada8a7b929fd3f82/results.json`.
  Linux app build: `.qa/linux-cross-fe3e8bc03b964a119d8b04f50cedc178/results.json`.
  Windows format/check, strict Clippy, 137 Rust tests, optimized release and
  isolated installer verification pass:
  `.qa/validation-056430fe103b4527837c3e407edb231a/results.json` (`-SkipSsh`).
  Multiline/bracketed snippet sends and changing destination tabs during review
  remain separate acceptance cases.

- 2026-09-09, fresh v0.1.52: added native Linux `--gui-session` acceptance
  using a separate unprivileged loopback OpenSSH server, generated client/host
  keys, strict client host verification and a forced synthetic command. The
  host's normal SSH service and authorized keys are untouched; temporary sshd
  StrictModes is disabled solely to allow the owned directory under /tmp.
  Native pointer/keyboard input connects from a saved profile, sends `first`,
  disconnects and verifies the first remote worker stops, reviews retained
  output, returns to the prefilled form, reconnects and sends `second` to a
  distinct worker. Input typed while reviewing disconnected output produces no
  remote input record. Captures show the first session's output retained during
  review and replaced by the second session's marker after reconnect succeeds.
  The event log records exactly one submitted line per connected session.
  Explicit cleanup checks confirm the app, isolated sshd, session workers and
  display stopped, and the remote directory was removed.
  Evidence: `.qa/linux-runtime-d56ae8ab74204e0da6c023557588517f/results.json`.
  Linux app build: `.qa/linux-cross-e9e3d9afaa6f4a449ac626088be2b7a3/results.json`.
  Windows format/check, strict Clippy, 137 Rust tests, optimized release and
  isolated installation pass:
  `.qa/validation-44267bcbef5642089973b21b16e2db69/results.json` (`-SkipSsh`).
  Native automatic-retry countdown, clipboard/search, transfers and other
  connected workflows remain separate acceptance requirements.

- 2026-09-09, fresh v0.1.51: added an egui event regression for disconnected
  output review with Sync input capture enabled. After focusing the actual
  terminal widget, text, single/multiline paste, Enter, and IME preedit/commit
  events produce neither outbound bytes nor captured sync events. Existing
  output remains intact and IME text is cleared. A positive control connects
  the same focused widget and confirms text is both sent and captured, avoiding
  a test that passes merely because the widget never received focus.
  The renderer's existing disconnected-input guard already satisfied this
  behavior; this increment adds regression coverage, not a production fix.
  Windows format/check, strict Clippy, all 137 Rust tests, optimized release
  and isolated installer checks pass:
  `.qa/validation-d340f49479fa4a65ac0355d98fd10d40/results.json` (`-SkipSsh`).
  Native output-review, copy/search and reconnect clicks remain pending.

- 2026-09-09, fresh v0.1.50: disconnected tabs now return to the prefilled
  connection form even after receiving output. Reconnect, last error and retry
  countdown remain available. Review previous output / Edit connection switch
  between retained terminal output and the form; reconnect still clears the
  old terminal only on successful connection. Manual disconnect clears the
  password and retry state while preserving identity, target, tmux and output.
  Regressions cover manual disconnect and dropped-session form routing with
  retained output/errors. Returning forms omit the welcome heading so their
  host field remains visible at minimum window size; remaining fields scroll.
  Native Windows loopback captures verify real output/CPR/resize followed by
  remote disconnect at 1180x780 and 760x480. Both final captures were inspected.
  Normal, minimum, alternate-screen and load cases also pass; the final load
  run processed 7,864,765 bytes with max frame 32.15 ms and p95 15.62 ms.
  Evidence: `.qa/connected-render-db73fc53baa44124b5a351a0d72a3a4f/results.json`.
  An initial load run narrowly exceeded the unchanged 100 ms limit (100.89 ms);
  subsequent runs passed. The new disconnect fixture was corrected to wait for
  both CPR and resize before closing, preserving its geometry assertion.
  Windows format/check, strict Clippy, 136 Rust tests, optimized release and
  isolated installer verification pass:
  `.qa/validation-9831e7f593694dfa96480a5265949228/results.json` (`-SkipSsh`).
  Final Linux all-target check passes using the configured Zig compiler:
  `.qa/linux-cross-d7c5a990c1f54f98ad8810b6f7b6300c/results.json`.
  Plain Cargo without that compiler failed because Linux GCC is unavailable.
  Native clicks through output review/manual disconnect/reconnect and Linux
  runtime acceptance of the changed flow remain pending.

- 2026-09-09, fresh v0.1.49 full regression refresh: the default
  `tests/validate.ps1` passes all 11 gates with no SSH or installer skips.
  This includes formatting/checking, strict all-target Clippy, 135 Rust tests,
  rebuilding the actual app and fixture probes, 48 OpenSSH `ssh -G` comparisons,
  the complete loopback SSH fixture, optimized Windows release and isolated
  installation/reinstallation. SSH checks cover authentication/fallback/prompt
  cancellation, strict/hashed/revoked/changed host keys, forwarding, one/two-hop
  routing, rekeying, SFTP/SCP, shell fallback, malformed responses, timeouts,
  transfer cancellation/cleanup and terminal IO after each scenario.
  Evidence: `.qa/validation-828206a1ea294d38bcc555a835b5bd26/results.json`.
  This is automated regression evidence, not completion of the native matrix.
  Reviewing `docs/redevelopment/01-product-requirements.md` and
  `04-validation-and-release.md` exposed an outstanding disconnect-form gap:
  the current GUI shows the connection form only when no session exists and
  no terminal bytes have been received. After a used connection is disconnected,
  it stays in the terminal view instead of the required prefilled form.
  The fix must also preserve tab identity, scrollback until reconnect succeeds,
  last error and the reconnect/countdown controls. This remains unresolved.

- 2026-09-09, fresh v0.1.49: fixed saved-profile renames leaving the old
  entry behind. `Config::rename_profile` commits the updated profile and old
  entry removal together; failures preserve live and disk state. The GUI
  updates matching open-tab identities only after persistence succeeds,
  retaining distinct unsaved draft names and the existing numeric-suffix
  collision policy. The regression covers failed writes, invalid names,
  missing source profiles, occupied destinations, reload, repeat save, and
  preservation of unrelated/unknown settings.
  Native Linux acceptance creates and edits a profile, renames it to `alpha`,
  saves twice, restarts and loads it with its edited host/user intact. The old
  identity is absent; the unrelated profile survives. Cancellation/deletion
  and final restart checks pass. Rename and reload captures were inspected.
  An initial fixture attempt focused the toolbar after expanding options;
  the corrected fixture targets the visible Saved name field directly.
  Evidence: `.qa/linux-runtime-e90255bf2cb4491b8d106c50f278e1fb/results.json`.
  Owned processes stopped and the remote directory was removed.
  Linux app build: `.qa/linux-cross-2903dab109924a76bd385f70870a7014/results.json`.
  Windows format/check, strict Clippy, 135 Rust tests, optimized release and
  isolated installer verification pass:
  `.qa/validation-a311f21a81d64db9889338fb457370f4/results.json` (`-SkipSsh`).
  Native name-collision suffix behavior and multiple open-tab rename/save
  interactions remain separate acceptance work.

- 2026-09-09, fresh v0.1.48: expanded native Linux profile acceptance to
  create a connection through the form, save twice without duplicates, restart
  and load it, edit its host and username, then save twice again. The same
  profile identity remains and the new values load after another restart.
  Synthetic passwords entered in both save phases are absent from the saved
  configuration; captures show the password field empty when reloaded.
  Cancel/Escape/close preserve the profiles, confirmed deletion survives a
  final restart, and the unrelated profile including its unknown field remains.
  Creation, loaded, edited and restarted captures were inspected. The fixture
  stopped its owned app/display processes and removed the remote directory.
  Evidence: `.qa/linux-runtime-f663554a602d4923bc1ef591a38ba935/results.json`.
  Linux app build: `.qa/linux-cross-5ff12cd08f1e4317a529381b02f9abc6/results.json`.
  Windows format/check, strict Clippy, 134 Rust tests, optimized release and
  isolated installer checks pass:
  `.qa/validation-4f1c91fbbd2c48a09cd35994f68615f1/results.json` (`-SkipSsh`).
  Advanced profile controls, rename/collision behavior and connected-session
  interaction remain separate acceptance work.

- 2026-09-09, fresh v0.1.47: added `--gui-profile` native Linux acceptance.
  The fixture opens a synthetic saved profile without connecting, uses its
  context menu, and dismisses deletion by Cancel, Escape and the close button.
  All three preserve both saved profiles; inspected captures show the review
  closed and the original profile tab retained. Confirmed deletion removes
  only the requested profile. Restart preserves that removal and the other
  profile's host and unknown field. Captures show the open draft tab retained
  immediately after deletion. Connected-session preservation remains separate
  acceptance. Owned app/display processes stopped and remote files were removed.
  Evidence: `.qa/linux-runtime-4c94074def9444498ea50855869e3365/results.json`.
  Initial fixture failures were corrected: mode dispatch skipped GUI startup,
  then whole-file cancellation comparison incorrectly included normal workspace
  autosaving. The final assertion compares the complete saved-profile data.
  Linux app build: `.qa/linux-cross-046d2cb01111441ba10850f007e28eb5/results.json`.
  Windows format/check, strict Clippy, 134 Rust tests, optimized release and
  isolated installer checks pass:
  `.qa/validation-4c601f5383694410b898f849d4a540cd/results.json` (`-SkipSsh`).

- 2026-09-09, fresh v0.1.46: added the missing saved-profile deletion UI.
  Right-click a saved host and choose Delete saved connection; a confirmation
  names the profile and explains that open sessions remain open. Cancel,
  Escape and the close control dismiss the review. Background controls and
  workspace shortcuts are disabled during review. Confirming uses transactional
  `Config::delete_profile`; only a successful write removes the live profile
  and clears matching open tabs' saved-profile identity, retaining their
  connection settings and sessions. The regression verifies failed writes
  preserve live/disk state and leave no staging files, successful deletion
  survives reload, unrelated profiles/settings/unknown fields survive, and
  deleting a missing profile leaves the saved file unchanged.
  Windows format/check, strict Clippy, 134 Rust tests, optimized release and
  isolated installer verification pass:
  `.qa/validation-711243ad06854c178a3b86d47382289a/results.json` (`-SkipSsh`).
  The first attempt was interrupted with STATUS_CONTROL_C_EXIT during test
  compilation; the subsequent complete run passed.
  Linux application compilation/linking passes:
  `.qa/linux-cross-c574f6f8cd7d48ca81cce6a14dea79ec/results.json`.
  Native profile context-menu/confirmation/cancel/restart acceptance remains
  pending; these build and persistence checks do not prove that UI workflow.

- 2026-09-09, fresh v0.1.45: removed the snippet command editor's Tab focus
  lock so keyboard users can reach Save and Delete. Native Linux acceptance
  creates, edits and cancels a multiline snippet, then selects and deletes it
  using Tab/Enter. A further application restart confirms no saved snippet
  remains; screenshots show the empty editor and disabled Delete control.
  Preferences save/restart/cancel checks also pass. Owned app/display
  processes stopped and both fixture directories were removed.
  GUI evidence: `.qa/linux-runtime-9e8859005ecd4f1f87e9b376ae610643/results.json`.
  Fresh native Linux test binaries pass 98 library and 33 application tests:
  `.qa/linux-runtime-2f5b8648f24742ab9d00b26c164fb5a1/results.json`.
  Linux app/test builds: `.qa/linux-cross-8deb9299dece4b5a94a14910783de4db/`
  and `.qa/linux-cross-9f75f8e288cf4f9396522d62701daae2/`.
  Windows formatting/checking, strict Clippy, 133 Rust tests, optimized
  release and isolated installer checks pass:
  `.qa/validation-bdf45c99bd2541a8b5ca6c432a2e2a6a/results.json` (`-SkipSsh`).
  Reviewed snippet sending and other connected native interactions remain
  separate acceptance work.

- 2026-09-09, fresh v0.1.44: snippet editing now focuses the name field on
  opening or selection, supports Ctrl/Cmd+S to save, and Escape to discard
  unsaved changes and close. Native Linux keyboard acceptance creates a
  multiline snippet, restarts and selects it to rename/edit, then restarts
  again and cancels unsaved changes. The saved file retains exactly one
  snippet with the edited name and multiline command. No command is sent to
  a remote shell in this test; native deletion and reviewed sending remain
  pending. Preferences save/restart/cancel checks also pass. Captures were
  inspected, owned display/app processes stopped, and remote files removed.
  Evidence: `.qa/linux-runtime-e980b74947c24f949b91232e9f484429/results.json`.
  Linux app build: `.qa/linux-cross-1f29589f1b0b42788d3a711dc0d86e86/results.json`.
  Windows formatting/checking, strict Clippy, all 133 Rust tests, optimized
  release and isolated installer checks pass:
  `.qa/validation-aa83a364b0df490cb092bdb8cad5705b/results.json` (`-SkipSsh`).

- 2026-09-09, fresh v0.1.43: fixed failed Save connection operations updating
  the live profile list before persistence. The GUI now calls transactional
  `Config::save_profile`: prepare a cloned config, persist it, then replace
  live state. Failed writes leave the sidebar's underlying profile map and
  draft identity unchanged. A real filesystem-failure regression covers new
  and existing profiles, preservation of existing disk data, staging cleanup,
  successful retry without duplicate profiles, unrelated settings, and
  validation failures preserving both live state and the saved file.
  Formatting/checking, strict Clippy, all 133 Rust tests, optimized Windows
  release and isolated installer checks pass:
  `.qa/validation-2cf00313e1da45c2917baf12a362417a/results.json` (`-SkipSsh`).

- 2026-09-09, fresh v0.1.42: theme choices now focus the current selection
  when opened. Keyboard selection closes the list and returns focus to the
  theme control. The dialog regression covers selection, popup dismissal,
  focus restoration and preservation of unsaved edits. Native Linux keyboard
  acceptance passes: save Forest, restart and save Amber, restart and choose
  Forest but cancel; Amber remains saved. Zoom, text size, keep-alive and
  scrollback save/reload/cancel checks also pass. Captures were inspected;
  owned display/app processes stopped and remote files were removed.
  Evidence: `.qa/linux-runtime-dd5dc99cb38a4fd88e8edebb57d50e24/results.json`.
  Linux app build: `.qa/linux-cross-6070464d8d464d668939177db981be8a/results.json`.
  Windows formatting/checking, strict Clippy, all 132 tests, optimized release
  and isolated installation pass:
  `.qa/validation-7ec8387d58e242438523e5f45bc48229/results.json` (`-SkipSsh`).

- 2026-09-09, fresh v0.1.41: native v0.1.40 testing exposed lost keyboard
  focus after Escape dismissed the theme popup. The dialog preserved edits,
  but Tab no longer continued to the connection-default checkboxes. The
  focused egui regression confirmed `focused() == None` instead of the theme
  control. Escape now restores focus to that control after closing its popup;
  the regression passes. Native before/after captures and failure evidence:
  `.qa/linux-runtime-1991320105f14c08b0ad0c63c39e0d23/`.
  The Linux fixture now retrieves screenshots and logs even on assertion
  failure, while still removing remote files and stopping owned processes.
  Native v0.1.41 acceptance now passes: zoom 1.05/font 16/keep-alive on/
  scrollback off save successfully; after restart, zoom 1.10/font 17/
  keep-alive off/scrollback on save successfully. A third launch changes all
  four settings, dismisses the theme list, then cancels Preferences: the
  second saved values remain intact. Before/after captures were inspected.
  Evidence: `.qa/linux-runtime-66a3b19f06d8483ab8a7629ed89900fb/results.json`.
  Owned app/display processes stopped and the remote directory was removed.
  Selecting a different theme remains separate from this popup-dismissal test.
  Windows formatting/checking, strict Clippy, all 132 tests, release build and
  isolated installation pass:
  `.qa/validation-9afa1073447545209fbf9737f246b593/results.json` (`-SkipSsh`).

- 2026-09-09, fresh v0.1.40: expanded Linux native Preferences acceptance to
  change terminal text size and keep-alive/scrollback defaults, open and dismiss
  the theme popup, save/reload the values, and cancel the next set of changes.
  The fixture asserts every resulting config value. Python syntax checks pass.
  Linux compilation/linking passes:
  `.qa/linux-cross-31b9148d3e8a4485a177d8cca5e5b521/results.json`.
  The user subsequently approved v0.1.40 and later test builds on the host.
  Native execution then exposed the popup focus issue documented in v0.1.41;
  the expanded native acceptance did not pass on v0.1.40.
  Windows formatting/checking, strict all-target Clippy, all 132 Rust tests,
  optimized release build and isolated installation pass:
  `.qa/validation-2c6e3665d321474595923ad6933d5492/results.json` (`-SkipSsh`).

- 2026-09-09, fresh v0.1.39: fixed Preferences Escape handling when the theme
  dropdown is open. The first Escape now dismisses the dropdown and preserves
  unsaved edits; the next Escape cancels Preferences. Previously the dialog
  consumed Escape before the popup could handle it and cancelled immediately.
  A regression navigates the actual widgets with Tab/Enter, opens the theme
  choices, and verifies both Escape actions and preservation of edited zoom.
  It failed before the fix and passes afterward. This is an egui event-path
  regression, not a claim of completed native testing for all theme controls.
  Formatting, checking, strict all-target Clippy, all 132 Rust tests, optimized
  Windows build and isolated install/reinstall checks pass:
  `.qa/validation-a4cb26c14f9c4e60b4dff008d6d84ed5/results.json` (`-SkipSsh`).

- 2026-09-09, fresh v0.1.38: added opt-in Linux native keyboard acceptance
  (`tests/linux_smoke.py --gui-input`). XTest sends real keyboard events to
  the sole app window in the owned Xvfb display. Using the v0.1.37 Linux app,
  Preferences saves zoom 1.05; after app restart, another increment saves 1.10,
  proving the saved value was loaded. A third launch changes zoom and presses
  Escape; the saved 1.10 remains unchanged. All three owned app processes stop,
  input logs are empty, the display stops and remote files are removed.
  Evidence: `.qa/linux-runtime-96bfb6679a5c4be1bea3c733d0f58da8/results.json`.
  This validates zoom save/reload/cancel through native input, not every theme
  control, Windows restart behavior or other GUI workflows. The application
  behavior is unchanged in v0.1.38; this increment adds the acceptance fixture.
  Python syntax/help checks and Windows formatting/checking, strict Clippy,
  all 131 Rust tests, release build and isolated installer checks pass:
  `.qa/validation-a0f16001287e4069ae4bbb3569af7049/results.json` (`-SkipSsh`).

- 2026-09-09, fresh v0.1.37: added opt-in `--gui` to the Linux runtime fixture.
  It uses host Xvfb with an automatically assigned display and isolated home,
  captures the native renderer, checks app exit and PNG dimensions, retrieves
  evidence, stops the owned display and performs symlink-safe cleanup of the
  exact generated remote directory. Python syntax/help checks pass. The Linux
  app and both test executables compile/link successfully:
  `.qa/linux-cross-166fbfd4fc074ddf97e4ae1cd3cd14e6/results.json` and
  `.qa/linux-cross-2afe7435e606457ca5ad423ea94a1c39/results.json`.
  After explicit user approval, all 97 library and 32 application tests passed
  natively on Linux, with zero failures or ignored tests. Startup/help, key
  generation, OpenSSH public-key agreement, owner-only private permissions and
  existing-key preservation also pass. The actual Linux GUI rendered a clean
  1180x780 capture under Xvfb, inspected visually; it exited successfully with
  empty stderr, the owned display stopped, and the remote directory was removed.
  Evidence: `.qa/linux-runtime-a2eed4f8d8ad46d08b306700f3b7ac00/results.json`
  and `gui.png` in that directory. Interactive Linux input, connected GUI
  sessions, Wayland and desktop file dialogs remain unverified.
  Windows formatting/checking, strict Clippy, all 131 tests, release build and
  isolated installer checks also pass:
  `.qa/validation-144295dfc7354e9bac1d13908fd47b44/results.json` (`-SkipSsh`).

- 2026-09-09, fresh v0.1.36: fixed explicit OSC 8 file destinations accepting
  Unicode direction controls that plain paths already rejected. Validation now
  runs after URI percent decoding, before the decoded destination is exposed
  to the tooltip or Remote files. The same direction-control predicate is
  shared by explicit file links, explicit web links and detected links.
  A regression test failed before the fix and passes afterward for all nine
  embedding/override/isolate controls, both literal and percent-encoded, while
  an ordinary Unicode filename with a space still resolves correctly.
  All 131 Rust tests, formatting, compile checking, strict all-target Clippy,
  optimized Windows build and isolated install/reinstall checks pass.
  Evidence: `.qa/validation-e6d48b17d78040cb846f20ca3ed5b859/results.json`.
  The focused run used `-SkipSsh`; transport code was unchanged.

- 2026-09-09, native v0.1.35 Preferences acceptance: keyboard navigation
  opened Preferences with the zoom slider focused and background controls
  disabled. Arrow Right changed zoom, and Enter on Save preferences closed
  the dialog and persisted zoom 1.05 to an isolated config. The application
  restarted and reopened Preferences, but the loaded numeric value was not
  exposed by accessibility and concurrent desktop input interrupted the
  second save attempt; restart persistence is not claimed. Mouse input
  remained unavailable with `coordinate input geometry is unavailable`.
  Evidence: `.qa/native-preferences-db790971d84840ef900974a6b017e1fa/results.json`.

- 2026-09-09, fresh v0.1.35: extended the live Linux directory/tmux probe
  to exercise unavailable-tmux recovery. In a disposable SSH login shell,
  an empty PATH hides executables without modifying the host installation;
  the exact shared startup command produces visible `tmux: command not found`
  and status 127. Restoring PATH and running Python to report the working
  directory proves the original shell remains usable. Existing literal-path,
  missing-directory, new-tmux and reattach checks also pass. This verifies
  runtime recovery with a controlled missing executable, rather than claiming
  the host has no tmux package installed. Evidence:
  `.qa/live-directory-27568f63c6584939ac9987a4fc2da089/results.toml`.
  A separate OpenSSH connection confirmed the owned directory and tmux session
  were removed. Formatting, checking, strict Clippy, all 130 Rust tests,
  optimized release compilation and isolated installation pass:
  `.qa/validation-a5a403ac32284002b6e6a4ee20de3de8/results.json`.
  This focused run used `-SkipSsh`; the unchanged loopback suite's latest full
  evidence remains the v0.1.34 run above.

- 2026-09-09, fresh v0.1.34: fixed release-validation build ordering.
  The SSH fixture invokes `target/debug/ssh4-fresh.exe` for real key generation,
  but `cargo check` and `cargo test --all-targets` do not produce that application
  executable. The runner now explicitly builds it before any SSH fixtures,
  preventing missing-binary failures on clean builds and stale-binary coverage
  on existing worktrees. All eleven release gates pass, including 130 Rust tests,
  48 OpenSSH configuration comparisons, SSH integration fixtures and isolated
  install/reinstall checks. Both debug and release executables report 0.1.34.
  Evidence: `.qa/validation-74f8120499f3468e9412700a8dbbf44b/results.json`.

- 2026-09-09, fresh v0.1.33: added reproducible real Pageant acceptance with
  an isolated generated RSA-2048 key and strict loopback host verification.
  Automatic discovery, explicit WM_COPYDATA and Pageant's OpenSSH-compatible
  named pipe all pass authentication, terminal input, CPR, resize, environment
  and worker shutdown. The owned agent stopped and its private fixture key was
  removed. Evidence: `.qa/pageant-live-1db4c0e25e074da6943f16490ffadf41/results.json`.
  The standalone Pageant 0.84 executable matches the official SHA-256
  `fa7f58a7dd496f1ae078a67d742a0f4919b39efbe0590a90a1a11e537f1e4d45`
  and has a valid Simon Tatham Authenticode signature. Existing user agents
  are refused by the fixture. Windows OpenSSH service and encrypted-key UI
  acceptance remain separate; no system service was changed.
  All ten automated release gates pass: formatting, compile checking, strict
  Clippy, 130 Rust tests, config-probe build, 48 OpenSSH config comparisons,
  session-probe build, SSH integration fixtures, optimized Windows build and
  isolated installer/reinstaller checks. Evidence:
  `.qa/validation-1b93b65fc666437a9b9f0a1b938067ff/results.json`.

- 2026-09-09, fresh v0.1.32: implemented the Ctrl-click file-path workflow
  listed in the acceptance matrix. Absolute POSIX paths, quoted paths containing
  spaces and explicit OSC 8 file URIs locate entries in Remote files on the
  current connection. Typed destinations keep web navigation separate from
  remote paths. The browser loads the containing directory, selects the exact
  entry and scrolls it into view; missing entries produce a notice. No download,
  local-file opening or shell command is triggered by the gesture. Shared
  terminal-cell hit testing handles Unicode and wrapped paths, and existing
  click/drag/modal guards remain in effect. New tests cover path recognition,
  literal browser targets and gesture routing without browser/remote input;
  the 10,000-entry renderer test verifies the final entry becomes visible.
  All 130 Rust tests, formatting, strict all-target Clippy and locked Windows
  release compilation pass. Native mouse
  interaction and end-to-end browser acceptance remain pending. Windows drive
  paths are not treated as local files; supported remote file paths are POSIX.

- 2026-09-09, fresh v0.1.31 Linux revalidation: rebuilt the executable and
  both test binaries after terminal resize, Preferences and scheduling changes.
  Build evidence: `.qa/linux-cross-bb7630ef19be45208a48bfb7acfc417e/results.json`
  and `.qa/linux-cross-73a623ea09304e939bf6e3cc6f41feb9/results.json`.
  After explicit user approval to upload/run the binaries on the authorized
  host, all 95 library and 30 application tests pass natively, with zero
  failures or ignored tests. Startup/help, key generation, private permissions,
  OpenSSH public-key agreement and existing-key preservation also pass.
  The generated remote directory was removed. Runtime evidence:
  `.qa/linux-runtime-a5c3767c4d2d4f68a213f405af12a221/results.json`.
  This evidence covers v0.1.31, before the v0.1.32 path-link changes.

- 2026-09-09, fresh v0.1.31: added `examples/live_directory_probe.rs` using the
  shared fresh SSH/file engines against the authorized Linux host. A generated
  owner-only temporary directory contains a literal name with spaces, a quote,
  semicolon, dollar/parentheses and Unicode. A new connection restores that exact
  path; Python emits a percent-encoded OSC 7 report which the terminal decodes
  back to it. A missing saved directory leaves the login shell usable at its
  normal starting directory. A new tmux session starts at the saved path, while
  reattaching with a different saved directory preserves the existing pane's
  working directory. All four checks pass; the file engine removes both owned
  directories and the probe removes its tmux session. An independent strict
  OpenSSH check confirms both generated resources are absent.
  Evidence: `.qa/live-directory-19494d2e7c8c4be4a82465e15012c27a/results.toml`.
  Formatting, strict all-target Clippy, all 127 Rust tests and locked release
  compilation pass. A broad version replacement initially touched `pxfm`;
  restored its checksum-verified locked version 0.1.30 before validation.
  Future version edits must target the ssh4-fresh package entry specifically.

- 2026-09-09, fresh v0.1.30: added `--flood` mode to the four-tab native GUI
  fixture. Each server sends continuously with no planned end; capture and normal
  GUI exit occur after every tab receives at least 256 KiB. All four servers
  observe closed channels/transports during sends and terminate before fixture
  release flags are set. The successful child exit establishes no GUI shutdown
  hang during queued output. Servers completed sends of about 3 MiB each while
  terminal counters reached 352,192 / 485,312 / 528,384 / 448,512 bytes, leaving
  output in flight at shutdown. Maximum measured frame CPU time is 28.68 ms and
  p95 is 20.14 ms; each tab advances with at most one stalled measured frame.
  The active continuous-output screenshot was inspected. Evidence:
  `.qa/multitab-load-415987f84b1f4af68d418f00a983591a/results.json`.
  Expected server-side connection-reset messages accompany this cancellation.
  Finite single/alternate/minimum/load scenarios and finite four-tab load pass
  again after the fixture refactor:
  `.qa/connected-render-c707c7325e2447aaade82393ef33307c/results.json` and
  `.qa/multitab-load-b55f135dccc24e69bc04867ec4691c75/results.json`.
  Formatting, cargo check, all 127 Rust tests, Python syntax checks and locked
  release compilation pass. Individual tab closure while the process remains
  open and native interaction under load remain separate acceptance checks.

- 2026-09-09, fresh v0.1.29: added `tests/multitab_load_fixture.py` to exercise
  four restored native GUI tabs with independent strictly verified loopback SSH
  connections. The screenshot-only QA restore flag explicitly enables configured
  reconnect; the normal reconnect-confirmation setting is still respected.
  Each tab receives all 7,864,765 bytes and returns its final cursor reply.
  Over 266 measured connected frames, maximum frame CPU time is 29.00 ms and
  the 95th percentile is 15.75 ms. Once output begins, no unfinished tab goes
  more than one measured frame without advancing in this workload. The final
  screenshot was inspected and shows four tabs and an intact active prompt.
  After GUI exit, all four server threads terminate without the fixture setting
  their release flags, verifying connection closure by the application.
  Evidence: `.qa/multitab-load-0d2d1dae5c99473594acd6bd96ed0050/results.json`.
  Strict all-target Clippy, all 127 Rust tests and locked Windows release
  compilation pass. This establishes finite-output multi-tab progress and
  shutdown after output completion; cancellation during an endless flood and
  native keyboard/mouse interaction under load remain separate checks.

- 2026-09-09, fresh v0.1.28: added sustained-output measurement to the native
  connected-render fixture. It sends 7,864,765 bytes of styled output, waits
  until all bytes reach the terminal, verifies its final cursor reply and
  shutdown, and retains the final screenshot and eframe CPU frame times.
  The existing count-only scheduler measured 48.90 ms maximum and 32.96 ms at
  the 95th percentile: `.qa/connected-render-b30a99be02b041e0ab2b5c79518e9a30/results.json`.
  GUI polling now shares an 8 ms deadline across tabs, rotates the next tab,
  and divides the allowance among connections (at least 2 ms per visited tab,
  still bounded by the shared deadline). Each tab also retains its 32-event
  limit. A single event can overrun the deadline; this is not a hard real-time
  guarantee. The final load run measured 31.10 ms maximum and 16.10 ms at the
  95th percentile over 73 measured frames; the complete final prompt is visible.
  Evidence: `.qa/connected-render-95e7b9181685436e8af647667a8593ff/results.json`.
  Normal, minimum and alternate-screen fixture checks also pass. The fixture
  enforces a local 100 ms maximum/50 ms p95 acceptance threshold, with performance
  dependent on the test machine. All 127 Rust tests pass; after final allowance
  tuning, all 30 application tests pass again. Formatting, strict all-target
  Clippy and locked release compilation pass. Multi-tab fairness and native
  interaction under load remain separate acceptance checks.

- 2026-09-09, fresh v0.1.27: resumed native keyboard acceptance. On v0.1.26,
  Ctrl+K opens the palette and Escape dismisses it, confirming the earlier
  fix in the native app. Opening Preferences with Tab/Enter reproduced focus
  escaping to the background Workspace settings button. Preferences now focuses
  its zoom slider on opening; workspace controls and shortcuts are disabled
  while it is open, and Escape cancels without saving. The new regression
  verifies initial keyboard editing and Escape consumption/cancellation.
  Native v0.1.27 inspection confirms initial slider focus, disabled background
  controls and Ctrl+N leaving the tab count unchanged. Keyboard traversal reaches
  Cancel and Save preferences, although it also visits window/area focus nodes.
  Saving could not be verified: activation failed, and one refreshed retry
  returned `GetCursorPos: Access denied (0x80070005)`. Mouse input separately
  reports unavailable coordinate geometry. No save success is claimed.
  Retained native evidence:
  `.qa/gui-input-c3aab087975e4921b0847c8bb361ed01/keyboard-evidence.json`.
  Formatting, strict all-target Clippy, all 127 Rust tests and locked Windows
  release compilation pass. All four native render captures succeed; the
  minimum-size Preferences capture was inspected after the change:
  `.qa/render-3629aae302f84b89a42f21c170076cad/results.json`.

- 2026-09-09, fresh v0.1.26: extended the live tmux probe to attach two fresh
  SSH sessions to the same generated tmux session. Both terminal engines receive
  the shared command output reporting two attached clients. After disconnecting
  the first transport, the second accepts another command and reports one
  remaining client. The existing three PTY resize checks and bracketed multiline
  paste checks also pass against the authorized Linux host, including the
  shared vt100 resize correction from v0.1.25. Cleanup returns to the login shell;
  an independent strict OpenSSH check confirms the exact generated session is
  absent. Evidence: `.qa/live-tmux-70b17719bcb747adb60c0da3b79b0760/results.toml`.
  Formatting, strict all-target Clippy, all 126 Rust tests and locked Windows
  release compilation pass. This verifies simultaneous engine clients, not
  native GUI input routing or clipboard interaction.

- 2026-09-09, fresh v0.1.25: added `tests/connected_render_fixture.py` with
  an isolated, strictly verified loopback SSH server. Native GUI captures cover
  normal/minimum sizes and the alternate screen, with Unicode, colors, box
  drawing, OSC 8 text and a live prompt; the fixture also verifies a cursor
  position reply, PTY sizing and worker shutdown. Minimum-size inspection
  reproduced lost prompt/output when shrinking the terminal. Shared vt100
  resizing now retains the cursor row, moves displaced top rows into available
  history, and clamps the saved cursor. Two regressions failed before the fix;
  all four resize tests and the vendor doctest pass afterward. All three rebuilt
  captures were inspected, including the preserved prompt at 760x480:
  `.qa/connected-render-70c786c05d1c4a0188ea84bdbcb6fe20/results.json`.
  Before-fix captures: `.qa/connected-render-f93714b137294e83b15a09a271023cdd/`.
  Full Windows validation passes: 126 Rust tests, formatting, strict Clippy,
  48 OpenSSH config comparisons, SSH fixtures, release build and installer smoke.
  Evidence: `.qa/validation-c7827838563a4c6fb5b6ba2e571a41b3/results.json`.
  Native keyboard/mouse, clipboard/drop and dialog acceptance remain separate;
  these screenshots do not establish those interactions or every SGR style.

- 2026-09-09, fresh v0.1.24: resumed native visual QA through the existing
  app-rendered screenshot hook, independent of the failing Windows capture
  service. Inspection at 760x480 reproduced sidebar keyboard hints overlapping
  an imported host button. Sidebar controls now scroll in a bounded area with
  reserved footer space. Added `tests/render_smoke.py` with isolated synthetic
  profiles and no SSH connections. Inspected all four rebuilt captures: normal
  1180x780, minimum 760x480, 20-host minimum-size list, and preferences at minimum
  size. Footer overlap is gone, primary connection actions remain visible, and
  preferences fits. Before-fix captures:
  `.qa/render-bd33a893130641f2846ecc465d9caf51/`.
  Passing rebuilt capture evidence:
  `.qa/render-d645e7a57db847c68412e1637e9076c7/results.json`.
  These captures establish those layouts only; connected terminal rendering,
  native interaction, clipboard/drop and file-dialog acceptance remain pending.
  All 126 Rust tests, formatting, strict all-target Clippy and locked Windows
  release compilation pass for v0.1.24.

- 2026-09-09, fresh v0.1.23: added optional Linux executable and test-binary
  linking to the isolated Zig cross-check script. The full Linux app links:
  `.qa/linux-cross-2e8744a6b3564628a7ef545e1c4e052e/results.json`.
  On the authorized Linux host it passes version/help startup, Ed25519 key
  generation, owner-only private permissions, public-key agreement with
  OpenSSH and preservation of an existing keypair. Its unique remote directory
  was removed. Initial smoke evidence:
  `.qa/linux-runtime-7042cdaaa29b45e4bbde6516abdc3a97/results.json`.
  Added optional native Linux test execution with retained stdout/stderr.
  Library/application test linking passes:
  `.qa/linux-cross-d141db791d0c4b04a1e45522f73fab75/results.json`.
  Native Linux execution passes all 95 library and 29 application tests,
  with zero failures/ignored tests; startup/key generation passes again and
  remote cleanup succeeds. Combined evidence:
  `.qa/linux-runtime-cebfb933dad64cfea589cd6fc0e0bad6/results.json`.
  Windows formatting, cargo check, strict Clippy and locked release build pass.
  Linux GUI rendering/interactive SSH and macOS remain unverified.

- 2026-09-09, fresh v0.1.22: removed the missing Linux C compiler blocker
  without changing machine PATH or the Windows compiler. Downloaded Zig 0.16.0
  from its official release index, verified the published SHA-256, and retained
  it only under `.qa/toolchains`. Added `tests/check_linux.ps1`, which uses
  target-scoped CC/AR wrappers, workspace caches, restores prior process
  environment variables, and retains command output/results. The Zig target
  override must follow cc-rs arguments so it replaces the incompatible Rust
  target spelling. Full `cargo check --locked --all-targets --target
  x86_64-unknown-linux-gnu` now passes, including the GUI, CLI, library, tests
  and examples. Evidence:
  `.qa/linux-cross-38fe1e5af0c34291b64773ed32572935/results.json`.
  Windows cargo check and locked release compilation pass for v0.1.22.
  This is type/build-script verification; no Linux executable was linked or
  run, and native GUI acceptance remains incomplete.

- 2026-09-09, fresh v0.1.21: audited outstanding status against the actual
  product requirements and validation matrix. Required SSH-004 fields are
  HostName/User/Port/IdentityFile/ProxyJump with explicit-profile precedence;
  broader Match/Include behavior is not an additional completion requirement.
  Required SSH-005 file operations now have real Linux evidence; unspecified
  extra attributes are not a parity blocker. Updated stale tmux and forwarding
  rows to reflect verified engine behavior while retaining native UI gaps.
  Full Linux `cargo check --locked --target x86_64-unknown-linux-gnu` fetched
  locked platform dependencies and reached `ring`'s C build, which fails because
  `x86_64-linux-gnu-gcc` is absent. No local Clang/LLVM/Zig command was found;
  the authorized host exposes cc but no cargo/rustc/pkg-config on its command
  path. Retained diagnostic: `.qa/linux-check.log`. This is an environment
  dependency, not evidence of a successful Linux build or a source defect.
  Windows formatting/check and locked release build pass for v0.1.21; this
  increment changes audit documentation and version metadata only.

- 2026-09-09, fresh v0.1.20: added opt-in real tmux engine acceptance using
  the provided Linux host, existing key and strict known-host verification.
  A random test session received 100x32, 120x40 and 90x28 PTY sizes, verified
  by `stty size` inside tmux and parsed by the fresh terminal engine while the
  alternate screen remained active. Bracketed multiline paste preserved
  Unicode and literal shell metacharacters, did not execute before Enter, and
  produced both expected outputs after Enter. The probe killed its own tmux
  session and observed return to the ordinary login screen. Evidence:
  `.qa/live-tmux-7750511d87984dff9e4e174c95485f27/results.toml`.
  This establishes live engine resize/paste behavior; it does not substitute
  for the still-pending native window-resize or clipboard interaction checks.
  Formatting, strict all-target Clippy, all 126 Rust tests and locked release
  compilation pass. A separate strict OpenSSH check confirmed that no tmux
  server/session remained for the test account after cleanup.

- 2026-09-09, fresh v0.1.19: added an opt-in real-host file acceptance probe
  using the fresh session/file engine with existing key authentication and
  strict known-host verification. On the user-authorized Linux host, both SFTP
  and SCP passed empty-file and 256 KiB binary round trips, exact byte comparison,
  literal UTF-8 filenames containing spaces, quote, dollar and semicolon,
  upload/download refusal to replace existing targets, listing/name/size checks,
  chmod verified by listing, rename, create-file, rename collision rejection,
  file removal and directory removal. Listings were empty after operations;
  local download staging files were absent and the unique remote root was
  removed. Evidence:
  `.qa/live-files-4e2f7517d14a4a119f79ec3e3a93f678/results.toml`.
  This proves real-host engine behavior, not GUI file-dialog interaction or
  clipboard/drop acceptance. No application behavior changes were required.
  All 126 Rust tests, formatting, strict all-target Clippy and locked release
  compilation pass. A separate strict OpenSSH read-only check confirmed that
  the exact temporary remote root no longer exists after probe completion.

- 2026-09-09, fresh v0.1.18: native accessibility-only GUI observation works
  despite screenshot capture failure. Ctrl+K opened the command palette, but
  Escape left it open on v0.1.17; recorded tree:
  `.qa/gui-live-bb4d7473346746b9b12e9b3868845d4a/palette-before-fix.json`.
  Escape now dismisses the palette and is consumed before terminal routing.
  A focused egui regression verifies that the event is consumed when open and
  remains available when closed. Native retesting confirmed palette opening,
  but Escape injection was blocked by activation failure and, after refreshed
  selection/retry, `GetCursorPos failed: Access is denied (0x80070005)`.
  Mouse input also reports unavailable coordinate geometry. Native verification
  of the fix and further GUI workflows remain pending; no visual QA is claimed.
  All 126 all-target Rust tests, formatting, strict Clippy and locked release
  compilation pass. The first final-build attempt encountered the QA GUI's
  executable lock; closing that owned process allowed the rebuild to succeed.

- 2026-09-09, fresh v0.1.17: added native Windows password and
  keyboard-interactive acceptance with synthetic loopback credentials. Six
  scenarios cover successful Unicode/Backspace password entry, wrong-password
  rejection, Escape and Ctrl+C cancellation, two-prompt echoed/secret
  keyboard-interactive success, and cancellation during its secret prompt.
  All cases check exact authentication attempt counts, console-mode restoration,
  server teardown and raw-output logs containing no entered credentials.
  Cancellation before connection opens requires no authentication request; the
  fixture listener now shuts down correctly in that case. No application
  behavior changes were needed for these cases; the version bump records the
  added acceptance coverage.
  The final v0.1.17 release passes all six cases:
  `.qa/cli-auth-8b64f7463d6d4996a87a43d8c0fed724/results.json`.
  Formatting, offline cargo check, all 125 all-target Rust tests, locked release
  build and isolated install/reinstall/locked-target/staging-cleanup checks pass.
  Installer artifact: `.qa/install [literal] 0431d7c294c6491c85c251cd8bed9cb2/ssh4-fresh.exe`.

- 2026-09-09, fresh v0.1.16: fixed CLI terminal-query replies from buffered
  final output replacing a successful remote exit with a disconnected-send
  error. Protocol replies after worker completion/cancellation are discarded;
  active-session queue errors still propagate and user input still fails after
  disconnect. A deterministic unit regression verifies both shutdown ordering
  and active input backpressure. The native fixture now includes final output
  with 512 CPR queries followed immediately by a successful shell close while
  the transport remains open. This reproduced exit code 1 on v0.1.15:
  `.qa/cli-console-e804c6afd43c48d8b45cddc74c2b0e22/results.json`.
  The rebuilt v0.1.16 passes all five native cases, including preservation of
  every final raw-output byte and successful exit for the query case:
  `.qa/cli-console-1fb4e28826d8486cb42b444455e26dd6/results.json`.
  All 125 tests and ten release gates pass without skips:
  `.qa/validation-e6c7edc522e946079598ce2c7b3ff369/results.json`.
  Native GUI automation initialization and window discovery succeeded for an
  isolated QA launch, but capture did not: initial app approval timed out;
  refreshed selection and one retry failed with `SetIsBorderRequired failed:
  No such interface supported (0x80004002)`. No GUI interaction acceptance is
  claimed from this attempt. QA configuration is retained in
  `.qa/gui-live-bb4d7473346746b9b12e9b3868845d4a/config.toml`.

- 2026-09-09, fresh v0.1.15: added a native Windows CLI fixture that runs the
  actual release executable in a dedicated console/ConPTY against a loopback
  SSH server with generated keys and isolated config/known_hosts. Target and
  profile launches pass. Exact remote bytes cover spaces, Unicode, Tab/Enter,
  Ctrl+C/D, application-cursor Up, Alt+X and SGR mouse press/release. A real
  console resize propagates 100 columns by 32 rows. CPR replies and raw-log
  append/output-only behavior pass. Local Ctrl+Shift+Q exit and server close both
  restore the original input/output console modes and stop the SSH worker.
  Debugging corrected the fixture: crossterm obtains the actual viewport size,
  so injecting a size record without resizing the viewport is insufficient.
  This requires a dedicated terminal and is separate from the redirected
  validation runner. Native clipboard/paste/password, GUI and tmux acceptance
  remain pending. A stricter server-close fixture exposed a real defect: the
  worker kept waiting after shell-channel CLOSE while the SSH transport stayed
  open. The worker now emits a typed shell-exit event after draining output and
  terminates promptly. CLI treats reported status 0 as success; nonzero/missing
  status reports an error, and the GUI records the status in its timeline.
  Four native cases pass for local exit, remote zero/nonzero/missing status,
  each with full input/log/resize/teardown verification. Native evidence:
  `.qa/cli-console-f9a3285b94f7432a9816896050dff83d/results.json`.
  A deterministic regression also reproduced completion overtaking the last
  worker event. Receivers now wait for the event channel to drain and disconnect
  before reading completion. The test failed before the fix and passes after it.
  All 124 tests and all ten release gates pass, including 48 OpenSSH comparisons,
  the SSH fixtures, release compilation and installer smoke:
  `.qa/validation-8d470114518a4674a8d84f400af3c263/results.json`.
  Live acceptance on the user-authorized Linux host passed with the existing
  key and strict known-host verification: Vim alternate-screen launch/keyboard
  exit, saved-profile tmux attachment, a running process surviving disconnect,
  reattachment and Ctrl+C interruption, and --no-tmux overriding the profile.
  All three CLI launches restored console modes; the unique test session was
  removed. Evidence: `.qa/live-cli-29b51e670bfd43c3b2b29d1a3c7bef64/results.json`.
  Live tmux resize remains unverified: Win32 viewport setters returned success
  without changing the alternate-screen dimensions in this ConPTY, and sizes
  were not retained across screen switches. The fixture now checks actual
  viewport dimensions. This limitation does not invalidate the passing loopback
  CLI resize check; it does not establish live tmux resize acceptance either.

- 2026-09-09, fresh v0.1.14: implemented OpenSSH Match final and !final,
  including final all, combined criteria, empty final blocks and conditions
  inherited through Include files. The second pass matches Host against the
  resolved hostname while originalhost retains the original alias. Initial
  values retain precedence, and final-only HostName cannot replace the already
  selected destination. Repeated forwarding entries are not replayed; identical
  forwarding declarations in separate matched blocks are suppressed.
  Three new unit regressions pass. Expanded the independent OpenSSH -G fixture
  to 48 passing cases, including forward-list comparison and explicit users.
  Host-dependent Include expansion, canonicalization and other unsupported Match
  conditions remain pending. Reference behavior was checked against OpenSSH's
  readconf.c and ssh.c implementations as well as the installed Windows client.
  Full release validation passes without skips: formatting/check/strict Clippy,
  123 tests, 48 OpenSSH comparisons, complete SSH fixture suite, release build
  and isolated installer checks. All ten gates have exit code zero in
  `.qa/validation-b7c02947e3904197a24e396ac3241d64/results.json`.

- 2026-09-09, fresh v0.1.13: added 19 loopback shell-fallback scenarios for
  directory listings and file actions: cancellation, dropped tasks, missing
  markers/status, nonzero exit, trailing output, fragmented responses, oversized
  output, malformed metadata, unexpected action output and a real 30-second
  action timeout. Each requests a successful listing afterward on the same
  connection. Stalled cases also require terminal input/output while the file
  request is pending and confirmation that the remote exec channel closed.
  Cancellation and dropped-task recovery must finish within two seconds.
  The app now distinguishes cancellation from timeout in its error message,
  retaining the notice that a submitted remote change may already have completed.
  All ten release gates pass without skips: formatting/check/strict Clippy,
  120 tests, 18 OpenSSH comparisons, the full SSH suite including all 19 new
  scenarios, release build and isolated installer checks. Verified evidence:
  `.qa/validation-0fbdf3416cb547698cff3c569d842d45/results.json` and adjacent logs.

- 2026-09-09, fresh v0.1.12: implemented common OpenSSH Match conditions with
  ordered context, per-block match results, combined/negated pattern lists,
  case-sensitive usernames, effective CLI/profile user overrides and inherited
  Include conditions. Repeated forwarding directives use the same resolved
  conditions as scalar settings. Unsupported conditions now emit bounded import
  warnings instead of silently skipping their blocks. Four regressions cover
  context changes inside a block, exclusions/case, conditional includes, warning
  propagation, malformed criteria and CLI/profile/target user precedence.
  Added an independent OpenSSH `-G` comparison fixture: all 18 generated config
  cases pass without network connections. Fixed generated Include-file ACLs to
  meet Windows OpenSSH's ownership checks. The comparison is a release-runner
  gate alongside the existing SSH integration suite. Host-token Includes and
  remaining Match criteria are still pending; no command execution is introduced.
  Full release validation passes with no skips: formatting/check/strict Clippy,
  120 tests, 18 OpenSSH comparisons, complete SSH fixture suite, version 0.1.12
  release build and isolated installer checks. All ten gates have exit code zero
  in `.qa/validation-3be6284d3fdb4e4f96397b3c8134b3d7/results.json`.

- 2026-09-09, fresh v0.1.11: the remote rename dialog now supports moving entries
  to relative or absolute destination paths and reviews the exact source and
  destination. Creation retains its single-name restriction. Move validation
  rejects empty/NUL/oversized paths, trailing slashes, final dot/dot-dot and
  identical source/destination paths. Literal whitespace and intermediate dot-dot
  components are preserved so remote symlink traversal semantics are unchanged.
  Added dialog-action regression coverage; the existing SSH action fixture covers
  moving a file into another folder, moving that folder and refusing replacement
  of an existing target. Native dialog interaction remains pending.
  Full validation passes with no skipped checks: formatting, compile check,
  strict Clippy, 116 tests, rebuilt SSH probe and complete fixture suite, release
  build, and install/reinstall/locked-target preservation/cleanup. Evidence:
  `.qa/validation-cf61e4f0db224fb797c9827c848890eb/results.json` and adjacent logs.

- 2026-09-09, fresh v0.1.10: strict all-target Clippy now passes with warnings
  denied. Applied behavior-preserving iterator/Option/error-inspection cleanups,
  byte-slice handling and test-module ordering. Added `tests/validate.ps1` to run
  the automated release gates with per-command logs and a JSON outcome that marks
  failures and skipped SSH/installer checks. Debugging the runner fixed Windows
  PowerShell native-stderr handling and multiple executable matches on PATH.
  The native/manual requirements in the full scope remain separate from this
  automated validation command.
  The complete runner passes with no skipped checks: format/check/strict Clippy,
  all 115 tests, rebuilt probe and full SSH fixture suite, release build, and
  isolated install/reinstall/failure-cleanup checks. Evidence is stored in
  `.qa/validation-44097a158eed4060a99e4c3b10cec2c2/results.json` and adjacent logs.

- 2026-09-09, fresh v0.1.9: completed local terminal context-menu actions and
  corrected copy behavior. Copy consumes the frozen selection and clears its
  highlight; Ctrl+C without selection interrupts, while Ctrl+Shift+C never does.
  Right-click with a selection exposes Copy/Paste/Search online; without one it
  reads clipboard text and uses the shared multiline-review policy. Remote mouse
  tracking retains its events unless Shift requests local behavior. Regressions
  verify output-stable copy, copy-only shortcuts, CR/LF/CRLF and forced bracketed
  paste, plus opening/clicking the context menu through headless egui events.
  Native clipboard/browser interaction still needs acceptance testing.
  All 115 fresh tests, formatting, compile check and release build pass.

- 2026-09-09, fresh v0.1.8: added `install.ps1` with locked build, version/hash
  checks, stale-build rejection, staged atomic replacement, temporary-file cleanup,
  configurable install directory and optional user PATH update. The default fresh
  install is separate from the root reference app. `tests/install_smoke.ps1` passes
  initial install, reinstall, locked-executable preservation, cleanup and unchanged
  user PATH in a directory with spaces/brackets. Fixed Windows PowerShell converting
  a null replacement backup path into an invalid empty string (`NullString.Value`).
  Windows GUI launch detaches only an exclusively owned console; errors after
  detachment use a native dialog. This follows Microsoft's GetConsoleProcessList
  and FreeConsole APIs. All 112 tests, formatting, check and release build pass.
  The isolated installed executable reports 0.1.8, passes help/version and opens
  the restored disconnected workspace (`.qa/installed-v018.png`, visually checked).
  Default user-PATH installation, Explorer console-window acceptance, interactive
  CLI/native workflows and non-Windows packaging remain unverified.

- 2026-09-09, fresh v0.1.7: added opt-in synchronized terminal input with an
  always-visible status toggle and destination banner. Semantic key/text/paste
  events are encoded separately for each connected terminal; protocol replies,
  mouse tracking, auth responses, snippets and file commands do not broadcast.
  Authentication-waiting tabs are excluded. Multiline paste reviews freeze the
  recipient indices and connection generations; reconnects/new connections cannot
  receive a previously reviewed paste. Tab closes invalidate pending reviews.
  All 24 GUI tests pass, including per-peer cursor/bracketed-paste modes,
  protocol-reply isolation and paste destination stability. Formatting passes.
  Live multi-session/native sync acceptance remains required.
  Full suite: 112 tests passed. Compile check and release build pass; release
  executable reports `ssh4-fresh 0.1.7`.

- 2026-09-09, fresh v0.1.6: added open-tab search, close-others/close-right
  context actions and persisted tab colors. Fixed closing an earlier tab changing
  the active connection: retained tabs now preserve identity across index shifts.
  Closing the active tab selects a neighbor; closing the last tab creates a blank
  form. Closing tabs clears pending paste review so it cannot target a shifted
  index. Tests cover all close modes, active identity in saved snapshots, search
  fields, color round-trip and unknown-color rendering. All 110 tests pass;
  formatting, compile check and release build pass. Native app-owned screenshot
  `.qa/tabs-v016.png` verifies restored colors, selected tab and search layout
  at 1000x760 with an isolated, disconnected config. Context-menu/native search
  interaction acceptance remains unverified; this capture is layout evidence.

- 2026-09-08, fresh v0.1.5: added bounded shell listing and file-action fallback
  on the same authenticated session after explicit SFTP subsystem refusal.
  Directory records are NUL-separated with validated numeric metadata, UTF-8
  paths, a 10000-entry limit and an 8 MiB response cap. GNU/BSD stat formats are
  supported; GNU mv is required for rename. Commands quote paths, use private
  creation permissions, avoid replacing existing create/rename targets, and only
  delete empty directories. Completion requires the unique marker and an explicit
  zero exit status; cancellation/deadline closes the exec channel and reports
  that a submitted change may already have completed. All 107 tests pass,
  including real Git Bash actions, literal shell metacharacters, no-clobber
  behavior, listing metadata and malformed-record rejection. BSD-host runtime,
  native UI acceptance and stalled shell-action fixtures remain unverified.
  Full SSH fixture run passes, including SFTP refusal followed by shell directory
  metadata, a confirmed create-folder request and terminal input/output afterward.
  Formatting, compile checks and release build pass; version reports 0.1.5.

- 2026-09-08, fresh v0.1.4: uploads/downloads automatically fall back to SCP
  after explicit SSH subsystem refusal, before any SFTP file operation starts.
  The classifier examines the typed local transport error; server-supplied SFTP
  messages, timeouts, malformed protocol and file-operation failures cannot
  authorize replay. Cancelled requests do not start fallback. The failed SFTP
  channel is dropped before opening SCP on the same authenticated session.
  Task progress exposes fallback to the GUI, and transfer review explains it.
  All 105 fresh tests pass, including a classifier regression. Directory listing
  and other file-action shell fallback remain required.
  The rebuilt SSH integration fixture passes all scenarios, including automatic
  upload/download fallback after real subsystem refusal and subsequent terminal
  input/output. Download tasks assert fallback is false for ordinary SFTP errors
  and explicit SCP requests. Formatting, check and release build pass.

- 2026-09-08, fresh v0.1.3: implemented incremental single-file SCP downloads
  and GUI selection/manual-path review, including no-SFTP manual downloads.
  The receiver bounds headers/input, rejects directory/path-traversal records,
  never uses server filenames as local paths, stages local output, checks the
  advertised length/status/extra data, and requires an explicit zero exit status
  before no-replace publication. Cancellation and failures remove local staging.
  `cargo check --locked` and 104 tests pass, including a real OpenSSH SCP sender
  under Git Bash with binary contents and literal shell metacharacters. All SSH
  fixtures pass, including nine added scenarios for success, empty, truncation,
  malicious header, read error, extra file, nonzero/missing status and cancellation.
  Native download picker/review acceptance and automatic shell/SCP fallback
  remain required. Protocol behavior was checked against
  [OpenSSH's SCP implementation](https://github.com/openssh/openssh-portable/blob/master/scp.c).
  Receive buffering now drains pending payload before reading another packet;
  the successful SCP fixture verifies 4 MiB byte-for-byte with progress totals.
  Formatting and the release build pass; the binary reports `ssh4-fresh 0.1.3`.

- 2026-09-08, fresh v0.1.2: added the missing CLI Ctrl+P clipboard PNG
  workflow using the existing asynchronous capture and session-owned SCP upload.
  Destination/transfer review, cancellation, temporary-file ownership, completed
  path copying and optional quoted insertion are implemented. A regression checks
  shell metacharacter quoting and insertion without Enter. Native clipboard/CLI
  interaction acceptance remains required. The pre-change 100-test baseline
  passes outside the sandbox; restricted Windows IPC/ACL and Git Bash tests fail
  inside the sandbox, so those failures must not be interpreted as app defects.
  Post-change verification: `cargo fmt --check`, `cargo check --locked`,
  `cargo test --locked --all-targets` (101 passed), the rebuilt session probe
  and `python fresh/tests/ssh_fixture.py` (all scenarios passed, exit 0), and
  `cargo build --release --locked` pass. The release executable reports
  `ssh4-fresh 0.1.2`. This verifies the reused SCP worker and clipboard PNG
  encoding/cleanup, but does not substitute for native Ctrl+P interaction QA.

- SCP stall fixtures now withhold the header acknowledgement and notify the
  terminal. Explicit task cancellation and dropping the transfer task both close
  the exec channel in under one second without sending file data prematurely;
  terminal input/output continues afterward. All 71 SSH scenarios pass. Queued
  local files and clipboard PNG reviews can now use the typed remote folder
  without an SFTP listing when SCP is selected. Empty remote folders are rejected
  rather than accidentally constructing a root-level destination. Native
  clipboard/drop review acceptance and cancellation during bulk SCP data remain
  pending; automatic fallback and SCP downloads remain in the full scope.

- SCP uploads now run incrementally in the session-owned file worker, using
  nonblocking exec-channel writes that respect SSH window credit. Upload reviews
  offer an explicit SCP option, including existing clipboard PNG reviews, and
  a separate manual SCP upload entry point does not require a directory listing.
  The remote script requires a POSIX shell, scp, mktemp and `ln -T`: it receives
  into a private random temporary directory, uses a fixed ASCII protocol filename,
  and atomically links the completed file into its final path without replacement.
  Shell quoting preserves literal remote names; directories and existing files
  cannot become accidental overwrite targets. Traps clean normal failures/success;
  cancellation/timeouts warn that remote staging or completion may remain.
  Success requires the final SCP acknowledgement, EOF exchange, unique completion
  marker and explicit zero SSH exit status. Progress and a 30-second idle deadline
  run alongside terminal IO. A real local SCP receiver under Git Bash verifies
  binary bytes, staging cleanup and refusal of existing files/directories. Loopback
  SSH fixtures verify 256 KiB/empty uploads, server denial, publication failure
  and terminal IO after each case. All 100 fresh tests, 31 vendor library tests,
  169 root regressions, app/probe builds and 69 SSH scenarios pass. Automatic
  fallback, SCP downloads, stalled/cancelled SCP live fixtures and native acceptance
  remain pending.

- Preferences now exposes saved interface zoom (75–200%), terminal text size,
  Midnight/Forest/Amber terminal themes, global keep-alive and scrollback choices.
  Save updates runtime display settings only after the atomic config save succeeds;
  Cancel discards the draft. Keep-alive and scrollback changes apply to new
  connections. Unknown settings and unsupported saved theme names survive edits;
  unknown themes render with Midnight colors until the user chooses a supported
  theme. Integer legacy font sizes and finite/clamped numeric loading are tested.
  Terminal default foreground/background/cursor rendering and OSC 10/11/12 query
  replies now share the selected colors, including after reconnect. This fixes
  the previously hardcoded query colors that differed from the GUI.
  All 97 fresh tests, app/probe builds and 65 SSH loopback scenarios pass.
  Native app-owned captures at 1000×760 and 760×480 with 200% zoom verify the
  preferences dialog; the settings body scrolls and Save/Cancel stay visible
  (`.qa/preferences-14fe81abeef34766bd2f3c2b17c8c6de/`). Native editing/save/relaunch
  interaction and CLI visual-color acceptance remain pending.

- SSH Include paths now expand `${VARIABLE}` and literal `%%` in one pass before
  tilde/path handling and ordered glob expansion. Inserted values are not expanded
  recursively, so percent signs/dollar expressions in a directory name remain
  literal. Missing/non-Unicode variables skip that path with a bounded diagnostic,
  shown in CLI output and the GUI connection timeline. Malformed expressions,
  unsupported host-dependent tokens and expanded paths over 32 KiB are rejected.
  This follows [OpenSSH Include expansion and missing-variable handling](https://github.com/openssh/openssh-portable/blob/master/readconf.c).
  Filesystem tests cover variable paths with spaces, ordered globs, nested Include,
  scope isolation, first-value precedence and cycles through expanded paths;
  parser tests cover literal/nonrecursive expansion, missing/malformed variables
  and limits. All 96 fresh tests and app/probe builds pass. Host-dependent Include
  tokens, other-user tilde expansion and Match remain pending.

- Unix SSH-agent connection setup now owns a nonblocking socket from creation.
  A full AF_UNIX backlog retries without a blocked worker; pending connects poll
  in at most 25 ms slices and share the RPC deadline/cancellation flag. Early
  errors, deadline and cancellation drop the owned descriptor. This follows the
  [socket2 nonblocking connection API](https://docs.rs/socket2/0.4.10/socket2/struct.Socket.html#method.connect).
  The isolated `tests/unix-agent` harness compiles the real agent module and its
  tests for `x86_64-unknown-linux-gnu` without GUI build dependencies. Live Linux
  tests cover normal setup/nonblocking reads, missing endpoints, a full listener
  backlog deadline and cancellation; they are cross-compiled but have NOT run on
  this Windows host. Windows fresh tests remain 94 passing. Installed-agent and
  Linux/macOS runtime acceptance remain pending.

- Public-key installation is available from the connected tab's Commands menu.
  The review shows the resolved account and selected public-key fingerprint.
  Parsing rejects private keys, multiple lines and authorization options; comments
  are removed and the command accepts only canonical shell-safe key text. A separate
  exec channel runs incrementally alongside the terminal and file/forward workers.
  Installation uses a POSIX shell, mkdir/chmod, awk and mktemp: existing entries are
  preserved, plain key duplicates are avoided, a per-account lock prevents concurrent
  app installs, symlink targets are rejected, and a staged 0600 file replaces
  authorized_keys after successful preparation. The .ssh folder becomes 0700.
  Cancel/timeout closes the channel and reports that remote completion may already
  have occurred. Success requires a fresh completion marker and an explicit zero exit.
  Tests execute the actual script under Git Bash in an isolated temporary home,
  verifying preservation of a final line without newline, repeat installation and
  lock refusal. SSH fixtures cover success, exec refusal, missing completion
  marker, nonzero exit and missing exit status, followed by terminal input/output.
  This exposed and fixed discarded exit statuses when another SSH channel owned
  the reader: statuses now remain with their registered channel until it polls,
  and callers can distinguish an explicit zero exit from a missing status.
  Fresh tests: 94; vendor library tests: 31; root regressions: 169. App/probe
  builds and all 65 SSH scenarios pass (`.qa/key-install-validation.txt`). Native
  picker/review and production-host access with the installed key remain pending.

- Live one-hop rekey fixtures verify terminal input/output after the upstream
  SSH server completes key renewal, and reject a replacement upstream host key
  without changing known_hosts. The negative probe waits for the rekey failure
  after connection establishment instead of accepting the initial shell response.
  Tunnel writes now propagate errors encountered while reading upstream packets;
  they previously discarded those errors before sending downstream data. A vendor
  regression verifies the failure reaches the caller without writing payload.
  Fresh tests: 92; vendor library tests: 30; root regression tests: 169. App and
  probe builds and all 60 SSH loopback scenarios pass (captured in
  `.qa/rekey-validation.txt`). Multi-hop rekey and production-host acceptance
  remain pending.

- Jump routing now authenticates each SSH hop through the preceding connection's
  direct-tcpip channel. Type-erased transports reuse the same authentication,
  host trust, cancellation and terminal/file/forwarding code. Only the first hop
  opens local TCP; later hostnames are resolved by the preceding server. Profiles,
  GUI connection options, OpenSSH ProxyJump and CLI `-J/--jump` share a resolver
  that flattens nested routes, limits chains to eight hops and rejects repeated
  host/port pairs, empty entries and excessive depth. `none` explicitly disables
  imported jumps. Hop identities/auth policies are resolved independently and
  destination passwords are not copied into hop configurations. Timeline and
  failure messages identify the route step and peer. Unit tests cover resolution,
  distinct users/keys, nested routes, cycles and overrides (92 fresh tests pass).
  New loopback-only jump servers require separate credentials and allow only named
  virtual destinations. One/two-hop terminal sessions, routed SFTP/SOCKS, changed
  upstream keys, denied forwarding and cancelled hop prompts pass; no direct path
  can resolve the fixture destination. All 58 SSH loopback scenarios and app/probe
  builds pass. Native interaction,
  production-host interoperability and multi-hop rekey acceptance remain.

- Jump-routing preparation exposed a library prerequisite: host-key verification
  and signature proof were looked up from thread-local state during every KEX.
  Each SSH client now captures its peer label, shared immutable verifier and an
  independent atomic proof flag at construction. Upstream rekeys cannot use the
  destination's verifier or authorize destination trust-file persistence, and
  clearing/replacing thread hooks cannot disable an existing connection's policy.
  Public thread-local installation/proof APIs remain compatible for callers.
  A focused regression simulates two nested peers, wrong keys, independent proof,
  hook cleanup and two clients created under the same installed policy. Vendor
  unit tests: 29; fresh tests: 91; root regressions: 169; all 51 SSH loopback
  scenarios and app/probe builds pass. Actual jump route
  construction and multi-hop live acceptance remain pending.

- Terminal IME integration now requests a native candidate window at the cursor,
  handles enabled/preedit/commit/disabled events, and paints a local underlined
  preedit overlay without changing remote terminal contents. Preedit storage is
  bounded to 4096 bytes and zeroized when replaced or cleared. Composition keys
  are suppressed during IME frames, including the Enter used to confirm a commit;
  multiline commits use the existing paste review. Focus loss, modals, search and
  inactive tabs clear composition and reject orphaned late commits. Headless egui
  coverage verifies candidate positioning output, local Japanese preedit rendering,
  exactly-once committed bytes, Enter suppression and modal cancellation. All 91
  fresh tests pass; installed Windows IME and other-platform acceptance remain.

- Remote directory rendering now builds only visible rows instead of all 10,000
  possible entries each frame. Fixed-height rows keep name, size and permission
  columns aligned; long names and large sizes truncate with full hover text.
  Selection retains the original entry index, and directory changes get separate
  scroll state. File-action buttons wrap to fit the window. A headless 10,000-entry
  regression, including a long Unicode name and maximum u64 size, verifies fewer
  than 20 rows render in a 230-pixel viewport, table width stays bounded, and an
  offscreen selection is preserved. All 90 fresh tests pass. Native file-browser
  interaction acceptance remains pending.

- Diagnostics now exposes an opt-in debug-log switch, defaulting off on launch.
  A typed metadata API accepts only tab index, connection/prompt/error flags,
  traffic counters and terminal dimensions; it has no terminal text, credentials
  or configuration input. Changed snapshots enter a bounded 256-record queue;
  overload is counted in subsequent output. One background writer appends beside
  the selected config (`.debug.log`) and resets before exceeding 1 MiB. Disabling
  drains records and syncs the file, with opening/active/closing/error UI states.
  Rotation testing caught Windows append-only handles denying truncation; the
  writer now opens for ordinary writes and seeks to the end. All 89 fresh tests
  and the app build pass. A native accessibility read confirms the Diagnostics
  entry is exposed. Automated clicking failed with `coordinate input geometry
  is unavailable`; keyboard focus remained at the window, so toggle interaction
  acceptance is not claimed. The isolated QA process was closed.

- Native connection-form captures exposed that Connect/Save scrolled below the
  entire advanced form. These actions now stay in a fixed footer while fields
  scroll above them. Disconnected tabs omit the unavailable Remote files button,
  and short windows use less top padding. The app's own native screenshot hook
  now supports bounded QA window sizes and initially expanded options, only when
  capture mode is enabled. Captures at 760x480 and 1180x1200 verify visible action
  buttons, aligned fields and wrapped directory guidance with expanded options.
  Evidence: `.qa/layout-e7aa1a41787a44208856c11867c25f93/minimum-final.png` and
  `options-final.png` in the same folder. The updated native app builds successfully.
  These are rendered-layout checks; keyboard, scrolling and native dialog
  acceptance remain separate checks.

- Shell input now uses an ordered queue bounded by 8 MiB of queued bytes rather
  than 128 messages; each submission remains limited to 1 MiB. Small keystrokes
  coalesce to limit allocation overhead, and an over-budget submission is rejected
  atomically with an explicit not-sent error. Consumed and abandoned queue buffers
  are zeroized. The session worker retains one in-flight submission and sends at
  most 16 available-window chunks per iteration. Zero window credit returns to
  output/resize/forward/file polling instead of waiting inside a whole-paste write.
  A new shell try-write API reuses the existing channel flow-control implementation.
  Unit coverage checks 1000 small submissions, byte ordering, the capacity limit,
  rejection and shutdown cleanup. A loopback scenario queues 4 MiB while the peer
  pauses reads for two seconds: terminal output arrives within 1.5 seconds and the
  peer verifies every byte, in order, before acknowledging normal terminal input.
  All 88 fresh tests, 51 SSH loopback scenarios, 169 root regressions and app/probe
  builds pass. Underlying
  socket write backpressure and native overload interaction remain pending.

- Connection setup no longer blocks the session worker on OS DNS/TCP calls or
  uses only the first resolved address. At most eight setup workers resolve up
  to 16 addresses and share the configured setup deadline across attempts. The
  caller checks cancellation every 20 ms; late results are dropped and their
  sockets closed. In-flight OS calls cannot be interrupted, so cancelled workers
  keep their capacity slot until those calls return. Connected sockets remain
  nonblocking with TCP_NODELAY, and a transport wrapper checks cancellation on
  each read/write/flush, including SSH negotiation. Timeline phases distinguish
  lookup/TCP setup from SSH negotiation. Tests cover blocked lookup cancellation,
  lookup timeout, address fallback, empty results and transport cancellation.
  A loopback peer that never sends an SSH banner verifies handshake termination
  within one second and EOF at the peer. All 87 fresh tests and app/probe builds
  pass, as do all 50 SSH loopback scenarios with the updated transport.
  Non-Windows acceptance, OS-call interruption, forward-destination lookup
  and transport backpressure remain separate work.

- Remote directory reports (OSC 7 file URIs, BEL or ESC backslash termination)
  now update a tab's saved workspace directory. Parsing preserves path spelling,
  decodes UTF-8 percent escapes and rejects malformed escapes, control characters,
  non-file URIs and paths above 4096 bytes. The URI host is descriptive: paths are
  used only on the tab's remote connection, never to access local files. Connection
  options expose the remembered path for editing/clearing. Reconnect sends a
  literal quoted POSIX `cd`; with tmux, `cd` precedes `tmux new-session -A`, avoiding
  input into existing panes. Missing-directory errors remain visible in the shell;
  shells without OSC 7 require manual paths. Tests cover fragmented reports,
  invalid updates, reset, workspace round-trip and shell metacharacter quoting.
  Two new SSH loopback cases verify exact startup bytes and subsequent directory
  reports, including tmux startup. All 50 loopback scenarios, 84 fresh tests and
  app/probe builds pass. The fixture does not execute a real shell or tmux; those
  acceptance checks and native directory-field interaction remain pending.
  Protocol reference: [iTerm2 directory reports](https://iterm2.com/documentation-escape-codes.html).

- OSC 8 HTTP(S) links now attach shared destination metadata to terminal cells,
  including wide-character continuations. Overwrites/erasures clear metadata;
  scrolling and cloned search snapshots retain it. The fresh protocol layer reads
  complete destinations before VTE's limited OSC parameter splitting, preserving
  semicolons. ESC ] sequences terminated by BEL or ESC backslash are supported;
  destinations are limited to 4096 bytes. Explicit destinations override URL-like
  labels; non-web schemes, whitespace/control characters and bidi controls cannot
  launch. Hover highlights the contiguous row segment and shows the destination;
  Ctrl-click uses the existing guarded input route. OSC id-based disjoint highlight
  grouping is not implemented. Tests cover every input split, wide/wrapped cells,
  history, overwrite/erase/reset, snapshots, unsafe destinations and headless GUI
  destination activation. Fresh tests: 81; root regressions: 169; app build passes.
  Native browser-launch acceptance remains pending. Protocol reference:
  [OSC 8 specification](https://gist.github.com/egmontkob/eb114294efbcd5adb1944c9f3cb5feda).

- Diagnostic reports now include terminal dimensions, cursor, modes, visible text,
  traffic counters and the latest 64 KiB of remote output encoded as base64.
  Local input buffers and connection configuration are excluded; echoed input
  can still appear in remote output. The command palette offers a native save
  dialog and background file writes, limited to two workers and one pending
  export per tab. Staged publication preserves existing files. Regression checks
  verify state capture, exact remote bytes, local-paste exclusion and no-clobber
  cleanup. All 78 fresh tests and the app build pass. Native dialog acceptance
  remains pending.

- Application startup installs a persistent Rust panic hook. It records epoch
  time, version, thread ID and source basename/line/column beside the selected
  config (`.panic.log` extension), omitting panic payloads and terminal/user data.
  The file resets before exceeding 64 KiB; a nonblocking mutex avoids waiting
  behind another panicking thread. Log-open/write failures produce a fixed
  diagnostic instead of preventing launch or exposing the payload. This covers
  Rust panics, not arbitrary OS crashes/abort signals. A bounded-write unit test
  and isolated `crash_probe` process verify rotation, source-location capture and
  absence of a synthetic secret from both log and stderr. The SSH fixture's key
  generator now uses an isolated config path for crash-log creation. Debug-log
  controls remain pending; terminal-state report exports are described above.

- Key generation now supports passphrase protection. The GUI defaults to protection
  and requires nonempty matching entries; both fields use zeroizing storage and
  clear when submitted/toggled. The CLI `--protect-key` prompts twice without
  accepting a passphrase argument or environment variable. Encryption occurs
  before the private staging file is written; private-file access restrictions
  and no-clobber publication remain in place. A generated-file regression verifies
  that the saved key is encrypted, rejects a wrong passphrase and decrypts to the
  matching public key. CLI parsing rejects protection without key generation.
  Native masked-input/dialog acceptance and public-key installation remain pending.

- Encrypted OpenSSH private keys prompt for a non-persisted passphrase in GUI/CLI.
  Reads are limited to 1 MiB; decryption uses zeroizing passphrase/result storage
  on at most two background workers, with cancellable session waits. Bcrypt
  work is limited to 256 rounds and 1024 salt bytes; higher-cost keys are directed
  to an SSH agent. A wrong passphrase ends the attempt; reconnect to retry.
  GUI challenge cancellation clears its answer strings. Both 70- and 76-column
  PEM wrapping are recognized. The live wider-wrapped encrypted fixture exposed
  a shared SSH-library panic: encrypted keys are now explicitly rejected before
  extracting private components. A vendor regression guards that case.
  Fresh tests: 75; vendor unit tests: 28; original root tests: 169, all pass.
  Vendor's separate nine algorithm integration tests could not connect to their
  default 127.0.0.1:22 server and failed with connection refused; they are not
  claimed as passed. Fresh loopback adds encrypted-key success, wrong-passphrase
  rejection and prompt cancellation (48 scenarios total). Native passphrase UI
  acceptance, encrypted key generation and key installation remain pending.

- Unencrypted OpenSSH Ed25519 key generation is available from the connection
  form and `--generate-key PATH`. The GUI reviews both filenames, saves off the
  UI thread, shows the fingerprint, copies the public key and optionally selects
  the private key for that connection. Seed/private encoding use zeroizing types.
  Private staging uses Unix 0600 or a Windows protected current-user-only DACL
  applied through the file handle before writing private bytes. Existing files
  are preserved with no-clobber publication. If the public-file commit fails
  after the private file succeeds, the result reports that partial save and
  retains the public text for recovery; the pair is not claimed to be atomic.
  Tests verify matching key material and existing-file preservation. The live
  fixture generates a key with the CLI, reads it using OpenSSH `ssh-keygen -y`,
  then authenticates with it against Paramiko. Encrypted-key generation/loading,
  public-key installation and native key-dialog interaction remain pending.

- Remote files now includes a local browser with absolute-path navigation,
  parent navigation, folder double-click, byte sizes, and selection of up to
  64 files for the upload review queue. Only visible rows render. Enumeration
  runs off the UI thread, limits directories to 10000 entries, and checks
  cancellation between filesystem calls. At most two directory-reader threads
  may remain active, including readers abandoned while a filesystem call blocks.
  Blocking network/filesystem calls themselves are not forcibly interrupted.
  A temporary-directory test verifies literal paths, metadata, ordering and
  cancellation. Native local/remote browser layout and interaction acceptance
  remain pending; file uploads use the existing reviewed transfer engine.

- Remote New file creates an empty file after name/path review. SFTP OPEN uses
  write/create/exclusive flags and 0600 permissions, followed by acknowledged
  CLOSE. Existing files are never truncated. The action sequence now has 16
  operations, including empty-file creation, duplicate rejection, and rejection
  against a pre-existing nonempty file; its final listing verifies size and mode.
  As with folder creation, cancelling or disconnecting after submission cannot
  undo server-side creation; errors direct the user to refresh before retrying.
  Native New file interaction acceptance remains pending.

- Remote files now offers Clipboard image: a single background reader captures
  RGBA pixels, encodes an owned temporary PNG, and returns a 320x200 preview for
  destination review. The UI retains the PNG through upload completion and drops
  it on completion, failure, cancelled review or window closure. Late capture
  results are destroyed after cancellation; a global reader guard prevents an
  accumulation of blocked clipboard threads. Limits after clipboard decoding:
  16777216 pixels, 16384 per side, checked four-byte RGBA length. The OS clipboard
  decoder's initial allocation and read duration are outside these bounds.
  Tests verify PNG pixel/alpha round-trip, invalid dimensions, cleanup on Drop,
  late-result cleanup and concurrent-reader rejection. Tests use synthetic pixels
  and do not alter the user's clipboard. SFTP handles the resulting PNG using
  the existing upload flow; required SCP transport and native clipboard/preview
  acceptance remain pending. Copy-path and quoted insertion work after success.

- File drops are consumed once and queued on the connection active at drop time.
  The remote browser offers a bounded 64-file review queue, folder navigation,
  editable destination names and explicit per-file upload. Queued paths require
  absolute Unicode filenames; invalid batches do not partially enqueue. Source
  regular-file checks remain on the transfer worker; recursive folder uploads
  are not implemented. Closing the browser discards waiting files.
  Successful uploads expose Copy remote path and optional POSIX-quoted terminal
  insertion without Enter; control-character paths are copy-only. Tests cover
  queue bounds, atomic rejection and literal paths/quoting. Existing SFTP upload
  fixtures cover the transfer engine; native OS drop/clipboard/insert acceptance
  is still pending. Clipboard PNG and SCP support remain in the original scope.

- Literal terminal bracket matching highlights paired parentheses, square brackets
  and braces at/before the cursor or under the pointer, using terminal cells.
  Nested pairs can span displayed rows; mismatched or unavailable partners are
  not highlighted. Scans stop after 8192 cells or 1024 nested pairs. This is a
  text aid, not a language parser: quotes/comments do not alter pairing, and
  offscreen partners are not searched. Selection/search highlights take priority;
  modal dialogs suppress matching. Unit tests cover wide cells, nesting, reverse
  scans, malformed pairs and bounds; headless rendering verifies exactly two
  highlighted cells and selection/modal precedence. Native acceptance remains.

- Terminal web URLs now underline on hover, show their destination, and open on
  Ctrl-click. HTTP(S) parsing uses the `url` crate; local-file/custom schemes are
  not launched. Cell-based hit testing supports wide characters, soft wraps,
  scrollback and immutable search screens. Bounds are 8192 cells per logical
  line, 32768 text bytes and 4096 URL bytes. Trailing punctuation and unmatched
  closing delimiters are excluded. A separate mouse gesture prevents forwarding
  link clicks to remote mouse applications; drag, modifier release and dialogs
  prevent activation. Parser tests cover Unicode, IPv6, wraps and malformed URLs;
  headless egui tests inspect browser-open output and remote mouse capture.
  Native external-browser launch and OSC 8 hyperlinks remain unverified/pending.

- Remote permission editing is implemented with three/four-digit octal validation,
  explicit path/mode review and server-acknowledged refresh. The SFTP v3 SETSTAT
  request sends only permission attributes, retaining ownership, size and times.
  The browser exposes it for listed regular files and directories; it is not
  recursive. The loopback action sequence now has 13 operations, with file and
  directory modes, special bits, mode 0000 and server rejection checked against
  the final listing. An editor regression rejects malformed octal input. Native
  permission-editor acceptance and concurrent remote path replacement remain
  unverified. Protocol reference: [SFTP v3 section 6.9](https://www.ietf.org/archive/id/draft-ietf-secsh-filexfer-02.txt).

- SFTP uploads now stream 32 KiB chunks with acknowledged-byte progress, a native
  source picker, destination review, and explicit cancellation. Random sibling
  staging files use exclusive creation and 0600 permissions; SFTP v3 rename
  publishes only after every WRITE and CLOSE is acknowledged, without replacing
  an existing destination. Source size/mtime changes fail the transfer.
  Cancellation and server errors attempt CLOSE/REMOVE cleanup while connected,
  including after the UI drops its task. Cleanup failure identifies the staging
  path; lost publication replies identify both paths to check. Requests have a
  30-second idle timeout, with 10 seconds per cleanup request. Session disconnect
  can leave staging files; persistent recovery and native picker acceptance remain.
  Eight added loopback cases verify binary/empty uploads, cancellation, dropped-task
  cleanup, write/close errors, existing-destination preservation and cleanup denial.
  A local filesystem regression detects source changes. Shared transport code did
  not change in this upload increment.

- Existing root reference app: `cargo test --locked --all-targets`, 2026-09-06:
  169 passed, 0 failed. This is a baseline, not evidence for the new code.
- Fresh `cargo fmt --manifest-path fresh/Cargo.toml --check`: pass, 2026-09-06.
- Fresh `cargo test --manifest-path fresh/Cargo.toml --locked --all-targets`:
  92 passed, 0 failed, 2026-09-06. Includes headless egui event routing for
  remote mouse capture, Shift release and modal cleanup, painter order/style
  checks, wide-character/wrapped-line copy selection, and snapshot search.
- Search scans retained scrollback off the UI thread, supports soft wraps,
  Unicode cell coordinates, case matching, previous/next navigation, and
  highlighting. Snapshot copying is isolated from live output. Match counts
  are capped at 10,000 with a visible indicator; query changes cancel scans.
  The shared vt100 Screen scrollback setter is now public for cloned snapshots;
  its existing behavior is unchanged.
- Worker completion uses a separate one-shot channel, delivered after queued
  output, so a saturated output queue cannot lose final status. A panic boundary
  marks the worker finished and returns a fixed reconnect message without
  forwarding the panic payload. Regression coverage injects a panic with a full
  queue and verifies output order and completion. All 13 loopback scenarios were
  rerun successfully after this lifecycle change.
- Snippets read both `command` and `body` formats, retain extension fields,
  and persist through atomic configuration saves. The editor offers create,
  update, delete, and explicit sending of reviewed text to the displayed active
  destination, with optional final Enter. Persistence regression tests pass;
  native interaction acceptance remains pending.
- Workspace snapshots save changed tab settings every two seconds and on close,
  omit blank tabs and passwords, preserve the active index after omission, and
  read legacy flat tab records. Settings expose restore, reconnect, and ask-first
  preferences. Save failure pauses autosave and provides retry/exit guidance.
  An isolated native launch (`.qa/restore.toml`) restored two tabs, selected the
  second, and showed the configured reconnect confirmation; app-owned capture
  inspected at `.qa/restore.png`. No connection was attempted during QA capture.
- Advanced connection settings expose `reconnect_delay_secs` (0 disables,
  maximum 300 seconds). An established session ending schedules one retry;
  failed authentication does not schedule a loop. Tests verify cancellation,
  deadline consumption, retained error/output and bounded connection history.
- OpenSSH Include import supports nested files, sorted wildcards, multiple
  paths, relative paths rooted at the main config directory, and `~/` paths.
  Parent Host scopes are inherited and restored after each include, following
  [OpenSSH readconf.c](https://github.com/openssh/openssh-portable/blob/master/readconf.c).
  Tests cover precedence, nested scope, aliases, missing globs and cycles.
  Limits: 16 levels, 128 files, 8 MiB total source. Unsupported Include path
  expansions fail explicitly; no local commands are executed.
- Fresh debug build and `--help`: pass, 2026-09-06.
- `python fresh/tests/ssh_fixture.py`: 48 scenarios pass, 2026-09-06:
  unknown/changed host rejection, Ask/strict/AcceptNew trust, password and
  password prompting, RSA PEM/OpenSSH and Ed25519 file keys, rejected-key
  fallback, two-prompt MFA, challenge cancellation, mock-agent signing, local,
  dynamic and simultaneous remote forwarding, remote-listener denial, and
  cancellation during stalled agent listing/signing with pipe closure verified.
  OpenSSH `ssh-keygen -H` generates an isolated hashed trust file; strict matching,
  changed-key rejection and AcceptNew revocation rejection pass without rewriting
  the known hashed record.
  Successful sessions verify shell input, ordered CPR, resize, environment,
  persisted trust, and cancellation within two seconds.
- Shared SSH dependency fixes: canonical RSA public blobs, client-key algorithm
  independent of server host key, alternate PEM wrapping, validated OpenSSH
  RSA conversion using both primes, and signing error propagation. Cryptographic
  regression tests cover both RSA formats and SHA-2 signatures.
- After the shared dependency fixes, the root app's 169 tests and the vendored
  SSH library's 22 tests pass. The test-only visibility warning is fixed.
- Native launch and app-owned screenshot inspected at `.qa/workspace.png`;
  improved field contrast and installed system/CJK font fallbacks after review.
  External Windows capture failed twice with `SetIsBorderRequired` E_NOINTERFACE;
  egui viewport capture succeeded. No native interaction test is claimed.

- Local forwarding uses session-owned listeners (loopback default), up to 64
  relays, bounded socket output buffers and local SSH EOF. The fixture transfers
  binary data while terminal CPR/input/resize continue, then verifies listener
  removal. This exposed lost terminal packets during channel open; the shared
  SSH open path now parks existing-channel traffic. Root regression: 169 passed.
  Forward writes now consume only available SSH window credit, retaining an
  unsent suffix in an 8 KiB relay buffer. A zero-window fixture verifies shell
  input/CPR/resize and cancellation within two seconds despite the blocked relay.
  Queued channel data is also retained when a close notification was parked by
  another reader. Five focused vendor channel tests, all 49 fresh tests, and
  all 169 root tests pass after these changes. Remote EOF now propagates as a
  socket write-half shutdown while client input can continue. The live fixture
  verifies input sent after remote EOF, and six focused vendor channel tests
  verify recipient-scoped EOF and queued-data ordering. Receive-side
  backpressure, stalled channel-open and underlying transport-write responsiveness
  still need work.

- Dynamic forwarding adds incremental SOCKS5 no-auth CONNECT negotiation for
  IPv4, IPv6 and domain targets, following the packet layout in
  [RFC 1928](https://www.rfc-editor.org/rfc/rfc1928.html). Negotiation buffers
  are capped at 512 bytes, idle negotiation expires after ten seconds, and
  pending plus active forwards share the 64-connection limit. Unsupported
  authentication/commands/addresses return rejection replies. Unit tests cover
  fragments, payload boundaries, address types, rejection and expiry. The live
  fixture verifies remote DNS, pipelined data, half-close and listener cleanup.

Remote-forward preparation found and fixed shared transport defects: global
requests now honor the reply flag, and listener requests/channel opens retain
interleaved data, EOF and incoming forwarded channels. Queued forwards expose
their connected address and port so callers can route multiple listeners.
Allocated ports must be present, nonzero and fit a TCP port; malformed values
are rejected instead of truncated. These follow
[RFC 4254 sections 4 and 7](https://www.rfc-editor.org/rfc/rfc4254.html).
Six new protocol regressions cover these cases. Validation: 51 fresh-app tests,
22 shared-library tests, 169 root regressions and all 17 loopback scenarios pass;
the app and session probe build successfully.

Remote forwarding now uses this metadata to match server listeners to configured
local destinations. Exact address matches take priority; wildcard fallback
respects IP address families. Local TCP connections run on background workers
with a five-second deadline, sharing the 64-connection relay limit. Destination
DNS is resolved at setup (still synchronously). The live fixture verifies two
simultaneous destinations with distinct binary payloads, half-close, server
rejection and listener cleanup. CLI help exposes `-R`, and profiles, the GUI
editor and repeated OpenSSH `RemoteForward` imports use the same resolver.
Current verification: 53 fresh tests, 19 loopback scenarios, app/probe builds.

## Next work

Remote downloads now use a reviewed absolute local destination, a native save
dialog, progress counters and cancellation. SFTP OPEN/FSTAT/READ/CLOSE run
incrementally, with 32 KiB reads and a 30-second idle timeout. Regular-file
metadata supplies total size when available; unexpected growth/truncation fails
without publishing the destination. A sibling temporary file is flushed and
published with no replacement only after EOF and CLOSE success. Drop/error
cleanup removes staging files, and concurrent destination creation is preserved.
The fixture verifies binary and empty files, cancellation after partial data,
server read failure, early EOF and an existing destination. Unit tests exercise
real-filesystem cleanup and publication races. App/probe builds, 61 fresh tests
and all 36 loopback scenarios pass. Native picker/browser interaction acceptance,
upload support and transfer overwrite workflows remain pending.

Remote file actions now offer create-folder, rename, file deletion and empty-folder
deletion. The review panel freezes the source path, validates one destination
filename and escapes control characters in displayed names. Requests run through
SFTP on the existing authenticated session, and the browser refreshes only after
server success. Pending actions explain that closing the window does not undo a
submitted change. The fixture checks a sequence of nine actions, including Unicode,
shell metacharacters treated as literal names, existing-destination rejection and
nonempty-folder rejection; final listings verify server state. Native interaction
acceptance remains pending.

A live run exposed an intermittent KEX signature failure. The unsigned `mpint`
encoder retained unnecessary leading zeroes in ECDH shared secrets. It now uses
the shortest positive representation required by
[RFC 4251 section 5](https://www.rfc-editor.org/rfc/rfc4251.html#section-5), including
empty zero and necessary sign padding. A forced leading-zero ECDH fixture fails
against the previous probe binary; two cases cover optional/required sign padding.
Ephemeral P-256/X25519 keys are now safely consumed once instead of copied through
unsafe pointer reads, and malformed public-key lengths return errors. The 27
vendor tests, 60 fresh tests and 169 root regressions pass; app/probe builds pass.
All 30 loopback scenarios now pass, including both forced leading-zero KEX cases
and the nine-operation SFTP action sequence.

The **Remote files** window now lists directories through the existing authenticated
session, with canonical paths, parent navigation, refresh, byte sizes, permissions
and double-click folder navigation. SFTP v3 follows the
[file-transfer protocol draft](https://www.ietf.org/archive/id/draft-ietf-secsh-filexfer-02.txt).
Requests poll alongside terminal and forwarding traffic; cancellation drops the
SFTP channel, and success closes the directory handle. Limits are 1 MiB per packet,
10,000 entries, 4096-byte UTF-8 names/paths and a 30-second listing deadline.
Non-UTF-8 names currently fail explicitly; symlink target classification is pending.
Initial SSH channel-open and underlying transport writes still need stronger
cancellation/backpressure. Native browser acceptance and row virtualization remain.
The fixture verifies a 42-entry multi-packet directory with Unicode and metadata,
empty-directory navigation, permission denial, subsystem refusal and a stalled
directory request while terminal IO continues. Shared channel failures now reach
their owning reader, and SFTP framing retains fragments and drains complete packets
before transport EOF. Fresh tests: 59; root regressions: 169; three new focused
vendor regressions pass. App/probe builds and all 27 loopback scenarios pass.

OpenSSH hashed known-host matching uses the `|1|salt|digest` HMAC-SHA1 format
from [OpenSSH hostfile.c](https://github.com/openssh/openssh-portable/blob/master/hostfile.c),
with fixed-size validated salt/digest fields and constant-time MAC verification.
DNS names are lowercased and IP literals normalized for lookup; IPv6 scope names
retain their case. Unit regressions cover malformed hashes, distinct ports,
revocation precedence and atomic persistence without plaintext duplication.
New accepted hosts still use the existing plaintext persistence format.
Latest build and 58 tests pass; the loopback fixture now has 24 scenarios.

Agent pipe operations now check cancellation every 50 ms and settle cancelled
overlapped IO before freeing buffers, following the
[Windows IO API contract](https://learn.microsoft.com/en-us/windows/win32/api/ioapiset/nf-ioapiset-getoverlappedresultex).
Socket RPCs preserve partial reads/writes with a total deadline and cancellation
checks. Pageant message calls run on at most eight isolated workers: cancelling
the session abandons the result promptly, while the worker owns its mapping
until the Windows call returns (up to the existing 120-second signing timeout).
This does not dismiss a Pageant-owned approval dialog. Regression tests cover
partial socket response cancellation, fragmented RPC and deadlines, and late
Pageant results; live pipe tests verify worker termination within two seconds
for both listing and signing, and that the agent sees its pipe close.
Unix socket setup now has cancellable nonblocking connection logic (see latest
evidence above); platform runtime verification remains. At this earlier stage,
the fresh build, 56 tests and 21 loopback scenarios passed.

Implement remaining SSH capabilities and integration fixtures, then finish
terminal interaction/search, workspace persistence/settings, and file tools.
Known first-pass limitations to address: GUI mouse routing needs native acceptance;
installed-agent UI acceptance and non-Windows connection cancellation remain;
clipboard PNG uploads offer SFTP/SCP; native clipboard/file-drop acceptance remains;
selection/wide-cell details and paste/shortcut routing need interaction tests.
Jump routes now fail at the failing hop without a direct-connection fallback.
Keep the full requirement scope above until current implementation and live
acceptance evidence prove parity; do not replace it with a minimal terminal.
