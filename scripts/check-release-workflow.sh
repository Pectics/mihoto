#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

workflow=.github/workflows/release.yml

test -f "$workflow"
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
	'needs: [validate, quality, assemble, attest, real-host-acceptance, real-host-acceptance-arm64]' \
	"github.event_name == 'push'" \
	'github.event.inputs.tag' \
	'--prerelease' \
	'--latest'; do
	grep -Fq -- "$required" "$workflow"
done
