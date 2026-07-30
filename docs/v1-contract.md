# Mihoto v1 product contract

This document freezes the public 1.x boundary. The corresponding static
assertion is `scripts/check-v1-contract.sh`; behavior beyond this document is
not implicitly part of the compatibility promise.

## Scope and support

Mihoto is a root-owned systemd Linux manager for Mihomo. It supports TUN as a
core use case, not as an experimental integration. The release targets are:

- `x86_64-unknown-linux-gnu`
- `x86_64-unknown-linux-musl`
- `aarch64-unknown-linux-gnu`
- `aarch64-unknown-linux-musl`

Ubuntu 22.04 and Ubuntu 24.04 are the required x86_64 acceptance hosts. At
least one native or virtualized aarch64 systemd Linux host must pass install,
service startup, and TUN acceptance before a stable release. QEMU version
detection alone is not acceptance evidence.

Non-systemd Linux, unprivileged containers, Android, BSD, macOS, and Windows
are not supported. i686 is not a v1 target.

## Public interface and ownership

The public commands are `init`, `update`, `apply`, `start`, `status`, `stop`,
`restart`, `log`, `timer`, `uninstall`, `completions`, and `upgrade`. Existing
flags, defaults, exits, and root behavior are compatibility commitments for
1.x. The global config flag defaults to `/etc/mihoto.toml`.

Persistent Mihoto state is root-owned:

| Purpose | Location |
| --- | --- |
| Manager config | `/etc/mihoto.toml` |
| Mihoto binary | `/usr/local/bin/mihoto` |
| Mihomo binary | `/usr/local/bin/mihomo` |
| Mihomo configuration and data | `/etc/mihomo` |
| Mihomo service | `/etc/systemd/system/mihomo.service` |
| Update service and timer | `/etc/systemd/system/mihoto-update.service`, `/etc/systemd/system/mihoto-update.timer` |

State-changing commands require root. `status`, `log`, shell completions,
`timer status`, and `upgrade --check` are read-only and must neither create
state nor require a manager configuration. User-level systemd, user cron,
`~/.config`, `~/.local`, and old user-state migration are outside the product.

## Lifecycle and failure behavior

Service and timer start/stop/enable/disable operations are idempotent, including
already-absent units. `uninstall` removes Mihoto-managed runtime units only;
`uninstall --purge` is the explicit destructive operation for Mihoto/Mihomo
persistent data. Both paths must leave unrelated units, user files, network
configuration, firewall rules, and software untouched.

Config and component updates use temporary files and replacement only after the
download and validation stage succeeds. On invalid YAML, network interruption,
failed download, failed binary extraction, or failed service start, the last
working configuration and binary remain in place.

## Distribution and updates

The installer accepts `--version <semver>` for a stable release or RC; its
default is the latest stable release. GitHub release metadata and `SHA256SUMS`
are the trust source even when `--mirror` proxies an archive. Downloaded files
are checked, the staged binary runs `--version`, and `/usr/local/bin/mihoto` is
replaced atomically.

Self-upgrade compares semantic versions after normalizing `v` prefixes. It
selects only stable releases by default, verifies `SHA256SUMS`, accepts exactly
the target archive, smoke-tests the replacement, and leaves the current binary
unchanged on any failure. `upgrade --check` has no write path.

Every release contains the four named archives, `SHA256SUMS`, and a GitHub
attestation/provenance record. A release job can consume only artifacts that
passed the local gates, target smoke tests, and real x86_64 plus aarch64
systemd/TUN gates.

## Migration and compatibility

Mihoro user installations are not migrated automatically. Migration is manual:
back up the user data you choose to keep, disable the old user service, clean
the old files, then initialize Mihoto. This intentional boundary is incompatible
with the pre-system-level product, while names, paths, fields, and uninstall
semantics above form the 1.x commitment.
