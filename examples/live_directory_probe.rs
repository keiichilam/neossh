//! Live directory restore and OSC 7 acceptance: TARGET KEY KNOWN_HOSTS EVIDENCE_DIR.
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
    connected: bool,
}
impl Probe {
    fn start(connect: Connect) -> Self {
        Self {
            session: Session::start(connect, 120, 32),
            terminal: Terminal::new(32, 120, 100),
            connected: false,
        }
    }
    fn pump(&mut self) -> Result<(), String> {
        match self.session.recv_event(Duration::from_millis(10)) {
            Ok(Event::Connected) => self.connected = true,
            Ok(Event::Output(bytes)) => {
                let output = self.terminal.feed(&bytes);
                self.session.reply(output.replies)?;
            }
            Ok(Event::Disconnected(error)) => return Err(error),
            Ok(Event::Trust { .. } | Event::Challenge { .. }) => {
                return Err("Unexpected trust/auth prompt".into())
            }
            _ => {}
        }
        Ok(())
    }
    fn until(&mut self, done: impl Fn(&Self) -> bool) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !done(self) {
            self.pump()?;
            if Instant::now() >= deadline {
                return Err("Live directory check timed out".into());
            }
        }
        Ok(())
    }
    fn wait<T>(&mut self, task: Task<T>) -> Result<T, String> {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            self.pump()?;
            if let Some(result) = task.poll() {
                return result;
            }
            if Instant::now() >= deadline {
                return Err("File operation timed out".into());
            }
        }
    }
    fn action(&mut self, action: Action) -> Result<(), String> {
        self.wait(self.session.file_action(action)?)
    }
    fn report_directory(&mut self) -> Result<String, String> {
        self.terminal.remote_directory = None;
        self.session.send(b"python3 -c 'import os,urllib.parse; print(\"\\033]7;file://localhost\"+urllib.parse.quote(os.getcwd())+\"\\007\",end=\"\",flush=True)'\r".to_vec())?;
        self.until(|probe| probe.terminal.remote_directory.is_some())?;
        Ok(self.terminal.remote_directory.clone().unwrap())
    }
    fn assert_directory(&mut self, path: &str, marker: &str) -> Result<(), String> {
        self.session.send(
            format!(
                "[ \"$PWD\" = {} ] && printf '__DIR_%s__\\n' {marker}\r",
                quote(path)
            )
            .into_bytes(),
        )?;
        self.until(|probe| {
            probe
                .terminal
                .screen()
                .contents()
                .contains(&format!("__DIR_{marker}__"))
        })
    }
}
fn quote(path: &str) -> String {
    format!("'{}'", path.replace('\'', "'\\''"))
}

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 5 {
        return Err("Expected TARGET KEY KNOWN_HOSTS EVIDENCE_DIR".into());
    }
    let evidence = PathBuf::from(&args[4]);
    std::fs::create_dir_all(&evidence).map_err(|e| e.to_string())?;
    let connection = Connect {
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
    };
    let mut setup = Probe::start(connection.clone());
    setup.until(|probe| probe.connected)?;
    let root = format!("/tmp/ssh4-dir-{:032x}", rand::random::<u128>());
    let child = format!("{root}/space 'quote';$(literal) 文");
    let name = format!("ssh4qa_{:032x}", rand::random::<u128>());
    setup.action(Action::CreateDirectory { path: root.clone() })?;
    let mut tmux = None;
    let mut checks = vec![];
    let result = (|| -> Result<(), String> {
        setup.action(Action::SetPermissions {
            path: root.clone(),
            mode: 0o700,
        })?;
        setup.action(Action::CreateDirectory {
            path: child.clone(),
        })?;
        let home = setup.report_directory()?;
        let mut restored = connection.clone();
        restored.remote_directory = Some(child.clone());
        let mut plain = Probe::start(restored.clone());
        plain.until(|probe| probe.connected)?;
        if plain.report_directory()? != child {
            return Err("Restored directory differs from literal path".into());
        }
        plain.session.disconnect();
        checks.push("literal directory restore and percent-encoded OSC 7".to_owned());

        let mut invalid = connection.clone();
        invalid.remote_directory = Some(format!("{root}/missing"));
        let mut missing = Probe::start(invalid);
        missing.until(|probe| probe.connected)?;
        if missing.report_directory()? != home {
            return Err("Missing directory did not leave the login shell usable at home".into());
        }
        missing.session.disconnect();
        checks.push("missing directory leaves login shell usable".into());

        // Hide executables only in this disposable shell. Exercise the same
        // startup bytes used by Session without changing the host installation.
        let mut unavailable = Probe::start(connection.clone());
        unavailable.until(|probe| probe.connected)?;
        unavailable.session.send(
            format!(
                "NEO_SSH_QA_OLD_PATH=$PATH; unset -f tmux command_not_found_handle; unalias tmux 2>/dev/null; PATH={}; hash -r; command -v tmux >/dev/null 2>&1; printf '__TMUX_ABSENT_%s__\\n' \"$?\"\r",
                quote(&child)
            )
            .into_bytes(),
        )?;
        unavailable.until(|probe| {
            probe
                .terminal
                .screen()
                .contents()
                .contains("__TMUX_ABSENT_1__")
        })?;
        unavailable
            .session
            .send(neo_ssh::remote_directory::startup(None, Some(&name))?)?;
        unavailable
            .session
            .send(b"printf '__TMUX_STATUS_%s__\\n' \"$?\"\r".to_vec())?;
        unavailable.until(|probe| {
            let text = probe.terminal.screen().contents();
            text.contains("tmux: command not found") && text.contains("__TMUX_STATUS_127__")
        })?;
        unavailable.session.send(
            b"PATH=$NEO_SSH_QA_OLD_PATH; export PATH; unset NEO_SSH_QA_OLD_PATH\r".to_vec(),
        )?;
        if unavailable.report_directory()? != home {
            return Err("Unavailable tmux did not leave the login shell usable".into());
        }
        unavailable.session.disconnect();
        checks.push("unavailable tmux shows command-not-found/status 127 and preserves usable shell (isolated PATH)".into());

        restored.tmux = Some(name.clone());
        tmux = Some(Probe::start(restored));
        let pane = tmux.as_mut().unwrap();
        pane.until(|probe| probe.terminal.screen().alternate_screen())?;
        pane.assert_directory(&child, "NEW")?;
        checks.push("new tmux session starts in restored directory".into());
        let mut attach = connection.clone();
        attach.tmux = Some(name.clone());
        attach.remote_directory = Some(root.clone());
        let mut peer = Probe::start(attach);
        peer.until(|probe| probe.terminal.screen().alternate_screen())?;
        peer.assert_directory(&child, "ATTACHED")?;
        peer.session.disconnect();
        checks.push("reattach does not change the existing pane directory".into());
        Ok(())
    })();
    let tmux_cleanup = if let Some(pane) = &mut tmux {
        let result = pane
            .session
            .send(format!("tmux kill-session -t {name}\r").into_bytes())
            .and_then(|()| pane.until(|probe| !probe.terminal.screen().alternate_screen()));
        pane.session.disconnect();
        result
    } else {
        Ok(())
    };
    let _ = setup.action(Action::RemoveDirectory { path: child });
    let cleanup = setup.action(Action::RemoveDirectory { path: root.clone() });
    setup.session.disconnect();
    #[derive(serde::Serialize)]
    struct Report<'a> {
        version: &'a str,
        completed: bool,
        checks: Vec<String>,
        failure: Option<&'a String>,
        remote_root: String,
        tmux_session: String,
        remote_root_removed: bool,
        tmux_cleanup_passed: bool,
    }
    let report = Report {
        version: env!("CARGO_PKG_VERSION"),
        completed: result.is_ok() && cleanup.is_ok() && tmux_cleanup.is_ok(),
        checks,
        failure: result.as_ref().err(),
        remote_root: root,
        tmux_session: name,
        remote_root_removed: cleanup.is_ok(),
        tmux_cleanup_passed: tmux_cleanup.is_ok(),
    };
    std::fs::write(
        evidence.join("results.toml"),
        toml::to_string_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    result?;
    tmux_cleanup?;
    cleanup?;
    println!(
        "PASS live directory restore; evidence {}",
        evidence.display()
    );
    Ok(())
}
