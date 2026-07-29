#!/bin/sh
set -eu
not_run() { echo 'SYSTEMD INTEGRATION NOT RUN: isolated systemd environment unavailable'; exit 2; }
[ "${MIHOTO_SYSTEMD_INTEGRATION:-}" = 1 ] || not_run
[ "${MIHOTO_ALLOW_SYSTEMD_MUTATION:-}" = 1 ] || not_run
[ "$(id -u)" = 0 ] || not_run
[ "$(ps -p 1 -o comm= | tr -d ' ')" = systemd ] || not_run
systemctl is-system-running >/dev/null || not_run
echo 'SYSTEMD INTEGRATION environment accepted; run dedicated VM acceptance workflow'
