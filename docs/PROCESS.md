# How we work

One rule: **every change is traceable from a force in VISION.md to an issue to a test.**

## Planning
- Work lives in GitHub issues, grouped in milestones, worked in milestone order, oldest issue first inside a milestone. No work without an issue; no issue without a milestone.
- Labels: `bug`, `ui-state`, `install`, `security`, `architecture`, `tests`, `design`.
- An issue states the problem, the acceptance test, and which force it serves.

## Definition of done
1. The acceptance test exists and passes in CI.
2. Every new control shows idle, running, done and failed (use `questions::action`; slow work runs as a `jobs` job).
3. A scene renders for each new screen state; a critic has looked at it.
4. Anything touching files, git, systemd, the network or the clipboard has a threat-model line in `docs/THREAT_MODEL.md` and a test.
5. No second way to do something that already exists.
6. Docs and changelog updated.

## Review boards
- **UI board** (before a milestone closes): first-time-user critic, visual/state critic, accessibility critic, each reading screenshots and code, each returning ranked defects as issues.
- **Red team** (before every release): attack-surface review against the threat model; findings become `security` issues, blockers before release.
- Findings are filed as issues, never fixed ad hoc.

## Environments and releases
- `main` is always releasable. CI: build, tests, clippy, a clean-container install test.
- Releases are tagged, built by CI, and checked on a clean machine.
- Nothing is pushed or published without an explicit go from the owner.

## Do not mainline
- **One thing at a time.** Pick one issue, work it on its own branch, merge it when its acceptance test passes. Nothing else rides along.
- **Found something else? File it and keep going.** A bug spotted during feature work becomes an issue with a repro and a severity, then you return to the feature. No drive-by fixes.
- **Separate lanes.** Features, bug passes and review-board triage happen in different sessions. Triage only files and orders issues; it never fixes.
- **Bug pass:** fix in severity order, each with a regression test that fails before the fix.
- **Reviews produce issues, not edits.**
