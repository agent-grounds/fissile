# Changelog

Records every release of `fissile`. Versions follow semver; the **latest
release is inline** in this file, and **older releases live one-per-file under
`docs/changelog/`** so a reader — human or agent — only loads the history they
ask for (§GOAL-004-token-thrift).

A release's notes are the pull requests merged since the previous release, one
linked title per line, newest first; `scripts/prepare_changelog_release.py
prepare <version>` writes them when the release is cut, and nobody writes them
by hand (§AR-001-ci.8.3). Follow a line to its pull request for the detail,
including the migration steps a breaking change carries. Releases up to 0.11.0
were written by hand and keep their prose.

## 1. [0.11.1] — 2026-10-05

- [Write release notes from the merged pull requests](https://github.com/agent-grounds/fissile/pull/89) (PR #89)

## 2. Older releases

- [0.11.0](changelog/0.11.0.md) — 2026-10-03: - `clean.sh` at the repository root gives back the disk a checkout's builds took: it runs `cargo clean`, then removes every directory holding a valid `CACHEDIR.TAG`, such as a plan's scratch build under `panta/`.
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
