# ssh4-fresh 0.1.146 - release candidate, 2026-09-10

The independent Rust rebuild now has audited GUI/CLI, terminal, file-transfer,
SSH and configuration behavior. Release acceptance remains incomplete because
native macOS linking/runtime has not been verified. This is a release-candidate
record, not a claim of full cross-platform parity.

## Changes

- Event-driven SSH GUI wakeups, changed-only traffic snapshots and conditional
  idle repainting reduce measured connected idle CPU to about 0.17-0.18 cores
  under Linux Xvfb/software rendering; other threads account for about 0.03.
- Connection history now reports configuration resolution, negotiation, host
  verification, authentication, PTY and shell-request phases at actual boundaries.
  A sent shell request is not labelled as server acknowledgement.
- Successful remote CLI exit honors the reconnect preference. Explicit local
  quit and --no-reconnect exit directly. Windows input/output modes are restored.
- Native tests cover exact newline/Unicode clipboard behavior, frozen selection,
  menu focus and IME interaction, search/links, tmux/TUI behavior, file workflows,
  forwarding/jump routes, auth/key management, workspace settings and cleanup.
- README fixture prerequisites and functional/SSH/nonfunctional/security/mode/
  release-matrix audits are reconciled. v0.1.146 adds documentation/version changes;
  the last application behavior changes were v0.1.145.

## Verification

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

## Known limits

- No Mac host/SDK runtime evidence is available. Apple/Intel cross-target checks
  establish source compatibility only; they do not produce a tested Mac package.
- Performance figures use software rendering and five-second idle windows;
  they are not hardware-GPU benchmarks or a zero-CPU guarantee.
- SSH PTY/shell requests currently do not ask for acknowledgements. Timeline
  messages accurately distinguish request transmission from server acceptance.
- Raw output logs and diagnostic output can contain secrets echoed/printed by a
  server. Application credential/input buffers are excluded; remote output is
  intentionally preserved rather than claimed to be redacted.
- ProxyCommand, arbitrary overwrite workflows and Mac app bundles remain outside
  the implemented behavior. POSIX fallback tools and tmux must exist remotely
  where those features are used. Existing destinations are preserved.
- Stock upstream SSH host/account-dependent integration binaries were not all
  executed. Direct SSH library/doc and VT100 suites plus owned SSH integration
  provide the explicitly documented coverage; legacy algorithm cases are not
  silently marked passed.

Install on Windows with fresh/install.ps1; native Linux prebuilt installation
uses fresh/install.sh. See README.md for exact paths, prerequisites and evidence.
