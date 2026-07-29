#!/bin/sh
set -eu

blocked() {
	echo "SYSTEMD INTEGRATION BLOCKED: $1" >&2
	exit 2
}

[ "${MIHOTO_SYSTEMD_INTEGRATION:-}" = 1 ] || blocked "MIHOTO_SYSTEMD_INTEGRATION=1 is required"
[ "${MIHOTO_ALLOW_SYSTEMD_MUTATION:-}" = 1 ] || blocked "MIHOTO_ALLOW_SYSTEMD_MUTATION=1 is required"
[ "$(id -u)" = 0 ] || blocked "root is required"
[ "$(ps -p 1 -o comm= | tr -d ' ')" = systemd ] || blocked "systemd must be PID 1"
systemctl is-system-running >/dev/null 2>&1 ||
	[ "$(systemctl is-system-running 2>/dev/null || true)" = degraded ] ||
	blocked "systemd is not running"

repo_root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
mihoto_source=${MIHOTO_BIN:-"$repo_root/target/release/mihoto"}
[ -x "$mihoto_source" ] || blocked "built mihoto binary not found at $mihoto_source"

managed_paths="/etc/mihoto.toml /etc/mihomo /usr/local/bin/mihoto /usr/local/bin/mihomo /etc/systemd/system/mihomo.service /etc/systemd/system/mihoto-update.service /etc/systemd/system/mihoto-update.timer"
for managed_path in $managed_paths; do
	[ ! -e "$managed_path" ] || blocked "refusing to overwrite existing $managed_path"
done

cleanup() {
	set +e
	systemctl disable --now mihoto-update.timer >/dev/null 2>&1
	systemctl disable --now mihomo.service >/dev/null 2>&1
	rm -f /etc/systemd/system/mihoto-update.service
	rm -f /etc/systemd/system/mihoto-update.timer
	rm -f /etc/systemd/system/mihomo.service
	rm -f /etc/mihoto.toml
	rm -f /usr/local/bin/mihoto
	rm -f /usr/local/bin/mihomo
	rm -rf /etc/mihomo
	systemctl daemon-reload >/dev/null 2>&1
}
trap cleanup EXIT INT TERM

install -m 0755 "$mihoto_source" /usr/local/bin/mihoto
install -d -m 0750 /etc/mihomo/ui
printf '%s\n' \
	'#!/bin/sh' \
	'if [ "${1:-}" = "-v" ]; then echo "Mihomo Meta test-v1"; exit 0; fi' \
	'trap "exit 0" INT TERM' \
	'while :; do sleep 30 & wait $!; done' \
	> /usr/local/bin/mihomo
chmod 0755 /usr/local/bin/mihomo
printf '%s\n' \
	'remote_config_url = "http://127.0.0.1:9/not-used"' \
	'auto_update_interval = 1' \
	> /etc/mihoto.toml
chmod 0600 /etc/mihoto.toml
printf '%s\n' \
	'tun:' \
	'  enable: false' \
	'rules:' \
	'  - MATCH,DIRECT' \
	> /etc/mihomo/config.yaml
chmod 0600 /etc/mihomo/config.yaml
: > /etc/mihomo/country.mmdb
: > /etc/mihomo/ui/index.html

/usr/local/bin/mihoto init --yes
systemctl is-active --quiet mihomo.service
systemctl is-enabled --quiet mihomo.service
systemctl is-active --quiet mihoto-update.timer
systemctl is-enabled --quiet mihoto-update.timer
[ "$(systemctl show mihomo.service -p User --value)" = root ]
systemctl show mihomo.service -p CapabilityBoundingSet --value |
	tr '[:lower:]' '[:upper:]' |
	grep -Fq CAP_NET_ADMIN
systemctl show mihomo.service -p CapabilityBoundingSet --value |
	tr '[:lower:]' '[:upper:]' |
	grep -Fq CAP_NET_RAW
grep -Fq 'WantedBy=multi-user.target' /etc/systemd/system/mihomo.service
grep -Fq 'tun:' /etc/mihomo/config.yaml
[ "$(stat -c %a /etc/mihoto.toml)" = 600 ]
[ "$(stat -c %a /etc/mihomo)" = 750 ]
[ "$(stat -c %a /etc/mihomo/config.yaml)" = 600 ]
[ "$(stat -c %a /etc/systemd/system/mihomo.service)" = 644 ]
[ "$(stat -c %a /etc/systemd/system/mihoto-update.service)" = 644 ]
[ "$(stat -c %a /etc/systemd/system/mihoto-update.timer)" = 644 ]

/usr/local/bin/mihoto restart
/usr/local/bin/mihoto timer status
/usr/local/bin/mihoto apply
/usr/local/bin/mihoto uninstall
[ ! -e /etc/systemd/system/mihomo.service ]
[ ! -e /etc/systemd/system/mihoto-update.service ]
[ ! -e /etc/systemd/system/mihoto-update.timer ]
[ -e /etc/mihoto.toml ]
[ -e /etc/mihomo/config.yaml ]
[ -x /usr/local/bin/mihomo ]
[ -x /usr/local/bin/mihoto ]

/usr/local/bin/mihoto init --yes
/usr/local/bin/mihoto uninstall --purge --yes
for managed_path in $managed_paths; do
	[ ! -e "$managed_path" ] || {
		echo "unexpected residue: $managed_path" >&2
		exit 1
	}
done

echo "SYSTEMD INTEGRATION PASSED"
