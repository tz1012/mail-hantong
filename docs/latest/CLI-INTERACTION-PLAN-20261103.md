# CLI interaction plan

The command line client and the communications-style fullscreen TUI share the
Core and authenticated IPC contracts, but they have separate interaction
models. This plan describes the ordinary `hantong` CLI flow.

## Menu interaction

- Menus remain usable with typed values for compatibility.
- On an interactive Windows terminal, the bounded command prompt also accepts
  Up/Down and Enter. The highlighted menu value is returned as its original
  command value, so numeric account and folder choices keep their existing
  semantics.
- Typing `/` immediately displays bounded command hints. Escape returns to the
  previous menu as `/back`; Ctrl+C cancels the current prompt.
- The same prompt is used for account selection, connection choices, feature
  choices, and the mail reader. Noninteractive and JSON paths retain their
  existing HumanStepRequired behavior.

## Commands available while browsing

`/ai codex`, `/ai claude`, and `/agent` enter an AI session through the normal
mail-consent and provider boundary. `/language ko|en` changes the display
language for the current CLI session. `/help` shows the current menu help and
`/back` returns to the previous screen.

## Mail reader

Both Gmail and generic IMAP accounts use the same ordinary CLI reader. Inbox
is listed first, folders and messages are selectable with arrows or numbers,
and the message view shows escaped, untrusted header fields before the bounded
body. Escape or `/back` returns to the previous list. AI and language commands
remain available while reading. No account, provider, or authority is changed
by display navigation.

Language selection is session-only; account aliases and authorization remain
separate display and policy concerns.

The reading view keeps its envelope above an 18-line body page; `/next` and
`/prev` move between pages without returning to the message list. Esc returns
to that list with the selected row preserved. Header decoding supports bounded
RFC 2047 B/Q words, including Windows Korean charset aliases. Controls and
direction overrides remain untrusted display data and cannot execute commands.

In the fullscreen TUI, `:` followed by `language ko` or `language en` changes
the current display language. The current message keeps its identity and
pending read; generated envelope labels follow the new locale when it finishes.
