# README and packaging examples - v0.1.140

Evidence paths below are relative to fresh/.

Current full suite: `.qa/validation-370a7146e6af437296eab4f93751358c/results.json`
(all 12 gates, no skips). Default Windows installation:
`.qa/windows-install-7fb086a6233443d887ff613f3c425b8f/results.json`.

Reviewed against README.md, src/main.rs, tests/validate.ps1 and the current
fixtures. Commands in the redevelopment packet refer to the historical root
package; for the independent rebuild use `--manifest-path fresh/Cargo.toml`,
`neo-ssh`, `fresh/install.ps1` and `fresh/target/...` as documented in README.
The original requirements still apply; executable naming does not change scope.

| Example or prerequisite | Verification and scope |
| --- | --- |
| Cargo launch commands and all-target tests | One binary target in Cargo.toml; Args/main routes no target to GUI, target/profile to CLI and --gui overrides. Locked build/check/all-target tests are in the current full validation. Native GUI/CLI launch evidence is in RELEASE_ACCEPTANCE.md. |
| Host/profile/port/log/tmux/jump forms | `.qa/readme-audit-f8cdf47b8cbd41e49becf3b38c39a3f3/results.json` checks ten forms with --help. These are argument checks, not live connections. CLI console/auth/tmux, GUI two-hop and profile fixtures in RELEASE_ACCEPTANCE.md cover their runtime behavior. |
| Standalone SSH fixture block | Fixed missing app build prerequisite: the fixture invokes both session_probe and the debug app for generated-key authentication. Documented Cargo command now builds both with --locked. Full validation separately builds both, runs 48 config comparisons and 101 SSH cases. |
| Key generation example | The README command ran with a unique owned output path and config; generated an Ed25519 keypair and removed it. Evidence: the README audit result above. Existing-key preservation and protected-key prompting are covered separately by keygen/native fixtures. |
| Referenced QA scripts | All 19 paths exist, recorded in the README audit result. Existence is not an execution claim; individual fixture results remain the execution evidence. |
| Windows install, custom destination and fresh-shell PATH | Isolated install/reinstall, locked-target preservation and NoPath in full validation; default install/reinstall and fresh-shell resolution in windows_install_acceptance evidence. |
| Unix prebuilt installation | Native v0.1.139 Linux runs verify literal paths, permissions, replacement, invalid/missing/stale source preservation, failed build preservation, directory/symlink refusal and default HOME installation. A macOS runtime result is still missing. |
| tmux/remote-tool requirements | README documents interactive POSIX shell requirements, scp and fallback tools. Real tmux/Vim/less/htop, missing/failing tmux, same-pane reconnect and detach are recorded in RELEASE_ACCEPTANCE.md. |

`user@host`, profile names, PATH_TO_ZIG and key/output paths are examples to
replace with authorized destinations. This audit did not contact a literal
placeholder host or create the example key in the user's normal SSH directory.
Native key-install acceptance text was corrected to reflect the isolated-account
review/install/reconnect evidence already obtained. Broader production-host
coverage is not implied by those fixtures.

The README remains explicit that parity/release acceptance is incomplete until
remaining release-audit and native macOS requirements are resolved. Overwrite
support and application bundles are documented limitations, not silently claimed
features. Cross-target macOS checking does not establish a runnable Mac build.
