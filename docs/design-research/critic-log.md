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
