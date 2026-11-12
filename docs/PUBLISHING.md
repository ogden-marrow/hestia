# Publishing Hestia

The repository is already public. This is the checklist for the **first release** and for anything that announces the project. **Cutting a release needs the maintainer's explicit go:** a tag publishes files to the world and cannot be fully taken back.

## Pre-release checklist (state on 2026-11-11)

| # | Check | How | Status |
|---|---|---|---|
| 1 | No credentials in the history | `lifestream secret-scan --repo . --range <first commit>..origin/main` | **Pass.** Five hits, all deliberate: the scanner's own rule examples and the planted fake keys in tests (`tests/watcher.sh`, `tests/clipboard-filter.js`, `lifestream/src/{secrets,semantic}.rs`). A fake, token-shaped test string from an early draft of the sign-in engine still sits in the closed pull request #114's GitHub reference; it is obviously fake (`FAKE…`) and not a credential. |
| 2 | No personal data in files | `bash tests/privacy.sh` (also a CI job) | **Pass.** Home paths, other people's emails, this computer's user and host name. The maintainer's own name and email on GitHub merge commits is an accepted choice (`docs/THREAT_MODEL.md` T19, issue #142). |
| 3 | Licences | `cargo deny check licenses` (CI) and `THIRD_PARTY.md` | **Pass.** Permissive licences only, plus file-level MPL-2.0 audio crates used unmodified; no GPL or AGPL. `THIRD_PARTY.md` lists every crate built into Linux and ships in the release; CI fails if it is out of date. Full licence texts are not bundled yet: the list names each crate and licence (follow-up if a distributor needs the texts: `cargo-about`). |
| 4 | Dependencies are pinned and sourced | `Cargo.lock` committed, builds use `--locked`; `cargo deny check sources` (CI); every GitHub Action pinned to a commit; Dependabot proposes updates | **Pass.** Known advisories are reported, not blocking (three "unmaintained" notices, #135). |
| 5 | The threat model release gate | `docs/THREAT_MODEL.md`, "Release gate" | **Not yet.** T16 (release signing) is only exercised by a real tag; item 4 below (a clean machine with a real session) is still open (#6). |
| 6 | The release packaging works | `bash tests/release-rehearsal.sh` (CI): the release script, checksums, install from the tarball with no compiler, the `.deb` contents | **Pass** (rehearsed). Signing and attestation only run on a real tag. |
| 7 | A clean-computer install end to end | `bash tests/install-e2e.sh` (CI) | **Pass** in an empty home folder with system commands stubbed. A real clean OS image with a real session is still untried (#6). |
| 8 | README, CHANGELOG, licence | `README.md` install and first run, `CHANGELOG.md`, `LICENSE` | **Pass.** CHANGELOG is all "Unreleased"; the version heading is added when tagging. |
| 9 | Sign-in works for a stranger | needs the project's GitHub OAuth App Client ID in the build | **Not yet.** Without it the welcome says sign-in is not set up and lets the person continue. Decide whether to publish a first release without it (see #110). |
| 10 | Known limits are written down | `CHANGELOG.md` "Known limits", `docs/VISION_FIT.md` | **Pass.** The `.deb` needs `hestia-setup` run once per person (it cannot enable per-user units itself); this is rehearsed in CI. |
| 11 | The protections' tests really bite | `bash tests/mutations.sh` (not in CI; rebuilds): breaks 15 protections on purpose in a scratch copy and expects a test to fail | **Pass** (15 of 15 caught on 2026-11-11). Run again after touching a protection. |

Release when every row is Pass, or when the maintainer accepts a row that is not, in writing on the tracking issue.

## Cutting the first release (maintainer, by hand)

1. Update `CHANGELOG.md`: rename "Unreleased" to the version and date, add a fresh "Unreleased" above it. Bump `version` in the three `Cargo.toml` files if it is not `0.1.0`. Merge that as a normal pull request.
2. Make sure `main` is green in CI.
3. Tag and push: `git tag -a v0.1.0 -m "Hestia 0.1.0" && git push origin v0.1.0`. This starts `.github/workflows/release.yml`, which builds, runs the tests, runs `packaging/build-release.sh` (the same script the rehearsal runs), signs a build attestation for the tarball and `.deb`, and publishes the GitHub release with four files: the tarball, the `.deb` and one `.sha256` for each.
4. Check the published files yourself, as a downloader would:
   ```bash
   sha256sum -c hestia-linux-x86_64.tar.gz.sha256
   gh attestation verify hestia-linux-x86_64.tar.gz --repo ogden-marrow/hestia
   ```
5. Install the tarball on a machine that is not the one you build on, and walk through the first run (`README.md`).
6. Paste the version's `CHANGELOG.md` section into the release notes.

## If something is wrong after release

- Edit the release and mark it as a pre-release or draft; delete the tag only if no one could have used it (`git push --delete origin v0.1.0`).
- A bad file is replaced by a new patch version, never silently edited in place: the checksum and attestation would no longer match what people already downloaded.
- A leaked secret: rotate it first, then remove it. Removing it from the repository does not un-leak it.

## Keeping this file true

Update the status column in the same pull request that changes any of these facts, and `docs/VISION_FIT.md` when a status there changes.
