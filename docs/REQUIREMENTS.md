# Hestia: requirements and application layer

## What it is for
Your whole computer as code in a private git repository: organised automatically, backed up automatically, editable in Hestia. Homebase/Home is the source of truth for every machine.

## What a person must be able to do (and the one place each happens)
| Need | Place | Action | Status |
|---|---|---|---|
| Start from nothing | Today "Get started" | Create home repo, Publish, first scan, Hestia Search | built |
| Find anything | Hestia Search (Alt+Space) | type | built |
| Answer what it is unsure about | Questions | File as this / Skip | built |
| See and change where files live | Files | rules, move, "File them now" | built |
| Define what the domains are | Settings > Domains | edit | built |
| Rebuild a machine | Machine | Capture now | built, needs capture.sh in the home repo |
| Look back | Time, History | step, compare | built |
| Stay backed up | automatic | watcher commits, pushes to a private origin | partly: not yet created for new home repos |

## The application layer (rules every screen follows)
1. **Anything that takes time is a job** (`ui/src/jobs.rs`): it runs off the UI thread, its button disables and says what it is doing ("Scanning…"), and the result lands in one place.
2. **One place per action.** A second button for the same thing is a bug.
3. **Every action has four visible states:** idle, running, done (what happened), failed (why, and what to do).
4. **Destructive or outward actions confirm first** (move files, publish).
5. **No state lives only in a string.** Whether the home repo exists, a scan is running, the extension is on: each comes from one derived fact, not from `msg`.

## Still to convert to jobs
- Create home repository (fast, but should show progress and block double press)
- "File them now…" (moves many files)
- Turn on Hestia Search (three call sites)
- Settings schedule changes (systemctl calls)
- Clipboard "Clear history"

## Gaps found in the audit
- `msg` is shown only on some screens: needs one status strip visible everywhere.
- A scan started from outside the app (timer) is invisible: derive "running" from the unit's state.
- The watcher and capture scripts are not shipped to a new home repo.
