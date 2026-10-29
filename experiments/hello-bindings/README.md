# mail-hantong Windows Hello bindings experiment

This is an isolated compile-only experiment for the generated Windows Rust
projection used by `hantong-platform`. It is not product code and is not a
trusted approval implementation.

Pinned direct dependencies:

- `windows = 0.62.2`, exact pin, `std`, `Security_Credentials_UI`,
  `Win32_Foundation`, `Win32_System_WinRT`, and
  `Win32_UI_WindowsAndMessaging` features.
- `windows-future = 0.3.2`, exact pin with `std`, for the non-blocking
  `AsyncOperationCompletedHandler` completion surface.

The experiment verifies compilation of:

- `factory::<UserConsentVerifier, IUserConsentVerifierInterop>()`;
- `IUserConsentVerifierInterop::RequestVerificationForWindowAsync` with an
  owned typed `HWND` and `HSTRING` message;
- typed `IAsyncOperation<UserConsentVerificationResult>`;
- availability operation and all current result constants;
- `AsyncOperationCompletedHandler`, `Cancel`, and `Close`;
- `RoInitialize(RO_INIT_SINGLETHREADED)` and `RoUninitialize` symbols.
- `MSG`, `GetMessageW`, `TranslateMessage`, and `DispatchMessageW` message-loop
  symbols.

No helper is called by a test or binary. No window is created, no native
Windows Hello prompt is shown, no enrollment/account is touched, and no
approval is generated. The typed `HWND` is only present at a compile-time
wrapper boundary; ownership validation remains a product implementation task.

Run from the repository root on Windows:

```powershell
python -X utf8 scripts/cargo_native.py check --manifest-path experiments/hello-bindings/Cargo.toml --target-dir D:/BuildCache/hantong-hello-bindings-experiment
python -X utf8 scripts/cargo_native.py clippy --manifest-path experiments/hello-bindings/Cargo.toml --target-dir D:/BuildCache/hantong-hello-bindings-experiment --all-targets --locked -- -D warnings
```

After the first successful dependency resolution creates `Cargo.lock`, repeat
both commands with `--locked`. The isolated manifest owns its own workspace and
does not participate in the product Cargo workspace or its target directory.

Evidence sources:

- <https://crates.io/crates/windows/0.62.2>
- <https://crates.io/crates/windows-future/0.3.2>
- <https://docs.rs/crate/windows/0.62.2/source/src/Windows/Win32/System/WinRT/mod.rs>
- <https://docs.rs/windows-future/0.3.2/windows_future/struct.IAsyncOperation.html>
- <https://learn.microsoft.com/en-us/windows/win32/api/userconsentverifierinterop/nf-userconsentverifierinterop-iuserconsentverifierinterop-requestverificationforwindowasync>

This experiment does not prove runtime behavior on a configured Windows Hello
device, message-loop correctness, real HWND ownership, or product approval
policy compliance.

## Validation record (2026-10-30)

Host: Rust `1.95.0`, `x86_64-pc-windows-msvc`. The first unlocked check
resolved and wrote this experiment's `Cargo.lock` (15 packages). The locked
verification then completed successfully:

```text
cargo fmt --manifest-path experiments/hello-bindings/Cargo.toml
  exit 0
python -X utf8 scripts/cargo_native.py check --manifest-path experiments/hello-bindings/Cargo.toml --target-dir D:/BuildCache/hantong-hello-bindings-experiment --locked
  Finished `dev` profile [unoptimized + debuginfo]
  exit 0
python -X utf8 scripts/cargo_native.py clippy --manifest-path experiments/hello-bindings/Cargo.toml --target-dir D:/BuildCache/hantong-hello-bindings-experiment --all-targets --locked -- -D warnings
  Finished `dev` profile [unoptimized + debuginfo]
  exit 0
cargo fmt --manifest-path experiments/hello-bindings/Cargo.toml -- --check
  exit 0
```

The checks compile only the projection and wrapper signatures. They do not
create a process window, initialize a Windows Runtime apartment, call the
factory, invoke `RequestVerificationForWindowAsync`, or exercise cancellation
against a live operation. The experiment therefore provides compile evidence,
not runtime or security approval evidence.
