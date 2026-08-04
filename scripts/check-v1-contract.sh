#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

contract=docs/v1-contract.md
version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)"
test -n "$version"
release_notes="docs/releases/v${version}.md"
audit_record="docs/audits/v${version}.md"

test -f "$contract"
test -f "$release_notes"
test -f "$audit_record"
grep -Fq "version = \"$version\"" Cargo.toml
grep -Fq "version = \"$version\"" Cargo.lock

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
	'Ubuntu 24.04' \
	'GitHub-hosted' \
	'not supported' \
	'not migrated automatically' \
	'uninstall --purge' \
	'SHA256SUMS' \
	'attestation' \
	'--version <semver>'; do
	grep -Fqi -- "$required" "$contract" README.md "$release_notes"
done

grep -Fq 'command_requires_root' src/main.rs
grep -Fq 'Commands::Upgrade { check: true' src/main.rs
grep -Fq 'SHA256SUMS' src/upgrade.rs
grep -Fq 'atomic_replace' src/upgrade.rs
scripts/check-system-scope.sh
scripts/check-release-workflow.sh
