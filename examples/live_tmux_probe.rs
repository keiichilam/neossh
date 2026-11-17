//! Opt-in live tmux engine acceptance: TARGET KEY KNOWN_HOSTS EVIDENCE_DIR.
//! Creates one random tmux session and removes it after checking real PTY sizes.
use neo_ssh::{
    connection::AuthMethod,
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
    fn pump(&mut self) -> Result<(), String> {
        match self.session.recv_event(Duration::from_millis(20)) {
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
    fn until(&mut self, predicate: impl Fn(&Terminal) -> bool) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !predicate(&self.terminal) {
            self.pump()?;
            if Instant::now() >= deadline {
                return Err("Terminal acceptance deadline exceeded".into());
            }
        }
        Ok(())
    }
}

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 5 {
        return Err("Expected TARGET KEY KNOWN_HOSTS EVIDENCE_DIR".into());
    }
    let directory = PathBuf::from(&args[4]);
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let name = format!("ssh4qa_{:032x}", rand::random::<u128>());
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
        tmux: Some(name.clone()),
        remote_directory: None,
        term: "xterm-256color".into(),
        timeout: Duration::from_secs(15),
        keepalive: Some(Duration::from_secs(5)),
        environment: vec![],
        local_forwards: vec![],
        remote_forwards: vec![],
        dynamic_forwards: vec![],
    };
    let session = Session::start(connection.clone(), 80, 24);
    let mut probe = Probe {
        session,
        terminal: Terminal::new(24, 80, 100),
    };
    let mut sizes = vec![];
    let mut bracketed_paste_verified = false;
    let mut second = None;
    let mut shared_output_verified = false;
    let mut remaining_client_verified = false;
    let result = (|| -> Result<(), String> {
        probe.until(|t| t.screen().alternate_screen())?;
        // Change only this generated session, leaving user/default tmux options alone.
        probe
            .session
            .send(format!("tmux set-option -t {name} status off\r").into_bytes())?;
        for (columns, rows) in [(100, 32), (120, 40), (90, 28)] {
            probe.terminal.resize(rows, columns);
            probe.session.resize(columns, rows);
            let marker = format!("__SSH4_SIZE_{rows}_{columns}__");
            let deadline = Instant::now() + Duration::from_secs(15);
            let mut next_query = Instant::now();
            loop {
                if Instant::now() >= next_query {
                    probe
                        .session
                        .send(b"printf '__SSH4_SIZE_%s_%s__\\n' $(stty size)\r".to_vec())?;
                    next_query = Instant::now() + Duration::from_millis(300);
                }
                probe.pump()?;
                if probe.terminal.screen().contents().contains(&marker) {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err(format!("Remote tmux PTY did not become {columns}x{rows}"));
                }
            }
            if !probe.terminal.screen().alternate_screen() {
                return Err("tmux alternate screen was lost".into());
            }
            sizes.push(format!("{columns}x{rows}"));
        }
        probe.until(|t| t.modes.bracketed_paste)?;
        let paste = "printf '__PASTE_%s__\\n' 'one; $(literal) 文'\nprintf '__PASTE_%s__\\n' 'two'";
        probe.session.send(probe.terminal.paste(paste))?;
        let before_enter = Instant::now() + Duration::from_millis(300);
        while Instant::now() < before_enter {
            probe.pump()?;
        }
        let output = probe.terminal.screen().contents();
        if output.contains("__PASTE_one;") || output.contains("__PASTE_two__") {
            return Err("Bracketed paste executed before Enter".into());
        }
        probe.session.send(vec![b'\r'])?;
        probe.until(|t| {
            let output = t.screen().contents();
            output.contains("__PASTE_one; $(literal) 文__") && output.contains("__PASTE_two__")
        })?;
        bracketed_paste_verified = true;
        second = Some(Probe {
            session: Session::start(connection, 90, 28),
            terminal: Terminal::new(28, 90, 100),
        });
        let peer = second.as_mut().unwrap();
        peer.until(|t| t.screen().alternate_screen())?;
        peer.session.send(
            b"printf '__CLIENTS_%s__\\n' \"$(tmux display-message -p '#{session_attached}')\"\r"
                .to_vec(),
        )?;
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            probe.pump()?;
            peer.pump()?;
            if [&probe.terminal, &peer.terminal]
                .iter()
                .all(|t| t.screen().contents().contains("__CLIENTS_2__"))
            {
                break;
            }
            if Instant::now() >= deadline {
                return Err("Both tmux clients did not receive shared output".into());
            }
        }
        shared_output_verified = true;
        probe.session.disconnect();
        // Wait for tmux to observe the first transport closing, while proving
        // the other client's shell still accepts commands and produces output.
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut next_query = Instant::now();
        loop {
            if Instant::now() >= next_query {
                peer.session.send(
                    b"printf '__REMAINING_%s__\\n' \"$(tmux display-message -p '#{session_attached}')\"\r"
                        .to_vec(),
                )?;
                next_query = Instant::now() + Duration::from_millis(300);
            }
            peer.pump()?;
            if peer
                .terminal
                .screen()
                .contents()
                .contains("__REMAINING_1__")
            {
                break;
            }
            if Instant::now() >= deadline {
                return Err("Remaining tmux client did not stay usable after disconnect".into());
            }
        }
        remaining_client_verified = true;
        Ok(())
    })();
    let cleanup_probe = second.as_mut().unwrap_or(&mut probe);
    let cleanup = cleanup_probe
        .session
        .send(format!("tmux kill-session -t {name}\r").into_bytes())
        .and_then(|()| cleanup_probe.until(|t| !t.screen().alternate_screen()));
    if let Some(peer) = &second {
        peer.session.disconnect();
    }
    probe.session.disconnect();
    #[derive(serde::Serialize)]
    struct Report<'a> {
        version: &'a str,
        completed: bool,
        failure: Option<&'a String>,
        tmux_session: String,
        verified_sizes: Vec<String>,
        bracketed_paste_verified: bool,
        shared_output_verified: bool,
        remaining_client_verified: bool,
        cleanup_returned_to_login_shell: bool,
        cleanup_error: Option<&'a String>,
        limitation: &'a str,
    }
    let report = Report {
        version: env!("CARGO_PKG_VERSION"),
        completed: result.is_ok() && cleanup.is_ok(),
        failure: result.as_ref().err(),
        tmux_session: name,
        verified_sizes: sizes,
        bracketed_paste_verified,
        shared_output_verified,
        remaining_client_verified,
        cleanup_returned_to_login_shell: cleanup.is_ok(),
        cleanup_error: cleanup.as_ref().err(),
        limitation:
            "Engine-level live tmux resize; native window resizing remains separate acceptance",
    };
    std::fs::write(
        directory.join("results.toml"),
        toml::to_string_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    result?;
    cleanup?;
    println!(
        "PASS live tmux engine resizing; evidence {}",
        directory.display()
    );
    Ok(())
}
