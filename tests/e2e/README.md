# e2e

End-to-end scenarios live here. This is the home of the non-citable `e2e`
kind: a case is exercised by being run, never cited, so no case carries an ID
and nothing under here declares one. `[citations.e2e]` in `grund.toml` says
this home must cite `FS` and should not cite `AR` — a scenario proves the
behavior fissile's commands give a user (§FS-004-check-audit, §FS-006-cli),
not the design behind them.

Each subdirectory under `cases/` named `E2E-*` is one scenario: a `case.toml`
manifest plus a `repo/` working tree, run by the harness in `main.rs`.

A case directory holds:

- `E2E-NNN-slug.md` — the scenario's write-up, describing the behavior it
  verifies;
- `case.toml` — the manifest: `args`, expected `exit`, optional `git`, and
  stdout/stderr/`creates`/`absent` assertions, plus `[[files]]` blocks asserting the bytes
  a command left behind — `contains` and `excludes` needles against one path,
  which is how a scenario checks a rewrite rather than the claim about it;
- `repo/` — the working tree copied into a throwaway directory before the run
  (omitted when the scenario starts from an empty repo, like `init`).

The harness drives the real `fissile` binary, so every documented behavior under
`docs/functional-spec/` has at least one executable scenario.

The bounded soft-edit contract in `soft_edit_limit.rs` creates real complete and
shallow Git histories rather than copying a static tree. It proves the crossing,
threshold, reset, exception, precedence, and fail-open behavior of
§FS-004-check-audit.1.4, plus the generated current-commit guidance.

History needs a real sequence of commits rather than one copied working tree.
The `history/` harness materializes its checked-in TOML timelines with fixed
committer dates, then exercises debt movement, continuous ages, rename evidence,
and all-or-nothing refusals through the real binary
(§FS-004-check-audit.2.1).

## Scenarios

- `E2E-001-check-clean` — a clean check prints `ok` and exits zero.
- `E2E-002-check-hard-blocks` — a hard overflow fails the commit with a named fix.
- `E2E-003-check-soft-warns` — a soft overflow warns without blocking.
- `E2E-004-check-json` — the JSON surface is one flat record per finding.
- `E2E-005-exception-silences-hard` — a deferred hard exception accepts the file; the soft warning survives.
- `E2E-006-audit-inventory` — audit reports overflows plus optional inventory sections.
- `E2E-007-init-installs-hook` — init bootstraps config, registries, agent block, and the hook.
- `E2E-008-exception-add` — exception add appends a structured registry entry.
- `E2E-009-check-token-rule` — a non-default config with a token-unit rule passes.
- `E2E-010-cli-version` — `--version` prints one stable line.
- `E2E-011-check-unreadable-continues` — one unmeasurable path never hides the rest.
- `E2E-012-check-binary-lines` — non-UTF-8 content still measures lines.
- `E2E-013-init-no-git` — init without a repository stays honest about the hook.
- `E2E-014-init-names-entrypoint` — the next block names the entrypoint it wrote.
- `E2E-015-check-groups-findings` — findings group under one copy of their guidance.
- `E2E-016-init-dry-run-explains` — a dry run explains the workflow without writing.
- `E2E-017-exception-kind` — a deferred exception cannot be open-ended.
- `E2E-018-audit-exception-kinds` — audit separates accepted-permanently from carrying-debt.
- `E2E-019-structural-silences-soft` — a structural hard exception silences the soft warning too.
- `E2E-020-registry-version-2` — an unmigrated registry is refused with both edits named.
- `E2E-021-measure-reports-headroom` — measure reports the count and the distance to what binds.
- `E2E-022-exception-retune-moves-a-ceiling` — retune moves a recorded ceiling to the quantized value.
- `E2E-023-audit-reports-loose-ceilings` — audit reports a ceiling that has drifted above its file.
- `E2E-024-retune-preserves-the-registry` — retune moves one line and reads the rest as TOML.
- `E2E-025-retune-refuses-a-mismatched-matcher` — a glob address does not retune an exact entry.
- `E2E-026-audit-top-ranks-unruled-files` — the largest file ranks even when no rule reaches it.
- `E2E-027-measure-headroom-is-spendable` — the headroom is room a caller can actually spend.
- `E2E-028-measure-agrees-at-a-ceiling` — a file exactly at its ceiling is accepted, and reads that way.
- `E2E-029-exception-add-names-the-entry-to-edit` — a glob over two entries is a conflict, not a broken registry.
- `E2E-030-retune-of-a-shrunk-file-names-removal` — a file under its limit cannot be followed down.
- `E2E-031-exception-add-measures-once` — a refusal reuses the measurement it already took.
- `E2E-032-bump-defaults-to-the-configured-step` — a ceiling is quantized even when nothing configures the step.
- `E2E-033-measure-staged-shares-check-selection` — measure --staged selects what check --staged selects.
- `E2E-034-hard-exception-needs-a-human` — a hard exception is refused off a terminal.
- `E2E-035-check-reports-a-stale-exception` — check names an entry whose file is gone.
- `E2E-036-staged-check-names-the-gate` — a blocked commit says so, and says not to bypass it.
- `E2E-037-staged-check-names-a-dead-entry` — a commit blocked by a dead entry names the registry, not a split.
- `E2E-038-an-unbuilt-file-is-not-a-dead-entry` — a path merely absent from the working tree blocks nothing.
- `E2E-039-the-offered-soft-route-runs` — the command the hard-severity refusal prints succeeds verbatim.
- `E2E-040-json-check-explains-its-exit-code` — a JSON run that fails says why, on stderr.
- `E2E-044-a-stated-ceiling-is-written-as-stated` — a `--max` is the ceiling; the step is only named.
- `E2E-045-retune-refuses-a-soft-ceiling-on-the-hard-limit` — the measured form stops where a soft ceiling would stop firing.
- `E2E-046-a-stated-ceiling-stays-under-the-hard-limit` — the stated form is the way through the last step below the limit.
- `E2E-047-a-stated-soft-ceiling-at-the-hard-limit-names-the-hard-route` — a stated value gets the same refusal, plus the other registry.
- `E2E-048-a-glob-ceiling-is-the-number-stated` — a class-wide ceiling is the policy number chosen.
- `E2E-049-audit-names-the-stated-form-on-the-hard-limit` — audit does not recommend a retune the command would refuse.
- `E2E-050-init-no-hook-points-at-the-commit-flow` — a declined hook sends the reader to their own commit flow.
- `E2E-051-a-shadowing-twin-needs-no-second-argument` — the soft twin points at the hard entry instead of restating it.
- `E2E-052-a-shadowing-twin-silences-the-soft-finding` — the pair reads back as one accepted file.
- `E2E-053-shadows-hard-needs-the-entry-it-points-at` — a shadow of a decision nobody recorded is refused.
- `E2E-054-an-orphan-shadow-fails-the-load` — deleting the original takes the twin with it.
- `E2E-055-shadows-hard-takes-none-of-the-three` — the flag replaces --kind/--reason/--until rather than joining them.
- `E2E-056-a-deferred-twin-lifts-a-soft-ceiling-over-the-hard-limit` — shadowing a deferred entry accepts a ceiling above the limit.
- `E2E-057-a-structural-twin-leaves-the-soft-ceiling-dead` — the same call is refused when the entry shadowed is structural.
- `E2E-058-an-orphan-shadow-names-a-registry-that-is-not-there` — a missing hard registry is named by its configured path.
- `E2E-059-remove-repairs-a-registry-that-blocks-every-command` — the entry that aborts every run can still be deleted.
- `E2E-060-audit-passes-after-a-removal` — the repaired registry loads again.
- `E2E-061-remove-deletes-a-stale-entry` — the entry whose file is gone is the one removal is for.
- `E2E-062-remove-refuses-an-entry-that-still-silences` — a working exception is not deleted.
- `E2E-063-remove-dry-run-writes-nothing` — a dry run says what it would delete.
- `E2E-064-audit-names-remove-for-an-entry-that-silences-nothing` — the report names the command, not just the state.
- `E2E-065-a-stated-add-names-the-step-it-did-not-take` — a pinned `add --max` learns the round number one step up.
- `E2E-070-audit-reports-a-ceiling-with-no-headroom` — a ceiling sitting exactly on its file is a finding.
- `E2E-071-a-ceiling-on-the-step-multiple-names-the-stated-form` — the advice is a call the command performs.
- `E2E-072-a-spent-soft-ceiling-names-the-range-under-the-hard-limit` — the range starts above the measurement.
- `E2E-073-no-soft-ceiling-under-the-hard-limit-grants-headroom` — the report says so rather than naming an empty range.
- `E2E-074-a-finding-names-what-an-exception-would-accept` — the finding names the ceiling a plain exception writes.
- `E2E-075-a-ceiling-on-the-hard-limit-is-not-named` — a number the command would refuse is withheld instead.
- `E2E-076-a-byte-finding-names-what-an-exception-would-accept` — a detail with no budget clause carries it too.
- `E2E-077-a-rule-exclusion-keeps-other-budgets` — a changelog leaves its line rule but stays under the byte catch-all.
- `E2E-078-limits-prints-rule-exclusions` — text inventory prints a non-empty rule exclusion after its includes.
- `E2E-079-limits-json-carries-rule-exclusions` — JSON inventory carries a non-empty exclusion and omits an empty one.
- `E2E-080-remove-reaches-an-orphan-shadow` — soft removal can reach the twin its earlier hard removal orphaned.
- `E2E-081-the-config-home-governs-the-run` — a config at `.agent-grounds/` is the one in force.
- `E2E-082-init-writes-the-config-home` — init installs the config at the new home and creates no `.agents/`.
- `E2E-083-the-deprecated-home-still-governs-and-says-so` — the old path keeps working and says to move.
- `E2E-084-the-config-home-wins-and-names-what-it-ignored` — with both present, precedence is stated rather than silent.
- `E2E-085-init-keeps-a-config-at-the-deprecated-home` — an existing config is never replaced by a generated one.
- `E2E-086-init-force-keeps-a-config-at-the-deprecated-home` — `--force` does not reach the config either.
- `E2E-087-audit-only-prints-one-section` — `--only` prints the section asked for and no other.
- `E2E-088-audit-only-keeps-a-failing-exit` — hiding a hard overflow does not unfail the run.
- `E2E-089-audit-only-renders-in-schema-order` — the order named is not the order printed.
- `E2E-090-audit-only-top-still-needs-a-count` — the one section naming does not compute.
- `E2E-091-audit-only-names-the-valid-sections` — an unknown name is a refusal, not an empty report.
- `E2E-092-audit-only-is-a-text-flag` — the selector stops where the JSON contract starts.
- `E2E-093-audit-only-findings-is-the-gate-output` — asked for findings, audit gives the gate's answer.
- `E2E-094-audit-only-refuses-a-json-config` — a config's JSON format refuses the selector too.
