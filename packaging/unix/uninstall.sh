#!/bin/sh
set -eu
sha256_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum -- "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 -- "$1" | awk '{print $1}'
    else
        return 1
    fi
}
sha256_matches() {
    expected=$(awk -v name="$2" 'substr($0,67)==name {print $1}' "$1")
    [ -n "$expected" ] && [ "$(sha256_file "$2")" = "$expected" ]
}
package_root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)
case "$package_root" in */mail-hantong-cli/versions/*) ;; *) printf '%s\n' 'Run the installed version-owned uninstaller.' >&2; exit 1 ;; esac
[ ! -L "$0" ] && [ -f "$package_root/installed-files.txt" ] || exit 1
version=$(cat "$package_root/VERSION")
[ "$(basename -- "$package_root")" = "$version" ] || exit 1
install_root=$(dirname -- "$(dirname -- "$package_root")")
# Remove only unchanged files listed by this package; unknown and modified files stay.
cd "$package_root"
while IFS= read -r name; do
    case "$name" in ''|/*|*\\*|*:*|../*|*/../*|*/..|./*|*/./*|*/.|-*) exit 1 ;; esac
    [ ! -L "$package_root/$name" ] || exit 1
done < installed-files.txt
list=$(mktemp)
trap 'rm -f -- "$list"' EXIT HUP INT TERM
cp -- installed-files.txt "$list"
while IFS= read -r name; do
    [ "$name" = FILES.sha256 ] && continue
    if [ -f "$name" ] && sha256_matches FILES.sha256 "$name"; then
        rm -f -- "$name"
    fi
done < "$list"
if [ -f "$install_root/current.txt" ] && [ "$(cat "$install_root/current.txt")" = "$version" ]; then
    if [ -f "$install_root/launchers.sha256" ] && [ -f "$install_root/launcher-dir.txt" ]; then
        bin_dir=$(cat "$install_root/launcher-dir.txt")
        case "$bin_dir" in /*) ;; *) exit 1 ;; esac
        if sha256_matches "$install_root/launchers.sha256" "$bin_dir/hantong" \
            && sha256_matches "$install_root/launchers.sha256" "$bin_dir/hantong-t" \
            && sha256_matches "$install_root/launchers.sha256" "$bin_dir/hantong-mcp"; then
            for command in hantong hantong-t hantong-mcp; do
                [ ! -L "$bin_dir/$command" ] && [ -f "$bin_dir/$command" ] \
                    && grep -Fxq "# HanTong managed launcher: $install_root" "$bin_dir/$command" || exit 1
            done
            rm -f -- "$bin_dir/hantong" "$bin_dir/hantong-t" "$bin_dir/hantong-mcp"
        fi
    fi
    rm -f -- "$install_root/current.txt"
fi
printf '%s\n' 'Removed unchanged package files. Accounts, keys, unknown files and other versions were preserved.'
