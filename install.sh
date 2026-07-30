#!/bin/sh
# shellcheck shell=dash

REPOSITORY='Pectics/mihoto'
API_BASE="https://api.github.com/repos/$REPOSITORY"
DOWNLOAD_BASE="https://github.com/$REPOSITORY/releases/download"

main() {
	set -eu

	mirror=''
	requested_version=''
	while [ "$#" -gt 0 ]; do
		case "$1" in
			--version)
				[ "$#" -ge 2 ] || err '--version requires a semantic version'
				requested_version="$2"
				shift 2
				;;
			--mirror)
				[ "$#" -ge 2 ] || err '--mirror requires a URL'
				mirror="$2"
				shift 2
				;;
			--no-mirror)
				mirror=''
				shift
				;;
			-h | --help)
				usage
				exit 0
				;;
			*)
				err "unknown option: $1"
				;;
		esac
	done

	need_cmd awk
	need_cmd cp
	need_cmd grep
	need_cmd head
	need_cmd id
	need_cmd mkdir
	need_cmd mktemp
	need_cmd mv
	need_cmd sed
	need_cmd sha256sum
	need_cmd tar

	if check_cmd curl; then
		downloader=curl
	elif check_cmd wget; then
		downloader=wget
	else
		err 'need curl or wget'
	fi

	install_dir=$(install_dir)
	if [ -z "${MIHOTO_INSTALL_TEST_ROOT:-}" ] && [ "$(id -u)" -ne 0 ]; then
		err 'installation to /usr/local/bin requires root; run with sudo'
	fi

	arch=$(get_architecture)
	require_supported_arch "$arch"
	echo "Detected architecture: $arch"

	tmp_dir=$(mktemp -d) || err 'mktemp: could not create a temporary directory'
	installed_temp=''
	trap cleanup EXIT INT TERM

	if [ -n "$requested_version" ]; then
		tag=$(normalize_version "$requested_version")
		metadata_url="$API_BASE/releases/tags/$tag"
	else
		metadata_url="$API_BASE/releases/latest"
	fi
	download_file "$metadata_url" "$tmp_dir/release.json"
	metadata_tag=$(extract_tag_name "$tmp_dir/release.json")
	validate_release_tag "$metadata_tag"
	if [ -n "$requested_version" ] && [ "$metadata_tag" != "$tag" ]; then
		err "release metadata tag $metadata_tag does not match requested $tag"
	fi
	tag="$metadata_tag"

	asset="mihoto-$tag-$arch.tar.gz"
	checksums_url="$DOWNLOAD_BASE/$tag/SHA256SUMS"
	download_file "$checksums_url" "$tmp_dir/SHA256SUMS"
	expected_sha=$(checksum_for "$tmp_dir/SHA256SUMS" "$asset")

	asset_url="$DOWNLOAD_BASE/$tag/$asset"
	if [ -n "$mirror" ]; then
		asset_url="${mirror%/}/$asset_url"
	fi
	archive="$tmp_dir/$asset"
	download_file "$asset_url" "$archive"
	verify_checksum "$expected_sha" "$archive"

	if [ "$(tar -tzf "$archive")" != 'mihoto' ]; then
		err "release archive $asset must contain exactly one mihoto binary"
	fi
	target_dir="$tmp_dir/extracted"
	mkdir "$target_dir"
	tar -xzf "$archive" -C "$target_dir"
	test -f "$target_dir/mihoto" || err 'release archive did not contain mihoto'

	ensure mkdir -p "$install_dir"
	installed_temp=$(mktemp "$install_dir/.mihoto.XXXXXX") || err 'could not create atomic install file'
	ensure cp "$target_dir/mihoto" "$installed_temp"
	ensure chmod 755 "$installed_temp"
	if [ -z "${MIHOTO_INSTALL_TEST_ROOT:-}" ]; then
		ensure chown root:root "$installed_temp"
	fi
	ensure "$installed_temp" --version
	ensure mv -f "$installed_temp" "$install_dir/mihoto"
	installed_temp=''

	echo "Installed Mihoto $tag to $install_dir/mihoto"
}

usage() {
	cat <<'EOF'
Usage: install.sh [--version <semver>] [--mirror <url>]

Installs the latest stable Mihoto release by default. Use --version for a
specific stable or RC release. Release metadata and SHA256SUMS always come
from GitHub; --mirror only proxies the archive transfer.
EOF
}

install_dir() {
	if [ -n "${MIHOTO_INSTALL_TEST_ROOT:-}" ]; then
		case "$MIHOTO_INSTALL_TEST_ROOT" in
			/*) printf '%s/usr/local/bin\n' "${MIHOTO_INSTALL_TEST_ROOT%/}" ;;
			*) err 'MIHOTO_INSTALL_TEST_ROOT must be absolute' ;;
		esac
	else
		printf '%s\n' /usr/local/bin
	fi
}

get_architecture() {
	ostype=$(uname -s)
	if [ "$ostype" != Linux ]; then
		err "unsupported operating system: $ostype (Mihoto supports systemd Linux only)"
	fi

	cputype=${MIHOTO_TEST_UNAME_MACHINE:-$(uname -m)}
	case "$cputype" in
		x86_64 | x86-64 | x64 | amd64) cputype=x86_64 ;;
		aarch64 | arm64) cputype=aarch64 ;;
		*) printf '%s-unknown-linux-gnu\n' "$cputype"; return ;;
	esac

	clibtype=gnu
	if check_cmd ldd && ldd --version 2>&1 | grep -q musl; then
		clibtype=musl
	fi
	printf '%s-unknown-linux-%s\n' "$cputype" "$clibtype"
}

require_supported_arch() {
	case "$1" in
		x86_64-unknown-linux-gnu | x86_64-unknown-linux-musl | aarch64-unknown-linux-gnu | aarch64-unknown-linux-musl)
			;;
		*)
			err "unsupported architecture: $1 (supported: x86_64/aarch64 GNU or musl Linux)"
			;;
	esac
}

normalize_version() {
	case "$1" in
		v*) tag=$1 ;;
		*) tag="v$1" ;;
	esac
	validate_release_tag "$tag"
	printf '%s\n' "$tag"
}

validate_release_tag() {
	if ! printf '%s\n' "$1" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+(-rc\.[0-9]+)?$'; then
		err "invalid Mihoto release version: $1"
	fi
}

extract_tag_name() {
	tag=$(sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$1" | head -n 1)
	[ -n "$tag" ] || err "release metadata did not contain tag_name"
	printf '%s\n' "$tag"
}

download_file() {
	url=$1
	destination=$2
	case "$downloader" in
		curl)
			curl -sSfL --output "$destination" "$url" || err "failed to download $url"
			;;
		wget)
			wget -qO "$destination" "$url" || err "failed to download $url"
			;;
	esac
}

checksum_for() {
	checksums=$1
	asset=$2
	match_count=$(awk -v asset="$asset" '$2 == asset || $2 == "*" asset { count += 1; checksum = $1 } END { print count + 0 }' "$checksums")
	[ "$match_count" -eq 1 ] || err "SHA256SUMS must contain exactly one checksum for $asset"
	checksum=$(awk -v asset="$asset" '$2 == asset || $2 == "*" asset { print $1 }' "$checksums")
	case "$checksum" in
		'' | *[!0123456789abcdefABCDEF]*) err "invalid SHA-256 for $asset" ;;
	esac
	[ "${#checksum}" -eq 64 ] || err "invalid SHA-256 length for $asset"
	printf '%s\n' "$checksum"
}

verify_checksum() {
	expected=$1
	archive=$2
	printf '%s  %s\n' "$expected" "$archive" | sha256sum --check --status - || err 'archive checksum verification failed'
}

check_cmd() {
	command -v "$1" >/dev/null 2>&1
}

need_cmd() {
	check_cmd "$1" || err "need '$1' (command not found)"
}

ensure() {
	"$@" || err "command failed: $*"
}

cleanup() {
	if [ -n "${installed_temp:-}" ]; then
		rm -f "$installed_temp"
	fi
	if [ -n "${tmp_dir:-}" ]; then
		rm -rf "$tmp_dir"
	fi
}

err() {
	echo "Error: $1" >&2
	exit 1
}

main "$@"
