# Source review issues

Reviewed on 2026-11-16. This checkout has no Git metadata, so these are findings
against the current source; their introducing commits could not be determined.
No application source was changed.

## 1. [P1] Preserve source files changed during a cross-volume move

**Location:** [src/local_ops.rs:402-407](src/local_ops.rs#L402-L407)

When another process edits a source file after its contents have been copied,
`move_by_copy` still unconditionally deletes that source. The copy bookkeeping
records only paths and directory flags, so it cannot distinguish the copied
version from newer contents or a replacement file. A successful move can therefore
discard the only copy of the newer data. Verify that each source still matches
what was copied before removing it, and preserve changed sources with an explicit
error.

**Reproduction:** The targeted probe invokes the existing cross-volume helper and
uses its byte-progress callback to simulate a concurrent writer. Starting with
`OLD`, the callback saves `NEW` after the last copied chunk. The move succeeds,
the destination contains `OLD`, and the source containing `NEW` no longer exists.

## 2. [P2] Apply configured forwards to unsaved GUI connections

**Location:** [src/desktop.rs:2531-2535](src/desktop.rs#L2531-L2535)

For a new, unsaved connection, `resolve_draft` passes no profile to the resolver,
discarding all three forwarding lists in `draft.options`. Unlike the other
advanced options, `resolve_connection` does not apply these lists afterward.
Consequently, listeners entered in Connection options, or supplied with
`--gui -L`, `--gui -R`, or `--gui -D` without a saved profile, are silently omitted
even though they remain visible in the form. Apply these explicit forward lists
independently of whether the connection has been saved.

**Reproduction:** Resolve a draft containing one local, remote, and dynamic
forward with no imported forwards. With `saved_profile = false`, the resulting
counts are `0/0/0`; changing only `saved_profile` to `true` produces `1/1/1`.

## 3. [P2] Preserve inherited forwards when saving CLI profiles

**Location:** [src/command_line.rs:304-311](src/command_line.rs#L304-L311)

When an alias inherits forwards from `~/.ssh/config` and a CLI flag adds another
forward, the active connection uses both. However, `profile_to_save` stores only
the base profile's lists plus the CLI arguments, omitting the inherited entries.
On the next `-P` connection, each nonempty saved list replaces its imported list
in `connection::resolve_single`, so previously active listeners disappear. Save
the effective forward configuration or retain an explicit additive representation
that preserves the inherited listeners.

**Reproduction:** Import `LocalForward 8080 localhost:80`, then resolve
`neo-ssh probe.invalid -L 8081:localhost:81 --save-profile probe`. The initial
connection has two local forwards, but the saved profile and its subsequent
resolution have only one. The same save logic handles remote and dynamic lists.

## Validation and limitations

- `cargo fmt --check`: passed.
- `cargo clippy --locked --all-targets -- -D warnings`: passed.
- `cargo test --all-targets --locked`: passed; 149 library tests and 69 binary
  tests, with no example-test failures.
- Targeted Rust probes reproduced all three findings using verbatim copies of
  the relevant functions and the built library. Probe source and observed output:
  [.qa/source-review-709e62d19152492ab6ae668d866791f1/](.qa/source-review-709e62d19152492ab6ae668d866791f1/).
  Run the retained `review_probe.exe` with `NEO_REVIEW_DIR` set to that directory
  to repeat the probes; all probe file operations stay inside its temporary folder.
- Native GUI acceptance, full SSH fixtures, installer checks, and native
  Linux/macOS execution were not run. No external SSH host was contacted.

## Resolution (2026-11-16)

All three findings were confirmed against the source and fixed; each new test
fails with the reported symptom when its fix is removed.

1. **Fixed.** Moves record the length and SHA-256 of every file they copy
   (`local_ops::Copier`). Before removal, `remove_unchanged` re-reads the
   original through a handle that does not follow links and keeps it, with an
   error naming it, unless it still matches. On Windows that handle refuses
   writers and deletes the verified file itself; on Unix the path must still
   name the verified file. Test:
   `cross_volume_move_keeps_originals_that_change_after_their_copy`.
2. **Fixed.** `connection::apply_forwards` gives a form's forward lists the
   precedence of a saved profile; `desktop::resolve_form` applies it whether
   or not the connection is saved. Test:
   `form_forwards_apply_whether_or_not_the_connection_is_saved`.
3. **Fixed.** `profile_to_save` (via `save_forwards`) saves the imported
   forwards that were active ahead of command-line additions when the saved
   list of that kind was empty, and skips equivalent duplicates. Forwards now
   implement `Display` with round-trippable specs. Tests:
   `saved_profile_keeps_inherited_forwards_of_every_kind`, the corrected
   `saved_profile_keeps_alias_and_pins_resolved_destination`, and
   `displayed_specs_parse_back_to_the_same_forward`.

## Follow-up review (2026-11-17)

Two further review passes over `src/`, plus a copy/paste and hotkey audit.
Each finding was checked against the code before fixing; each fix has a test.

Security and correctness:

- Bracketed paste dropped nothing, so pasted text containing `ESC[201~`
  could end the paste and run the rest. ESC and ^C are now removed inside
  bracketed paste (`Terminal::paste`).
- The CLI multiline-paste prompt printed raw clipboard text, so escape
  sequences could hide what was sent. Both the CLI and GUI reviews now use
  `terminal::paste_preview`, which escapes control and bidirectional-formatting
  characters.
- "Require a known host key" covered only the destination. It now covers every
  ProxyJump hop (`connection::require_known_host_keys`).
- A known host presenting a key type not yet recorded was treated like a new
  host, so `accept-new` trusted it silently. It is now `Status::NewKeyType`: it
  always prompts with a warning, and strict mode rejects it.
- Install key and Copy install command appended a bare copy of a key already
  present with options (`restrict`, `command=`), lifting its restrictions.
  Both now match the key type and data anywhere in an entry.
- An SFTP download whose server reported neither type nor size was unbounded.
- Cross-volume moves stopped at read-only originals. They now remove them, as
  a rename within a volume does.
- A profile holding both `key_path` and `identity_file` made the whole
  configuration (or the first-run legacy import) fail to parse.
- An edit upload whose result was uncertain was retried against a stale
  fingerprint, giving a false conflict. Syncing now stops with the uncertainty
  reported (`Task::outcome_uncertain`).
- Plain SCP reported failure when output followed its completion marker, even
  though the file was published.
- On Windows, an unreachable `SSH_AUTH_SOCK` (such as a Git Bash socket) now
  falls back to the OpenSSH pipe and Pageant.
- The remote-edit folder's current-user ACL now passes to files created inside
  it. Web pages and SVG are opened with the **edit** verb, not a browser.
- The shell fallback created folders as 0700 and listed entries unsorted; it
  now matches SFTP (755, folders first).

Keyboard (GUI): Ctrl+X was consumed as Cut and never reached the terminal (fixed in
the egui-winit patch). Ctrl+N/K/F/P were app shortcuts, which took
readline/nano/emacs keys; app commands moved to Ctrl+Shift+T/P/F/I. Ctrl+[ ]
\ / _ and Ctrl+Space were dropped and now send their control codes. egui's
unsaved keyboard zoom (which also took Ctrl+_) is replaced by Ctrl+= / - / 0
for terminal text size. CLI: clipboard image moved from Ctrl+P to
Ctrl+Shift+I, and Ctrl+V passes through outside Windows.

Known limitation: on Unix, a program that already has a file open can still
write to it in the moment between a move's verification and its removal (no
mandatory locks); Windows closes this window.
