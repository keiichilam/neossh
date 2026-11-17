//! Isolated crash-hook acceptance probe; expects a temporary config path.
fn main() {
    let path = std::path::PathBuf::from(std::env::args_os().nth(1).expect("temporary config path"));
    neo_ssh::diagnostics::install(&path).unwrap();
    panic!("PANIC_FIXTURE_SECRET_DO_NOT_LOG");
}
