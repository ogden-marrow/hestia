# Redesign proposal: the Stream is the app

Written 2026-11-12 after three critic rounds. The critics' verdict was that polishing the existing panes tops out around 7/10, because the problem is the shape of the app, not the paint. This proposes a new shape. It builds on decision #81 (Today as the live stream) and `docs/VISION.md`.

## The problem, in one table

Hestia holds a lot of data and shows it as ten separate screens that each print a slice of it.

| What we know | Where it shows today | What a person actually wants to know |
|---|---|---|
| Every file that arrived, what kind it is, where it went, who decided (rule, model, you) | Time (list per day), Files (folders), Questions (queue), Today (counts) | "What did Hestia do for me? Did it get anything wrong?" |
| Whether the computer is backed up, when, what differs, what failed | Machine, History, System, header | "Am I safe? If not, what do I do?" |
| Which questions it could not answer, with the models' suggestions and reasons | Questions | "Just ask me, with a good default, right where it happened." |
| Rules, domains, folder tree | Files, Settings > Domains | "Where is my stuff, and why is it there?" |
| Background jobs, models, GPU | System | "Is it running?" (almost never needed) |

Ten places, four of which answer overlapping questions (Today, Time, History, System all describe "what happened"). That is the duplication the "one way to do things" rule forbids.

## The shape

Three places and one search box.

1. **Stream** (the front door). One time-ordered feed of everything that happened: files that arrived, files filed (with Undo), questions (answered in place), saves of this computer. A short briefing at the top says what changed since you were last here. Filters, not screens: Everything, Filed, Saved from this computer, Your answers. Time travel is part of it: pick yesterday, last week, October, and the whole app shows how it was (this replaces Time and most of History).
2. **Library**. Where things live, by purpose (domains with counts), the folders Hestia watches, and the rules in plain words ("PDFs from Downloads go to Documents / Finance, learned from your answers 12 times"), each editable. Replaces Files and Domains.
3. **Computer**. What is kept so this machine could be rebuilt, what differs from the backup, the helpers (overnight scan, models, backup service) and their state. Replaces Machine, System, and the raw git view in History (diffs stay one click down).

Plus: **Search** is a box in the top bar, always there (Ctrl+F), the same engine the launcher uses. **Settings** stays separate and small. **Safety** is one status at the top right on every screen: "Backed up 12 minutes ago", or "Backup needs you" with a reason and one action.

Sidebar: Stream, Library, Computer, Settings. Four items instead of ten.

## Why this answers "how do we surface the data meaningfully"

- **Say what happened, in order, in words.** The Stream is the Lifestreams idea in the vision: time is the organising principle, not folders. Every row is a sentence ("Filed 18 photos to Pictures / 2026 / November") with its reason underneath and the one action that makes sense (Undo, See change).
- **Ask where it happened.** A question is a card in the stream at the moment it arose, with a recommended answer, the reason ("12 earlier answers point the same way") and one button that does it. Answering turns the card into a row you can Undo. No Questions screen to visit.
- **Show provenance.** Rule, model, or you decided each filing. People trust automation when it shows its work and lets them undo (never destructive, vision force 6).
- **Counts become sentences, and only the surprising ones.** Research on personal data dashboards says avoid a single score and show what is unexpected (see sources). The briefing leads with what needs attention and stays quiet when nothing does.
- **Status is passive; interruption is for failure.** Apple HIG Feedback: match delivery to importance. Safe = one quiet line. Not safe = a reason and a button.
- **Fewer places means fewer duplicate explanations.** One list of kinds, one list of rules, one list of helpers.

## What changes in the code

Mostly composition of what exists: the engine, jobs, backup verdict, kind labels, header status and fixtures are already there. New work:

1. A **stream feed** model: merge records (arrived/filed/asked), repo commits (saves) and watcher results into one ordered list of typed events. Needs the engine's file log with reasons (issue #27) so each filing says who decided.
2. A **Stream** view with filters, inline question cards, Undo.
3. **Library** and **Computer** views assembled from Files/Domains and Machine/System/History.
4. Delete the retired screens as each is replaced (lean: no old and new side by side).

## What this does not decide

- Whether the stream is the front door (option A of #81, recommended) or a briefing plus a separate stream (option B).
- Whether History's raw diffs stay at all, or only appear behind "See change".

## Sources

Apple HIG [Feedback](https://developer.apple.com/design/human-interface-guidelines/feedback) and [Settings](https://developer.apple.com/design/human-interface-guidelines/settings); Freeman and Gelernter on Lifestreams (cited in `docs/VISION.md`); University of Washington work on personal-data discovery ("discovery happens when you see something you weren't expecting") and personal-activity dashboard guidance against a single score; a community complaint that 1Password's Watchtower redesign removed at-a-glance problem counts from the sidebar (the lesson: keep the attention count visible).
