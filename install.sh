#!/bin/sh
# Public Unix installer/updater. Download only this repository's release assets.
set -eu
umask 077
fail() { printf '%s\n' "HanTong installation failed: $1" >&2; exit 1; }
os=$(uname -s)
case "$os" in Linux|Darwin) ;; *) fail 'this installer requires Linux or macOS' ;; esac
machine=$(uname -m)
case "$os:$machine" in
    Linux:x86_64) target=linux-x86_64 ;;
    Linux:aarch64) target=linux-aarch64 ;;
    Darwin:x86_64) target=macos-x86_64 ;;
    Darwin:arm64|Darwin:aarch64) target=macos-aarch64 ;;
    *) fail 'unsupported CPU architecture' ;;
esac
command -v curl >/dev/null 2>&1 || fail 'curl is required to download the release'
if command -v sha256sum >/dev/null 2>&1; then
    sha256_file() { sha256sum -- "$1" | awk '{print $1}'; }
elif command -v shasum >/dev/null 2>&1; then
    sha256_file() { shasum -a 256 -- "$1" | awk '{print $1}'; }
else
    fail 'sha256 tool is required to verify the release'
fi
temporary=$(mktemp -d)
case "$temporary" in /*) ;; *) fail 'temporary path is not absolute' ;; esac
cleanup() { [ -n "${temporary:-}" ] && rm -rf -- "$temporary"; }
trap cleanup EXIT HUP INT TERM
base=https://github.com/tz1012/mail-hantong
curl --fail --silent --show-error --location --proto '=https' --proto-redir '=https' \
    --connect-timeout 15 --max-time 60 --max-filesize 1048576 "$base/releases.atom" -o "$temporary/releases.atom"
version=$(sed -n 's@.*<id>tag:github.com,2008:Repository/[^/]*/v\([^<]*\)</id>.*@\1@p' "$temporary/releases.atom" | head -n 1)
printf '%s\n' "$version" | LC_ALL=C grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+-[A-Za-z0-9]+([.-][A-Za-z0-9]+)*$' || fail 'release feed has no supported version'
asset=mail-hantong-cli-$version-$target.tar.gz
curl --fail --silent --show-error --location --proto '=https' --proto-redir '=https' \
    --connect-timeout 15 --max-time 60 --max-filesize 65536 "$base/releases/download/v$version/SHA256SUMS" -o "$temporary/SHA256SUMS"
expected=$(awk -v name="$asset" '$2==name && $1 ~ /^[0-9a-f]+$/ && length($1)==64 {print $1}' "$temporary/SHA256SUMS")
[ "${#expected}" = 64 ] || fail 'release has no unique checksum for this Unix package'
printf 'Downloading HanTong %s for %s...\n' "$version" "$target"
curl --fail --show-error --location --proto '=https' --proto-redir '=https' \
    --connect-timeout 15 --max-time 300 --max-filesize 268435456 "$base/releases/download/v$version/$asset" -o "$temporary/$asset"
actual=$(sha256_file "$temporary/$asset")
[ "$actual" = "$expected" ] || fail 'archive checksum mismatch'
tar -tzf "$temporary/$asset" > "$temporary/members"
[ "$(wc -l < "$temporary/members")" -le 4096 ] || fail 'too many archive entries'
[ -z "$(sort "$temporary/members" | uniq -d)" ] || fail 'duplicate archive entries'
while IFS= read -r member; do
    case "$member" in ''|/*|*\\*|*:*|../*|*/../*|*/..|./*|*/./*|*/.|-*) fail 'unsafe archive member' ;; esac
done < "$temporary/members"
tar -tvzf "$temporary/$asset" > "$temporary/details"
[ -z "$(cut -c1 "$temporary/details" | grep -vE '^[-d]$' || true)" ] || fail 'archive contains links or special files'
awk '{sum+=$3} END {exit(sum>1073741824)}' "$temporary/details" || fail 'archive expands beyond its size limit'
mkdir "$temporary/package"
if [ "$os" = Linux ]; then
    tar -xzf "$temporary/$asset" --no-same-owner --no-same-permissions -C "$temporary/package"
else
    tar -xzf "$temporary/$asset" -C "$temporary/package"
fi
sh "$temporary/package/install.sh" "$@"
