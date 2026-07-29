#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
test ! -e src/cron.rs
test ! -e src/proxy.rs
for pattern in '--user' 'user_systemd_root' 'MIHORO_GITHUB_MIRROR' 'XDG_RUNTIME_DIR|/run/user/' 'systemd/user|~/.config|~/.local'; do
	if rg -n -- "$pattern" src Cargo.toml install.sh .github; then exit 1; fi
done
if rg -n -- 'mihoro' src Cargo.toml Cargo.lock install.sh .github; then exit 1; fi
cargo metadata --no-deps --format-version 1 >/dev/null
sh -n install.sh
