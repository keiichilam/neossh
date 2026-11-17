# Release-matrix reconciliation - v0.1.146

Scope: every checklist group and command family in
`docs/redevelopment/04-validation-and-release.md`, plus Definition of Parity in
`01-product-requirements.md`. Paths below are relative to fresh/. Historical root
commands map to the independent fresh package as documented in README_ACCEPTANCE.md.
The table preserves environment-dependent limits instead of treating all green
app tests as proof of native behavior. Native macOS execution remains unverified.

| Original checklist items / command family | Authoritative coverage and assessment |
| --- | --- |
| fmt --check, check --locked, test --locked --all-targets; baseline tests/warnings | Full validate.ps1 gates run these exact Cargo operations in fresh/. Strict Clippy also passes; historical tolerated warnings are not needed. Final version results are appended below. |
| Direct vendored SSH tests | 32 library tests, including scoped phase/panic/thread isolation, pass at v0.1.144; direct doc test also passes. Upstream algorithms/connect/exec tests require supplied host/accounts/key files; SCP tests additionally mutate fixed paths and are feature-gated. Those stock environment-dependent binaries were not run wholesale. Owned 101-case app/SSH integration executes the changed library over actual SSH, including negotiation, key forms/password, agents, exec/file framing, SFTP/SCP, routing/rekey and phase order. This is stated substitute coverage, not a claim that every legacy algorithm test passed. Deprecated algorithms remain outside default features. |
| Direct vendored VT100 tests | cargo test --manifest-path vendor/vt100-0.15.2/Cargo.toml: four resize integration tests and one doc test pass. The library target contains zero unit tests; app terminal/renderer regressions supplement it. |
| Unit auth/config: parsing, username/default keys, generation/public key, defaults/load/save/deletion/snippets, tmux persistence/validation/startup/CLI parsing | REQUIREMENTS_AUDIT.md FR-001-005/010 and current all-target tests. Full SSH fixture supplements native authentication; no secret-value CLI arguments. |
| Unit terminal: keys/bracketed paste/mouse/scanner/split/query/CPR order and stripping/UTF-8/selection normalization/wide cells/brackets | FR-005/006; named scanner/input regressions and GUI paint/input tests. Real TUI/native clipboard/mouse cases in RELEASE_ACCEPTANCE.md and CLIPBOARD_ACCEPTANCE.md cover interactions. |
| Unit transfer: joining/temp/staging/quoting/list parsing/chmod/public-key command | File/upload/download/key-install regressions and native file actions. The fresh shell fallback uses framed NUL-separated listing, not brittle ls -la parsing; equivalent remote-list behavior and hostile-name preservation are exercised. This is an implementation difference, not a missing required list action. |
| Unit GUI: URL encoding/display-width/dialog routing/search | links/search/desktop/dialog tests plus native search/link/bracket, resize/wide-selection and menu/paste cases. |
| CLI command forms: host, port, -P, --log, --tmux, --tmux=name, profile --no-tmux | README_ACCEPTANCE.md distinguishes parser/help checks from actual connections. Native console target/profile/port/log cases and real CLI tmux inheritance/override fixtures verify runtime behavior. |
| CLI password/key, MOTD/prompt, Ctrl+C/D/V, resize, ANSI raw-log append and teardown | Native five-case console fixture verifies server banner, exact control/mouse/resize/CPR bytes, appended output and modes; clipboard/auth fixtures cover text/image prompts. Reconnect native fixture covers successful remote exit, accept/decline and complete console restoration at v0.1.145. |
| tmux CLI: create, command persistence/reconnect, second client, mouse/resize/alternate screen/bracketed paste, profile inheritance/override, missing tmux fallback | Real CLI and two-client engine cases plus native real tmux/Vim/less/htop and missing/failing tmux cases in RELEASE_ACCEPTANCE.md. Runtime layers are combined explicitly; no claim of every TUI test repeated on each OS. |
| SSH matrix known-host first/second/changed/strict; agent/no key/disable/fallback/discovery override | SSH-001/002 audit, 101 SSH scenarios, native Pageant automatic/WM_COPYDATA/pipe. Matrix allows Windows OpenSSH and/or Pageant; disabled OpenSSH service is not an additional gate. |
| SSH matrix KBI single/multiple/cancel; import alias fields/precedence/safe unsupported | SSH-003/004; native GUI/CLI auth and 48 ssh -G comparisons plus resolver regressions. |
| SSH matrix local/remote/SOCKS reachability/lifecycle/bind failure; one/multiple hops/per-hop trust/intermediate failure; SFTP actions/fallback | SSH-005-007 plus native forwarding and encrypted two-hop/rekey/SFTP results; loopback failures verify denied/stalled/changed-key paths. |
| GUI no-args/form/profile/autosave duplicate prevention; tabs/close-other/right/color/filter/focus/sync | Native launch/profile/workspace fixtures, transactional profile regression and v0.1.139 workspace result; exact input and cleanup assertions. |
| GUI terminal search, Ctrl-click URL/path, clipboard/drop/file list/upload/download/create/rename/chmod/confirmed delete | Native search/link/path, clipboard/drop and complete file-tool results in RELEASE_ACCEPTANCE.md; security confirmation/quoting audit. |
| GUI theme persistence, state dump, disconnected prefilled form, advanced tmux/header/reconnect/profile/workspace | Native preferences, diagnostics, default session, real tmux and workspace results; FR-004/009/010. |
| Copy/paste full sequence: frozen selection after output/history/resize; copy/clear/no-selection interrupt/copy-only/Escape; right-click/menu; LF/CRLF/CR/trailing review cancel/confirm; normal/forced shortcuts; bracketed editor paste; whitespace/Unicode/empty | CLIPBOARD_ACCEPTANCE.md maps the full finite sequence, including raw-byte checks and real Vim. Control-C foreground interruption is verified in native performance runs. |
| TUI commands vim/tmux/less/htop and CPR probe; alternate screen/mouse/Shift local/uncorrupted CPR/bracketed paste/detach | Real native TUI fixture and exact protocol/CPR Windows console fixtures. Native history and phase changes do not replace these terminal checks. |
| Performance yes/head and find; incremental painting/responsive UI/no idle spin/tab stops work | Optimized native fixture executes both specified commands, drains GUI counters, tests interrupt and focus interaction during output and busy-tab closure. 8 ms app poll budget, single/four-tab frame tests and measured software-renderer idle overhead are recorded in RELEASE_ACCEPTANCE.md. |
| Security all checklist items | SECURITY_REVIEW.md reconciles source, auth/output/temp/drop/delete/quoting/permissions and CLI exposure, with explicit server-echo limits. |
| Packaging release build/install/executable/fresh shell/no stuck console/README/packet/tmux requirements | Windows full validation and default-install acceptance, native console teardown and Linux installer evidence. README_ACCEPTANCE.md maps root vs fresh commands. Mac runtime is missing; cross-target checking is not a runnable package claim. |
| Release notes | ../RELEASE_NOTES.md records current behavior, validation and unresolved limits; it does not declare parity while native Mac verification is pending. |

Definition of Parity cross-check: local gates; GUI no-args/connect/render/input/
resize/search/paste/close; CLI host/profile/connect/key/mouse/paste/image/exit;
profile/snippet/tmux persistence and reconnect; clipboard/drop/file tools;
protocol regressions; SSH-001-011 are all represented above and in the numbered
requirement audit. NFR-004 remains incomplete due to missing native Mac execution.
No new implementation gap was found in this cross-check beyond the phase and
CLI reconnect/console issues already fixed at v0.1.144-145.

Final current-version gates and host-dependent exceptions must remain attached
to the release notes; future changes invalidate affected evidence and require
appropriate retesting. The exact stock SSH algorithm/connect/exec/SCP binaries
remain additional environment-dependent coverage, explicitly not recorded as green.

## Final candidate evidence

Final v0.1.146 verification passed on 2026-09-10:

- Full Windows validation: all 12 gates, 149 app tests, one GUI backend test,
  48 OpenSSH configuration comparisons and 101 owned SSH scenarios, with no
  skipped gates: `.qa/validation-b258de8078c149f88ca2ed210e4727cb/results.json`.
- Optimized Linux linking:
  `.qa/linux-cross-1d2a945af2254927baea4682aede1b3a/results.json`.
  Native startup/help, key generation/permissions/preservation, GUI startup/clean
  exit and installer replacement/failure preservation passed on <test-host>:
  `.qa/linux-runtime-1c0b9fec3d504406a703b4ab5711bdcf/results.json`.
  The startup screenshot was inspected; owned display and remote directory were removed.
- Default Windows install/reinstall, saved user PATH and fresh-shell version/help:
  `.qa/windows-install-739166244f1c4eeca349f0b336a50f28/results.json`.
- Apple Silicon and Intel all-target strict Clippy checks:
  `.qa/macos-cross-f823ae9b869e480292a79dea333c67d7/results.json` and
  `.qa/macos-cross-295578a9959c4d688f52f23f7781657c/results.json`.
  These do not establish native Mac linking/runtime. Cargo also reports a future
  incompatibility warning for dependency block v0.1.6; current checks pass.
- Direct VT100 tests (four integration and one doc) and SSH doc test (one) passed;
  the unchanged SSH library's 32-test result is retained from v0.1.144:
  `.qa/vendor-final-bdcba8b08d0641b6a3797e478c9df98a/results.json`.

Paths above are relative to fresh/. Earlier native interaction evidence retains
its original version and scope in the acceptance audits.
