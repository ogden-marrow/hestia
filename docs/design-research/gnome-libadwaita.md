# GNOME HIG and libadwaita (what feels native on this desktop)

Sources: https://developer.gnome.org/hig/ (placeholders, boxed lists, UI styling) and libadwaita `_lists.scss`, `_buttons.scss` (gitlab.gnome.org, read 2026-11-11).

## Metrics (from libadwaita stylesheet)
- Button: min-height 24px plus padding 5px 10px (about 34px tall). Pill button padding 10px 32px.
- Boxed-list row: content min-height 50px (button row 40px); rows padded 2px inside a card; 1px rule between rows; separate cards 12px apart.
- Rich list rows: 32px min-height, padding 8px 12px. List section header: padding 18px top, 6px bottom.
- Toolbars: 6px between widgets.
- Subtitles are `smaller` than titles; differentiate text by size, weight and colour.

## Patterns
- **Status page (empty state):** image, heading that says the state and what to do, optional description, optional action. Illustration only for the app's first empty view; symbolic monochrome icon elsewhere.
- **Boxed list:** for small static lists (preferences). At most two controls per row; clicking the row triggers its control; trailing arrow when it opens another view. Not for long or dynamic lists.
- **Colour:** offer light, dark and follow-system; never colour alone to carry meaning; keep custom styling minimal.

## Takeaways for Hestia
1. Rows: 50px for two-line rows (title + subtitle), 32 to 36 for single-line list rows. Hestia's 32 default is fine for lists; Settings rows should be 50.
2. Buttons about 34px tall (we have 32). Primary pill 40.
3. Empty states follow the status-page structure with one action; Questions' empty pane breaks it (#41).
4. Settings should be boxed lists with title + subtitle per row and one control per row (#72).
5. Group headers need 18px above and 6 below: tighter header-to-content than content-to-next-header.
