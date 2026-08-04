#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

workflow=.github/workflows/release.yml
real_host_workflow=.github/workflows/real-host.yml
version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)"
test -n "$version"
audit_record="docs/audits/v${version}.md"

test -f "$workflow"
test -f "$real_host_workflow"
test -f "$audit_record"
test ! -e .github/workflows/systemd-integration.yml
grep -Fq 'v*.*.*' "$workflow"
grep -Fq 'v[0-9]+\.[0-9]+\.[0-9]+(-rc\.[0-9]+)?' "$workflow"
grep -Fq "version = \"$version\"" Cargo.toml
grep -Fq "version = \"$version\"" Cargo.lock

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
	'needs: [validate, quality, assemble, attest, audit-acceptance]' \
	"github.event_name == 'push'" \
	'github.event.inputs.tag' \
	'--repo "$REPOSITORY"' \
	'--prerelease' \
	'--latest' \
	'git merge-base --is-ancestor' \
	'Hosted systemd/TUN acceptance: PASS' \
	'Release decision: Accepted'; do
	grep -Fq -- "$required" "$workflow"
done
grep -Fq -- 'audit="docs/audits/v${VERSION}.md"' "$workflow"
audit_job="$(sed -n '/^  audit-acceptance:/,/^  release:/p' "$workflow")"
printf '%s\n' "$audit_job" | grep -Fq -- 'fetch-depth: 0'

if grep -R -n -- 'self-hosted' .github/workflows; then
	echo 'public repository workflows must not execute on self-hosted runners' >&2
	exit 1
fi

for required in \
	'workflow_dispatch:' \
	'runs-on: ubuntu-24.04' \
	'runs-on: ubuntu-24.04-arm' \
	'Install stable Mihomo fixture' \
	'/usr/local/libexec/mihoto-test/mihomo' \
	'scripts/test-systemd-integration.sh' \
	'scripts/test-tun-integration.sh'; do
	grep -Fq -- "$required" "$real_host_workflow"
done
