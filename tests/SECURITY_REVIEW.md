# Security checklist reconciliation - fresh v0.1.141

Reviewed 2026-09-10 against current source and the security release checklist in
`docs/redevelopment/04-validation-and-release.md`. Evidence paths below are relative
to fresh/. This reconciles the specified checklist; it is not a claim of formal
security certification or completed release acceptance. Historical notes below
are superseded only by the explicit evidence in this table.

| Checklist requirement | Current source and authoritative verification |
| --- | --- |
| No local ssh process for core SSH | session.rs uses the SSH library. All Command::new occurrences under fresh/src are inside cfg(test) modules in files/shell.rs, files/scp.rs, files/scp_download.rs and key_install.rs. External OpenSSH is used by QA harnesses, not the application transport. |
| No password/private key in application metadata or crash logs | debug_log::Record accepts only boolean/numeric metadata; diagnostics::install writes location/version/thread/time and omits panic payloads. The native password/KBI result `.qa/linux-runtime-176dd455cd2646cd9f73de0c550e7e4d/results.json` verifies secret exclusion from profiles/app logs in six scenarios. Encrypted direct-key metadata checks: `.qa/linux-runtime-d061d732119f49ee9c2826a3f8ec5c0e/results.json`; encrypted-hop source preservation and secret exclusion: `.qa/linux-runtime-08cd014b6abc42fa8cd95983b7266a8a/results.json`. |
| Diagnostic dump contains output, not local input | diagnostics::terminal_report reads screen cells and recent_output only. Its regression distinguishes local paste from remote bytes. command_line.rs writes raw logs only in Event::Output. Native export checks `.qa/linux-runtime-c54ab7ca7d45473787ffcd6245911364/results.json` verify metadata-only logs, remote output, private permissions, cancellation and collision recovery. |
| Credentials are not persisted | config.rs strips known credential fields recursively from legacy profiles/workspaces and Config::set. Current regression tests cover nested mixed-case sentinels, save/reload and malformed-input errors without source disclosure. Authentication and encrypted-hop native results above verify representative secrets are absent. |
| Temp clipboard image removed after failure | `.qa/linux-runtime-5560f76b0c7046f8955cafe54a94fe5a/results.json` and `.qa/linux-runtime-03e2d56884e446fc88aa37c9852360a6/results.json` cover native SFTP/fallback SCP collision and cancellation cleanup. `.qa/cli-image-20caf1fa234349f7bdf764e0b502b110/results.json` verifies Windows server rejection cleanup, active cancellation, local staging removal and restored console. |
| Dropped-file staging removed after failure | `.qa/linux-runtime-990714a5788a451799fe4f9fba77abbc/results.json` verifies native XDND cancellation/collision staging cleanup and source preservation. GUI path-based drops use the original local file, so there is no separate local staging directory to delete. |
| Destructive remote delete confirms | file_browser.rs submits the reviewed Action only after Confirm delete. `.qa/linux-runtime-bf0c2b7a641244b8b6b059cc73092732/results.json` verifies Cancel preserves the selected file, Confirm deletes only that file, and nonempty-folder failure preserves its contents. |
| Remote shell commands quote paths | File shell/SCP paths use shared literal quoting and validated actions; key_install.rs parses and restricts public-key encoding before command construction. Native Linux command-generation tests and `.qa/linux-runtime-2bef9997297a4680b97c79fcd135508f/results.json` cover automatic SCP and paths with a space/apostrophe. Current all-target and SSH fixtures cover hostile names, command framing, failures and cancellation. |
| Owner-only Unix configuration | config.rs explicitly sets atomic staging permissions to 0600 before replacement. Linux regression verifies replacement of an existing 0644 file; native workspace `.qa/linux-runtime-4b4c833bdc804fe1a844f4008d31b545/results.json` verifies private config and persistence through restart. |
| CLI avoids password arguments | main.rs exposes no password/passphrase-value argument. command_line.rs prompts for secrets, and --protect-key prompts rather than accepting a secret value. Native Windows auth `.qa/cli-auth-8b64f7463d6d4996a87a43d8c0fed724/results.json` covers prompts/cancellation/console restoration; GUI auth evidence is listed above. |

The current full automated baseline is v0.1.140:
`.qa/validation-370a7146e6af437296eab4f93751358c/results.json` (12 gates, 149 app
tests, one backend regression, 48 OpenSSH comparisons, 101 SSH scenarios, no skips).
The v0.1.141 change reconciles documentation only; source behavior is unchanged.

Limits: raw remote output may contain echoed input or secrets printed by the
server. Reports/raw logs explicitly retain remote output and do not promise its
redaction. This checklist covers application-controlled credential handling,
not arbitrary server output. Native Mac execution is still unverified. Existing
passing cases do not imply every possible failure or platform combination was
executed. Optional chooser paths and alternate platform dialogs are additional
coverage unless a concrete required behavior remains unproved.

Reviewed-source hashes and completion checks for all 13 cited result files are
retained in `.qa/security-audit-04f92b144640472f87b1ddf9f26cadce/results.json`.
This record supports traceability; it is not an automated vulnerability scan.

## Historical evidence

Initial v0.1.89 evidence (follow-ups below supersede its pending notes):

- Windows validation: `../.qa/validation-a2701f6ad339478f989ac18f8bc9687d/results.json` (144 app tests, one backend regression, Clippy, release, isolated installer).
- Linux test linking: `../.qa/linux-cross-765bbf7a0794413ebc1236029c1c8e0e/results.json`.
- Linux application build: `../.qa/linux-cross-36c6c13413504874b86846869c986a29/results.json`.
- Linux execution: `../.qa/linux-runtime-ed48b2b1401240c388e1a005ddde2627/results.json` (104 library and 38 application tests; startup/key-generation checks and owned-directory cleanup).

No production logic changed in this version. The two new regression tests strengthen
credential persistence/error coverage. Authentication logging, native diagnostic UI,
clipboard/drop failures and other unverified release requirements remain open.

## Native diagnostics follow-up ? v0.1.90

Native logging enable/stop now passes with a live SSH terminal: connection/counter
records are present and the recognizable terminal marker is absent from the log.
The in-app export form saves the expected remote output with 0600 permissions,
keeps destination input out of SSH, preserves existing reports and recovers to a
new path. Cancel, Escape, close and staging cleanup pass. See
`../.qa/linux-runtime-c54ab7ca7d45473787ffcd6245911364/results.json` and PROGRESS.md.
The optional system chooser and credential-bearing authentication/error scenarios
remain separate acceptance work; this does not close the full security checklist.

## Native clipboard follow-up ? v0.1.91

Clipboard PNG staging is removed after review cancellation, successful upload and
existing-destination collision failure, with native SFTP and automatic SCP fallback.
The 32x16 RGBA payload, 0600 permissions, existing remote destination and owned
cleanup were verified. Evidence is recorded in PROGRESS.md for the two native runs.
File-drop staging and other clipboard interruption/failure scenarios remain open;
these results do not establish cleanup for every possible failure mode.

## Native file-drop follow-up ? v0.1.92

Native XDND drops now verify queue disposal, review cancellation, exact binary
upload and remote staging removal after a confirmed collision with different
source bytes. Sources and the existing destination survive. The fresh path-based
drop handler uploads the original local file; it does not create a separate local
staging directory. Evidence: `../.qa/linux-runtime-990714a5788a451799fe4f9fba77abbc/results.json`.
Other interruption modes and platform drag sources remain separate acceptance.

## Native encrypted-key follow-up - v0.1.99

The native unlock fixture enables metadata logging before cancellation, an
incorrect passphrase and a successful retry. Neither passphrase, key material nor
submitted terminal text appears in that log; authentication/failure/connection
states remain available. The encrypted source bytes and private permissions are
unchanged, configuration contains neither passphrase and all owned resources
are cleaned. Evidence:
`../.qa/linux-runtime-d061d732119f49ee9c2826a3f8ec5c0e/results.json`.
Other authentication methods, jump-hop prompts and platform-specific dialogs
remain separate acceptance work.

## Native encrypted jump-key follow-up - v0.1.105

The two-hop fixture now checks cancellation, incorrect passphrase, successful
retry and a fresh prompt on reconnect for an encrypted second-hop key. Failed
attempts do not start a target terminal session. Encrypted key bytes remain
unchanged; neither entered passphrase appears in saved profile/import settings,
app/debug logs or either hop log. Evidence:
`../.qa/linux-runtime-c354db800c2344318d20fef83331e137/results.json`.
This covers the native encrypted-key hop prompt; other authentication methods
and platform-specific dialogs remain separate acceptance work.
