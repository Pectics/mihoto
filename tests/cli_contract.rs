use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn mihoto_help_exposes_only_system_commands() {
    Command::cargo_bin("mihoto")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("mihoto"))
        .stdout(predicate::str::contains("timer"))
        .stdout(predicate::str::contains("proxy").not())
        .stdout(predicate::str::contains("cron").not())
        .stdout(predicate::str::contains("setup").not())
        .stdout(predicate::str::contains("/etc/mihoto.toml"));
}

#[test]
fn removed_commands_are_rejected() {
    for command in ["setup", "proxy", "cron"] {
        Command::cargo_bin("mihoto")
            .unwrap()
            .arg(command)
            .assert()
            .failure();
    }
}

#[cfg(unix)]
#[test]
fn read_only_commands_do_not_need_config_or_real_systemd() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let systemctl = dir.path().join("systemctl");
    fs::write(
        &systemctl,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$CALL_LOG\"\nexit 0\n",
    )
    .unwrap();
    fs::set_permissions(&systemctl, fs::Permissions::from_mode(0o755)).unwrap();
    let log = dir.path().join("calls");
    let path = format!(
        "{}:{}",
        dir.path().display(),
        std::env::var("PATH").unwrap()
    );
    for args in [
        vec!["--config", "/missing/mihoto.toml", "status"],
        vec!["--config", "/missing/mihoto.toml", "timer", "status"],
    ] {
        Command::cargo_bin("mihoto")
            .unwrap()
            .args(args)
            .env("PATH", &path)
            .env("CALL_LOG", &log)
            .assert()
            .success();
    }
    let calls = fs::read_to_string(log).unwrap();
    assert!(calls.contains("status mihomo.service"));
    assert!(calls.contains("status mihoto-update.timer"));
    assert!(!calls.contains("--user"));
    Command::cargo_bin("mihoto")
        .unwrap()
        .args(["--config", "/missing/mihoto.toml", "completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("mihoto"));
}

#[cfg(unix)]
#[test]
fn service_commands_use_systemctl_and_completion_shells_render() {
    if unsafe { libc::geteuid() } != 0 {
        return;
    }

    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let systemctl = dir.path().join("systemctl");
    let calls = dir.path().join("calls");
    fs::write(
        &systemctl,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexit 0\n",
            calls.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&systemctl, fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        dir.path().display(),
        std::env::var("PATH").unwrap()
    );

    for command in ["start", "stop", "restart", "status"] {
        Command::cargo_bin("mihoto")
            .unwrap()
            .args(["--config", "/missing/mihoto.toml", command])
            .env("PATH", &path)
            .assert()
            .success();
    }
    for shell in ["zsh", "fish"] {
        Command::cargo_bin("mihoto")
            .unwrap()
            .args(["--config", "/missing/mihoto.toml", "completions", shell])
            .assert()
            .success()
            .stdout(predicate::str::contains("mihoto"));
    }

    let calls = fs::read_to_string(calls).unwrap();
    for command in ["start", "stop", "restart", "status"] {
        assert!(calls.contains(&format!("{command} mihomo.service")));
    }
}

#[cfg(unix)]
#[test]
fn a_fake_id_command_cannot_bypass_the_root_guard() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    if unsafe { libc::geteuid() } == 0 {
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let id = dir.path().join("id");
    let systemctl = dir.path().join("systemctl");
    let calls = dir.path().join("calls");
    fs::write(&id, "#!/bin/sh\nprintf '0\\n'\n").unwrap();
    fs::write(
        &systemctl,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexit 0\n",
            calls.display()
        ),
    )
    .unwrap();
    for program in [&id, &systemctl] {
        fs::set_permissions(program, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = format!(
        "{}:{}",
        dir.path().display(),
        std::env::var("PATH").unwrap()
    );

    Command::cargo_bin("mihoto")
        .unwrap()
        .args(["--config", "/missing/mihoto.toml", "start"])
        .env("PATH", path)
        .assert()
        .failure()
        .stderr(predicate::str::contains("requires root"));
    assert!(!calls.exists());
    assert!(!dir.path().join("mihoto.toml").exists());
}

#[test]
fn timer_requires_an_explicit_subcommand() {
    Command::cargo_bin("mihoto")
        .unwrap()
        .arg("timer")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage"));
}
