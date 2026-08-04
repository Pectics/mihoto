# Mihoto

Mihoto is a system-level CLI for installing and operating Mihomo on Linux with
systemd. The current stable product contract is `1.0.1`.

The supported public contract is in [docs/v1-contract.md](docs/v1-contract.md).
It is part of the test suite rather than a best-effort guide.

## Supported systems

Mihoto supports systemd Linux only. The release artifacts are:

- `x86_64-unknown-linux-gnu`
- `x86_64-unknown-linux-musl`
- `aarch64-unknown-linux-gnu`
- `aarch64-unknown-linux-musl`

Release acceptance covers Ubuntu 22.04 and Ubuntu 24.04 on x86_64, plus a
separate native or virtualized aarch64 systemd/TUN host. Containers without the
required privileges, non-systemd Linux, Android, BSD, macOS, and Windows are
not supported.

## Install

The installer selects the latest stable release by default, obtains release
metadata and `SHA256SUMS` from GitHub, checks the archive, runs the staged
binary's version smoke test, and atomically replaces `/usr/local/bin/mihoto`.

```sh
curl -fsSL https://raw.githubusercontent.com/Pectics/mihoto/main/install.sh | sudo sh
```

Install a particular stable release or RC with `--version <semver>`:

```sh
curl -fsSL https://raw.githubusercontent.com/Pectics/mihoto/main/install.sh \
  | sudo sh -s -- --version v1.0.1
```

`--mirror` may proxy the archive download, but it never replaces GitHub release
metadata or the trusted `SHA256SUMS` source. An interrupted or failed install
leaves the previous binary untouched.

## System ownership and root model

Mihoto owns only these system locations:

- Manager config: `/etc/mihoto.toml`
- Manager binary: `/usr/local/bin/mihoto`
- Mihomo binary: `/usr/local/bin/mihomo`
- Mihomo state/config: `/etc/mihomo`
- Service: `/etc/systemd/system/mihomo.service`
- Update units: `/etc/systemd/system/mihoto-update.service` and
  `/etc/systemd/system/mihoto-update.timer`

Use `sudo` for every state-changing command. `status`, `log`, `completions`,
`timer status`, and `upgrade --check` are read-only: they do not create or load
the manager config. No user-level service, user cron, or user-directory
installation is supported.

```sh
sudo mihoto init
sudo mihoto update --all
sudo mihoto apply
sudo mihoto timer enable
mihoto status
mihoto upgrade --check
sudo mihoto upgrade
```

`mihomo.service` runs as root with `CAP_NET_ADMIN` and `CAP_NET_RAW`, which
makes TUN a supported core use case. The controller address comes from the
remote profile unless it is explicitly overridden in `mihoto.toml`; exposing it
beyond loopback must be an explicit configuration choice protected by a secret.

## Configuration, recovery, and removal

`init` downloads the subscription YAML, preserves fields that are not locally
set, and applies Mihoto's sparse TOML overrides. Omitted local fields leave the
remote value untouched; objects merge recursively, while explicitly provided
arrays replace the remote array (including an empty array). The dynamic
collections `proxies`, `proxy-groups`, `rules`, `proxy-providers`,
`rule-providers`, `sub-rules`, and `tunnels` remain subscription-owned.

Common local configuration uses Rust-style `snake_case` and is rendered to
Mihomo's kebab-case YAML keys:

```toml
[mihomo_config.tun]
enable = true
stack = "mixed"
auto_route = true
dns_hijack = ["any:53"]

[mihomo_config.dns]
enable = true
enhanced_mode = "fake-ip"
nameserver = ["1.1.1.1"]
```

Stable non-dynamic fields are strongly typed. Fields not yet modeled can use
the controlled native-key escape hatch, for example
`[mihomo_config.extra]` with a quoted `external-controller-cors` key. Dynamic
keys, duplicate typed keys, unknown TOML fields outside `extra`, and `null` or
environment-variable expansion are not supported. See the [Mihomo support
matrix](docs/mihomo-config-support.md) for the complete boundary.

Mihoto's `ui` setting controls dashboard asset management, while
`mihomo_config.external_ui` controls Mihomo's final `external-ui` path.
When the local path is omitted, Mihoto follows the rendered remote
`external-ui` path for asset placement and otherwise uses `/etc/mihomo/ui`
without injecting an `external-ui` override.
Likewise, `geox_url` and the Geo update fields continue to control Mihoto's
Geo-data downloads; Mihomo-native `external-ui-url` only changes the rendered
Mihomo configuration. Profiles exported by GUI clients may omit DNS and TUN
settings that the GUI injects at runtime; such profiles are not standalone
Mihomo configs. For full-host TUN, provide an explicit working `dns` section
and the required TUN DNS hijack settings.

Mihoto validates staged configuration with the installed Mihomo core before
replacement. After replacement it verifies that the service remains active
without increasing its restart counter; a failed health check atomically
restores the previous config and service. A failed config, core, UI, or geodata
update does not restart the service on partial state.

`mihoto uninstall` stops and removes Mihoto's units while retaining `/etc` data
and binaries for a future reinstall. `mihoto uninstall --purge --yes` removes
only the Mihoto-managed data listed above; it must not modify unrelated units,
networking, firewall settings, or user files.

## Updates and release verification

`mihoto upgrade` selects only a newer stable semantic version. It ignores
pre-releases by default, checks `SHA256SUMS`, validates the archive, smoke-tests
the staged executable, then atomically replaces the running binary. `upgrade
--check` only queries GitHub and never writes local state.

Each release provides four archives, one `SHA256SUMS` file, and a GitHub build
attestation/provenance record. Verify a downloaded archive before installing:

```sh
sha256sum --check SHA256SUMS
gh attestation verify mihoto-v1.0.1-x86_64-unknown-linux-gnu.tar.gz \
  --repo Pectics/mihoto
```

## Migration from Mihoro

Earlier per-user Mihoro installations are not migrated automatically. Back up
any data you need, disable the old user service manually, and remove old user
files before using Mihoto. The old `setup`, `proxy`, and `cron` commands do not
exist in the v1 CLI.

## Development and acceptance

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo check --all-targets
cargo test --all-targets
cargo test --all-targets --no-default-features
scripts/check-system-scope.sh
scripts/check-release-workflow.sh
scripts/check-v1-contract.sh
scripts/test-installer-contract.sh
```

The real systemd/TUN scripts intentionally require an isolated rootful host.
They mutate only Mihoto fixtures under `/etc`, `/usr/local/bin`, and
`/etc/systemd/system`, then check cleanup. A missing root/systemd/TUN
prerequisite is a blocked acceptance result, never a pass.
