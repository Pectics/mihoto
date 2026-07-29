#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
test ! -e src/cron.rs
test ! -e src/proxy.rs
for pattern in 'systemctl[[:space:]]+--user' 'journalctl[[:space:]]+--user' 'user_systemd_root' 'MIHORO_GITHUB_MIRROR' 'XDG_RUNTIME_DIR|/run/user/' 'systemd/user|~/.config|~/.local'; do
	if grep -R -n -E --exclude=check-system-scope.sh -- "$pattern" Cargo.toml README.md install.sh .github scripts src tests; then exit 1; fi
done
if grep -R -n -E --exclude=check-system-scope.sh -- 'mihoro' Cargo.toml README.md install.sh .github scripts src tests; then exit 1; fi
grep -Fq 'name = "mihoto"' Cargo.toml
grep -Fq 'version = "1.0.0-rc.1"' Cargo.toml
grep -Fq '"/usr/local/bin/mihoto"' src/config.rs
grep -Fq '"/usr/local/bin/mihomo"' src/config.rs
grep -Fq '"/etc/mihomo"' src/config.rs
cargo metadata --no-deps --format-version 1 >/dev/null
sh -n install.sh
sh -n scripts/test-systemd-integration.sh
sh -n scripts/test-tun-integration.sh
sh -n scripts/test-installer-contract.sh
