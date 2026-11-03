# Terminal display modes

Status: implemented candidate; human security review pending. Requirements: CLI-01, CLI-16, OPS-07/08 display scope. Latest user decision: 2026-11-03.

`hantong` remains the command-oriented/guided CLI. `hantong-t` is a separate presentation entry point built in the same workspace and shipped in the same package. Both use the existing CLI/Core services, account bindings and policies. No provider credentials, provider adapters or database are accessed by the display layer.

## Screen and preferences

`hantong-t [--lang en|ko] [--account ID] [--theme blue|green|amber|mono|bbs8|bbs16]` enters an alternate-screen Windows console with fixed navigation, message list, body and status areas. The default theme is blue. `F` cycles blue, green, amber, mono, bbs8 and bbs16 without changing selection, mail state or authority. `bbs8` and `bbs16` are ANSI eight-color and sixteen-color communications-terminal palettes. Theme choice lasts for the current process only. Wide windows split the screen; the body receives about 60% of the remaining width after navigation, while compact windows stack list/body. Body scrolling refers to wrapped display rows; it is not a send preview or approval receipt.

`--plain` retains the previous line-oriented compatibility view. `--demo` uses only synthetic in-memory data and disables account actions and command handoffs. Neither mode grants any capability. Non-console full-screen input fails instead of synthesizing input in another window.

The `u` action creates a bounded read-only projection of cached Inbox pages for
each ready account, adds an account tag to every displayed row, and sorts the
combined view for navigation. A failed or unavailable account is kept as a
tagged status; an empty account set does not mean all accounts. The `v` action
expands the focused body viewport. These actions do not change provider state.

The `a` action stores an account alias in the terminal display preferences.
Aliases are persistent labels only and never replace the account ID used for
authentication, From, provider scope, policy checks, or provider requests.
The preference file is a separate display-only record with bounded values and
compare-and-swap updates; theme selection is still process-local.

## Input

Arrows or j/k move; Enter opens; Tab changes area; Esc/h goes back; PageUp/PageDown and Home/End scroll the body. `u` opens the unified cached-Inbox projection, `v` expands the focused body, and `a` edits the selected account's display alias. Mouse clicks select a row, double-click opens it, and wheel scrolls the region under the pointer. Footer buttons share their exact geometry with the mouse hit-test. The left sidebar contains quick access slots; the complete account/folder list remains keyboard-accessible without an eight-account limit.

`c` opens the existing composing flow; `t` opens the existing single-message Trash review; `:` accepts only the existing Google login and AI session command forms. The console's raw input, alternate screen and cursor are restored before inherited interactive prompts, then the display resumes. These keys never directly issue a send/MOVE/approval. Core-owned full previews, account/epoch/content binding, single claim, audit and unknown/no-retry semantics remain authoritative.

All display values are untrusted. Terminal control sequences, bidi/zero-width controls and line separators must not execute. Layout clips labels and wraps reading text. A truncated provider body is labelled as partial. Console modes and original cursor visibility are restored on normal exit, cancellation and recoverable errors. Physical input injection into another process is not part of this API.

No schema, grants, OAuth scope or external AI consent changes are introduced by the terminal display mode. Mouse/keyboard selection is application intent, not OS human-presence proof.
