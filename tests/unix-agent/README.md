# Unix SSH-agent checks

This small harness compiles the actual agent module independently of the GUI.
On Linux, run:

```sh
cargo test --manifest-path fresh/tests/unix-agent/Cargo.toml --locked
```

It includes live Unix-socket connection, nonblocking reads, missing endpoints,
full listener backlog timeout/cancellation, and shared agent RPC tests.

From Windows, with the Linux Rust standard library installed, compile all tests:

```powershell
cargo check --manifest-path fresh/tests/unix-agent/Cargo.toml --locked --target x86_64-unknown-linux-gnu --tests
```

Cross-compilation does not execute the Linux socket tests.
