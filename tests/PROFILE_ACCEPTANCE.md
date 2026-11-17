# Advanced profile acceptance reconciliation

Scope: SSH-011 and the connection controls in the product requirements and
release matrix. Native editing, persistence, resolver behavior and live SSH
behavior are separate evidence layers. Result directories are under `.qa/`.

| Setting | Implementation and evidence |
| --- | --- |
| Strict host key mode | Connection-options checkbox binds to the draft; native workspace fixture toggles it and checks saved/restored value. Known/unknown/changed/revoked policies are exercised by the SSH fixture, including the full v0.1.119 run with 101 SSH scenarios. Native routed changed-key rejection is recorded at v0.1.106. |
| Agent on/off and auth order | Native workspace fixture enables agent fallback with key,agent and checks saved/restored values. `connection::tests::profile_options_and_auth_restrictions_survive_resolution` verifies disabled-agent and ordered auth mapping. Native Pageant authentication is recorded at v0.1.33; GUI password and keyboard-interactive methods at v0.1.123. The workspace fixture authenticates by key; it does not claim to exercise agent fallback failure. |
| TERM and remote environment | Native workspace fixture enters vt220 and changes SSH4_RESTORE from before to first; remote PTY/session records verify both before and after restart, alongside a second profile using vt100 and a different environment value. Resolver regression also verifies multiple environment assignments and embedded equals. |
| Connect timeout and keepalive | Native workspace fixture edits 12 to 13 seconds and 17 to 18 seconds, checks saved/restored values, and keeps a distinct second profile. `connection.rs` resolves nonzero per-profile values; `desktop.rs` applies current edits; `session.rs` enforces connection timing and schedules transport keepalive. Native persistence does not measure heartbeat timing. |
| Tmux session | Native workspace fixture edits daily, saves/restores and records the startup command. Real tmux attach/reconnect, state retention, resize, detach and TUI behavior are recorded in RELEASE_ACCEPTANCE.md. |
| Jump hosts and L/R/SOCKS forwarding | Native controls and saved options plus real relays, failure recovery and cleanup are covered at v0.1.102 and v0.1.106. |
| Remote directory | `remote_directory.rs` validates and quotes literal POSIX paths and avoids changing an existing tmux pane. Real shell directory reporting/startup restoration and missing-directory recovery are recorded at v0.1.31. This is a workspace capability, beyond the seven SSH-011 fields. |
| Retry delay | Native countdown, cancellation, manual retry and explicit-disconnect suppression are recorded at v0.1.93. |
| Saved name and schema compatibility | Native profile CRUD/repeated-save/restart cases are recorded at v0.1.39-49. Config tests verify both schemas, preservation of extensions, transactional save/delete and credential stripping. |

The native workspace result is
`.qa/linux-runtime-8cb11bf2056745f7bdde190610dd09dd/results.json` at v0.1.133.
The clear numeric-control screenshot is from the preceding run
`linux-runtime-0da79e3dd739438bb607bc1d21037fba/workspace-advanced-edited.png`,
which passed edits/persistence but stopped on a stale expected TERM assertion.
The corresponding capture in the final run is blank and is not used as visual
evidence; final saved/restored assertions and SSH records pass. These checks cover the required behavior without claiming every
possible profile combination or platform interaction. Final requirements/security
reconciliation and macOS runtime acceptance remain separate work.
