//! Embeds the application icon and version details in the Windows executable.
//! The artwork comes from assets/make_icon.py.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/neo-ssh.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon("assets/neo-ssh.ico")
        // Task Manager and Explorer show FileDescription as the program name.
        .set("FileDescription", "NEO-SSH")
        .set("ProductName", "NEO-SSH")
        .set("InternalName", "neo-ssh")
        .set("OriginalFilename", "neo-ssh.exe")
        // Version metadata as in ssh4 / ssh4gui: unsigned executables without
        // VERSIONINFO score worse with antivirus heuristics.
        .set("CompanyName", "Keith Lam")
        .set(
            "LegalCopyright",
            "Copyright (c) 2026 Keith Lam. MIT License.",
        );
    // The icon is cosmetic: a missing resource compiler must not block a build.
    if let Err(error) = resource.compile_for("neo-ssh") {
        println!("cargo:warning=Windows icon not embedded: {error}");
    }
}
