# mihoto

`mihoto` is a Linux CLI for managing Mihomo as a system service. Version 0.15 uses an explicit root model and system-wide paths.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/Pectics/mihoto/main/install.sh | sudo sh
sudo mihoto init
```

The installer places `mihoto` at `/usr/local/bin/mihoto`. The default manager configuration is `/etc/mihoto.toml`; Mihomo and its configuration live at `/usr/local/bin/mihomo` and `/etc/mihomo`.

## Commands

```text
init, update, apply
start, status, stop, restart, log
timer enable|disable|status
uninstall [--purge]
completions, upgrade
```

State-changing commands require explicit root execution, normally with `sudo`. Read-only status, log, completions, and `upgrade --check` operations do not require the manager configuration to exist.

## TUN and systemd

`mihomo.service` is installed to `/etc/systemd/system/mihomo.service`, runs as root, and receives `CAP_NET_ADMIN` and `CAP_NET_RAW`. Unknown subscription YAML fields—including `tun`, `dns`, listeners, proxies, groups, and rules—are preserved when local overrides are applied.

Automatic updates use `/etc/systemd/system/mihoto-update.service` and `mihoto-update.timer`. `auto_update_interval` accepts 0 through 24; zero disables timer installation.

## Testing

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo check --all-targets
cargo test --all-targets
scripts/check-system-scope.sh
```

Real systemd/TUN acceptance is intentionally opt-in and must run in an isolated, rootful Linux VM. See `scripts/test-systemd-integration.sh`, `scripts/test-tun-integration.sh`, and the manual `systemd-integration` workflow.

## Migration from versions before 0.15

Old per-user installations are not migrated automatically. Back up any old user configuration and disable the old user service manually before initializing 0.15. The removed setup, shell-export, and user-scheduler interfaces are intentionally not accepted by the new CLI.
