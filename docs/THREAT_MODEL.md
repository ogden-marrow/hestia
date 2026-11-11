# Hestia threat model

Status as of 2026-11-11. Written from the red-team review of 2026-11-10 and what has been fixed since. Every row names the issue or PR that holds the evidence. A release is gated on the checklist at the end.

## What we protect

| Asset | Why it matters |
|---|---|
| Secrets on the computer (SSH and GPG keys, tokens, passwords, sessions, browser and mail data) | Theft is the worst outcome; Hestia must never read, copy or send them. |
| The person's files and what they are called | Names alone can be private. They must not leave the computer except into the person's own private repository, and never reach a remote model. |
| The home repository | Holds how the computer is set up. If it were public, or if what it contains ran code on every machine, that is a compromise. |
| The GitHub token | Gives access to the person's private repositories. |
| The computer's integrity | Hestia moves and copies files and can restore a machine; a bug there can destroy data. |

## Who and what we defend against

1. **A stranger who can put a file on the computer** (a download, an attachment, a shared folder). They control file names and contents.
2. **A malicious or compromised repository** that Hestia is pointed at or restores from (a poisoned home repository, a public fork, a changed dependency).
3. **Another program running as the same user** (it can read the user's files and processes anyway; we only avoid making that easier).
4. **A careless moment by the person** (publishing the wrong repository, pushing a secret, a wrong click).
5. **A network attacker** between the computer and GitHub.
6. **A supply-chain attacker** (a compromised crate, GitHub Action or release).

Out of scope: an attacker with root, a compromised desktop session or keyring, physical access to an unlocked machine, and GitHub itself being malicious.

## Trust boundaries

- **Computer to GitHub:** only commits to the person's own private repository, only after a scan, only over https with the person's token.
- **Computer to model:** only the local model on loopback. File names reach it, so names are treated as hostile data.
- **Repository to computer:** restoring brings in untrusted content. Nothing that runs by itself is restored without an explicit flag.
- **GitHub token:** lives only in the system keyring.

## Threats and what stops them

| # | Threat | Mitigation | Status | Evidence |
|---|---|---|---|---|
| T1 | A secret is committed and pushed | One shared scanner (modern token formats, quoted and exported passwords, binaries, any case) runs on everything captured and on the new commits before every push; secret lines are left out of the backup, private keys drop the whole file | Fixed | #62, #80, #107, #129 |
| T2 | The scanner misses a secret (short, unusual format, encoded) | Scanner is a filter, not a guarantee; the allowlist captures only named files; the push is restricted to a private repository | Residual risk | #129; keep adding rules |
| T3 | Pushing to a public, foreign or renamed repository | The push target is checked on the address git will really use (after pushurl and insteadOf), must be a private repository of the signed-in person; redirects and odd answers fail closed | Fixed | #80, #122, #132 |
| T4 | Publishing a leaky history | Whole history scanned before any create or push | Fixed | #62, #121 |
| T5 | Capture deletes or overwrites the person's live files | New copy built beside the old and swapped in; live symlinks into the repository are followed to their target; a capture that would keep under half of the last one is refused | Fixed | #99 |
| T6 | The mover follows a symlink, races, overwrites or files into a hidden folder | Plain files only, no symlinks at either end, exclusive temp file, publish without replacing, hidden destinations refused | Fixed | #65, #66 |
| T7 | The repository controls code that runs on every machine | The watcher ships with Hestia, not in the repository. Restore is a dry run by default, copies instead of linking, and holds back autostart entries, systemd units and shortcut commands until `--allow-executable` | Fixed | #63, #108 |
| T8 | Desktop history and accounts end up in the backup | Desktop settings are captured by an allowlist of sections, dropping keys that remember what you did | Fixed | #116 |
| T9 | The clipboard history keeps secrets | Off until turned on, filters codes and tokens, file readable only by the person, pruned, deleted when turned off | Fixed | #105, #106 |
| T10 | A hostile file name steers the model | Names reach the model only as cleaned, shortened JSON data it is told to treat as data; its answer is limited to the known kinds; names are cleaned before embedding | Fixed | #125 |
| T11 | File names or contents leave the computer through the model | The model host must be loopback, with no user info and no redirects | Fixed | #64 |
| T12 | The GitHub token leaks | Only in the system keyring, refused when there is none, never printed, logged, put on a command line or in a file; git gets it through a credential helper that answers only for github.com over https; no redirects; base URLs overridable only to loopback | Fixed | #118 |
| T13 | Three lists of "never capture" drift apart | One deny-list read by capture, the Machine screen and the starter manifest | Fixed | #129 |
| T14 | A bulk action is triggered by a deceptive name | "Answer similar folders together" is off until chosen | Fixed | #131 |
| T15 | A compromised dependency or CI action | Every action is pinned to a commit; Dependabot proposes updates; cargo-deny blocks a dependency from an unexpected registry or git source and reports known advisories (three 'unmaintained' notices, no vulnerabilities: #135) | Mitigated | #59, #135 |
| T16 | Releases are not verifiable | No signed or attested releases yet | **Open** | #12, #59 |
| T17 | A private home repository is published by mistake | Publishing is never automatic; the person's repository is created private and verified private before every push | Mitigated | #121, #122 |
| T18 | Uninstall leaves things behind, or an install writes an unsafe unit | Uninstall removes programs, units and overrides, always deletes the clipboard history, gives Alt+Space back, and only `--purge` deletes settings; install refuses path characters that could change a systemd unit; the launcher ignores toggles while the screen is locked | Fixed | #60 |
| T19 | The public repository holds personal data | Files and new commits are checked in CI (`tests/privacy.sh`: home paths, email addresses, this computer's user and host name, and a no-reply identity on every new commit); fixtures read no real data; test tokens are built at run time; pull requests are merged with 'rebase', never 'squash', because GitHub fills a squash commit from the merging account's public profile | **Open: history** | #61, #142 |
| T20 | Sign-in uses a broader permission than needed | GitHub's `repo` scope is the narrowest that can create a private repository; a narrower mechanism (a GitHub App on one repository) is to be investigated | Residual risk | #110 |
| T21 | The GitHub CLI fallback in the watcher checks "private" but not owner | Disappears when Hestia sign-in is live | Residual risk | #110 |

## Residual risks we accept, and why

- **Public history already contains the owner's real name and personal email** on 41 merge commits made on 2026-11-11 by GitHub's squash merge (#142). New commits are protected; the existing ones need an owner decision (rewrite the history, or accept).
- A secret the scanner cannot recognise can still reach the person's own private repository. The repository stays private and the person can rewrite it; we do not claim otherwise in the product.
- The person's own GitHub account is the root of trust for backups. If it is compromised, so is the backup.
- The system keyring is trusted: any program that can unlock it as the same user can read the token.

## Release gate

A release is blocked unless all of these hold:

1. No row above is **Open** with a severity of high or critical (T16, signed releases, must be closed or explicitly accepted by the owner).
2. `cargo test --workspace` and every shell test pass in CI on a clean runner.
3. The secret scanner finds nothing in the repository's history, apart from the named test fixtures.
4. A clean-machine install and first run has been done end to end (#6).
5. This file is updated in the same change as any change to what leaves the computer, what is read, or what runs by itself.
