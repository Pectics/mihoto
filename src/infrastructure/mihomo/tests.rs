use super::*;
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

fn fake_systemctl(dir: &Path, query_exit: i32) -> PathBuf {
    let program = dir.join("systemctl");
    let log = dir.join("systemctl.log");
    fs::write(
			&program,
			format!(
				"#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\ncase \"$1\" in\n  is-active|is-enabled) exit {query_exit} ;;\n  show) printf '0\\n'; exit 0 ;;\n  *) exit 0 ;;\nesac\n",
				log.display()
			),
		)
		.unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    program
}

fn fake_mihomo(dir: &Path, validation_exit: i32) -> PathBuf {
    let program = dir.join("mihomo");
    let log = dir.join("mihomo.log");
    fs::write(
			&program,
			format!(
				"#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\ncase \"$1\" in\n  -t) exit {validation_exit} ;;\n  *) exit 0 ;;\nesac\n",
				log.display()
			),
		)
		.unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    program
}

fn fake_unhealthy_then_recovered_systemctl(dir: &Path) -> PathBuf {
    let program = dir.join("systemctl-health");
    let log = dir.join("systemctl-health.log");
    let state = dir.join("systemctl-health.state");
    fs::write(
			&program,
			format!(
				"#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nif [ \"$1\" = show ]; then printf '0\\n'; exit 0; fi\nif [ \"$1\" = is-active ]; then\n  count=$(cat '{}' 2>/dev/null || printf '0')\n  count=$((count + 1))\n  printf '%s' \"$count\" > '{}'\n  [ \"$count\" -gt 1 ]\n  exit\nfi\nexit 0\n",
				log.display(),
				state.display(),
				state.display()
			),
		)
		.unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    program
}

fn fake_restart_loop_then_recovered_systemctl(dir: &Path) -> PathBuf {
    let program = dir.join("systemctl-restarts");
    let log = dir.join("systemctl-restarts.log");
    let state = dir.join("systemctl-restarts.state");
    fs::write(
			&program,
			format!(
				"#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\ncase \"$1\" in\n  is-active) exit 0 ;;\n  show)\n    count=$(cat '{}' 2>/dev/null || printf '0')\n    count=$((count + 1))\n    printf '%s' \"$count\" > '{}'\n    case \"$count\" in\n      1) printf '0\\n' ;;\n      *) printf '4\\n' ;;\n    esac\n    exit 0\n    ;;\n  *) exit 0 ;;\nesac\n",
				log.display(),
				state.display(),
				state.display()
			),
		)
		.unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    program
}

#[test]
fn system_service_has_tun_capabilities() {
    let unit = render_service_string("/usr/local/bin/mihomo", "/etc/mihomo");
    for value in [
        "User=root",
        "Wants=network-online.target",
        "After=network-online.target",
        "CapabilityBoundingSet=CAP_NET_ADMIN",
        "AmbientCapabilities=CAP_NET_ADMIN",
        "ExecStart=/usr/local/bin/mihomo -d /etc/mihomo",
        "ExecReload=",
        "WantedBy=multi-user.target",
    ] {
        assert!(unit.contains(value));
    }
    for forbidden in [
        format!("{}{}", "--", "user"),
        format!("{}{}", "default", ".target"),
        format!("{}/{}", "systemd", "user"),
        format!("{}/{}", "~", ".config"),
    ] {
        assert!(!unit.contains(&forbidden));
    }

    let escaped = render_service_string("/opt/mihomo core", "/etc/mihomo config");
    assert!(escaped.contains("ExecStart=\"/opt/mihomo core\" -d \"/etc/mihomo config\""));
}

#[tokio::test]
async fn invalid_remote_config_does_not_replace_existing_config() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/config.yaml"))
        .respond_with(ResponseTemplate::new(200).set_body_string("tun: [invalid"))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let original = "tun:\n  enable: true\nrules:\n  - MATCH,DIRECT\n";
    let config_path = dir.path().join("config.yaml");
    fs::write(&config_path, original).unwrap();

    let config = Config {
        remote_config_url: format!("{}/config.yaml", server.uri()),
        mihomo_config_root: dir.path().display().to_string(),
        ..Config::default()
    };
    let mihoto = Mihoto::from_config(config);
    assert!(mihoto.update_config(&Client::new()).await.is_err());
    assert_eq!(fs::read_to_string(config_path).unwrap(), original);
}

#[tokio::test]
async fn semantically_invalid_remote_config_does_not_replace_existing_config() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/config.yaml"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "proxies:\n  - name: broken\n    type: invalid\nrules:\n  - MATCH,broken\n",
        ))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let original = "tun:\n  enable: true\nrules:\n  - MATCH,DIRECT\n";
    let config_path = dir.path().join("config.yaml");
    fs::write(&config_path, original).unwrap();
    let mihomo_binary = fake_mihomo(dir.path(), 1);

    let config = Config {
        remote_config_url: format!("{}/config.yaml", server.uri()),
        mihomo_binary_path: mihomo_binary.display().to_string(),
        mihomo_config_root: dir.path().display().to_string(),
        ..Config::default()
    };
    let mihoto = Mihoto::from_config(config);

    assert!(mihoto.update_config(&Client::new()).await.is_err());
    assert_eq!(fs::read_to_string(config_path).unwrap(), original);
    let calls = fs::read_to_string(dir.path().join("mihomo.log")).unwrap();
    assert!(calls.contains("-t"));
    assert!(calls.contains("-d"));
    assert!(calls.contains("-f"));
}

#[tokio::test]
async fn unhealthy_restart_restores_previous_config() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/config.yaml"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("tun:\n  enable: true\nrules:\n  - MATCH,DIRECT\n"),
        )
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let original = "tun:\n  enable: false\nrules:\n  - MATCH,DIRECT\n";
    let config_path = dir.path().join("config.yaml");
    fs::write(&config_path, original).unwrap();
    let mihomo_binary = fake_mihomo(dir.path(), 0);
    let systemctl = fake_unhealthy_then_recovered_systemctl(dir.path());
    let config = Config {
        remote_config_url: format!("{}/config.yaml", server.uri()),
        mihomo_binary_path: mihomo_binary.display().to_string(),
        mihomo_config_root: dir.path().display().to_string(),
        ..Config::default()
    };
    let mihoto = Mihoto::from_config(config);

    assert!(mihoto
        .update_config_and_restart_with_program(
            &Client::new(),
            &systemctl,
            std::time::Duration::ZERO,
        )
        .await
        .is_err());
    assert_eq!(fs::read_to_string(config_path).unwrap(), original);
    let calls = fs::read_to_string(dir.path().join("systemctl-health.log")).unwrap();
    assert!(calls.contains("stop mihomo.service"));
    assert!(calls.contains("reset-failed mihomo.service"));
    assert_eq!(calls.matches("restart mihomo.service").count(), 2);
}

#[tokio::test]
async fn healthy_restart_commits_updated_config() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/config.yaml"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("tun:\n  enable: true\nrules:\n  - MATCH,DIRECT\n"),
        )
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let original = "tun:\n  enable: false\nrules:\n  - MATCH,DIRECT\n";
    let config_path = dir.path().join("config.yaml");
    fs::write(&config_path, original).unwrap();
    let mihomo_binary = fake_mihomo(dir.path(), 0);
    let systemctl = fake_systemctl(dir.path(), 0);
    let config = Config {
        remote_config_url: format!("{}/config.yaml", server.uri()),
        mihomo_binary_path: mihomo_binary.display().to_string(),
        mihomo_config_root: dir.path().display().to_string(),
        ..Config::default()
    };
    let mihoto = Mihoto::from_config(config);

    assert!(matches!(
        mihoto
            .update_config_and_restart_with_program(
                &Client::new(),
                &systemctl,
                std::time::Duration::ZERO,
            )
            .await
            .unwrap(),
        StageStatus::Installed
    ));
    assert_ne!(fs::read_to_string(config_path).unwrap(), original);
    let calls = fs::read_to_string(dir.path().join("systemctl.log")).unwrap();
    assert_eq!(calls.matches("restart mihomo.service").count(), 1);
    assert!(!calls.contains("stop mihomo.service"));
    assert!(!calls.contains("reset-failed mihomo.service"));
}

#[tokio::test]
async fn restart_counter_growth_rolls_back_even_while_service_is_active() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/config.yaml"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("tun:\n  enable: true\nrules:\n  - MATCH,DIRECT\n"),
        )
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let original = "tun:\n  enable: false\nrules:\n  - MATCH,DIRECT\n";
    let config_path = dir.path().join("config.yaml");
    fs::write(&config_path, original).unwrap();
    let mihomo_binary = fake_mihomo(dir.path(), 0);
    let systemctl = fake_restart_loop_then_recovered_systemctl(dir.path());
    let config = Config {
        remote_config_url: format!("{}/config.yaml", server.uri()),
        mihomo_binary_path: mihomo_binary.display().to_string(),
        mihomo_config_root: dir.path().display().to_string(),
        ..Config::default()
    };
    let mihoto = Mihoto::from_config(config);

    assert!(mihoto
        .update_config_and_restart_with_program(
            &Client::new(),
            &systemctl,
            std::time::Duration::ZERO,
        )
        .await
        .is_err());
    assert_eq!(fs::read_to_string(config_path).unwrap(), original);
    let calls = fs::read_to_string(dir.path().join("systemctl-restarts.log")).unwrap();
    assert!(calls.contains("reset-failed mihomo.service"));
}

#[tokio::test]
async fn updated_remote_config_is_private() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/config.yaml"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("tun:\n  enable: true\nrules:\n  - MATCH,DIRECT\n"),
        )
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let mihomo_binary = fake_mihomo(dir.path(), 0);
    let config = Config {
        remote_config_url: format!("{}/config.yaml", server.uri()),
        mihomo_binary_path: mihomo_binary.display().to_string(),
        mihomo_config_root: dir.path().display().to_string(),
        ..Config::default()
    };
    let mihoto = Mihoto::from_config(config);
    mihoto.update_config(&Client::new()).await.unwrap();
    let mode = fs::metadata(dir.path().join("config.yaml"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn semantically_invalid_override_does_not_replace_existing_config() {
    let dir = tempfile::tempdir().unwrap();
    let original = "tun:\n  enable: true\nrules:\n  - MATCH,DIRECT\n";
    let config_path = dir.path().join("config.yaml");
    fs::write(&config_path, original).unwrap();
    let mihomo_binary = fake_mihomo(dir.path(), 1);
    let mut config = Config {
        mihomo_binary_path: mihomo_binary.display().to_string(),
        mihomo_config_root: dir.path().display().to_string(),
        ..Config::default()
    };
    config.mihomo_config.mixed_port = Some(17890);
    let mihoto = Mihoto::from_config(config);

    assert!(mihoto.apply_existing_config_atomically().is_err());
    assert_eq!(fs::read_to_string(config_path).unwrap(), original);
}

#[tokio::test]
async fn invalid_gzip_does_not_leave_a_binary() {
    use std::io::Write;

    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("mihomo");
    let config = Config {
        mihomo_binary_path: target.display().to_string(),
        mihomo_config_root: dir.path().join("config").display().to_string(),
        ..Config::default()
    };
    let mihoto = Mihoto::from_config(config);
    let mut staged = NamedTempFile::new().unwrap();
    staged.write_all(b"not a gzip stream").unwrap();

    assert!(mihoto.install_binary(staged).await.is_err());
    assert!(!target.exists());
}

#[tokio::test]
async fn unchanged_service_has_its_permissions_repaired() {
    let dir = tempfile::tempdir().unwrap();
    let service_path = dir.path().join("mihomo.service");
    let config = Config {
        mihomo_binary_path: "/usr/local/bin/mihomo".into(),
        mihomo_config_root: "/etc/mihomo".into(),
        ..Config::default()
    };
    let mut mihoto = Mihoto::from_config(config);
    mihoto.mihomo_target_service_path = service_path.display().to_string();
    fs::write(
        &service_path,
        render_service_string(
            &mihoto.mihomo_target_binary_path,
            &mihoto.mihomo_target_config_root,
        ),
    )
    .unwrap();
    fs::set_permissions(&service_path, fs::Permissions::from_mode(0o600)).unwrap();

    assert!(matches!(
        mihoto.ensure_service().await.unwrap(),
        StageStatus::Skipped(_)
    ));
    assert_eq!(
        fs::metadata(service_path).unwrap().permissions().mode() & 0o777,
        0o644
    );
}

#[test]
fn uninstall_disables_timer_first_and_retains_data_without_purge() {
    let dir = tempfile::tempdir().unwrap();
    let manager_config = dir.path().join("mihoto.toml");
    let config_root = dir.path().join("mihomo");
    let mihomo_binary = dir.path().join("mihomo-bin");
    let mihoto_binary = dir.path().join("mihoto-bin");
    let service = dir.path().join("mihomo.service");
    let timer_service = dir.path().join("mihoto-update.service");
    let timer = dir.path().join("mihoto-update.timer");
    fs::create_dir(&config_root).unwrap();
    for path in [
        &manager_config,
        &mihomo_binary,
        &mihoto_binary,
        &service,
        &timer_service,
        &timer,
    ] {
        fs::write(path, "fixture").unwrap();
    }
    let systemctl = fake_systemctl(dir.path(), 0);
    let config = Config {
        mihoto_binary_path: mihoto_binary.display().to_string(),
        mihomo_binary_path: mihomo_binary.display().to_string(),
        mihomo_config_root: config_root.display().to_string(),
        ..Config::default()
    };
    let mut mihoto = Mihoto::from_config(config);
    mihoto.mihomo_target_service_path = service.display().to_string();

    mihoto
        .uninstall_with_paths(false, &manager_config, &timer_service, &timer, &systemctl)
        .unwrap();

    assert!(!service.exists());
    assert!(!timer_service.exists());
    assert!(!timer.exists());
    assert!(manager_config.exists());
    assert!(config_root.exists());
    assert!(mihomo_binary.exists());
    assert!(mihoto_binary.exists());
    let calls = fs::read_to_string(dir.path().join("systemctl.log")).unwrap();
    let timer_disable = calls.find("disable --now mihoto-update.timer").unwrap();
    let service_disable = calls.find("disable --now mihomo.service").unwrap();
    assert!(timer_disable < service_disable);
    assert!(
        !calls.lines().any(|call| call == "reset-failed"),
        "uninstall must not reset unrelated failed units"
    );
}

#[test]
fn purge_removes_all_managed_system_data_and_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let manager_config = dir.path().join("mihoto.toml");
    let config_root = dir.path().join("mihomo");
    let mihomo_binary = dir.path().join("mihomo-bin");
    let mihoto_binary = dir.path().join("mihoto-bin");
    let service = dir.path().join("mihomo.service");
    let timer_service = dir.path().join("mihoto-update.service");
    let timer = dir.path().join("mihoto-update.timer");
    fs::create_dir(&config_root).unwrap();
    for path in [
        &manager_config,
        &mihomo_binary,
        &mihoto_binary,
        &service,
        &timer_service,
        &timer,
    ] {
        fs::write(path, "fixture").unwrap();
    }
    let systemctl = fake_systemctl(dir.path(), 1);
    let config = Config {
        mihoto_binary_path: mihoto_binary.display().to_string(),
        mihomo_binary_path: mihomo_binary.display().to_string(),
        mihomo_config_root: config_root.display().to_string(),
        ..Config::default()
    };
    let mut mihoto = Mihoto::from_config(config);
    mihoto.mihomo_target_service_path = service.display().to_string();

    for _ in 0..2 {
        mihoto
            .uninstall_with_paths(true, &manager_config, &timer_service, &timer, &systemctl)
            .unwrap();
    }

    for path in [
        &manager_config,
        &config_root,
        &mihomo_binary,
        &mihoto_binary,
        &service,
        &timer_service,
        &timer,
    ] {
        assert!(!path.exists(), "{} should be removed", path.display());
    }
}

#[test]
fn purge_rejects_dangerous_directory_targets() {
    let dir = tempfile::tempdir().unwrap();
    let systemctl = fake_systemctl(dir.path(), 1);
    let config = Config {
        mihomo_config_root: "/".into(),
        ..Config::default()
    };
    let mihoto = Mihoto::from_config(config);
    assert!(mihoto
        .uninstall_with_paths(
            true,
            &dir.path().join("mihoto.toml"),
            &dir.path().join("update.service"),
            &dir.path().join("update.timer"),
            &systemctl,
        )
        .is_err());
}

#[test]
fn purge_rejects_targets_that_do_not_look_mihoto_managed() {
    for (label, target) in [
        ("mihomo config root", "/home/pectics"),
        ("mihomo config root", "/var/lib"),
        ("mihomo binary", "/usr/bin/bash"),
        ("mihoto binary", "/usr/bin/curl"),
        ("manager config", "/etc/passwd"),
    ] {
        assert!(
            validate_safe_purge_target(label, Path::new(target)).is_err(),
            "{target} must not be accepted as a {label} purge target"
        );
    }
}
