# Native UI Agent Guidelines

## Read the Design Contract

Read [UI_DESIGN.md](../../UI_DESIGN.md) and [UI_THEME.md](../../UI_THEME.md) before UI work. Separate implemented behavior from future work; record contract changes and verification. Root rules extend this guide to `src/ui.rs` and UI-facing worker/model changes.

## Preserve the Existing Zalo Interface

Preserve native Rust/egui and the original Zalo interface. Maintain a complete original [theme/icon inventory](../../UI_ASSET_INVENTORY.md) and native mapping: every theme selector, token/state, font face, glyph/alias, and referenced visual asset across source bundles. Include Light/Dark/Custom palettes and System selection behavior. Verify normal, hover, pressed, focused, selected, and disabled states. Mark unmapped entries pending; a representative subset is not completion. Reuse `visuals.rs`, original fonts/assets, layouts, and components. Do not substitute invented chrome, replacement artwork, a WebView, or another chat client's implementation.

Keep painted controls aligned with their interactive rectangles. Verify the full composer, search, navigation, clipping, scrolling, compact back navigation, and long Vietnamese labels. Scope composer IDs by account and conversation. Preserve per-recipient drafts and IME-aware shortcuts. Clear drafts only after the matching durable acknowledgement, retaining newer edits. Exercise preedit, commit, cancellation, input-method ON/OFF, editor switches, and window focus; Unicode injection alone does not verify IME.

## Keep Identity and State Correct

Preserve account, sender, thread, and message IDs for routing, deduplication, and storage. Resolve display names and avatar initials through scoped presentation snapshots. Direct titles may name only their bound sender; group titles never name members. Use a neutral fallback when a name is unknown.

Reject stale account/thread/generation/query results. Reset presentation caches on account changes and logout. Keep database, filesystem, network, and image decoding off the rendering thread. Use existing bounded workers, queues, caches, event draining, and repaint scheduling; handle queue saturation without dropping drafts or fabricating success. Never load the entire contact directory to name visible bubbles.

## Verify and Coordinate

Keep unsupported actions disabled with honest states; separate demo, offline, sending, accepted, unknown, and failed behavior. Coordinate UI changes with backend/protocol and QA specialists when needed; the manager reviews their outputs.

For affected flows, inspect actual synthetic screenshots at 1200×780 and 640×480. Check interaction as well as appearance. Add regression tests for meaningful behavioral risks, not every cosmetic adjustment. Run relevant formatting, lint, and tests; use [VALIDATION.md](../../VALIDATION.md) for existing fixtures. Report synthetic versus real-server evidence separately. Use disposable profiles and preserve Electron behavior; never capture personal messages, credentials, or account data in fixtures, screenshots, logs, or commits.
