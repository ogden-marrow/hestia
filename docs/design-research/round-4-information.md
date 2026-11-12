# Round 4: what the app must tell you

Research notes (2026-11-12). The question: does each screen give the person the *right information* to get things done? Sources are Apple's Human Interface Guidelines pages for Feedback, Settings, Onboarding and Writing, the GNOME HIG on banners and toasts, libadwaita's status page, and backup-UX discussions (Roon, Seedvault, a cloud-sync recovery write-up).

## What the sources say, and what Hestia does about it

| Source says | Hestia rule |
|---|---|
| Match the way feedback is delivered to its importance. Show status passively, near what it describes; interrupt only for failure or data loss. | Backup status lives in the header on every screen. Colour and a card appear only when something needs the person. No success toasts. |
| Confirm only what people do not expect to succeed. | No "saved" confirmations for routine actions; failures say why and what to do. |
| A banner is attention-grabbing, so only for important things; toasts for short-lived one-offs. | One banner-like card per screen at most (Today's backup card). |
| Label what a timestamp really measures; show an old state as last-known, not a permanent green tick. | "Backed up 12 minutes ago" is the later of the newest commit and the watcher's last good run. A failed run says "Backup needs you" and the reason, never "ok". |
| Settings: few, infrequent, sensible defaults; task options belong in the task; do not duplicate system settings; Command-comma opens them; reopen on the last pane. | Ctrl+, opens Settings. Folder rules live in Files, not Settings. Dark/light "Auto" follows the system. |
| Describe what a setting does when on; the opposite is implied. | Setting descriptions say what happens when it is on. |
| Onboarding: fast, optional, interactive; postpone non-essential setup; no licence text. | Welcome stays short; the checklist on Today shows one next step at a time. |
| Empty screens give a clear next step with a button. | Every empty state says what to do and offers the action (Models, Background jobs, Machine). |
| Errors: next to the problem, no blame, say what to do; no "oops". | Reason first ("The last backup failed: git commit: no identity"), then one action. |
| Buttons start with a verb; avoid "my"/"your" unless needed; never "we". | "Keep the backup up to date" rather than "Keep my backup up to date". Hestia never says "we". |
| Capitalisation: pick one style per element type. | Sentence case for headings and labels everywhere. |

## Honesty audit (fixed with this note)

Two sentences in the app said nothing leaves the computer, while Hestia pushes the home repository to the person's own private GitHub repository once they sign in. The backup copies the contents of settings and config files and of any folder the person adds (secret-looking lines are withheld); documents are never uploaded. The copy now says exactly that, and before sign-in it says "Nothing has left this computer yet." Rule: say "documents" for what never leaves, and name config files for what does.

## Information each screen owes the person

- **Every screen (header):** is the backup safe, and how long ago; otherwise how many questions wait.
- **Today:** what happened while away, what needs an answer, the next setup step.
- **Machine:** what differs from the last backup, per category, and what changed in it.
- **History:** each saved change in plain words, and what it touched.
- **Time:** the files that existed on a day, in friendly kind names (still raw: "unknown/image").
- **Files / Questions:** where each kind of file goes and why a suggestion was made.
- **System:** whether the background jobs and models are running, with the one action to fix it.

## Still open from this round

- Friendly kind names instead of `unknown/image`, `document/finance` (Time, Questions, Files).
- Dark-mode accent too saturated; cards use hard 1px borders everywhere.
- History is three columns for one change; Settings should reopen on the last pane.
- A permission or sign-in prompt belongs next to the feature that needs it.

Sources: Apple HIG [Feedback](https://developer.apple.com/design/human-interface-guidelines/feedback), [Settings](https://developer.apple.com/design/human-interface-guidelines/settings), [Onboarding](https://developer.apple.com/design/human-interface-guidelines/onboarding), [Writing](https://developer.apple.com/design/human-interface-guidelines/writing); GNOME HIG [banners](https://developer.gnome.org/hig/patterns/feedback/banners.html) and [toasts](https://developer.gnome.org/hig/patterns/feedback/toasts.html).
