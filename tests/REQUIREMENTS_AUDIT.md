# Final requirement audit - fresh v0.1.146

Release acceptance remains incomplete. This audit preserves the scope of
`docs/redevelopment/01-product-requirements.md` and
`docs/redevelopment/04-validation-and-release.md`; the independent executable is
neo-ssh. Evidence paths are relative to fresh/. A verified functional row
covers its named behaviors, not every platform/feature combination. Pending
sections below must be reconciled before final acceptance.

## Functional requirements

Baseline A: `.qa/validation-370a7146e6af437296eab4f93751358c/results.json`, all
12 gates at v0.1.140: 149 app tests, one backend test, 48 OpenSSH comparisons and
101 SSH cases. The tests.log was inspected for the specific target/config/input/
terminal regressions below. v0.1.141 and v0.1.142 change documentation/version only.

| Requirement and all named behavior | Source and verification | Assessment |
| --- | --- | --- |
| FR-001: no args / --gui open GUI; host / -P select CLI | main.rs Args and dispatch; README_ACCEPTANCE.md records argument checks separately from native GUI startup and Windows CLI target/profile connection evidence in RELEASE_ACCEPTANCE.md. | Verified |
| FR-002: host, host:port, user@host, user@host:port; default 22 | target.rs validates and resolves all forms. Baseline A target_forms_round_trip and rejects_ambiguous_or_invalid_targets pass; CLI/native fixture connections exercise user and explicit ports. | Verified |
| FR-003: embedded/override username, password/key/default-key auth, GUI password, CLI prompt, GUI Ed25519 generation and public-key install; no credential logging/default persistence | connection.rs/session.rs/main.rs resolve username and key discovery; baseline auth/keygen/config regressions and live SSH methods pass. RELEASE_ACCEPTANCE.md records native GUI password/KBI, protected/unprotected key generation, encrypted-key cancellation/retry and reviewed key installation followed by login. SECURITY_REVIEW.md reconciles credential handling and limits. | Verified |
| FR-004: ~/.ssh4.toml profiles, snippets, zoom/theme/keepalive/display preferences, per-profile tmux; existing schema compatibility | config.rs path/schema/atomic persistence, preferences.rs/settings_dialog.rs and workspace.rs. Baseline A accepts_both_profile_schemas_without_silent_duplicate_loss, snippets_round_trip_both_schemas_and_preserve_other_configuration and save/delete failure tests pass. Native preferences/theme/snippet CRUD persistence: `.qa/linux-runtime-9e8859005ecd4f1f87e9b376ae610643/results.json`; workspace/tmux/options restore: `.qa/linux-runtime-4b4c833bdc804fe1a844f4008d31b545/results.json`. | Verified |
| FR-005: shared testable VT key mapping, bracketed paste, DEC/SGR mouse, DA/DSR/CPR/OSC colors, CPR stripping, split buffering, unknown passthrough | input.rs and terminal.rs are shared by GUI/CLI. Baseline A input modifier/cursor/mouse tests and terminal all_read_boundaries_preserve_cursor_reply_order_and_unicode, osc_and_device_queries_work_one_byte_at_a_time, strips_cpr_responses_and_confirms_all_line_endings, mouse_modes_are_independent_and_reset_with_terminal and unknown_sequences_pass_through_without_answering_embedded_dcs_queries pass. Native CLI keys/mouse and GUI real TUI/mouse/paste evidence in RELEASE_ACCEPTANCE.md. | Verified |
| FR-006: ANSI/default/indexed/RGB, cursor, selection/search/link/bracket highlights, scrollback/wide text with shared cell coordinates | desktop.rs cell grid uses vt100 colors, wide continuation checks and the same cell geometry for paint/hit tests. Baseline A renderer/input tests plus native Unicode/wrapped selection and history/resize freeze tests, exact clipboard, extended search and Ctrl-click link/path/bracket cases are reconciled in CLIPBOARD_ACCEPTANCE.md and RELEASE_ACCEPTANCE.md. Color/theme rendering and real Vim/less/htop captures complement these cases. | Verified |
| FR-007: PNG over SCP, drop upload, copy destination, optional quoted shell insertion, success/failure cleanup | clipboard_image.rs/upload.rs/file_browser.rs; SECURITY_REVIEW.md records Windows CLI SCP image success/rejection/cancel and Linux SFTP/automatic SCP image/drop cases. Exact pixels/bytes, destination copy, quoted insertion without Enter, existing-target preservation and staging cleanup verified. | Verified |
| FR-008: remote list/upload/download/create file/folder/rename-move/chmod/confirmed delete; local browse/selected uploads; quoted shell paths | files.rs, files/shell.rs, SCP workers, file_browser.rs and local_browser.rs implement reviewed actions. Baseline A covers request protocol/framing/quoting/failures. Native file operations, local selected-only uploads, SFTP and fallback SCP plus active cancellation/closure are recorded in RELEASE_ACCEPTANCE.md and PROGRESS.md v0.1.63-0.1.88. SECURITY_REVIEW.md verifies delete confirmation and shell quoting. | Verified |
| FR-009: GUI counters/worker heartbeat information, panic log, debug toggle, state/recent-output dump excluding local input | session.rs emits changed traffic snapshots on its heartbeat cadence; desktop.rs displays counts and polls worker state. diagnostics.rs/debug_log.rs provide bounded output reports and payload-free crash/metadata logs. Baseline A report/capture/crash tests and native `.qa/linux-runtime-c54ab7ca7d45473787ffcd6245911364/results.json` verify toggle/stop/export/security. Heartbeat snapshots do not generate unchanged idle notifications since v0.1.138; this is distinct from protocol keepalive. | Verified |
| FR-010: optional attach/create, -A -s semantics, advanced GUI name, profile/workspace persistence, --tmux[=name]/--no-tmux, reconnect reuse, safe names; usable shell on failure; no tmux in jumps or one-shot requests | target.rs/session.rs construct startup only on the final interactive shell, after PTY/open; input validation and CLI parser regressions pass in A. Real tmux same-pane reconnect/state/resize/detach, missing/failing command recovery, two-client attachment and GUI settings/restore are reconciled in RELEASE_ACCEPTANCE.md. PROFILE_ACCEPTANCE.md distinguishes recorded startup-command persistence from actual tmux execution. | Verified |

## SSH capabilities

| Requirement and named behavior | Source and verification | Assessment |
| --- | --- | --- |
| SSH-001: every connection verifies host keys; unknown prompt, changed rejection, strict unknown/changed failure | session.rs installs the host verifier for every authentication attempt/hop and persists only after signature proof. trust.rs implements supported OpenSSH entries plus hashed/revoked checking. Baseline A first/second connect, strict/changed/revoked/hash scenarios and native two-hop changed-key rejection in RELEASE_ACCEPTANCE.md pass. Transfer/forward channels reuse authenticated sessions. | Verified |
| SSH-002: Windows OpenSSH/Pageant and Unix socket transports; ordered identities before password, successful identity fingerprint, profile opt-out | agent.rs honors explicit SSH_AUTH_SOCK before platform discovery; connection.rs controls order/use_agent; session.rs emits successful agent fingerprint. Baseline A covers agent success/fallback/cancellation and resolver opt-out; native Pageant 0.84 automatic/WM_COPYDATA/pipe auth is recorded at PROGRESS.md v0.1.33. The release matrix permits Windows OpenSSH and/or Pageant; a stopped OpenSSH service does not negate that pass. | Verified for specified matrix; no claim that every agent service ran |
| SSH-003: nonblocking GUI prompts, inline CLI, multiple prompts, no answer logging | session.rs callback waits on the worker; GUI/CLI receive Challenge and return answers. Native GUI six-case password/KBI result and native Windows CLI auth result are reconciled in SECURITY_REVIEW.md; baseline A includes multi-prompt and cancellation. | Verified |
| SSH-004: import HostName/User/Port/IdentityFile/ProxyJump aliases into picker; explicit profile precedence; safe unsupported fields | ssh_config.rs and connection.rs share import/resolution; desktop.rs populates imported aliases. Baseline A has 48 ssh -G comparisons plus profile precedence and import regressions; native alias picker observations and jump controls are recorded in RELEASE_ACCEPTANCE.md/PROGRESS.md. ProxyCommand remains future scope. | Verified |
| SSH-005: SFTP list/upload/download/rename/delete/chmod and shell/SCP fallback | files.rs workers use the authenticated session; baseline A exercises operations/failures. Native file/local/transfer acceptance and automatic subsystem-refusal fallback are reconciled in FR-008 and RELEASE_ACCEPTANCE.md. | Verified |
| SSH-006: L/R/D forwarding, session lifetime, GUI active/error display, CLI flags | forward.rs/socks.rs/session.rs implement local/remote/SOCKS channels and listeners. Baseline A relays/half-close/zero-window/stalled-channel cases and native GUI entry/save/relay/bind failure/rollback/reconnect/listener cleanup at v0.1.102 pass. | Verified |
| SSH-007: one/multiple jumps, profile/imported ProxyJump, clear per-hop timeline | connection.rs resolves bounded chains; session.rs loops over hops with each host verifier and wraps failures with route-step context. Baseline A one/two-hop and failures plus native encrypted two-hop rekey/SFTP/changed-key cases pass. Route-step visibility is present; the additional phase detail required by SSH-008 is verified below. | Verified |
| SSH-008: config resolution, DNS/TCP, key exchange, host verification, auth attempts, PTY request, shell open | v0.1.144 adds scoped library phase observers at actual operation boundaries, GUI config-resolution records and verifier events. PHASE_ACCEPTANCE.md records observer isolation, ordered successful/failing SSH assertions and inspected native history. Shell-request-sent is explicitly not server acceptance. | Verified |
| SSH-009: optional tabs/profile identity restore, no passwords, confirmation, remote directory where available | workspace.rs/config.rs persist secret-free snapshots; desktop.rs restores tab order/identity and asks per settings. Native v0.1.139 two-tab settings/restore/input/focus/color result passes. remote_directory.rs/terminal.rs capture OSC 7 and quote startup paths; real shell missing-directory/new-existing-tmux cases are recorded at PROGRESS.md v0.1.31. | Verified |
| SSH-010: reconnect button/countdown, identity, old scrollback until success, last error | desktop.rs preserves prior terminal until Connected and retains failure details; retry is cancellable. Native v0.1.139 automatic/cancel/manual retry/input/explicit-disconnect suppression passes; default GUI session fixtures cover disconnected-output review/reconnect. | Verified |
| SSH-011: keepalive/auth order/agent/TERM/environment/timeout/strict options with schema compatibility | connection.rs/session.rs apply settings; native v0.1.139 GUI editing and workspace restore checks pass. PROFILE_ACCEPTANCE.md separates persisted controls, resolver tests and live effects. Config compatibility tests pass in baseline A. | Verified |

### SSH-008 audit follow-up

The gap discovered at v0.1.143 is resolved at v0.1.144; see PHASE_ACCEPTANCE.md
for implementation boundaries, test evidence and request-acknowledgement limits.

## Nonfunctional requirements

| Requirement and every named behavior | Verification | Assessment |
| --- | --- | --- |
| NFR-001: no GUI network blocking; batched SSH output; per-frame budget; worker transfers | session.rs creates bounded event/file channels and a worker thread; desktop.rs rotates tabs under an 8 ms aggregate poll budget, slicing output and coalescing dimensions. Native single/four-tab frame-budget checks and final optimized output/idle/interrupt/closure runs are reconciled in RELEASE_ACCEPTANCE.md. | Verified |
| NFR-002: tab closure stops worker; CLI raw-mode restoration; temporary upload cleanup; disconnected prefilled form | Session cancellation/retired cleanup tracking and Console Drop/suspend guards are present. Native CLI console and clipboard fixtures prove restored modes; native transfer cancellation/closure/stalled-exit and GUI disconnected-review/reconnect results prove cleanup/form behavior. SECURITY_REVIEW.md covers image/drop staging. | Verified |
| NFR-003: Rust SSH transport, credential exclusion, destructive confirmation, owner-only Unix config, safe shell commands | SECURITY_REVIEW.md maps each item to current source and native/regression evidence with remote-output echo limitations. | Verified for specified checklist |
| NFR-004: Windows first-class, Linux/macOS viability, platform font fallbacks, USERPROFILE/HOME config | Current Windows full tests/install and optimized Linux startup/native checks pass. config.rs resolves USERPROFILE then HOME; desktop.rs includes Windows/Linux/macOS font candidates. Apple Silicon/Intel strict cross-checks passed at v0.1.111, but no native Mac link/run evidence exists. | Incomplete: native macOS verification remains |

## Required modes

| Mode requirements | Coverage | Assessment |
| --- | --- | --- |
| GUI connection form, saved profiles, multiple tabs | FR-001/003/004; native profile/workspace fixtures. | Verified |
| GUI terminal, local selection, visible/scrollback search, multiline review | FR-005/006; CLIPBOARD_ACCEPTANCE.md and extended search/native history cases. | Verified |
| GUI clipboard image, drop upload, file tools | FR-007/008; native SFTP/fallback/clipboard/drop results. | Verified |
| GUI snippets, command palette, theme picker | FR-004; native snippet CRUD/send, preferences and palette-triggered key-install/diagnostic dialogs. | Verified |
| GUI focus mode, sync input | Native workspace exact routing/PTY resize and continuous-output focus cases in RELEASE_ACCEPTANCE.md. | Verified |
| GUI debug/state dump and optional named tmux | FR-009/010 and companion security/phase/profile audits. | Verified |
| CLI raw loop, keys, mouse, text/image clipboard, optional raw logging, clean raw/mouse teardown | Native Windows console/auth/clipboard/image evidence in RELEASE_ACCEPTANCE.md; Console guard and output-only logging source inspected. | Verified |
| CLI optional tmux with inheritance/override | FR-010 and real CLI tmux fixtures. | Verified |
| CLI reconnect prompt unless disabled | The audit exposed successful remote exit bypassing the prompt and Windows output mode not being restored. Both are fixed at v0.1.145. `.qa/cli-reconnect-48685294b76b4263b4afd2315ade9936/results.json` verifies accept/reconnect/second-session input/decline, prompt input mode, final console modes, output-only log and cleanup. Existing five console and six auth cases pass again. | Verified |

## Final matrix and parity reconciliation

RELEASE_MATRIX_AUDIT.md maps every remaining checklist group and command family,
including Definition of Parity, applicable direct vendor checks and explicit
host-dependent exceptions. RELEASE_NOTES.md records candidate status and limits.
All 12 v0.1.146 automated gates, native Linux startup/installer smoke, default
Windows installation and both Mac cross-target checks pass. Exact evidence is
recorded in RELEASE_MATRIX_AUDIT.md and ../RELEASE_NOTES.md.

## Remaining acceptance

NFR-004 native macOS linking/runtime remains unverified. Cross-target checking
cannot establish a runnable Mac release. Stock upstream host-dependent SSH test
binaries are explicitly unexecuted, with owned fixture substitute coverage and
scope explained in RELEASE_MATRIX_AUDIT.md; they are not silently counted green.

Any newly failed final gate must be resolved before the candidate can be accepted.
