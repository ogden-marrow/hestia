# Hestia design

Hestia should feel like it was made by someone who cared about every pixel, and then got out of the way. The aesthetic is already chosen: **homey, flat, classic Mac**, with light, dark and auto, and one accent colour. This document is the rulebook; `ui/src/design.rs` is the only place its numbers live. A literal size, radius, weight or duration in a screen is a bug.

## Principles (Apple HIG, applied)

1. **Clarity.** One job per screen and one primary action. If you cannot say what the screen is for in six words, split it.
2. **Deference.** Content is the interface. Chrome is quiet: hairline rules, no shadows on things that do not float, colour only where it means something.
3. **Depth.** Hierarchy comes from size, weight and space, in that order, never from boxes around everything. Only sheets and popovers float.
4. **Calm words.** Plain language a non-technical person would say. No *submodule, remote, diff, sha, repo* on a primary screen. Verbs on buttons ("Create my private copy", not "Initialise").
5. **Every state is designed.** Empty, loading, busy, done and failed each have a look. An error is never a grey line.
6. **Everything is reachable.** Keyboard for every control, a visible focus ring, readable at 150% text and at 960px wide, never colour alone.

## Spacing: the 4pt grid

`space::XS 4 · SM 8 · MD 12 · LG 16 · XL 24 · XXL 32` (and `HAIR 2` inside a single control). Nothing else.

| Use | Token |
|---|---|
| Icon to its label | `HAIR`/`XS` |
| Neighbouring controls | `Design::gap()` (4 / 8 / 12 by density) |
| Inside a row, left and right | `Design::item_px()` (8 / 12 / 16) |
| Inside a pane | `Design::pad()` (12 / 16 / 20) |
| Between groups on a screen | `Design::section()` (32) |

Alignment rule: text in a list shares one left edge; numbers share one right edge; the pane title, the first group and the first row share the same left edge.

## Type scale

Six roles, three weights (400 / 500 / 600); large titles are Medium (Apple's are Regular) and Semibold is for 15 and below (`docs/design-research/apple-hig.md`), the Ubuntu Sans family, mono only for `Caption`.

| Role | Size / weight | Used for |
|---|---|---|
| `Display` | 28 / 500 | First launch and designed empty states; at most one per screen |
| `Heading` | 20 / 500 | The pane's single title |
| `Title` | 15 / 600 | A group or card name |
| `Body` | 13 / 400 | Reading text, row names |
| `Label` | 12 / 500 | Secondary text, button labels, column heads |
| `Caption` | 11 / 500 mono | Counts, paths, timestamps |

Rules: one heading per pane; never a title that repeats the sidebar item or the group below it (issue #38). All sizes multiply by the user's text size. Secondary text uses `text_secondary` and never goes below `Label` for anything a person must read.

## Colour roles

Neutral surfaces, one accent. The accent is the user's choice and appears in a few places only: the primary button, the selected row tint, the focus ring, the "needs you" count, and sidebar icons (Apple's default, so the user's accent reaches them).

| Role | Source |
|---|---|
| Backdrop, pane, sidebar | `Theme::bg`, `Theme::card`, `Design::side_fill()` |
| Text, secondary text | `Theme::text`, `Theme::text_secondary` |
| Rule | `Theme::card_border` (1px) |
| Selection | `Design::select()` |
| Primary fill | `Design::accent_color()` with `on_accent()` text |
| Danger (to add, #35) | one red, only on destructive confirms and errors |

Every text/fill pair must pass 4.5:1 in light and dark for every accent (issue #37).

## Radii (concentric)

`CONTROL 4` buttons, fields, rows · `CARD 6` tiles and cards · `SHEET 10` sheets and popovers · `PILL` chips and badges. An inner radius is the outer radius minus the padding between them, never below `CONTROL`.

## Elevation

Two levels. **Flat** (everything, separated by a 1px rule) and **Raised** (sheets and popovers only: one soft shadow, used nowhere else).

## Controls and hit targets

Rows and buttons are at least 28pt tall (`hit::MIN`), the primary action 36 (`hit::PRIMARY`). One button component with states idle, hover, pressed, busy, done, failed, disabled and focused (issue #34). One primary button per screen.

## Motion

`INSTANT 100ms` hover and press, `QUICK 180ms` selection and appearing rows, `SETTLE 280ms` a pane or sheet arriving. Motion shows where something came from or went to. With reduced motion on, durations are zero.

## Components (inventory; each lives once)

Button · Chip/Badge · Row · Card · Field with label · Segmented/Toggle · Sheet · Empty state · Busy/Error banner · Status strip. If a screen needs something not listed, add it here first.

## Adoption

Tokens land first (this change); each surface then moves over in its own PR, in this order: Welcome, Today, Questions, sidebar, Settings, Files, History, Machine. Each PR removes the literals it replaces; `design::Design::r()` and `style()` go away when the last surface has moved.

## Checking

`hestia --snap out.png --scene <look>-<pane> --size 960x640` renders headless. Every surface PR attaches light and dark snapshots at 960px and 150% text, and the review board (industrial designer, first-time user, keyboard user) signs off in the PR.
