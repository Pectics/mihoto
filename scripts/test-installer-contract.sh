#!/bin/sh
set -eu

cd "$(dirname "$0")/.."

temp_dir="$(mktemp -d)"
trap 'rm -rf "$temp_dir"' EXIT INT TERM

fixtures="$temp_dir/fixtures"
fake_bin="$temp_dir/fake-bin"
install_root="$temp_dir/install-root"
asset='mihoto-v1.0.0-rc.1-x86_64-unknown-linux-gnu.tar.gz'
mkdir -p "$fixtures/payload" "$fake_bin" "$install_root/usr/local/bin"

cat > "$fixtures/payload/mihoto" <<'EOF'
#!/bin/sh
if [ "${1:-}" = '--version' ]; then
	printf '%s\n' 'mihoto 1.0.0-rc.1'
	return 0 2>/dev/null || exit 0
fi
exit 0
EOF
chmod 755 "$fixtures/payload/mihoto"
tar -C "$fixtures/payload" -czf "$fixtures/$asset" mihoto
(
	cd "$fixtures"
	sha256sum "$asset" > SHA256SUMS
)

cat > "$fake_bin/curl" <<'EOF'
#!/bin/sh
set -eu

output=''
url=''
while [ "$#" -gt 0 ]; do
	case "$1" in
		-o | --output)
			output="$2"
			shift 2
			;;
		*)
			url="$1"
			shift
			;;
	esac
done

printf '%s\n' "$url" >> "$MIHOTO_CURL_LOG"
case "$url" in
	*/releases/latest)
		printf '%s\n' '{"tag_name":"v1.0.0-rc.1"}' > "$output"
		;;
	*/releases/tags/v1.0.0-rc.1)
		printf '%s\n' '{"tag_name":"v1.0.0-rc.1"}' > "$output"
		;;
	*/SHA256SUMS)
		cat "$MIHOTO_INSTALLER_FIXTURES/SHA256SUMS" > "$output"
		;;
	*/mihoto-v1.0.0-rc.1-x86_64-unknown-linux-gnu.tar.gz)
		cat "$MIHOTO_INSTALLER_FIXTURES/mihoto-v1.0.0-rc.1-x86_64-unknown-linux-gnu.tar.gz" > "$output"
		;;
	*)
		exit 22
		;;
esac
EOF
chmod 755 "$fake_bin/curl"

old_binary="$install_root/usr/local/bin/mihoto"
printf '%s\n' old > "$old_binary"
chmod 755 "$old_binary"

curl_log="$temp_dir/curl.log"
PATH="$fake_bin:$PATH" \
	MIHOTO_CURL_LOG="$curl_log" \
	MIHOTO_INSTALLER_FIXTURES="$fixtures" \
	MIHOTO_INSTALL_TEST_ROOT="$install_root" \
	sh install.sh --version v1.0.0-rc.1

"$old_binary" --version | grep -Fqx 'mihoto 1.0.0-rc.1'
test -x "$old_binary"
grep -Fqx 'https://api.github.com/repos/Pectics/mihoto/releases/tags/v1.0.0-rc.1' "$curl_log"
grep -Fqx 'https://github.com/Pectics/mihoto/releases/download/v1.0.0-rc.1/SHA256SUMS' "$curl_log"

PATH="$fake_bin:$PATH" \
	MIHOTO_CURL_LOG="$curl_log" \
	MIHOTO_INSTALLER_FIXTURES="$fixtures" \
	MIHOTO_INSTALL_TEST_ROOT="$install_root" \
	sh install.sh --mirror https://mirror.example
"$old_binary" --version | grep -Fqx 'mihoto 1.0.0-rc.1'
grep -Fqx 'https://api.github.com/repos/Pectics/mihoto/releases/latest' "$curl_log"
grep -Fqx 'https://mirror.example/https://github.com/Pectics/mihoto/releases/download/v1.0.0-rc.1/mihoto-v1.0.0-rc.1-x86_64-unknown-linux-gnu.tar.gz' "$curl_log"

printf '%s  %s\n' '0000000000000000000000000000000000000000000000000000000000000000' "$asset" > "$fixtures/SHA256SUMS"
if PATH="$fake_bin:$PATH" \
	MIHOTO_CURL_LOG="$curl_log" \
	MIHOTO_INSTALLER_FIXTURES="$fixtures" \
	MIHOTO_INSTALL_TEST_ROOT="$install_root" \
	sh install.sh --version 1.0.0-rc.1; then
	echo 'installer accepted a mismatched checksum' >&2
	exit 1
fi
"$old_binary" --version | grep -Fqx 'mihoto 1.0.0-rc.1'

if PATH="$fake_bin:$PATH" \
	MIHOTO_CURL_LOG="$curl_log" \
	MIHOTO_INSTALLER_FIXTURES="$fixtures" \
	MIHOTO_INSTALL_TEST_ROOT="$install_root" \
	MIHOTO_TEST_UNAME_MACHINE=riscv64 \
	sh install.sh --version v1.0.0-rc.1; then
	echo 'installer accepted an unsupported architecture' >&2
	exit 1
fi
"$old_binary" --version | grep -Fqx 'mihoto 1.0.0-rc.1'
