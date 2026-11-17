//! Opt-in real-host file acceptance: TARGET KEY KNOWN_HOSTS LOCAL_EVIDENCE_DIR.
//! Creates and removes only a random /tmp/ssh4-qa-* directory and its own files.
use neo_ssh::{
    connection::AuthMethod,
    files::{Action, Task},
    session::{Connect, Event, Session},
    terminal::Terminal,
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

struct Probe {
    session: Session,
    terminal: Terminal,
}
impl Probe {
    fn drain(&mut self) -> Result<(), String> {
        while let Ok(event) = self.session.try_event() {
            match event {
                Event::Output(bytes) => {
                    let output = self.terminal.feed(&bytes);
                    self.session.reply(output.replies)?;
                }
                Event::Disconnected(error) => return Err(error),
                Event::Trust { .. } | Event::Challenge { .. } => {
                    return Err("Unexpected trust/auth prompt".into())
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn wait<T>(&mut self, task: Task<T>) -> Result<T, String> {
        let deadline = Instant::now() + Duration::from_secs(40);
        loop {
            self.drain()?;
            if let Some(result) = task.poll() {
                return result;
            }
            if Instant::now() >= deadline {
                return Err("File operation deadline exceeded".into());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn action(&mut self, action: Action) -> Result<(), String> {
        let task = self.session.file_action(action)?;
        self.wait(task)
    }
}

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 5 {
        return Err("Expected TARGET KEY KNOWN_HOSTS LOCAL_EVIDENCE_DIR".into());
    }
    let directory = PathBuf::from(&args[4]);
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let directory = directory.canonicalize().map_err(|e| e.to_string())?;
    let session = Session::start(
        Connect {
            target: args[1].parse()?,
            key_path: Some(args[2].clone().into()),
            known_hosts: args[3].clone().into(),
            password: Default::default(),
            strict: true,
            accept_new: false,
            use_agent: false,
            auth: vec![AuthMethod::Key],
            jumps: vec![],
            tmux: None,
            remote_directory: None,
            term: "xterm-256color".into(),
            timeout: Duration::from_secs(15),
            keepalive: Some(Duration::from_secs(5)),
            environment: vec![],
            local_forwards: vec![],
            remote_forwards: vec![],
            dynamic_forwards: vec![],
        },
        80,
        24,
    );
    let mut probe = Probe {
        session,
        terminal: Terminal::new(24, 80, 100),
    };
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match probe.session.recv_event(Duration::from_millis(50)) {
            Ok(Event::Connected) => break,
            Ok(Event::Disconnected(error)) => return Err(error),
            Ok(Event::Trust { .. } | Event::Challenge { .. }) => {
                return Err("Unexpected trust/auth prompt".into())
            }
            _ if Instant::now() >= deadline => return Err("Connection deadline exceeded".into()),
            _ => {}
        }
    }
    let root = format!("/tmp/ssh4-qa-{:032x}", rand::random::<u128>());
    probe.action(Action::CreateDirectory { path: root.clone() })?;
    // Every cleanup target is constructed below this newly created root.
    let mut files = vec![];
    let mut folders = vec![];
    let mut checks = vec![];
    let result = (|| -> Result<(), String> {
        probe.action(Action::SetPermissions {
            path: root.clone(),
            mode: 0o700,
        })?;
        for scp in [false, true] {
            let label = if scp { "scp" } else { "sftp" };
            let folder = format!("{root}/{label}");
            probe.action(Action::CreateDirectory {
                path: folder.clone(),
            })?;
            folders.push(folder.clone());
            for empty in [false, true] {
                let name = format!("quote ' $ ; 文-{empty}.bin");
                let remote = format!("{folder}/{name}");
                let moved = format!("{folder}/moved-{empty}.bin");
                files.extend([remote.clone(), moved.clone()]);
                let source = directory.join(format!("{label}-{empty}.source"));
                let target = directory.join(format!("{label}-{empty}.download"));
                let payload: Vec<u8> = if empty {
                    vec![]
                } else {
                    (0..262_144).map(|n| (n % 251) as u8).collect()
                };
                std::fs::write(&source, &payload).map_err(|e| e.to_string())?;
                let upload = if scp {
                    Action::UploadScp {
                        local: source.clone(),
                        remote: remote.clone(),
                    }
                } else {
                    Action::Upload {
                        local: source.clone(),
                        remote: remote.clone(),
                    }
                };
                probe.action(upload.clone())?;
                if probe.action(upload).is_ok() {
                    return Err("Upload replaced an existing destination".into());
                }
                let listing = probe.wait(probe.session.list_directory(folder.clone())?)?;
                let entry = listing
                    .entries
                    .iter()
                    .find(|e| e.name == name)
                    .ok_or("Uploaded name missing")?;
                if entry.size != Some(payload.len() as u64) {
                    return Err("Incorrect remote size".into());
                }
                probe.action(Action::SetPermissions {
                    path: remote.clone(),
                    mode: 0o640,
                })?;
                let listing = probe.wait(probe.session.list_directory(folder.clone())?)?;
                if !listing
                    .entries
                    .iter()
                    .any(|e| e.name == name && e.permissions.is_some_and(|m| m & 0o777 == 0o640))
                {
                    return Err("Permissions did not change".into());
                }
                let download = if scp {
                    Action::DownloadScp {
                        remote: remote.clone(),
                        local: target.clone(),
                    }
                } else {
                    Action::Download {
                        remote: remote.clone(),
                        local: target.clone(),
                    }
                };
                probe.action(download.clone())?;
                if probe.action(download).is_ok() {
                    return Err("Download replaced existing destination".into());
                }
                if std::fs::read(&target).map_err(|e| e.to_string())? != payload {
                    return Err("Round-trip bytes differ".into());
                }
                probe.action(Action::Rename {
                    from: remote.clone(),
                    to: moved.clone(),
                })?;
                probe.action(Action::CreateFile {
                    path: remote.clone(),
                })?;
                if probe
                    .action(Action::Rename {
                        from: remote.clone(),
                        to: moved.clone(),
                    })
                    .is_ok()
                {
                    return Err("Rename replaced existing destination".into());
                }
                probe.action(Action::RemoveFile { path: remote })?;
                probe.action(Action::RemoveFile { path: moved })?;
                checks.push(format!("{label}, empty={empty}: byte-exact round trip, literal paths, listing, chmod, rename, no-clobber, removal"));
            }
            let listing = probe.wait(probe.session.list_directory(folder.clone())?)?;
            if listing
                .entries
                .iter()
                .any(|e| !matches!(e.name.as_str(), "." | ".."))
            {
                return Err("Remote staging files remain".into());
            }
            probe.action(Action::RemoveDirectory { path: folder })?;
        }
        if std::fs::read_dir(&directory)
            .map_err(|e| e.to_string())?
            .any(|entry| {
                entry.is_ok_and(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .starts_with(".ssh4-download-")
                })
            })
        {
            return Err("Local staging files remain".into());
        }
        Ok(())
    })();
    for path in files {
        let _ = probe.action(Action::RemoveFile { path });
    }
    for path in folders {
        let _ = probe.action(Action::RemoveDirectory { path });
    }
    let cleanup = probe.action(Action::RemoveDirectory { path: root.clone() });
    #[derive(serde::Serialize)]
    struct Report<'a> {
        version: &'a str,
        completed: bool,
        failure: Option<&'a String>,
        checks: Vec<String>,
        remote_root: String,
        remote_root_removed: bool,
        cleanup_error: Option<&'a String>,
    }
    let report = Report {
        version: env!("CARGO_PKG_VERSION"),
        completed: result.is_ok() && cleanup.is_ok(),
        failure: result.as_ref().err(),
        checks,
        remote_root: root,
        remote_root_removed: cleanup.is_ok(),
        cleanup_error: cleanup.as_ref().err(),
    };
    std::fs::write(
        directory.join("results.toml"),
        toml::to_string_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    probe.session.disconnect();
    result?;
    cleanup?;
    println!(
        "PASS live file operations; evidence {}",
        directory.display()
    );
    Ok(())
}
