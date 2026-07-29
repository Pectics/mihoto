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
