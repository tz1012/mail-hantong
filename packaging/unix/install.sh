#!/bin/sh
set -eu
umask 077
fail() { printf '%s\n' "Install failed: $1" >&2; exit 1; }
sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum -- "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 -- "$1" | awk '{print $1}'
    else
        fail 'sha256 tool is unavailable'
    fi
}
sha256_check() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum --strict --status -c "$1"
        return
    fi
    command -v shasum >/dev/null 2>&1 || fail 'sha256 tool is unavailable'
    while IFS=' ' read -r expected name; do
        [ -n "$expected" ] || continue
        [ "$(sha256_file "$name")" = "$expected" ] || return 1
    done < "$1"
}
package_root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)
install_root=${XDG_DATA_HOME:-"$HOME/.local/share"}/mail-hantong-cli
bin_dir=$HOME/.local/bin
while [ "$#" -gt 0 ]; do
    case "$1" in
        --install-root) [ "$#" -ge 2 ] || fail 'missing install root'; install_root=$2; shift 2 ;;
        --bin-dir) [ "$#" -ge 2 ] || fail 'missing bin directory'; bin_dir=$2; shift 2 ;;
        *) fail 'unknown argument' ;;
    esac
done
[ "$(basename -- "$install_root")" = mail-hantong-cli ] || fail 'installation root must end with mail-hantong-cli'
for path in "$install_root" "$bin_dir"; do
    case "$path" in /*) ;; *) fail 'paths must be absolute' ;; esac
    case "$path" in /|*/../*|*/..|*/./*|*/.|*'
'*) fail 'unsafe path' ;; esac
    cursor=$path
    while [ "$cursor" != / ]; do
        [ ! -L "$cursor" ] || fail 'linked installation path'
        cursor=$(dirname -- "$cursor")
    done
done
for name in .mail-hantong-owner launcher-dir.txt launchers.sha256 current.txt; do
    path=$install_root/$name
    [ ! -L "$path" ] || fail 'linked installation metadata'
    if [ -e "$path" ]; then
        [ -f "$path" ] && [ -O "$path" ] || fail 'unsafe installation metadata'
    fi
done
if [ -f "$install_root/.mail-hantong-owner" ]; then
    [ "$(cat "$install_root/.mail-hantong-owner")" = 'mail-hantong-cli-v1' ] || fail 'unrecognized installation marker'
fi
[ -f "$package_root/VERSION" ] && [ ! -L "$package_root/VERSION" ] || fail 'missing package version'
version=$(cat "$package_root/VERSION")
printf '%s\n' "$version" | LC_ALL=C grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+-[A-Za-z0-9]+([.-][A-Za-z0-9]+)*$' || fail 'invalid version'
[ -f "$package_root/FILES.sha256" ] || fail 'missing file checksums'
[ -f "$package_root/installed-files.txt" ] || fail 'missing owned file list'
while IFS= read -r name; do
    case "$name" in ''|/*|*\\*|*:*|../*|*/../*|*/..|./*|*/./*|*/.|-*) fail 'unsafe package member' ;; esac
    [ -f "$package_root/$name" ] && [ ! -L "$package_root/$name" ] || fail 'missing or linked package member'
done < "$package_root/installed-files.txt"
(cd "$package_root" && sha256_check FILES.sha256) || fail 'package checksum mismatch'
mkdir -p -- "$install_root/versions" "$bin_dir"
install_root=$(CDPATH= cd -- "$install_root" && pwd -P)
bin_dir=$(CDPATH= cd -- "$bin_dir" && pwd -P)
[ -O "$install_root" ] && [ -O "$bin_dir" ] || fail 'installation directories belong to another user'
for command in hantong hantong-t hantong-mcp; do
    launcher=$bin_dir/$command
    if [ -e "$launcher" ] || [ -L "$launcher" ]; then
        [ -f "$launcher" ] && [ ! -L "$launcher" ] || fail 'existing launcher is not an owned file'
        grep -Fxq "# HanTong managed launcher: $install_root" "$launcher" || fail 'existing launcher is not managed by this installation'
    fi
done
if [ -f "$install_root/current.txt" ]; then
    current=$(cat "$install_root/current.txt")
    printf '%s\n' "$current" | LC_ALL=C grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+-[A-Za-z0-9]+([.-][A-Za-z0-9]+)*$' || fail 'invalid existing version pointer'
    latest=$(printf '%s\n%s\n' "$current" "$version" | LC_ALL=C sort -V | tail -n 1)
    [ "$latest" = "$version" ] || fail 'refusing to downgrade'
fi
destination=$install_root/versions/$version
if [ -e "$destination" ]; then
    [ -d "$destination" ] && [ ! -L "$destination" ] || fail 'existing version path is unsafe'
    cmp -s "$destination/release-manifest.json" "$package_root/release-manifest.json" || fail 'same version has different contents'
    (cd "$destination" && sha256_check FILES.sha256) || fail 'installed version has changed'
else
    stage=$(mktemp -d "$install_root/versions/.install.XXXXXXXX")
    cleanup() {
        case "${stage:-}" in "$install_root"/versions/.install.*) rm -rf -- "$stage" ;; esac
    }
    trap cleanup EXIT HUP INT TERM
    (cd "$package_root" && tar -cf - -T installed-files.txt) | (cd "$stage" && tar -xf -)
    (cd "$stage" && sha256_check FILES.sha256) || fail 'staged package checksum mismatch'
    chmod 755 "$stage/bin/"* "$stage/"*.sh
    mv -- "$stage" "$destination"
    stage=
    trap - EXIT HUP INT TERM
fi
quoted_root=$(printf '%s' "$install_root" | sed "s/'/'\\\\''/g")
for command in hantong hantong-t hantong-mcp; do
    launcher=$(mktemp "$bin_dir/.hantong.XXXXXXXX")
    {
        printf '#!/bin/sh\n# HanTong managed launcher: %s\n' "$install_root"
        printf "root='%s'\n" "$quoted_root"
        printf 'version=$(cat "$root/current.txt") || exit 1\n'
        printf 'case "$version" in *[!A-Za-z0-9.-]*|""|.*) exit 1 ;; esac\n'
        printf 'exec "$root/versions/$version/bin/%s" "$@"\n' "$command"
    } > "$launcher"
    chmod 755 "$launcher"
    mv -- "$launcher" "$bin_dir/$command"
done
if [ -e "$install_root/.mail-hantong-owner" ]; then
    [ -f "$install_root/.mail-hantong-owner" ] && [ ! -L "$install_root/.mail-hantong-owner" ] || fail 'unsafe installation marker'
    [ "$(cat "$install_root/.mail-hantong-owner")" = 'mail-hantong-cli-v1' ] || fail 'unrecognized installation marker'
else
    printf '%s\n' 'mail-hantong-cli-v1' > "$install_root/.mail-hantong-owner"
fi
printf '%s\n' "$bin_dir" > "$install_root/launcher-dir.txt"
for launcher in "$bin_dir/hantong" "$bin_dir/hantong-t" "$bin_dir/hantong-mcp"; do
    printf '%s  %s\n' "$(sha256_file "$launcher")" "$launcher"
done > "$install_root/launchers.sha256"
pointer=$(mktemp "$install_root/.current.XXXXXXXX")
printf '%s\n' "$version" > "$pointer"
mv -- "$pointer" "$install_root/current.txt"
printf 'Installed HanTong %s. Run: %s/hantong\nExisting accounts and data were preserved.\n' "$version" "$bin_dir"
