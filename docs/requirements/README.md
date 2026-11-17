# Development guide

Last updated: 2026-09-06

The user requested a fresh implementation on 2026-09-06. Active new code lives
in [fresh/](../../fresh/README.md); its [progress ledger](../../fresh/PROGRESS.md)
tracks the complete requirement scope and current evidence separately from
the existing reference app's history. The requirements below still govern it.

This directory is the authoritative development packet for `ssh4`. Read it in
this order:

1. [Current state](00-current-state.md): source layout and existing capabilities.
2. [Requirements](01-product-requirements.md): behavior a rebuild must preserve.
3. [Architecture](02-architecture.md): current ownership and runtime invariants.
4. [Implementation plan](03-implementation-plan.md): milestone acceptance criteria.
5. [Validation and release](04-validation-and-release.md): checks and manual scenarios.
6. [Rebuild status](05-rebuild-status.md): implementation history and remaining verification.

Before development, inspect `git status --short`, preserve existing user changes,
and record baseline check results. Select work from current source evidence and
remaining requirements; the original milestone sequence is not an instruction
to rebuild already implemented features. Record command, date, revision, result,
and any missing live-test environment for each acceptance gate.

The root `01-current-state.md` through `08-handoff-checklist.md` are historical
June 2026 notes. Their `ssh-clip` name, `.ssh-clip.toml` path, `--once` and
`--save-profile` flags, monolithic source layout, and deferred file tools/themes
describe an older product. Current development uses `ssh4`, `~/.ssh4.toml`, and
the flags in `src/cli.rs`. Neither the old migration script nor `HELP.md` nor
`docs/reengineering/README.md` exists in this checkout; they are not current
prerequisites. Historical notes and `backup.doc/` do not override this packet.

Use the commands in [validation](04-validation-and-release.md) directly;
`rtk` is not a prerequisite. File tools, themes, command palette, host-key
verification, agent authentication, forwarding, and jump hosts are existing
parity requirements, not deferred additions.
