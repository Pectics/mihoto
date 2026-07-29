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
uninstall [--purge --yes]
completions, upgrade
```

State-changing commands require explicit root execution, normally with `sudo`. Read-only status, log, completions, and `upgrade --check` operations do not require the manager configuration to exist.

## TUN and systemd

`mihomo.service` is installed to `/etc/systemd/system/mihomo.service`, runs as root, and receives `CAP_NET_ADMIN` and `CAP_NET_RAW`. The default controller listens on `127.0.0.1:9090`. Binding it to a non-loopback address should be an explicit choice and should be paired with a secret. Unknown subscription YAML fields—including `tun`, `dns`, listeners, proxies, groups, and rules—are preserved when local overrides are applied.

Automatic updates use `/etc/systemd/system/mihoto-update.service` and `mihoto-update.timer`. `auto_update_interval` accepts 0 through 24; zero disables timer installation.

## Testing

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo check --all-targets
cargo test --all-targets
scripts/check-system-scope.sh
```

Real systemd/TUN acceptance is intentionally opt-in and should run on a dedicated, rootful Linux host. Both scripts create and remove files under `/etc`, `/usr/local/bin`, and `/etc/systemd/system`, and refuse to overwrite an existing mihoto installation. `test-tun-integration.sh` additionally requires `MIHOTO_REAL_MIHOMO` to identify a pinned real Mihomo binary. Its automatic-route/firewall check is restricted to the benchmarking range `198.18.0.0/15`; it snapshots and verifies restoration of routes and policy rules. The self-hosted workflow expects the pinned fixture at `/usr/local/libexec/mihoto-test/mihomo` and fails before mutation when it is absent. Missing prerequisites are reported as blocked with a non-zero exit status; they are never treated as a passing test.

## Migration from versions before 0.15

Old per-user installations are not migrated automatically. Back up any old user configuration and disable the old user service manually before initializing 0.15. The removed `setup`, `proxy`, and `cron` interfaces are intentionally not accepted by the new CLI. Normal uninstall removes system units but retains manager configuration, Mihomo, and its data; `uninstall --purge --yes` removes the complete managed system installation.
