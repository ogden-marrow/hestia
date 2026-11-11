# Apple Human Interface Guidelines

Source: https://developer.apple.com/design/human-interface-guidelines/ (typography, sidebars, onboarding), read 2026-11-11.

## macOS text styles (published table, points: size / line height)
| Style | Regular size/lh | Emphasized |
|---|---|---|
| Large Title | 26 / 32 | Bold |
| Title 1 | 22 / 26 | Bold |
| Title 2 | 17 / 22 | Bold |
| Title 3 | 15 / 20 | Semibold |
| Headline | 13 / 16 | Heavy (is bold) |
| Body | 13 / 16 | Semibold |
| Callout | 12 / 15 | Semibold |
| Subheadline | 11 / 14 | Semibold |
| Footnote / Caption 1 | 10 / 13 | Semibold / Medium |

Measured observations
- Body is 13 on a 16 line (1.23). Every large title style is **Regular** weight; emphasis is an option, not the default.
- The ladder is 26, 22, 17, 15, 13, 12, 11, 10: steps of roughly 1.15 to 1.3.
- Hestia (DESIGN.md) uses 28/20/15/13/12/11 with Heading and Display at 600. Heavier than Apple's titles.

## Sidebars
- Two levels of hierarchy at most; short group titles.
- Icons use the **accent colour by default**; fixed colours only sparingly, with purpose.
- Row height, text and glyph size come from one of three sidebar sizes (small / medium / large) the user can pick.
- Avoid critical information or actions at the bottom (windows get moved off-screen there).
- Let people hide the sidebar, but not hidden by default.

## Onboarding
- "Fast, fun, and optional." Happens after launch, not as part of it.
- Teach by doing; prefer context-specific tips near the thing they explain.
- Prerequisite flows must be brief; teaching too much overwhelms.
- Skipped tutorials must not reappear, and must be findable later (help/settings).
- Postpone non-essential setup; ship reasonable defaults. Ask for permission where the feature needs it, with the reason.

## Takeaways for Hestia
1. Headings: Regular or Medium at the larger sizes; reserve Semibold for 15 and below. (Issue: type weight, DESIGN.md.)
2. Body line height 16/13 (1.23), not 1.25 of the size, and Title 3 = 15/20.
3. Sidebar icons may carry the accent; selection is a tint. Update accent rule.
4. Onboarding: five steps is the ceiling; the repository/GitHub step is setup, not teaching, so it moves to a later, contextual prompt (backend flow: #25).
5. Settings gets a "reopen the tour" entry (exists) and the tour is never shown twice (exists).
