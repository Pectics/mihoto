#!/bin/sh
set -eu
[ "$(id -u)" = 0 ] || { echo 'BLOCKED: root unavailable'; exit 2; }
[ -c /dev/net/tun ] || { echo 'BLOCKED: /dev/net/tun unavailable'; exit 2; }
command -v ip >/dev/null || { echo 'BLOCKED: ip unavailable'; exit 2; }
(command -v nft >/dev/null || command -v iptables >/dev/null) || { echo 'BLOCKED: firewall tooling unavailable'; exit 2; }
echo 'TUN environment accepted; run dedicated VM acceptance workflow'
