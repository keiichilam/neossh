# Repository Guidelines

## Project Structure & Module Organization

NEO-SSH is a Rust 2021 SSH workspace with egui GUI and crossterm CLI front ends. `src/lib.rs` exposes the shared `neo_ssh` engine; `src/main.rs`, `desktop.rs`, and `command_line.rs` provide application entry points. File backends live in `src/files/`; platform agent implementations live in `src/agent/`.

`tests/` contains Python/PowerShell acceptance fixtures and audits; `examples/` contains diagnostic probes. Consult `docs/REFERENCE.md`, `docs/requirements/`, and `FEATURES.md` for behavior and architecture. `vendor/` contains patched dependencies: audit local changes before replacing them. Generated outputs belong in ignored `target/` or `.qa/`.

## Build, Test, and Development Commands

- `cargo run --locked`: launch the desktop workspace.
- `cargo run --locked -- user@host`: start a CLI connection to an authorized host.
- `cargo build --release --locked`: build the optimized executable.
- `cargo fmt --check`: verify Rust formatting; use `cargo fmt` to apply it.
- `cargo clippy --all-targets -- -D warnings`: lint all targets, treating warnings as errors.
- `cargo test --all-targets`: run Rust tests and example harnesses.
- `powershell -File tests/validate.ps1`: run full automated gates, including SSH fixtures, release build, and isolated installer smoke checks.

## Coding Style & Naming Conventions

Follow rustfmt defaults: four-space indentation, `snake_case` modules/functions, `PascalCase` types, and `SCREAMING_SNAKE_CASE` constants. Keep shared protocol logic in the library. Network I/O belongs on worker threads communicating through channels; never block the egui thread.

## Testing Guidelines

Place Rust unit tests in module-local `#[cfg(test)]` blocks with descriptive behavior-based names. Add regressions for protocol and security changes. Python SSH fixtures use Paramiko; install it with `python -m pip install --target .qa/deps paramiko` before full validation. Fixtures must use loopback or explicitly authorized hosts. No numeric coverage threshold is documented. Record native GUI acceptance separately from automated results.

## Commit & Pull Request Guidelines

Git history is unavailable in this checkout. Use concise, imperative commit subjects describing one focused change. PRs should describe behavior changes, link relevant issues, report checks and limitations, and include screenshots for visible GUI changes.

## Security & Configuration

Configuration lives in `~/.neo-ssh.toml`. Never persist or log secrets, accept command-line passwords, or bypass changed-host-key rejection. Preserve transfer collision protection and explicit remote-edit conflict checks.
