# Mihomo local configuration support matrix

Mihoto treats the downloaded subscription as the source of truth and applies a
sparse local overlay from `[mihomo_config]`. This document records the schema
boundary implemented in the current branch.

## Schema baseline

The baseline is Mihomo stable `v1.19.28`, checked against its `RawConfig` and
the stable configuration documentation on 2026-08-04:

- [Mihomo v1.19.28 `config/config.go`](https://github.com/MetaCubeX/mihomo/blob/v1.19.28/config/config.go)
- [Mihomo configuration index](https://wiki.metacubex.one/en/config/)
- [Mihomo general configuration](https://wiki.metacubex.one/en/config/general/)
- [Mihomo TUN configuration](https://wiki.metacubex.one/en/config/inbound/tun/)
- [Mihomo DNS configuration](https://wiki.metacubex.one/en/config/dns/)

The installed Mihomo binary's `-t` validation remains authoritative for
version-specific behavior. When Mihomo stable changes, this matrix and the
fixtures should be reviewed together.

## Strongly typed local fields

TOML uses `snake_case`; Mihoto renders the corresponding Mihomo kebab-case
keys. Every field is optional, so omission means “do not modify the remote
value”. The supported top-level groups are:

| TOML group | Coverage |
| --- | --- |
| `[mihomo_config]` | Ports, inbound TFO/MPTCP, authentication and LAN address lists, LAN/bind/mode/logging/IPv6, controller and CORS, UI/secret, interface/routing marks, process matching, TCP concurrency, keep-alive, Geo flags and global client settings |
| `[mihomo_config.dns]` | DNS enablement, transports, hosts, nameservers, fallback filters, fake-IP, cache, policy maps, listen and direct/proxy nameserver behavior |
| `[mihomo_config.tun]` | TUN enablement, stack, DNS hijack, routing, interface/UID/package/MAC selectors, MTU/GSO, marks, timeouts, NAT, file descriptor and platform route flags |
| `[mihomo_config.sniffer]` | Sniffing switches, domain/address/port filters, DNS mapping and per-protocol sniff settings |
| `[mihomo_config.hosts]` | Local host mapping keys and arbitrary TOML scalar/array/table values |
| `[mihomo_config.listeners]` | Listener definitions with common fields typed and listener-type-specific fields represented as TOML key/value fields |
| `[mihomo_config.ntp]` | NTP server, interval, dialer proxy and system-write behavior |
| `[mihomo_config.tls]` | Certificate, private key, client auth, ECH and Mihomo's `custom-certifactes` field |
| `[mihomo_config.experimental]` | Fingerprints, QUIC GSO/ECN switches and IP4P conversion |
| `[mihomo_config.profile]` | Selected-proxy and fake-IP persistence |
| `[mihomo_config.geox_url]` | GeoIP, MMDB, ASN and GeoSite URLs |
| `[mihomo_config.iptables]` | IPTables enablement, inbound interface, bypass and DNS redirect |
| `[mihomo_config.tuic_server]` | TUIC listener, credentials, certificates, congestion and relay limits |
| `[mihomo_config.clash_for_android]` | Android system DNS and subtitle pattern options |

## Deliberately subscription-owned fields

The following top-level collections are never generated or replaced by local
TOML fields:

`proxies`, `proxy-groups`, `rules`, `proxy-providers`, `rule-providers`,
`sub-rules`, and `tunnels`.

They are preserved from the remote YAML. Supplying any of these keys through
`[mihomo_config.extra]` is an error, so the exclusion cannot be bypassed.

## `extra` escape hatch

Use native Mihomo YAML keys under `[mihomo_config.extra]` when a stable,
non-dynamic field is not yet modeled:

```toml
[mihomo_config.extra]
"external-controller-cors" = { allow-origins = ["http://127.0.0.1:8080"] }
```

`extra` is intentionally not raw YAML and does not perform environment-variable
expansion. Its keys are emitted exactly as written. A key that conflicts with
an explicitly configured strongly typed field, or one of the subscription-owned
dynamic keys, is rejected.

## Merge and management semantics

- Scalars and enums explicitly set in TOML replace the remote value.
- Explicit local objects recursively merge into remote objects.
- Explicit local arrays replace remote arrays; an empty array is a valid clear-to-empty operation.
- Omitted fields, remote unknown fields and subscription-owned collections remain unchanged.
- There is no `null` deletion syntax and no environment-variable substitution.
- `ui` controls Mihoto dashboard asset downloads; `external_ui` controls Mihomo's rendered `external-ui` field. If the local path is omitted, Mihoto follows the remote rendered path for asset placement and otherwise uses `/etc/mihomo/ui` without injecting an override.
- `geox_url`, `geodata_mode`, `geo_auto_update` and `geo_update_interval` continue to drive Mihoto's Geo-data management. Native `external-ui-url` only affects the generated `config.yaml`.

The generated default `/etc/mihoto.toml` contains Mihoto's management
settings plus short commented examples for common, TUN and DNS overrides. It
does not serialize every optional Mihomo field.
