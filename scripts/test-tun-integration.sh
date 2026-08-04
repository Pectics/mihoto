#!/bin/sh
set -eu

blocked() {
	echo "TUN INTEGRATION BLOCKED: $1" >&2
	exit 2
}

[ "${MIHOTO_TUN_INTEGRATION:-}" = 1 ] || blocked "MIHOTO_TUN_INTEGRATION=1 is required"
[ "${MIHOTO_ALLOW_SYSTEMD_MUTATION:-}" = 1 ] || blocked "MIHOTO_ALLOW_SYSTEMD_MUTATION=1 is required"
[ "$(id -u)" = 0 ] || blocked "root is required"
[ "$(ps -p 1 -o comm= | tr -d ' ')" = systemd ] || blocked "systemd must be PID 1"
[ -c /dev/net/tun ] || blocked "/dev/net/tun is unavailable"
command -v ip >/dev/null || blocked "ip is unavailable"
command -v curl >/dev/null || blocked "curl is unavailable"

repo_root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
mihoto_source=${MIHOTO_BIN:-"$repo_root/target/release/mihoto"}
mihomo_source=${MIHOTO_REAL_MIHOMO:-}
[ -x "$mihoto_source" ] || blocked "built mihoto binary not found at $mihoto_source"
[ -n "$mihomo_source" ] || blocked "MIHOTO_REAL_MIHOMO must point to a real Mihomo binary"
[ -x "$mihomo_source" ] || blocked "real Mihomo binary is not executable"

managed_paths="/etc/mihoto.toml /etc/mihomo /usr/local/bin/mihoto /usr/local/bin/mihomo /etc/systemd/system/mihomo.service /etc/systemd/system/mihoto-update.service /etc/systemd/system/mihoto-update.timer"
for managed_path in $managed_paths; do
	[ ! -e "$managed_path" ] || blocked "refusing to overwrite existing $managed_path"
done

state_dir=$(mktemp -d /var/tmp/mihoto-tun-state.XXXXXX)
snapshot_routes() {
	ip route show table all |
		sed -E 's/ expires [0-9]+sec//g' |
		sort
}
snapshot_routes > "$state_dir/routes.before"
ip rule show > "$state_dir/rules.before"

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
	ip link delete mihoto-tun0 >/dev/null 2>&1
	rm -rf "$state_dir"
}
trap cleanup EXIT INT TERM

install -m 0755 "$mihoto_source" /usr/local/bin/mihoto
install -m 0755 "$mihomo_source" /usr/local/bin/mihomo
install -d -m 0750 /etc/mihomo/ui
printf '%s\n' \
	'remote_config_url = "http://127.0.0.1:9/not-used"' \
	'auto_update_interval = 0' \
	'' \
	'[mihomo_config]' \
	'port = 17891' \
	'socks_port = 17892' \
	'mixed_port = 17890' \
	'allow_lan = false' \
	'bind_address = "127.0.0.1"' \
	'mode = "direct"' \
	'log_level = "info"' \
	'ipv6 = false' \
	'external_controller = "127.0.0.1:19090"' \
	'external_ui = "ui"' \
	'' \
	'[mihomo_config.tun]' \
	'enable = true' \
	'stack = "system"' \
	'device = "mihoto-tun0"' \
	'auto_route = true' \
	'auto_redirect = true' \
	'auto_detect_interface = true' \
	'route_address = ["198.18.0.0/15"]' \
	> /etc/mihoto.toml
chmod 0600 /etc/mihoto.toml
printf '%s\n' \
	'proxies: []' \
	'proxy-groups: []' \
	'rules:' \
	'  - MATCH,DIRECT' \
	> /etc/mihomo/config.yaml
chmod 0600 /etc/mihomo/config.yaml
: > /etc/mihomo/country.mmdb
: > /etc/mihomo/ui/index.html

/usr/local/bin/mihoto init --yes
systemctl is-active --quiet mihomo.service || {
	journalctl -u mihomo.service --no-pager -n 100 >&2
	exit 1
}

attempt=0
while ! ip link show mihoto-tun0 >/dev/null 2>&1; do
	attempt=$((attempt + 1))
	[ "$attempt" -lt 10 ] || {
		journalctl -u mihomo.service --no-pager -n 100 >&2
		exit 1
	}
	sleep 1
done

curl --fail --silent --show-error http://127.0.0.1:19090/version >/dev/null
[ "$(systemctl show mihomo.service -p User --value)" = root ]
systemctl show mihomo.service -p CapabilityBoundingSet --value |
	tr '[:lower:]' '[:upper:]' |
	grep -Fq CAP_NET_ADMIN
systemctl show mihomo.service -p CapabilityBoundingSet --value |
	tr '[:lower:]' '[:upper:]' |
	grep -Fq CAP_NET_RAW
grep -A6 '^tun:' /etc/mihomo/config.yaml | grep -Fq 'enable: true'
grep -A6 '^tun:' /etc/mihomo/config.yaml | grep -Fq 'device: mihoto-tun0'
ip route show table all | grep -F '198.18.0.0/15' | grep -Fq 'dev mihoto-tun0'
ip rule show | grep -Fq 'lookup 2022'
nft list ruleset 2>/dev/null | grep -iq mihomo

/usr/local/bin/mihoto stop
attempt=0
while ip link show mihoto-tun0 >/dev/null 2>&1; do
	attempt=$((attempt + 1))
	[ "$attempt" -lt 10 ] || {
		echo "TUN interface remained after service stop" >&2
		exit 1
	}
	sleep 1
done

/usr/local/bin/mihoto uninstall --purge --yes
snapshot_routes > "$state_dir/routes.after"
ip rule show > "$state_dir/rules.after"
cmp "$state_dir/routes.before" "$state_dir/routes.after"
cmp "$state_dir/rules.before" "$state_dir/rules.after"

echo "TUN INTEGRATION PASSED"
