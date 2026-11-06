# Native UI Agent Guidelines

## Read the Design Contract

Read [UI_DESIGN.md](../../UI_DESIGN.md), [UI_THEME.md](../../UI_THEME.md) and [UI_REDESIGN_GUIDE.md](../../UI_REDESIGN_GUIDE.md) before UI work. Separate implemented behavior from future work; record contract changes and verification. Root rules extend this guide to `src/ui.rs` and UI-facing worker/model changes.

## Follow the Redesign, Keep Native Rust/egui

The owner approved the "Zalo Chat Redesign" (Light and Dark) on 2026-11-06 as the target look of the native client. [UI_REDESIGN_GUIDE.md](../../UI_REDESIGN_GUIDE.md) is the contract for colors, layout dimensions and control states; where it disagrees with the original Zalo CSS or with the "Hiện tại" text in `UI_DESIGN.md`/`UI_THEME.md`, the guide wins. The guide describes the target only: code that has not been migrated is still the original look, so never document the redesign as implemented before it is.

Stay native Rust/egui. Do not add a WebView, and do not copy code or assets from another chat client (including ctdozlite). Keep the embedded Segoe UI faces and the original glyph/icon registry for text and icons; do not add web fonts or hand-drawn icon substitutes, and mark any control with no source glyph as unmapped in the [inventory](../../UI_ASSET_INVENTORY.md). Light, Dark, System and Custom stay the only theme modes. Resolve every color through `crate::theme::palette`, never a hex literal in `ui/`. Changing a `Palette` value or role means updating the guide's token table in the same change. Verify normal, hover, pressed, focused, selected and disabled states in both Light and Dark, and measure contrast on rendered pixels when a token changes.

Keep painted controls aligned with their interactive rectangles. Verify the full composer, search, navigation, clipping, scrolling, compact back navigation, and long Vietnamese labels. Scope composer IDs by account and conversation. Preserve per-recipient drafts and IME-aware shortcuts. Clear drafts only after the matching durable acknowledgement, retaining newer edits. Exercise preedit, commit, cancellation, input-method ON/OFF, editor switches, and window focus; Unicode injection alone does not verify IME.

## Keep Identity and State Correct

Preserve account, sender, thread, and message IDs for routing, deduplication, and storage. Resolve display names and avatar initials through scoped presentation snapshots. Direct titles may name only their bound sender; group titles never name members. Use a neutral fallback when a name is unknown.

Reject stale account/thread/generation/query results. Reset presentation caches on account changes and logout. Keep database, filesystem, network, and image decoding off the rendering thread. Use existing bounded workers, queues, caches, event draining, and repaint scheduling; handle queue saturation without dropping drafts or fabricating success. Never load the entire contact directory to name visible bubbles.

## Verify and Coordinate

Keep unsupported actions disabled with honest states; separate demo, offline, sending, accepted, unknown, and failed behavior. Coordinate UI changes with backend/protocol and QA specialists when needed; the manager reviews their outputs.

For affected flows, inspect actual synthetic screenshots at 1200×780 and 640×480. Check interaction as well as appearance. Add regression tests for meaningful behavioral risks, not every cosmetic adjustment. Run relevant formatting, lint, and tests; use [VALIDATION.md](../../VALIDATION.md) for existing fixtures. Report synthetic versus real-server evidence separately. Use disposable profiles and preserve Electron behavior; never capture personal messages, credentials, or account data in fixtures, screenshots, logs, or commits.
