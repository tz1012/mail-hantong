# Unix installation and use - beta.23 release scope

Beta.23 release scope has native CLI/Core packages for macOS 15 (Intel and Apple
Silicon) and Linux x86_64 with glibc >= 2.39. The four release assets are
Windows x64, Linux x64, macOS Intel and macOS ARM. There is no Linux ARM asset.
For installation, download the matching archive from the beta.23 release tag
and verify its SHA256SUMS. Published beta.22 remains the frozen v2 baseline.

Release metadata and SHA256SUMS identify the exact beta.23 source and archive;
this guide does not pin a moving source commit.

## Install

Download the installer to a file, inspect the file if required by local policy,
then execute it from a protected interactive terminal. Do not pipe a download
directly to a shell, and do not put passphrases in argv or environment
variables.

Linux x86_64:

```sh
curl -fsSLo install.sh https://raw.githubusercontent.com/tz1012/mail-hantong/v0.1.0-beta.23/install.sh
sh install.sh
```

macOS 15 Intel or Apple Silicon:

```sh
curl -fsSLo install.sh https://raw.githubusercontent.com/tz1012/mail-hantong/v0.1.0-beta.23/install.sh
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
provider browser; ordinary IMAP/SMTP does not. Actual provider login, sending and Trash movement on Unix remain outside this
synthetic/startup verification. SSH Mac vault use is unsupported.

## Secrets and headless use

On macOS, the product uses the user's real login Keychain. Do not test with an
empty temporary `HOME`; a real `HOME` is required. macOS SSH vault use is not supported. Desktop Secret Service behavior on Linux is not claimed as verified
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
capabilities; beta.23 makes no all-features claim. SMTP attachments are omitted,
and generic IMAP Trash requires server `MOVE` support and exactly one selectable mailbox advertised as `\Trash`.
Advanced custom Google client configuration is unsupported on Unix; the
ordinary bundled publisher path is supported.

Human security review remains pending. macOS signing and notarization are not
claimed.

## Diagnosing Google callback failures (post-beta.23 correction)

An OAuth callback only confirms that the browser returned. Token exchange,
verified identity, protected credential storage, account registration and local
profile activation must also finish. `CLIENT_NOT_REGISTERED` and an empty
account list after an earlier OAuth failure do not establish which stage failed.

The beta.23 Unix HTTP reader could classify a fully received response as
`OFFLINE` when a TLS peer omitted `close_notify`. The correction validates exact
Content-Length or chunked termination and returns once that bounded frame is
complete. Truncated/ambiguous frames and TLS verification failures still fail;
certificate checks remain enabled. This reproduced defect is not proof that it
caused every reported OAuth failure. Close-delimited bodies and chunk trailers
remain unsupported by this strict transport.

Background Core detaches its standard streams. For an intentional Linux diagnostic
run, use the installed Core in a foreground terminal instead. Stop any existing
Core through its verified lifecycle command, then keep the first terminal open:

```sh
root="${XDG_DATA_HOME:-$HOME/.local/share}/mail-hantong-cli"
version=$(cat "$root/current.txt")
core="$root/versions/$version/bin/mail-hantong-core"
"$core" unix-stop
umask 077
"$core" run 2> "$HOME/hantong-google-diagnostic.log"
```

This uses the default Linux installation location; use your chosen installation root
if you installed elsewhere. On macOS, use
`root="$HOME/Library/Application Support/mail-hantong-cli"` instead. The stop command preserves accounts and encrypted
data; it does not unlock the vault. Complete any protected vault input in that
terminal, run `hantong` in a second terminal and repeat your own Google consent.
Do not redirect the foreground Core's stderr to `/dev/null`. Ctrl+C in the first
terminal ends this diagnostic run.

Only share the closed diagnostic records, not a complete terminal transcript:

```sh
grep -E '"event":"google_(oauth|transport)_stage"|^hantong_google stage=' \
  "$HOME/hantong-google-diagnostic.log"
```

Transport records distinguish DNS/connect/TLS/write/read/framing and fixed
timeout/EOF/I/O/TLS/frame/quota kinds. Existing OAuth records distinguish token
exchange, response validation and verified identity. Core records distinguish
OAuth completion, secret store, account registration, profile publication and
scope activation at the instrumented failure boundaries. An absent stage record
does not prove that a stage succeeded. These diagnostics accept only fixed stage/kind/error values;
they do not include URLs, account/request IDs, authorization codes, tokens,
credentials, or provider response text. They cannot recover diagnostics that
were discarded by an earlier `/dev/null` run. Never send a credential, callback
URL, database or key file with a bug report.

## Diagnosing protected IMAP/SMTP input (post-beta.23 correction)

beta.23 collapsed failures from hidden terminal input and encrypted temporary
staging into `APPROVAL_REQUIRED` (exit18). That code did not establish that an
extra approval was missing, nor that the provider had accepted the credential.
The corrected foreground helper preserves these existing stable categories:

| Failure | Error code |
| --- | --- |
| Empty or invalid input | `INVALID_INPUT` |
| No usable protected TTY | `HUMAN_STEP_REQUIRED` |
| Capture I/O failure | `INTERNAL_ERROR` |
| Ticket expired before or after input | `APPROVAL_EXPIRED` |
| Protected staging/root failure | `INTERNAL_ERROR` |
| Credential/ticket binding mismatch | `SCOPE_DENIED` |

Secrets emits `hantong_terminal_secret stage=input` or
`stage=protected_staging` with a fixed kind such as `empty_input`,
`tty_unavailable`, `input_io`, `timeout`, `timeout_after_input` or
`protected_store`. The foreground helper emits a separate `stage=capture`
result and stable error code. Input and staging errors do not submit a provider
login; cancellation cleanup is attempted once. If cleanup fails its stable
error may be returned, while an already expired capture remains expired.

To capture these foreground helper diagnostics, use an interactive terminal:

```sh
umask 077
hantong --lang ko 2> "$HOME/hantong-input-diagnostic.log"
grep '^hantong_terminal_secret ' "$HOME/hantong-input-diagnostic.log"
```

The records contain no password, content, length, digest, path, account ID or
key. Keep the existing app password and stored account files. Do not create a
new app password or grant broader permissions merely because beta.23 printed
exit18. A successful hidden input is still separate from successful IMAP/SMTP
authentication. The existing protected input reader can wait for Enter; ticket
expiry is rechecked before staging and cannot authorize an expired submission.

## Diagnosing IMAP/SMTP submission after protected staging (beta.25)

Successful protected input and an encrypted stage do not establish that the
background Core opened its credential backend or authenticated with IMAP.
An unlocked GNOME vault and working D-Bus are useful evidence; do not infer
that another unlock or a new app password is required from `INTERNAL_ERROR`.

beta.25 separates these failure boundaries:

| Record | Meaning |
| --- | --- |
| `hantong_secret_store stage=root_validation` | Protected directory guard rejected the root; includes the existing stable guard error code |
| `hantong_secret_store stage=platform_store_init` | Host credential backend initialization failed; includes a closed backend error category |
| `hantong_terminal_secret stage=store_open` | Core could not open Secrets for submission |
| `hantong_terminal_secret stage=stage_handle_open` | Encrypted stage lookup, ticket expiry or binding validation failed |
| `hantong_terminal_secret stage=secret_job_submit` | Job admission failed, for example queue capacity or expiry |
| `hantong_terminal_secret stage=secret_job_execute` | An admitted setup job failed later; this alone does not establish an IMAP login attempt |

Use the foreground Core diagnostic procedure above to retain Core-side stderr;
redirecting only `hantong` cannot capture a detached Core's diagnostics. Share
only matching closed records from the protected log:

```sh
grep -E '^hantong_(secret_store|terminal_secret) stage=' "$HOME/hantong-google-diagnostic.log"
```

These records contain no credential, path, account/request/ticket ID, backend
exception text or encrypted file content. A failed store/stage open leaves
the live review intact and admits no provider job. Normal foreground cancellation
and expiry rules still apply; preserving a review does not extend its lifetime.
Keep any existing encrypted stage and app password while investigating; do not
open or upload the stage, DB or key files. These diagnostics narrow the failure
boundary and do not by themselves fix or prove the reported provider connection.


## Interrupted IMAP setup and retry (beta.26)

A queued protected-input job is not evidence of a successful provider login.
An earlier interrupted attempt can leave a Connecting account and endpoint.
Creating another account with the same canonical server/login then conflicts
before authentication. beta.26 reuses the exact recorded attempt and offers an
explicit Continue/Cancel choice. Continue creates a fresh resume request bound
to the existing account, current endpoint versions and observations. Enter the
existing app password again through protected input; old encrypted stages are
not silently reused. Cancel creates no resume child or provider job.

Changing the server, port, TLS mode or account display settings for an already
staged target is rejected rather than overwriting the earlier configuration.
An active capture, queued job or running job cannot receive another capture
ticket or duplicate submit. A failed worker no longer remains shown as running
when no job exists. Existing accounts, keys and setup history are preserved.

Core also emits fixed `hantong_imap_setup stage=... kind=transition_failed`
records with the stable error code. Stages distinguish `load`, `review_state`,
`account_stage`, `credential_begin`, `credential_finish`, `credential_store`,
`probe_begin` and `probe_finish`. Use the foreground Core procedure above and
share only the following records:

```sh
grep -E '^hantong_(secret_store|terminal_secret|imap_setup) stage=' \
  "$HOME/hantong-google-diagnostic.log"
```

No record includes credentials, account/request IDs, provider replies or file
paths. A `CONFLICT` summary from beta.25 does not establish which transition
failed on a particular machine. These corrections reproduce one interrupted
setup defect with synthetic data; actual Daum authentication and mail reading
still require reconnecting and testing on the reporting machine. Keep the
existing app password and encrypted files; do not reset the vault or upload them.
