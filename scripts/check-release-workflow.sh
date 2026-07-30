#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

workflow=.github/workflows/release.yml
real_host_workflow=.github/workflows/real-host.yml

test -f "$workflow"
test -f "$real_host_workflow"
grep -Fq 'v*.*.*' "$workflow"
grep -Fq 'v[0-9]+\.[0-9]+\.[0-9]+(-rc\.[0-9]+)?' "$workflow"
grep -Fq 'version = "1.0.0"' Cargo.toml

for target in \
	'x86_64-unknown-linux-gnu' \
	'x86_64-unknown-linux-musl' \
	'aarch64-unknown-linux-gnu' \
	'aarch64-unknown-linux-musl'; do
	grep -Fq "$target" "$workflow"
done

if grep -n 'i686' "$workflow"; then
	echo 'release workflow must not publish i686 artifacts' >&2
	exit 1
fi

for required in \
	'contents: write' \
	'attestations: write' \
	'id-token: write' \
	'SHA256SUMS' \
	'attest-build-provenance' \
	'libgcc-s1-arm64-cross' \
	'audit-acceptance' \
	'docs/audits/v1.0.0.md' \
	'needs: [validate, quality, assemble, attest, audit-acceptance]' \
	"github.event_name == 'push'" \
	'github.event.inputs.tag' \
	'--prerelease' \
	'--latest'; do
	grep -Fq -- "$required" "$workflow"
done

if grep -Fq 'runs-on: [self-hosted' "$workflow"; then
	echo 'release publication must not wait forever on unregistered runners' >&2
	exit 1
fi

for required in \
	'workflow_dispatch:' \
	'runs-on: [self-hosted, linux, systemd, tun, x64]' \
	'runs-on: [self-hosted, linux, systemd, tun, arm64]' \
	'scripts/test-systemd-integration.sh' \
	'scripts/test-tun-integration.sh'; do
	grep -Fq -- "$required" "$real_host_workflow"
done
