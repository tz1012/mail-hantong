# Design

## Source of truth
- Status: Active; refreshed 2026-11-04.
- Surfaces: index.html installation/product guide; download.html; shared legal pages.
- User direction: follow https://zele.sh/ design and contents structure, while highlighting HanTong's strengths. This authorizes that reference direction, not a pixel clone.
- Evidence: public zele HTML and 1440x1100 headless capture in the owned development target; existing beta22 guides, immutable release notes, synthetic TUI assets and tools/check-share-preview.py.

## Brand
- Quiet, readable, practical. HanTong green, neutral white, real blue TUI imagery.
- Trust through exact package conditions, synthetic-image labels and separated verification history.
- Avoid invented speed/security claims, borrowed logos/copy, calendar/free-address capabilities or fake search/chat controls.

## Product goals
- Explain the product before asking users to install; make install/account/mail/AI instructions easy to find.
- Lead with external local-stdio MCP connectivity and concrete user control of account/tool/data-transfer scope and per-message send/Trash confirmation. Mailbox change hints are the third strength; CLI/TUI follows.
- Success: clear first-screen product/OS/CTA; functioning TOC/search and copy controls; readable mobile documentation.
- Non-goals: app UI changes, new product features, signup/backend, competitor parity.

## Personas and jobs
- Personal/work multi-account users; keyboard-oriented readers; people optionally connecting AI mail tools.
- Desktop installation and headless Linux TTY setup; no developer-tool prerequisites for product use.

## Information architecture
- Centered hero and three compact product strengths, then a left grouped sticky TOC and readable right documentation column.
- Order: external agent connection, permissions/security, opt-in mailbox hints, installation/account connection, mail and TUI, built-in AI, troubleshooting, validation and limits.
- Download route provides four platform assets and directs users back to the relevant guide anchors.

## Design principles
- Adopt zele's white-space, restrained dividers, serif hero, document-like code/table content and grouped sidebar.
- Give HanTong's screenshot and distinctive advantages higher placement; do not import reference product claims.
- Keep one source of product truth: release23 source and verification record, including historical caveats.

## Visual language
- Neutral white/ink, forest-green accents, muted gray and subtle rules; optional dark display theme.
- System sans for prose, available Korean serif for hero, system monospace for commands; no third-party font requests.
- Content frame up to 1120px, sidebar 210px, 52px gap; prose about 760px; 8px spacing rhythm.
- Small 6–10px corners for controls/code, no heavy shadows; CSS-only subtle dotted hero backdrop.
- Synthetic screenshot remains unchanged, versioned, with accessible alt/caption. No motion required.

## Components
- Reuse existing OG tags, images, download URLs, source-pinned install commands and guide sections.
- Add grouped contents links, TOC filter, copy-code buttons, compact strengths row and validation/FAQ sections.
- Plain HTML/CSS, tiny progressive-enhancement script; shared styles.css owns tokens.

## Accessibility
- Aim for WCAG AA: semantic landmarks/headings, skip link, visible focus, descriptive labels, native details and buttons.
- Keyboard-operable menu/search/theme/copy; readable contrast; search shortcut excludes input fields.
- Honor reduced motion; no essential content hidden until JS runs.

## Responsive behavior
- Desktop >=1000px: sticky left TOC plus right document. Smaller screens: collapsible native contents above document.
- At 390px, no document overflow; tables and command lines scroll only inside their containers.
- Links/buttons work without hover; no fixed overlay obscures content.

## Interaction states
- Search filters TOC links only and announces no matches; Escape clears.
- Copy reports success or a selection hint on permission failure; no false success.
- Theme changes presentation only; storage failure does not break page. Static content remains available offline after load.
- Downloads are ordinary public links; no fabricated progress/backend state.

## Content voice
- Korean user language; short explanations before commands. Explain Core as an automatically started background mail process.
- Separate shipped capabilities, test coverage and remaining work. No new whole-suite/provider-test claim.

## Implementation constraints
- GitHub Pages static files; no new dependencies/build framework. Preserve social-preview URLs and immutable release binaries.
- Test HTML IDs/links/metadata, JS syntax/interaction, desktop/mobile screenshots and overflow before publication.
- Existing runtime/source worktree and installed accounts/keys remain untouched.

## Open questions
- None blocking this authorized reference-inspired implementation. Actual Kakao cached-card display remains outside the site QA.
