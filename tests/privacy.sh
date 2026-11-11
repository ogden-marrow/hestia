#!/usr/bin/env bash
# This repository is public. Fail if it holds personal data, in files or in new commits. The check never names a
# person: it looks for the SHAPE of leaks (home-directory paths, email addresses, commit identities) and, when run on a
# real computer, for the current user and host name.
#   RANGE=<a..b>   which commits to check the identities of (default: origin/main..HEAD). Older history is not rewritten
#                  here; see docs/THREAT_MODEL.md T19.
set -uo pipefail
cd "$(git rev-parse --show-toplevel)"
bad=0; say() { echo "PRIVACY: $*" >&2; bad=1; }

# 1. files: home-directory paths other than obvious placeholders
files=(':!Cargo.lock' ':!docs/design-research' ':!tests/privacy.sh' ':!tests/privacy-allow.txt')
while IFS= read -r l; do say "a real-looking home path: $l"; done < <(git grep -nIE '(^|[^A-Za-z0-9_./-])/home/[A-Za-z0-9_.-]+' -- . "${files[@]}" | grep -vE '/home/(x|u|you|me|user|ada|someone|runner|name|USER|jane|john|\$|\{)' | grep -vE '\$T/home|"\$HOME|/home/%h' | cut -c1-160)

# 2. files: email addresses other than the allowed shapes
allow="$(grep -vE '^\s*(#|$)' tests/privacy-allow.txt | paste -sd'|')"
while IFS= read -r l; do say "an email address: $l"; done < <(git grep -nIoE '[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}' -- . "${files[@]}" | grep -vE "$allow" | cut -c1-160)

# 3. files: this computer's user and host name (only meaningful on a real computer; CI runners are skipped)
for w in "$(id -un)" "$(hostname -s 2>/dev/null)"; do
  case "$w" in ""|runner|root|localhost|ubuntu|"") continue;; esac
  [ "${#w}" -ge 4 ] || continue
  while IFS= read -r l; do say "this computer's name '$w' appears: $l"; done < <(git grep -nIiF -- "$w" -- . "${files[@]}" | cut -c1-160)
done

# 4. commits: author and committer must be a no-reply identity (a hidden, GitHub-owned address)
range="${RANGE:-origin/main..HEAD}"
okid='(@users\.noreply\.github\.com|^noreply@github\.com|^noreply@anthropic\.com|^noreply@example\.invalid)$'
while IFS='|' read -r h ae ce an; do
  echo "$ae" | grep -qE "$okid" || say "commit $h is authored with a real email address (author '$an'): use a no-reply identity, and merge pull requests with 'rebase', never 'squash' (GitHub fills a squash commit from your public profile)"
  echo "$ce" | grep -qE "$okid" || say "commit $h has a real committer email"
done < <(git log --format='%h|%ae|%ce|%an' "$range" 2>/dev/null)

[ "$bad" = 0 ] && echo "privacy ok"
exit "$bad"
