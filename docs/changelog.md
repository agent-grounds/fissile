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

### Added

- Report intake: `.github/ISSUE_TEMPLATE/` carries four GitHub issue forms — bug
  report, feature request, usability report, and token or time waste — each
  applying the matching kind label (`bug`, `enhancement`, `usability`, `tokens`)
  as the issue is opened, and `config.yml` turns blank issues off so no issue
  can arrive without a kind. Every form asks for the fields that make a report
  actionable: the context (command, directory, version), what happened, what was
  expected, an optional workaround, and, on the token form, the cost. (PR #N)

## 2. [0.9.0] — 2026-09-05

### Added

- §FS-004-check-audit.2: `fissile audit --only <section>[,<section>]` prints the
  named sections of the text report and nothing else, so tuning config or
  pruning the registry stops paying for a findings block the reader is not
  looking at. The names are the seven top-level keys of
  `schema/audit.schema.json` — `findings`, `silenced`, `exceptions`, `top`,
  `stale`, `loose`, `coverage` — and they render in that order whatever order
  they were named in. Naming a section is the request to compute it, so `--only
  coverage` needs no `--rule-coverage`; `--only top` still needs `--top <N>`,
  which carries a count no default could stand in for. Selection reaches the
  screen and nothing else: exit status is computed from the whole run, so a
  standing hard overflow still exits non-zero under `--only coverage`. An
  unknown or empty name is a usage error naming the valid set, and `--only`
  with `--format json` is one too — `findings`, `silenced` and `exceptions` are
  `required` in the schema, so a filtered object would not validate. `audit`
  with no `--only` prints what it printed before, and the JSON surface is
  unchanged. Resolves #15. (PR #63)
- §GOAL-004-token-thrift.1, §FS-006-cli.2: `fissile audit --help` names the JSON
  route, `fissile audit --format json --rule-coverage | jq .coverage`, and says
  that `--format json` is the agent surface. The goals document has designated
  it one since §GOAL-004-token-thrift was written; the screen's two examples
  were both text and said so nowhere. (PR #63)

### Changed

- §FS-004-check-audit.2: `fissile::audit::AuditOptions` gains a public
  `only: Option<Vec<Section>>` field, and `fissile::audit` gains the public
  `Section` enum and the `SECTIONS` array that fixes its canonical order. A
  library caller constructing the options with a struct literal must initialize
  the new field, normally with `None`, which is the whole report. A 0.x source
  break, so the minor number moves. (PR #63)

- §AR-001-ci.2: CI's `grund check` job pins `grund` 0.13.0, up from 0.12.3, and
  the agent entrypoint's grund managed block moves from v7 to v8 to match.
  0.13.0 turns an unindexed declaration into a `grund check` error instead of a
  warning and stops accepting the `prefix` spelling of `[[kinds]] kind`; this
  repository already satisfied both, so pinning the new release found nothing
  else to fix. (PR #64)

## 3. Older releases

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
