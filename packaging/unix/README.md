# HanTong Unix CLI beta

Linux packages require x86_64 or aarch64 with glibc 2.39 or newer. macOS
packages target x86_64 and arm64. The archive contains native binaries: users
do not need Rust, Python, Docker, WSL or Go. macOS packaging is covered by the
release asset checks; native macOS runtime and real-provider integration still
require an OS-specific release validation.

Verify the release archive against its public SHA256SUMS, extract it to a new
directory, and run `sh install.sh`. The installer creates versioned program
files under `${XDG_DATA_HOME:-$HOME/.local/share}/mail-hantong-cli` and launchers
under `$HOME/.local/bin`. It preserves mail data and credentials. Add that bin
directory to your shell's PATH if it is not already present.

Run `hantong` for account setup or `hantong-t` for the terminal view. Desktop
Secret Service and an explicitly selected encrypted headless vault are
distinct secret backends. Protected input belongs to Core/Secrets. Never put
passwords or tokens in command arguments, environment variables or chat.

On an SSH server, a protected TTY is required to unlock the headless vault.
After Core exits or the server restarts, it must be unlocked again. This does
not provide unattended unlock or permission for unattended sends. Sending
and Trash still require their message-specific terminal confirmation.

Google sign-in uses the standard browser consent flow. A browser on a different
machine needs a secure loopback callback connection to the server; no password
collection or deprecated out-of-band code flow is used. IMAP/SMTP does not need
a browser. Verify the release's exact tested flows before using real accounts.

`hantong update` runs the package-owned updater. Run the installed version's
`uninstall.sh` to remove only unchanged owned files; unknown/modified files,
other program versions and all account data are preserved.

Security-sensitive Unix changes still require human security review. Installing
a build or opening its TUI alone does not prove mail/account integration works.
