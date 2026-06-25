use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[test]
#[ignore = "requires MIHOTO_RUN_SYSTEMD_TESTS=1 and a systemd-capable environment"]
fn systemd_backend_privileged_smoke() {
    if env::var("MIHOTO_RUN_SYSTEMD_TESTS").as_deref() != Ok("1") {
        eprintln!("set MIHOTO_RUN_SYSTEMD_TESTS=1 to run the systemd backend smoke test");
        return;
    }

    assert!(
        Command::new("systemctl")
            .arg("--version")
            .status()
            .expect("systemctl must be executable")
            .success(),
        "systemctl --version must succeed"
    );

    let bin = env!("CARGO_BIN_EXE_mihoro");
    let root = env::temp_dir().join(format!("mihoro-systemd-test-{}", std::process::id()));
    let config_path = root.join("mihoro.toml");
    fs::create_dir_all(&root).expect("create temp test root");
    write_test_config(&config_path, &root);

    assert_success(Command::new(bin).arg("--help"));
    assert_success(
        Command::new(bin)
            .arg("--mihoro-config")
            .arg(&config_path)
            .args(["deploy", "apply", "--backend", "systemd-user", "--dry-run"]),
    );
    assert_success(
        Command::new(bin)
            .arg("--mihoro-config")
            .arg(&config_path)
            .args(["schedule", "status"]),
    );
}

fn write_test_config(config_path: &PathBuf, root: &std::path::Path) {
    let config = format!(
        r#"
remote_config_url = "https://example.com/sub.yaml"
active_profile = "default"
profile_config_root = "{root}/config"
profile_data_root = "{root}/data"
profile_state_root = "{root}/state"
ui = "metacubexd"
mihomo_channel = "stable"
mihomo_binary_path = "{root}/bin/mihomo"
mihomo_config_root = "{root}/mihomo"
user_systemd_root = "{root}/systemd/user"
mihoro_user_agent = "mihoro"
auto_update_interval = 12

[deployment]
backend = "systemd-user"

[scheduler]
backend = "systemd-timer"
on_calendar = "0/12:00:00"
persistent = true
randomized_delay_sec = "15min"

[mihomo_config]
port = 7891
socks_port = 7892
mixed_port = 7890
allow_lan = false
bind_address = "*"
mode = "rule"
log_level = "info"
ipv6 = true
external_controller = "127.0.0.1:9090"
external_ui = "ui"
geodata_mode = false
geo_auto_update = true
geo_update_interval = 24
"#,
        root = root.display()
    );
    fs::write(config_path, config).expect("write test config");
}

fn assert_success(command: &mut Command) {
    let output = command.output().expect("command must run");
    assert!(
        output.status.success(),
        "command failed with {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
