//! Local integration probe, driven by tests/ssh_fixture.py.
use neo_ssh::{
    session::{Connect, Event, Session},
    target::Target,
    terminal::Terminal,
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let port: u16 = args[1].parse().unwrap();
    let path = PathBuf::from(&args[2]);
    let strict = args[3] == "strict";
    let requested_scenario = args.get(4).map(String::as_str).unwrap_or("password");
    let scenario = match requested_scenario {
        "jump-sftp" => "sftp",
        "jump-socks" => "socks",
        value => value,
    };
    let is_shell_files =
        scenario.starts_with("shell-list-") || scenario.starts_with("shell-action-");
    let expect_failure = matches!(
        scenario,
        "reject"
            | "cancel"
            | "encrypted-wrong"
            | "encrypted-cancel"
            | "remote-denied"
            | "agent-cancel"
            | "agent-list-cancel"
            | "jump-changed"
            | "jump-denied"
            | "jump-cancel"
            | "jump-rekey-changed"
    );
    use neo_ssh::connection::AuthMethod;
    let auth = match scenario {
        "interactive" | "cancel" => vec![AuthMethod::Interactive],
        "agent" | "agent-cancel" | "agent-list-cancel" => vec![AuthMethod::Agent],
        "encrypted" | "encrypted-wrong" | "encrypted-cancel" | "key" | "key-ed25519"
        | "key-openssh-rsa" | "key-generated" => vec![AuthMethod::Key],
        "fallback" => vec![
            AuthMethod::Key,
            AuthMethod::Interactive,
            AuthMethod::Password,
        ],
        _ => vec![AuthMethod::Password],
    };
    let mut remote_services = Vec::new();
    if scenario == "remote" {
        for _ in 0..2 {
            remote_services.push(std::net::TcpListener::bind("127.0.0.1:0").unwrap());
        }
    }
    let mut connection = Connect {
        jumps: Vec::new(),
        target: Target {
            user: Some(
                if matches!(scenario, "interactive" | "cancel") {
                    "fixture-kbd"
                } else {
                    "fixture"
                }
                .into(),
            ),
            host: "127.0.0.1".into(),
            port,
        },
        password: Zeroizing::new(
            if scenario == "password-prompt" || scenario == "fallback" {
                ""
            } else {
                "fixture-password"
            }
            .into(),
        ),
        key_path: match scenario {
            "encrypted" | "encrypted-wrong" | "encrypted-cancel" => {
                Some(path.with_file_name("key-encrypted"))
            }
            "fallback" => Some(path.with_file_name("rejected-key")),
            "key" => Some(path.with_file_name("client-key")),
            "key-ed25519" | "key-openssh-rsa" | "key-generated" => {
                Some(path.with_file_name(scenario))
            }
            _ => None,
        },
        known_hosts: path.clone(),
        strict,
        remote_directory: scenario
            .starts_with("restore-")
            .then(|| "/fixture/a'b;$(literal)".into()),
        tmux: (scenario == "restore-tmux").then(|| "daily".into()),
        term: "xterm-256color".into(),
        timeout: Duration::from_secs(5),
        keepalive: None,
        accept_new: args[3] == "accept-new",
        environment: if scenario.starts_with("scp-") || scenario.starts_with("download-scp") {
            vec![
                ("FIXTURE".into(), "yes".into()),
                ("SCP_MODE".into(), scenario.into()),
            ]
        } else if scenario.starts_with("key-install") {
            vec![
                ("FIXTURE".into(), "yes".into()),
                ("KEY_INSTALL".into(), scenario.into()),
            ]
        } else if scenario == "shell-pressure" {
            vec![
                ("FIXTURE".into(), "yes".into()),
                ("SHELL_PRESSURE".into(), "yes".into()),
            ]
        } else if scenario.starts_with("upload") {
            vec![
                ("FIXTURE".into(), "yes".into()),
                ("UPLOAD".into(), scenario.into()),
            ]
        } else if is_shell_files {
            vec![
                ("FIXTURE".into(), "yes".into()),
                ("SHELL_FILES".into(), scenario.into()),
            ]
        } else if scenario == "sftp-unavailable" {
            vec![
                ("FIXTURE".into(), "yes".into()),
                ("NO_SFTP".into(), "yes".into()),
            ]
        } else {
            vec![("FIXTURE".into(), "yes".into())]
        },
        auth,
        use_agent: matches!(scenario, "agent" | "agent-cancel" | "agent-list-cancel"),
        remote_forwards: if scenario == "remote-denied" {
            vec!["192.0.2.1:0:127.0.0.1:1".parse().unwrap()]
        } else {
            remote_services
                .iter()
                .map(|listener| {
                    format!("0:127.0.0.1:{}", listener.local_addr().unwrap().port())
                        .parse()
                        .unwrap()
                })
                .collect()
        },
        dynamic_forwards: if scenario == "socks" {
            vec!["0".parse().unwrap()]
        } else {
            Vec::new()
        },
        local_forwards: if scenario == "forward-half" {
            vec!["0:fixture.local:1236".parse().unwrap()]
        } else if scenario == "forward-stall" {
            vec!["0:fixture.local:1235".parse().unwrap()]
        } else if scenario == "forward" {
            vec!["0:fixture.local:1234".parse().unwrap()]
        } else {
            Vec::new()
        },
    };
    if requested_scenario.starts_with("jump-") {
        connection.target.host = "destination.fixture".into();
        let ports: Vec<u16> = args[5]
            .split(',')
            .map(|value| value.parse().unwrap())
            .collect();
        for (index, port) in ports.into_iter().enumerate() {
            let mut hop = connection.clone();
            hop.jumps.clear();
            hop.target.host = if index == 0 {
                "127.0.0.1"
            } else {
                "hop2.fixture"
            }
            .into();
            hop.target.port = port;
            hop.target.user = Some("jump-user".into());
            hop.password = Zeroizing::new(
                if scenario == "jump-cancel" {
                    ""
                } else {
                    "jump-password"
                }
                .into(),
            );
            hop.auth = vec![neo_ssh::connection::AuthMethod::Password];
            connection.jumps.push(hop);
        }
    }
    let session = Session::start(connection, 80, 24);
    let mut terminal = Terminal::new(24, 80, 100);
    let deadline = Instant::now()
        + Duration::from_secs(if scenario.ends_with("timeout") {
            40
        } else {
            15
        });
    let mut output = Vec::new();
    let mut accepted = false;
    let mut phases = Vec::new();
    let mut prompted = false;
    let mut forward_address: Option<String> = None;
    let mut forward_client = None;
    let mut stalled_client = None;
    let mut remote_addresses = Vec::new();
    let mut agent_cancelled_at = None;
    let mut pressure_started = None;
    let mut pressure_output_seen = false;
    let mut rekey_input_sent = false;
    let mut directory_task: Option<neo_ssh::files::DirectoryTask> = None;
    let mut directory_stage = 0;
    let mut shell_wait_started = None;
    let mut shell_live_seen = false;
    let mut shell_cancel_started = None;
    let mut shell_recovering = false;
    let mut directory_done = !is_shell_files
        && !matches!(
            scenario,
            "sftp" | "sftp-stall" | "sftp-unavailable" | "sftp-actions"
        );
    use neo_ssh::files::Action;
    let mut file_actions = std::collections::VecDeque::from([
        (
            Action::CreateFile {
                path: "/fixture/new empty.txt".into(),
            },
            true,
        ),
        (
            Action::CreateFile {
                path: "/fixture/new empty.txt".into(),
            },
            false,
        ),
        (
            Action::CreateFile {
                path: "/fixture/file-01.txt".into(),
            },
            false,
        ),
        (
            Action::SetPermissions {
                path: "/fixture/file-01.txt".into(),
                mode: 0o640,
            },
            true,
        ),
        (
            Action::SetPermissions {
                path: "/fixture/sub".into(),
                mode: 0o2750,
            },
            true,
        ),
        (
            Action::SetPermissions {
                path: "/fixture/file-02.txt".into(),
                mode: 0o777,
            },
            false,
        ),
        (
            Action::SetPermissions {
                path: "/fixture/file-03.txt".into(),
                mode: 0,
            },
            true,
        ),
        (
            Action::CreateDirectory {
                path: "/fixture/new 文件".into(),
            },
            true,
        ),
        (
            Action::CreateDirectory {
                path: "/fixture/new 文件".into(),
            },
            false,
        ),
        (
            Action::Rename {
                from: "/fixture/report 文件.txt".into(),
                to: "/fixture/new 文件/a ' ; $(echo x).txt".into(),
            },
            true,
        ),
        (
            Action::Rename {
                from: "/fixture/new 文件".into(),
                to: "/fixture/renamed folder".into(),
            },
            true,
        ),
        (
            Action::Rename {
                from: "/fixture/renamed folder/a ' ; $(echo x).txt".into(),
                to: "/fixture/file-01.txt".into(),
            },
            false,
        ),
        (
            Action::RemoveDirectory {
                path: "/fixture/renamed folder".into(),
            },
            false,
        ),
        (
            Action::RemoveFile {
                path: "/fixture/renamed folder/a ' ; $(echo x).txt".into(),
            },
            true,
        ),
        (
            Action::RemoveDirectory {
                path: "/fixture/renamed folder".into(),
            },
            true,
        ),
        (
            Action::RemoveFile {
                path: "/fixture/file-00.txt".into(),
            },
            true,
        ),
    ]);
    let mut action_task: Option<neo_ssh::files::ActionTask> = None;
    let mut action_expected_success = true;
    let is_upload = scenario.starts_with("upload");
    let mut upload_task: Option<neo_ssh::files::ActionTask> = None;
    let mut upload_cancelled = false;
    let upload_path = path.with_file_name(format!("{scenario}-source.bin"));
    let is_download = scenario.starts_with("download");
    let download_path = path.with_file_name(format!("{scenario}.bin"));
    let mut download_task: Option<neo_ssh::files::ActionTask> = None;
    if is_download || is_upload {
        directory_done = false;
    }
    let mut key_install_task: Option<neo_ssh::key_install::Task> = None;
    let mut scp_task: Option<neo_ssh::files::ActionTask> = None;
    let mut scp_cancel_started: Option<Instant> = None;
    if scenario.starts_with("scp-") {
        directory_done = false;
    }
    if scenario.starts_with("key-install") {
        directory_done = false;
    }
    loop {
        assert!(Instant::now() < deadline, "SSH fixture timed out");
        if let Some(result) = scp_task.as_ref().and_then(|task| task.poll()) {
            if scenario == "scp-cancel" {
                assert!(
                    scp_cancel_started.unwrap().elapsed() < Duration::from_secs(1),
                    "SCP cancellation was delayed"
                );
                assert!(
                    result.as_ref().unwrap_err().contains("stopped"),
                    "{result:?}"
                );
            }
            assert_eq!(
                result.is_ok(),
                matches!(scenario, "scp-upload" | "scp-empty" | "scp-fallback"),
                "{result:?}"
            );
            assert_eq!(
                scp_task.as_ref().unwrap().used_scp_fallback(),
                scenario == "scp-fallback"
            );
            scp_task = None;
            directory_done = true;
            session.send(b"filedone\r".to_vec()).unwrap();
        }
        if let Some(result) = key_install_task.as_ref().and_then(|task| task.poll()) {
            assert_eq!(result.is_ok(), scenario == "key-install", "{result:?}");
            key_install_task = None;
            directory_done = true;
            session.send(b"filedone\r".to_vec()).unwrap();
        }
        if scenario == "jump-rekey" && !rekey_input_sent {
            let marker = path.with_file_name("jump-rekey.done");
            if marker.is_file() {
                assert_eq!(std::fs::read_to_string(marker).unwrap(), "ok");
                session.send(b"after-rekey\r".to_vec()).unwrap();
                rekey_input_sent = true;
            }
        }
        if matches!(scenario, "upload-cancel" | "upload-drop")
            && !upload_cancelled
            && upload_task
                .as_ref()
                .is_some_and(|task| task.bytes_transferred() >= 32768)
        {
            upload_cancelled = true;
            if scenario == "upload-drop" {
                upload_task = None;
                directory_task = Some(session.list_directory("/fixture".into()).unwrap());
            } else {
                upload_task.as_ref().unwrap().cancel();
            }
        }
        if let Some(result) = upload_task.as_ref().and_then(|task| task.poll()) {
            if matches!(scenario, "upload" | "upload-empty") {
                result.unwrap();
                let total = if scenario == "upload-empty" {
                    0
                } else {
                    262144
                };
                assert_eq!(upload_task.as_ref().unwrap().bytes_transferred(), total);
                assert_eq!(upload_task.as_ref().unwrap().total_bytes(), Some(total));
            } else {
                let error = result.unwrap_err();
                if scenario == "upload-cleanup-error" {
                    assert!(
                        error.contains("Remote temporary file may remain")
                            && error.contains(".ssh4-upload-"),
                        "{error}"
                    );
                } else {
                    assert!(error.contains("Remote temporary file removed"), "{error}");
                    if scenario == "upload-cancel" {
                        assert!(error.contains("cancelled"), "{error}");
                    } else {
                        assert!(error.contains("SFTP error 4"), "{error}");
                    }
                }
            }
            upload_task = None;
            directory_done = true;
            session.send(b"filedone\r".to_vec()).unwrap();
        }
        if matches!(scenario, "download-cancel" | "download-scp-cancel")
            && download_task
                .as_ref()
                .is_some_and(|task| task.bytes_transferred() >= 32768)
        {
            download_task = None;
            directory_done = true;
            session.send(b"filedone\r".to_vec()).unwrap();
        }
        if let Some(result) = download_task.as_ref().and_then(|task| task.poll()) {
            if matches!(
                scenario,
                "download"
                    | "download-empty"
                    | "download-scp"
                    | "download-scp-empty"
                    | "download-scp-fallback"
            ) {
                result.unwrap();
                let expected: Vec<u8> =
                    if matches!(scenario, "download-empty" | "download-scp-empty") {
                        Vec::new()
                    } else if scenario == "download-scp" {
                        (0..=255u8).cycle().take(4 * 1024 * 1024).collect()
                    } else {
                        (0..=255u8).cycle().take(262144).collect()
                    };
                assert_eq!(std::fs::read(&download_path).unwrap(), expected);
                assert_eq!(
                    download_task.as_ref().unwrap().bytes_transferred(),
                    expected.len() as u64
                );
                assert_eq!(
                    download_task.as_ref().unwrap().total_bytes(),
                    Some(expected.len() as u64)
                );
            } else {
                let error = result.unwrap_err();
                assert!(
                    error.contains("SFTP error 4")
                        || error.contains("SCP")
                        || error.contains("reported size")
                        || error.contains("already exists"),
                    "Unexpected download error: {error}"
                );
            }
            assert_eq!(
                download_task.as_ref().unwrap().used_scp_fallback(),
                scenario == "download-scp-fallback"
            );
            download_task = None;
            directory_done = true;
            session.send(b"filedone\r".to_vec()).unwrap();
        }
        if let Some(result) = action_task.as_ref().and_then(|task| task.poll()) {
            if is_shell_files {
                check_shell_result(result, scenario, shell_wait_started, shell_cancel_started);
                action_task = None;
                shell_recovering = true;
                directory_task = Some(session.list_directory(".".into()).unwrap());
                continue;
            }
            if scenario == "sftp-unavailable" {
                result.unwrap();
                action_task = None;
                directory_done = true;
                session.send(b"filedone\r".to_vec()).unwrap();
                continue;
            }
            assert_eq!(
                result.is_ok(),
                action_expected_success,
                "Unexpected SFTP action result: {result:?}"
            );
            if let Err(error) = result {
                assert!(error.contains("SFTP error 4"));
            }
            action_task = None;
            if let Some((action, expected)) = file_actions.pop_front() {
                action_expected_success = expected;
                action_task = Some(session.file_action(action).unwrap());
            } else {
                directory_task = Some(session.list_directory("/fixture".into()).unwrap());
            }
        }
        if let Some(result) = directory_task.as_ref().and_then(|task| task.poll()) {
            directory_task = None;
            if is_shell_files {
                if shell_recovering {
                    if scenario.ends_with("drop") {
                        assert!(
                            shell_cancel_started.unwrap().elapsed() < Duration::from_secs(2),
                            "Dropped shell task blocked the next request"
                        );
                    }
                    let listing = result.unwrap();
                    assert_eq!(listing.path, "/fixture");
                    assert_eq!(listing.entries.len(), 1);
                    assert_eq!(listing.entries[0].name, "line\nbreak 'name");
                    directory_done = true;
                    session.send(b"filedone\r".to_vec()).unwrap();
                } else {
                    check_shell_result(
                        result.map(|_| ()),
                        scenario,
                        shell_wait_started,
                        shell_cancel_started,
                    );
                    shell_recovering = true;
                    directory_task = Some(session.list_directory(".".into()).unwrap());
                }
                continue;
            }
            if scenario == "upload-drop" {
                let listing = result.unwrap();
                assert!(!listing
                    .entries
                    .iter()
                    .any(|entry| entry.name.starts_with(".ssh4-upload-")
                        || entry.name == "upload 文件.bin"));
                directory_done = true;
                session.send(b"filedone\r".to_vec()).unwrap();
            } else if scenario == "sftp-actions" {
                let listing = result.unwrap();
                assert_eq!(listing.entries.len(), 41);
                let created = listing
                    .entries
                    .iter()
                    .find(|entry| entry.name == "new empty.txt")
                    .unwrap();
                assert_eq!(created.size, Some(0));
                assert_eq!(created.permissions, Some(0o100600));
                for (name, expected) in [
                    ("file-01.txt", 0o100640),
                    ("sub", 0o042750),
                    ("file-02.txt", 0o100600),
                    ("file-03.txt", 0o100000),
                ] {
                    assert_eq!(
                        listing
                            .entries
                            .iter()
                            .find(|entry| entry.name == name)
                            .unwrap()
                            .permissions,
                        Some(expected)
                    );
                }
                assert_eq!(
                    listing
                        .entries
                        .iter()
                        .find(|entry| entry.name == "file-01.txt")
                        .unwrap()
                        .size,
                    Some(1)
                );
                assert!(!listing
                    .entries
                    .iter()
                    .any(|entry| entry.name == "report 文件.txt"
                        || entry.name == "file-00.txt"
                        || entry.name == "renamed folder"));
                directory_done = true;
                session.send(b"filedone\r".to_vec()).unwrap();
            } else if scenario == "sftp-unavailable" {
                let listing = result.unwrap();
                assert_eq!(listing.path, "/fixture");
                assert_eq!(listing.entries.len(), 1);
                assert_eq!(listing.entries[0].name, "line\nbreak 'name");
                assert_eq!(listing.entries[0].size, Some(7));
                action_task = Some(
                    session
                        .file_action(Action::CreateDirectory {
                            path: "/fixture/new".into(),
                        })
                        .unwrap(),
                );
            } else {
                assert_eq!(
                    scenario, "sftp",
                    "Stalled request completed before cancellation"
                );
                match directory_stage {
                    0 => {
                        let listing = result.unwrap();
                        assert_eq!(listing.path, "/fixture");
                        assert_eq!(listing.entries.len(), 42);
                        assert!(listing.entries[0].is_directory());
                        let file = listing
                            .entries
                            .iter()
                            .find(|entry| entry.name == "report 文件.txt")
                            .unwrap();
                        assert_eq!(file.size, Some(123));
                        assert_eq!(file.permissions.unwrap() & 0o777, 0o640);
                        assert_eq!(file.modified, Some(1700000000));
                        directory_task =
                            Some(session.list_directory("/fixture/sub".into()).unwrap());
                    }
                    1 => {
                        assert!(result.unwrap().entries.is_empty());
                        directory_task = Some(session.list_directory("/denied".into()).unwrap());
                    }
                    2 => {
                        assert!(result.unwrap_err().contains("SFTP error 3"));
                        directory_done = true;
                        session.send(b"filedone\r".to_vec()).unwrap();
                    }
                    _ => panic!("Unexpected directory result"),
                }
                directory_stage += 1;
            }
        }
        if matches!(scenario, "agent-cancel" | "agent-list-cancel")
            && agent_cancelled_at.is_none()
            && path.with_file_name("agent-waiting").exists()
        {
            session.disconnect();
            agent_cancelled_at = Some(Instant::now());
        }
        if let Some(started) = agent_cancelled_at {
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "Agent cancellation did not stop the worker"
            );
        }
        match session.recv_event(Duration::from_millis(200)) {
            Ok(Event::Phase(text)) => {
                phases.push(text.clone());
                if let Some(address) = text.strip_prefix("Remote forward listening on ") {
                    remote_addresses.push(address.split(" · ").next().unwrap().to_owned());
                }
                if let Some(address) = text.strip_prefix("SOCKS forward listening on ") {
                    forward_address = Some(address.split(" · ").next().unwrap().to_owned());
                }
                if let Some(address) = text.strip_prefix("Local forward listening on ") {
                    forward_address = Some(address.split(" · ").next().unwrap().to_owned());
                }
            }
            Ok(Event::Trust { answer, .. }) => {
                assert!(!strict && args[3] != "accept-new");
                accepted = true;
                answer.send(true).unwrap();
            }
            Ok(Event::Challenge {
                prompts,
                answer,
                instruction,
                ..
            }) => {
                if scenario == "jump-cancel" {
                    assert!(instruction.contains("jump-user@127.0.0.1"));
                    drop(answer);
                    continue;
                }
                prompted = true;
                if scenario.starts_with("encrypted") {
                    assert_eq!(prompts, vec![("Key passphrase".into(), false)]);
                    if scenario == "encrypted-cancel" {
                        drop(answer);
                    } else {
                        answer
                            .send(vec![if scenario == "encrypted-wrong" {
                                "wrong"
                            } else {
                                "fixture-passphrase"
                            }
                            .into()])
                            .unwrap();
                    }
                    continue;
                }
                if scenario == "cancel" {
                    drop(answer);
                    continue;
                }
                if scenario == "interactive" {
                    assert_eq!(prompts.len(), 2);
                    assert_eq!(prompts[0], ("Account".into(), true));
                    assert_eq!(prompts[1], ("One-time code".into(), false));
                    answer
                        .send(vec!["fixture".into(), "123456".into()])
                        .unwrap();
                } else {
                    assert_eq!(prompts.len(), 1);
                    answer.send(vec!["fixture-password".into()]).unwrap();
                }
            }
            Ok(Event::Connected) => {
                let mut remaining = phases.as_slice();
                for expected in [
                    "SSH version negotiation",
                    "Key exchange",
                    "Host key verification",
                    "SSH authentication request",
                    "PTY request",
                    "Shell open request",
                    "Shell open request sent",
                ] {
                    let index = remaining
                        .iter()
                        .position(|text| text.contains(expected))
                        .unwrap_or_else(|| panic!("Missing ordered phase {expected}: {phases:?}"));
                    remaining = &remaining[index + 1..];
                }

                if scenario.starts_with("scp-") {
                    let local = path.with_file_name("scp-source.bin");
                    let bytes: Vec<u8> = if scenario == "scp-empty" {
                        Vec::new()
                    } else {
                        (0..=255u8).cycle().take(262144).collect()
                    };
                    std::fs::write(&local, bytes).unwrap();
                    scp_task = Some(
                        session
                            .file_action(if scenario == "scp-fallback" {
                                Action::Upload {
                                    local,
                                    remote: "/fixture/result.bin".into(),
                                }
                            } else {
                                neo_ssh::files::Action::UploadScp {
                                    local,
                                    remote: "/fixture/result.bin".into(),
                                }
                            })
                            .unwrap(),
                    );
                }
                if scenario.starts_with("key-install") {
                    let key = ssh_key::PrivateKey::random(
                        &mut rand::rngs::OsRng,
                        ssh_key::Algorithm::Ed25519,
                    )
                    .unwrap();
                    key_install_task = Some(
                        session
                            .install_public_key(
                                neo_ssh::key_install::Key::parse(
                                    &key.public_key().to_openssh().unwrap(),
                                )
                                .unwrap(),
                            )
                            .unwrap(),
                    );
                }
                if scenario == "shell-pressure" {
                    pressure_started = Some(Instant::now());
                    for byte in b'A'..=b'D' {
                        session.send(vec![byte; 1024 * 1024]).unwrap();
                    }
                }
                if is_upload {
                    let payload: Vec<u8> = if scenario == "upload-empty" {
                        Vec::new()
                    } else {
                        (0..=255u8).cycle().take(262144).collect()
                    };
                    std::fs::write(&upload_path, payload).unwrap();
                    upload_task = Some(
                        session
                            .file_action(Action::Upload {
                                local: upload_path.clone(),
                                remote: if scenario == "upload-existing" {
                                    "/fixture/file-01.txt"
                                } else {
                                    "/fixture/upload 文件.bin"
                                }
                                .into(),
                            })
                            .unwrap(),
                    );
                }
                if is_download {
                    if scenario == "download-existing" {
                        std::fs::write(&download_path, b"original file").unwrap();
                    }
                    let remote = match scenario {
                        "download-empty" => "/empty.bin",
                        "download-cancel" => "/stall.bin",
                        "download-error" => "/error.bin",
                        "download-short" => "/short.bin",
                        _ => "/download.bin",
                    };
                    download_task = Some(
                        session
                            .file_action(
                                if scenario.starts_with("download-scp")
                                    && scenario != "download-scp-fallback"
                                {
                                    Action::DownloadScp {
                                        remote: remote.into(),
                                        local: download_path.clone(),
                                    }
                                } else {
                                    Action::Download {
                                        remote: remote.into(),
                                        local: download_path.clone(),
                                    }
                                },
                            )
                            .unwrap(),
                    );
                }
                if scenario == "sftp-actions" {
                    let (action, expected) = file_actions.pop_front().unwrap();
                    action_expected_success = expected;
                    action_task = Some(session.file_action(action).unwrap());
                }
                if is_shell_files {
                    if scenario.starts_with("shell-action-") {
                        action_task = Some(
                            session
                                .file_action(Action::CreateDirectory {
                                    path: "/fixture/new".into(),
                                })
                                .unwrap(),
                        );
                    } else {
                        directory_task = Some(session.list_directory(".".into()).unwrap());
                    }
                }
                if matches!(scenario, "sftp" | "sftp-stall" | "sftp-unavailable") {
                    directory_task = Some(
                        session
                            .list_directory(
                                if scenario == "sftp-stall" {
                                    "/stall"
                                } else {
                                    "."
                                }
                                .into(),
                            )
                            .unwrap(),
                    );
                }
                if scenario == "remote" {
                    assert_eq!(remote_addresses.len(), 2);
                    let addresses = remote_addresses.clone();
                    let services = std::mem::take(&mut remote_services);
                    forward_client = Some(std::thread::spawn(move || {
                        use std::io::{Read, Write};
                        let mut checks = Vec::new();
                        for (index, (address, service)) in
                            addresses.into_iter().zip(services).enumerate()
                        {
                            let server = std::thread::spawn(move || {
                                let (mut socket, _) = service.accept().unwrap();
                                socket
                                    .set_read_timeout(Some(Duration::from_secs(5)))
                                    .unwrap();
                                let mut payload = Vec::new();
                                socket.read_to_end(&mut payload).unwrap();
                                socket.write_all(&[index as u8]).unwrap();
                                socket.write_all(&payload).unwrap();
                            });
                            checks.push(std::thread::spawn(move || {
                                let mut client = std::net::TcpStream::connect(address).unwrap();
                                client
                                    .set_read_timeout(Some(Duration::from_secs(5)))
                                    .unwrap();
                                let payload = vec![index as u8 + 42; 32000];
                                client.write_all(&payload).unwrap();
                                client.shutdown(std::net::Shutdown::Write).unwrap();
                                let mut received = Vec::new();
                                client.read_to_end(&mut received).unwrap();
                                assert_eq!(received[0], index as u8);
                                assert_eq!(&received[1..], payload);
                                server.join().unwrap();
                            }));
                        }
                        for check in checks {
                            check.join().unwrap();
                        }
                    }));
                }
                if scenario == "socks" {
                    let address = forward_address.clone().unwrap();
                    forward_client = Some(std::thread::spawn(move || {
                        use std::io::{Read, Write};
                        let mut client = std::net::TcpStream::connect(address).unwrap();
                        client
                            .set_read_timeout(Some(Duration::from_secs(5)))
                            .unwrap();
                        for byte in [5, 1, 0] {
                            client.write_all(&[byte]).unwrap();
                        }
                        let mut greeting = [0; 2];
                        client.read_exact(&mut greeting).unwrap();
                        assert_eq!(greeting, [5, 0]);
                        let mut request = vec![5, 1, 0, 3, 13];
                        request.extend_from_slice(b"fixture.local");
                        request.extend_from_slice(&1234u16.to_be_bytes());
                        request.extend_from_slice(b"pipelined SOCKS payload");
                        client.write_all(&request).unwrap();
                        client.shutdown(std::net::Shutdown::Write).unwrap();
                        let mut response = [0; 10];
                        client.read_exact(&mut response).unwrap();
                        assert_eq!(response[1], 0);
                        let mut payload = Vec::new();
                        client.read_to_end(&mut payload).unwrap();
                        assert_eq!(payload, b"pipelined SOCKS payload");
                    }));
                }
                if scenario == "forward-half" {
                    let address = forward_address.clone().unwrap();
                    forward_client = Some(std::thread::spawn(move || {
                        use std::io::{Read, Write};
                        let mut client = std::net::TcpStream::connect(address).unwrap();
                        client
                            .set_read_timeout(Some(Duration::from_secs(5)))
                            .unwrap();
                        let mut response = Vec::new();
                        client.read_to_end(&mut response).unwrap();
                        assert_eq!(response, b"READY");
                        client.write_all(b"after remote EOF").unwrap();
                        client.shutdown(std::net::Shutdown::Write).unwrap();
                    }));
                }
                if scenario == "forward-stall" {
                    use std::io::Write;
                    let mut client =
                        std::net::TcpStream::connect(forward_address.as_ref().unwrap()).unwrap();
                    client.write_all(&[42; 8192]).unwrap();
                    stalled_client = Some(client);
                }
                if scenario == "forward" {
                    let address = forward_address
                        .clone()
                        .expect("No forward listener reported");
                    forward_client = Some(std::thread::spawn(move || {
                        use std::io::{Read, Write};
                        let mut client = std::net::TcpStream::connect(&address).unwrap();
                        client
                            .set_read_timeout(Some(Duration::from_secs(5)))
                            .unwrap();
                        let sent = b"forwarded bytes\0\xff".repeat(2048);
                        client.write_all(&sent).unwrap();
                        client.shutdown(std::net::Shutdown::Write).unwrap();
                        let mut received = Vec::new();
                        client.read_to_end(&mut received).unwrap();
                        assert_eq!(received, sent);
                    }));
                }
                assert!(
                    !expect_failure || scenario == "jump-rekey-changed",
                    "Untrusted connection succeeded"
                );
                session.send(b"ping\r".to_vec()).unwrap();
                session.resize(100, 32);
            }
            Ok(Event::Output(bytes)) => {
                output.extend_from_slice(&bytes);
                let replies = terminal.feed(&bytes).replies;
                session.send(replies).unwrap();
                let text = String::from_utf8_lossy(&output);
                if matches!(scenario, "scp-cancel" | "scp-drop")
                    && text.contains("SCP_WAIT")
                    && scp_cancel_started.is_none()
                {
                    scp_cancel_started = Some(Instant::now());
                    assert_eq!(scp_task.as_ref().unwrap().bytes_transferred(), 0);
                    if scenario == "scp-drop" {
                        scp_task = None;
                        directory_done = true;
                        session.send(b"filedone\r".to_vec()).unwrap();
                    } else {
                        scp_task.as_ref().unwrap().cancel();
                    }
                }
                if matches!(scenario, "scp-cancel" | "scp-drop") && text.contains("SCP_CLOSED") {
                    assert!(
                        scp_cancel_started.unwrap().elapsed() < Duration::from_secs(1),
                        "SCP channel was not closed promptly"
                    );
                }
                if scenario == "shell-pressure"
                    && text.contains("STALL_MARKER")
                    && !pressure_output_seen
                {
                    assert!(
                        pressure_started.unwrap().elapsed() < Duration::from_millis(1500),
                        "Queued input starved remote output"
                    );
                    pressure_output_seen = true;
                }
                if is_shell_files
                    && text.contains("SHELL_FILES_WAIT")
                    && shell_wait_started.is_none()
                {
                    shell_wait_started = Some(Instant::now());
                    session.send(b"during-files\r".to_vec()).unwrap();
                }
                if is_shell_files
                    && text.contains("SHELL_FILES_LIVE")
                    && !shell_live_seen
                    && !shell_recovering
                {
                    assert!(
                        shell_wait_started.unwrap().elapsed() < Duration::from_secs(2),
                        "Stalled file request blocked terminal input/output"
                    );
                    shell_live_seen = true;
                    if scenario.ends_with("cancel") {
                        if let Some(task) = &action_task {
                            task.cancel();
                        }
                        if let Some(task) = &directory_task {
                            task.cancel();
                        }
                        shell_cancel_started = Some(Instant::now());
                    } else if scenario.ends_with("drop") {
                        shell_cancel_started = Some(Instant::now());
                        action_task = None;
                        directory_task = Some(session.list_directory(".".into()).unwrap());
                        shell_recovering = true;
                    }
                }
                if scenario == "sftp-stall" && text.contains("SFTP_WAIT") && !directory_done {
                    directory_task = None;
                    directory_done = true;
                    session.send(b"filedone\r".to_vec()).unwrap();
                }
                if text.contains("PONG")
                    && !expect_failure
                    && (!scenario.starts_with("key-install") || text.contains("FILES_DONE"))
                    && (!scenario.starts_with("scp-") || text.contains("FILES_DONE"))
                    && (!matches!(scenario, "scp-cancel" | "scp-drop")
                        || text.contains("SCP_CLOSED"))
                    && (scenario != "jump-rekey" || text.contains("REKEY_DATA_OK"))
                    && (!scenario.starts_with("restore-")
                        || (text.contains("DIRECTORY_OK")
                            && terminal.remote_directory.as_deref()
                                == Some("/fixture/a'b;$(literal)")))
                    && (!(is_download || is_upload) || text.contains("FILES_DONE"))
                    && directory_done
                    && (!is_shell_files || text.contains("FILES_DONE"))
                    && (!is_shell_files
                        || !["cancel", "drop", "timeout"]
                            .iter()
                            .any(|suffix| scenario.ends_with(suffix))
                        || (shell_live_seen && text.contains("SHELL_FILES_CLOSED")))
                    && (!matches!(
                        scenario,
                        "sftp" | "sftp-stall" | "sftp-unavailable" | "sftp-actions"
                    ) || text.contains("FILES_DONE"))
                    && text.contains("CPR_OK")
                    && text.contains("RESIZE_OK")
                    && text.contains("ENV_OK")
                    && (scenario != "forward-half" || text.contains("HALF_OK"))
                {
                    if matches!(scenario, "password-prompt" | "fallback" | "interactive") {
                        assert!(prompted);
                    }
                    assert!(
                        path.is_file(),
                        "Host key not persisted after verified handshake"
                    );
                    if let Some(client) = forward_client.take() {
                        client.join().unwrap();
                    }
                    if stalled_client.is_some() {
                        std::thread::sleep(Duration::from_millis(250));
                    }
                    session.disconnect();
                    let stop_deadline = Instant::now() + Duration::from_secs(2);
                    while !session.is_finished() {
                        assert!(Instant::now() < stop_deadline, "Worker did not stop");
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    if is_download {
                        if scenario == "download-existing" {
                            assert_eq!(std::fs::read(&download_path).unwrap(), b"original file");
                        } else if !matches!(
                            scenario,
                            "download"
                                | "download-empty"
                                | "download-scp"
                                | "download-scp-empty"
                                | "download-scp-fallback"
                        ) {
                            assert!(
                                !download_path.exists(),
                                "Failed download published a destination"
                            );
                        }
                        assert!(
                            !std::fs::read_dir(download_path.parent().unwrap())
                                .unwrap()
                                .any(|entry| entry
                                    .unwrap()
                                    .file_name()
                                    .to_string_lossy()
                                    .starts_with(".ssh4-download-")),
                            "Download staging file survived completion/cancellation"
                        );
                    }
                    if let Some(address) = &forward_address {
                        assert!(
                            std::net::TcpStream::connect(address).is_err(),
                            "Forward listener survived disconnect"
                        );
                    }
                    for address in &remote_addresses {
                        while std::net::TcpStream::connect(address).is_ok() {
                            assert!(
                                Instant::now() < stop_deadline,
                                "Remote listener survived disconnect"
                            );
                            std::thread::sleep(Duration::from_millis(20));
                        }
                    }
                    println!("PASS {requested_scenario}: shell, CPR, input, resize, environment, trust persistence, worker cancellation; trust_prompt={accepted}");
                    break;
                }
            }
            Ok(Event::Disconnected(error)) => {
                if matches!(
                    scenario,
                    "reject"
                        | "cancel"
                        | "encrypted-wrong"
                        | "encrypted-cancel"
                        | "jump-changed"
                        | "jump-denied"
                        | "jump-cancel"
                ) {
                    assert!(
                        !phases.iter().any(|text| text.contains("PTY request")
                            || text.contains("Shell open request")),
                        "Terminal phases after failed setup: {phases:?}"
                    );
                }

                assert!(expect_failure, "Unexpected disconnect: {error}");
                if scenario == "jump-changed" {
                    assert!(error.contains("Host key changed"), "{error}");
                }
                if scenario == "jump-rekey-changed" {
                    assert!(error.contains("Host key changed"), "{error}");
                }
                if scenario == "jump-denied" {
                    assert!(error.contains("Route step 2"), "{error}");
                }
                assert!(
                    scenario == "jump-denied"
                        || scenario == "jump-cancel" && error.to_lowercase().contains("cancel")
                        || scenario == "encrypted-cancel"
                            && error.to_lowercase().contains("cancel")
                        || error.to_lowercase().contains("host")
                        || error.to_lowercase().contains("key")
                        || scenario == "cancel" && error.to_lowercase().contains("cancel")
                        || matches!(scenario, "agent-cancel" | "agent-list-cancel")
                            && agent_cancelled_at.is_some()
                            && error.to_lowercase().contains("cancel")
                        || scenario == "remote-denied"
                            && error.contains("tcpip-forward request rejected"),
                    "Unexpected failure: {error}"
                );
                println!("PASS rejection: {scenario}");
                break;
            }
            Ok(_) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(error) => panic!("Worker channel closed: {error}"),
        }
    }
}

fn check_shell_result(
    result: Result<(), String>,
    scenario: &str,
    started: Option<Instant>,
    cancelled: Option<Instant>,
) {
    if scenario.ends_with("fragmented") {
        result.unwrap();
        return;
    }
    let error = result.expect_err("Invalid shell response was accepted");
    let expected = if scenario.ends_with("cancel") {
        assert!(
            cancelled.unwrap().elapsed() < Duration::from_secs(2),
            "Shell cancellation did not complete promptly"
        );
        "cancelled"
    } else if scenario.ends_with("timeout") {
        let elapsed = started.unwrap().elapsed();
        assert!(
            (Duration::from_secs(28)..Duration::from_secs(34)).contains(&elapsed),
            "Unexpected shell timeout: {elapsed:?}"
        );
        "timed out"
    } else if scenario.ends_with("oversize") {
        "exceeds 8 MiB"
    } else if scenario.ends_with("malformed") {
        "Invalid shell file metadata"
    } else if scenario.ends_with("unexpected") {
        "Unexpected output"
    } else {
        "Shell file operation failed"
    };
    assert!(
        error.contains(expected),
        "Unexpected shell error: {error:?}; wanted {expected:?}"
    );
}
