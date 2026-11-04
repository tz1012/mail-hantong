# Unix installation and use — beta.22

beta.22 has native CLI/Core packages for macOS 15 (Intel and Apple Silicon) and
Linux x86_64 with glibc >= 2.39. The four release assets are Windows x64,
Linux x64, macOS Intel and macOS ARM. There is no Linux ARM asset. Download the
matching archive from the [beta.22 release](https://github.com/tz1012/mail-hantong/releases/tag/v0.1.0-beta.22).

The executable source hash is
`c50c01dd88fe75e2b643e580fb25ba85430639d6a4e9e55650cf7595c86332fd` from
branch `feature/linux-product-20261104` at source HEAD `3eabc14`. The installer below is pinned to that verified source; `main` may still contain an older installer.

## Install

Download the installer to a file, inspect the file if required by local policy,
then execute it from a protected interactive terminal. Do not pipe a download
directly to a shell, and do not put passphrases in argv or environment
variables.

Linux x86_64:

```sh
curl -fsSLo install.sh https://raw.githubusercontent.com/tz1012/mail-hantong/3eabc14b9ed8463aa58db0304dbf86d6da00bb94/install.sh
sh install.sh
```

macOS 15 Intel or Apple Silicon:

```sh
curl -fsSLo install.sh https://raw.githubusercontent.com/tz1012/mail-hantong/3eabc14b9ed8463aa58db0304dbf86d6da00bb94/install.sh
sh install.sh
```

The installer selects the matching asset, verifies the release checksums and
installs the CLI and Core for the current user. Product use requires no Rust,
Go, Docker, WSL or Python. Repeat the pinned command to update; it preserves
the encrypted database, registration and keys, and does not silently unlock or
send mail. The installed `hantong update` runs the same package-owned updater.

If your shell does not already include the launcher directory, add it for this session:

```sh
export PATH="$HOME/.local/bin:$PATH"
hantong --version
hantong init
```

`hantong-t` provides the full-screen terminal client. Google login uses the
provider browser. On SSH, Google login requires a secure loopback tunnel to a
provider browser; ordinary IMAP/SMTP does not. Actual provider login, sending
and Trash movement on Unix remain outside this synthetic/startup verification.

## Secrets and headless use

On macOS, the product uses the user's real login Keychain. Do not test with an
empty temporary `HOME`; a real `HOME` is required. macOS SSH vault use is not
supported. Desktop Secret Service behavior on Linux is not claimed as verified
by this release.

On Linux without Secret Service, the first startup offers `Secret storage: 1)
headless passphrase 2) OS credential store`; choose `1` for the encrypted
headless passphrase vault. The TTY passphrase is required on every Core restart. There is
no unattended unlock and no passphrase through argv or environment variables.
In a non-TTY context, startup stops with `HumanStepRequired`; it does not fall
back to plaintext storage. A wrong unlock preserves the ciphertext, and a
successful stop/reopen verifies that the encrypted database remains usable.

## Scope and known limits

The native CI and disposable-install smoke covered startup, protected
first-start/menu behavior, Core survival after CLI use, encrypted storage,
verified stop and reopen. Separate engine integration tests used synthetic providers. Real Unix provider
login/send/Trash and an AI conversation were not tested. AI use still requires
installing and logging into the selected Codex/Claude provider and checking its
capabilities; beta.22 makes no all-features claim. SMTP attachments are omitted,
and generic IMAP Trash requires server `MOVE` support and exactly one selectable mailbox advertised as `\Trash`.
Advanced custom Google client configuration is unsupported on Unix; the
ordinary bundled publisher path is supported.

Human security review remains pending. macOS signing and notarization are not
claimed.
