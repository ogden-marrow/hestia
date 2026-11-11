# Critic log

One entry per pass: who looked, at what, what they found, where it went (issue / PR). Snapshots come from `ui/scripts/snap-matrix.sh` and are kept outside the repository because they render the machine's real data.

## 2026-11-11 pass 1: baseline of `main` (Today, dark/light, 100% and 150% text, 960x640)
Persona: hostile industrial designer + QA (self-run; independent critics follow once boards exist).
- **QA, 150% text, Today:** tile 3 caption "folders both models agree on" and tile 4 caption "files gone, kept in history" overprint each other; tile 4 is cut at the right edge. Fixed widths do not follow text size. Existing #39, #40; evidence added.
- **Designer, Today light:** greeting is ~40px at bold weight, the largest thing on the screen, ahead of anything the person should act on; four accent-blue numerals compete with it. Apple's Large Title is 26 Regular. -> #68, #73, DESIGN.md heading weight lowered to 500.
- **Designer, sidebar:** full-bleed solid accent selection plus an accent icon on the same row; badges: `8`, `git`, `0`, hollow circle. -> #50.
- **First-time user, Today:** "folders both models agree on", "kept in history", "Jump back in" mean nothing without context. -> #73, #22.
- **Data hygiene:** snapshots show the machine's real counts, dates and folder names; they must not be committed. Fixture-driven scenes are part of #46.

## 2026-11-11 pass 2: Today rebuilt (PR ui/today-calm)
- Before: 4 accent numerals, 4 icon cards, 2 duplicate headings, jargon ("both models agree"), 150% text overprint.
- After, dark/light, 100%/150%, 960x640: one greeting (Display), one sentence, "Nothing needs you." or a folder list with one primary button, one quiet facts line. No overprint at 150%.
- Honest remaining critique (designer): the lower half of the screen is empty when nothing needs you; this is where the stream goes (#81, #13). Sidebar badges (`git`, hollow circle) and the duplicated "0 need you"/status-strip are untouched (#50, #30). Needs-you state (folder list + primary) is covered by a render test but there is no snapshot of it until fixture scenes exist (#46).

## 2026-11-11 pass 3: four independent critic agents, 16 snapshots (real-data boards kept outside the repo; 960x640 dark and light@150%)
Agents: industrial designer; first-time user + copy editor; keyboard/low-vision + QA; GNOME/libadwaita purist. Findings were de-duplicated against open issues; each landed as a comment or a new issue.
- **Agreed by all four (S1):** Machine and History collapse at 960 wide (detail column about 60pt, one character per line; diff pane shows only line numbers); Files clips and overprints at 150%. -> #39 (evidence added), #94, new pane-grid issue #93.
- **Designer:** solid full-bleed sidebar selection plus a second solid bar in second-level lists; no shared left edge across panes; Settings swatches/controls ragged; icon set inconsistent; launcher clip at 150%. -> #50, #93, #72, #95, #96.
- **Copy editor:** `git`, `main…origin/main`, hashes, `+107 −0`, dotfiles/apt/snaps/flatpaks, `regex`, `inboxes`, `Remove from tree`, "0 need you"; nothing says "your files stay where they are". -> #90 (full replacement table).
- **Keyboard/low-vision:** no focus ring or keyboard operation beyond Time; white-on-accent about 3.7:1; borders about 2:1; tint-only selection; hit targets under 28pt; glyph-only controls without names; fixed-width nested panes; unvirtualised lists. -> #32, #37, #34, #40, new large-list issue.
- **GNOME purist:** missing window controls (verify SSD), Settings as page/dialog with boxed rows and real switches, status-page empty states, button 34/40, one suggested action, destructive styling, shortcut hints. -> #71, #91, #41, #34, #35, #92.
- **Caveat:** estimated contrast ratios and unmeasured performance claims are the critics' estimates; verify before acting (#37 requires a test).
