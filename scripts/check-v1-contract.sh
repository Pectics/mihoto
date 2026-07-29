#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

contract=docs/v1-contract.md
release_notes=docs/releases/v1.0.0-rc.1.md
audit_record=docs/audits/v1.0.0-rc.1.md

test -f "$contract"
test -f "$release_notes"
test -f "$audit_record"
rg -Fq 'version = "1.0.0-rc.1"' Cargo.toml
rg -Fq '1.0.0-rc.1' Cargo.lock

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
	rg -Fqi -- "$required" "$contract" README.md "$release_notes"
done

rg -Fq 'command_requires_root' src/main.rs
rg -Fq 'Commands::Upgrade { check: true' src/main.rs
rg -Fq 'SHA256SUMS' src/upgrade.rs
rg -Fq 'atomic_replace' src/upgrade.rs
scripts/check-system-scope.sh
scripts/check-release-workflow.sh
