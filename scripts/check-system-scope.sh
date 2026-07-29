#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
test ! -e src/cron.rs
test ! -e src/proxy.rs
for pattern in 'systemctl[[:space:]]+--user' 'journalctl[[:space:]]+--user' 'user_systemd_root' 'MIHORO_GITHUB_MIRROR' 'XDG_RUNTIME_DIR|/run/user/' 'systemd/user|~/.config|~/.local'; do
	if rg -n --hidden --glob '!target/**' --glob '!.git/**' --glob '!scripts/check-system-scope.sh' -- "$pattern" .; then exit 1; fi
done
if rg -n --hidden --glob '!target/**' --glob '!.git/**' --glob '!scripts/check-system-scope.sh' -- 'mihoro' .; then exit 1; fi
rg -Fq 'name = "mihoto"' Cargo.toml
rg -Fq 'version = "0.15.0"' Cargo.toml
rg -Fq '"/usr/local/bin/mihoto"' src/config.rs
rg -Fq '"/usr/local/bin/mihomo"' src/config.rs
rg -Fq '"/etc/mihomo"' src/config.rs
cargo metadata --no-deps --format-version 1 >/dev/null
sh -n install.sh
sh -n scripts/test-systemd-integration.sh
sh -n scripts/test-tun-integration.sh
