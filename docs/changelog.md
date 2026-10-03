# Changelog

Records every notable change to `fissile`. Versions follow semver; the
**latest release is inline** in this file, and **older releases live
one-per-file under `docs/changelog/`** so a reader — human or agent — only
loads the history they ask for (§GOAL-004-token-thrift). Each entry cites the
FS/AR/GOAL/DA IDs it touches, so the changelog is part of the grounded tree.

Schema-version bumps are called out explicitly: `fissile_config_version`
(§FS-001-config.1), the exception registry version (§FS-003-exceptions.1), and
the managed block versions written by `init` (§FS-002-init.4). A bump to any of
these is a breaking change for the consumer and must appear under **Changed**
with a migration note. A change that breaks a library caller's source is called
out the same way and names the release it forces: the crate publishes a `[lib]`,
and at 0.x semver puts the minor number in charge of it.

## 1. Conventions

- **Sections per release:** `Added`, `Changed`, `Deprecated`, `Removed`,
  `Fixed`, `Security` — the Keep-a-Changelog set; omit any with no entries. A
  large entry (a first release, most of all) may add narrative subsection
  headings when the standard six would bury the structure.
- **Entry style:** one bullet per change, present tense, leading with the
  affected ID, e.g. `§FS-004-check-audit.5: skip unmeasurable paths instead of
  aborting`.
- **Progressive discovery:** only **Unreleased** and the most recent release
  are inline. When a release ships, `scripts/prepare_changelog_release.py
  prepare <version>` promotes Unreleased, archives the previous inline release
  to `docs/changelog/<version>.md`, and links it under
  [§3 Older releases](#3-older-releases). The release workflow reads the
  published notes back with the same script (§AR-001-ci.8).

## Unreleased

## 2. [0.11.0] — 2026-10-03

### Added

- `clean.sh` at the repository root gives back the disk a checkout's builds
  took: it runs `cargo clean`, then removes every directory holding a valid
  `CACHEDIR.TAG`, such as a plan's scratch build under `panta/`. It is the clean
  verb `ephor clean` runs at the root of a branch checkout no live run holds.
  (PR #82)
- §FS-004-check-audit.2.2: an `audit` whose scan selects no files at all says
  so in one note on stderr, naming the directory it scanned and, inside a git
  repository, the repository root. A run from a directory the enclosing
  repository ignores no longer passes as a silent `ok`; stdout and the exit
  status are unchanged. (PR #84)
- §FS-004-check-audit.1.4: a staged soft finding's text detail separates the
  prior committed over-soft edits from the staged one (`8 prior committed
  over-soft edits + 1 staged edit`) and says when the prior edits had already
  reached the promotion limit; with incomplete history it reports `at least N`.
  The JSON fields are unchanged. (PR #83)

### Changed

- §FS-003-exceptions.4: the refusal when two entries of one registry match the
  same overflow names the `path` of the first two colliding entries, in
  registry order, and says to remove or narrow the overlap, instead of naming
  only the registry and the measured file. **Library break, forcing 0.11.0:**
  `ExceptionError::MultipleMatches` gains a `patterns: Box<[String; 2]>` field,
  so code that constructs the variant or destructures it without `..` stops
  compiling, and a caller comparing the exact message text must update.
  (PR #86)

### Fixed

- §FS-004-check-audit.5: every spelling of the repository root passed to
  `measure` or `check` (`.`, `./`, `src/..`, the absolute path) is reported as
  the directory `.`, a file-level failure that still lets the other paths be
  measured, instead of an invalid argument or an empty path name.
  §FS-003-exceptions.3: `exception add`, `retune` and `remove` refuse an exact
  path that names the repository root and leave the registry untouched.
  (PR #85)

## 3. Older releases

- [0.10.1](changelog/0.10.1.md) — 2026-09-21: - §FS-004-check-audit.1.2, §FS-004-check-audit.1.4, §FS-002-init.6: staged failures describe the `fissile check --staged` verdict instead of claiming an unknown commit hook blocked a commit, explain that filename-passing snapshot hooks keep soft findings advisory, and document how hook managers enforce bounded soft-edit promotion.
- [0.10.0](changelog/0.10.0.md) — 2026-09-14: - The links to the sibling tools now name `agent-grounds`, which `grund` and `rhei` joined after this repository's own move: the `AGENTS.md` and `README.md` pointers to `grund`, and the config-home decision's reference to `rhei`'s matching issue.
- [0.9.1](changelog/0.9.1.md) — 2026-09-07: - Report intake: `.github/ISSUE_TEMPLATE/` carries four GitHub issue forms — bug report, feature request, usability report, and token or time waste — each applying the matching kind label (`bug`, `enhancement`, `usability`, `tokens`) as the issue is opened, and `config.yml` turns blank issues off so no issue can arrive without a kind.
- [0.9.0](changelog/0.9.0.md) — 2026-09-05: - §FS-004-check-audit.2: `fissile audit --only <section>[,<section>]` prints the named sections of the text report and nothing else, so tuning config or pruning the registry stops paying for a findings block the reader is not looking at.
- [0.8.3](changelog/0.8.3.md) — 2026-09-05: - §FS-001-config.8, §FS-002-init.2: the config's home is `.agent-grounds/fissile.toml`.
- [0.8.2](changelog/0.8.2.md) — 2026-08-31: - §FS-001-config.0.1: the built-in defaults budget a Markdown file by how it is read.
- [0.8.1](changelog/0.8.1.md) — 2026-08-30: - §FS-002-init.5: `init::Report` carries one `HookStatus` — `Installed`, `SkippedNotGit`, `SkippedByFlag` — in place of the `hook_skipped_not_git` boolean, so the hook step 2 reports is a value every path has to answer for instead of a flag that can be left unset.
- [0.8.0](changelog/0.8.0.md) — 2026-08-26: - §FS-005-exception-add.2, §FS-008-exception-retune.1: a ceiling stated with `--max` is written as stated; the `[exceptions.bump]` step rounds only what the command measured (§DF-010-stated-ceilings-are-exact).
- [0.7.1](changelog/0.7.1.md) — 2026-08-24: - §FS-002-init.3: `AGENTS.md` is the one entrypoint that holds the managed block, and every other one `init` touches is a **symbolic link** to it (§DF-009-one-file-agents-read).
- [0.7.0](changelog/0.7.0.md) — 2026-08-21: - §FS-004-check-audit.1.1: a run that reports a finding adds one `hint:` line naming `fissile measure`, beneath the findings it is about.
- [0.6.0](changelog/0.6.0.md) — 2026-08-21: - §FS-007-measure: new `fissile measure <paths>...
- [0.5.0](changelog/0.5.0.md) — 2026-08-12: - §FS-003-exceptions.1: the exception registry schema is now `fissile_exceptions_version = 2`.
- [0.4.0](changelog/0.4.0.md) — 2026-08-11: - §FS-003-exceptions.2.1: exception entries carry `kind = "structural" | "deferred"`, which fixes what `reason` must establish — the architectural constraint that makes the split illegal, or the boundary that is missing and what has to exist first (§DF-004-exception-kind).
- [0.3.0](changelog/0.3.0.md) — 2026-08-11: - §FS-006-cli.2: the usage screen opens with a short paragraph — what `fissile` is for, the two tiers, the `check --staged` habit, and the rule that a budget is never met by damaging the design — closing with a pointer to `fissile init --dry-run` for the full agent instructions.
- [0.2.0](changelog/0.2.0.md) — 2026-08-11: - §FS-001-config.3: rules take `soft_message` and `hard_message`, so a warning and a block can say different things (§DF-003-severity-guidance).
- [0.1.0](changelog/0.1.0.md) — 2026-08-11: The first release: the commit-time file-size gate, its adoption tooling, and the evidence chain behind them (§GND-001-fissile).
<!-- Populated by `prepare_changelog_release.py prepare` when a release ships. -->
