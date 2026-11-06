#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

# Public behavior is checked against the maintained README and executable contracts.
cargo metadata --locked --no-deps --format-version 1 >/dev/null

for required in \
	'/etc/mihoto.toml' \
	'/usr/local/bin/mihoto' \
	'/usr/local/bin/mihomo' \
	'/etc/mihomo' \
	'/etc/systemd/system/mihomo.service' \
	'x86_64-unknown-linux-gnu' \
	'x86_64-unknown-linux-musl' \
	'aarch64-unknown-linux-gnu' \
	'aarch64-unknown-linux-musl' \
	'Ubuntu 22.04' \
	'Ubuntu 24.04' \
	'not supported' \
	'not migrated automatically' \
	'uninstall --purge' \
	'SHA256SUMS' \
	'attestation' \
	'--version <semver>'; do
	grep -Fqi -- "$required" README.md
done

grep -Fq 'command_requires_root' src/cli/policy.rs
grep -Fq 'Commands::Upgrade { check: true' src/cli/policy.rs
grep -Fq 'SHA256SUMS' src/infrastructure/upgrade.rs
grep -Fq 'atomic_replace' src/infrastructure/upgrade.rs
scripts/check-system-scope.sh
scripts/check-release-workflow.sh
