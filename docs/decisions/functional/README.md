# Functional decisions

One file per decision, each declaring its own ID at the H1.

- [§DF-001-tool-name](DF-001-tool-name.md#df-001-tool-name-name-the-tool-fissile) — Name the tool `fissile`.
- [§DF-002-explicit-config](DF-002-explicit-config.md#df-002-explicit-config-the-config-is-fully-populated-with-every-value-including-defaults) — The config is fully populated with every value, including defaults.
- [§DF-003-severity-guidance](DF-003-severity-guidance.md#df-003-severity-guidance-soft-and-hard-overflows-carry-different-instructions-and-neither-cites-another-repositorys-docs) — Soft and hard overflows carry different instructions, and neither cites another repository's docs.
- [§DF-004-exception-kind](DF-004-exception-kind.md#df-004-exception-kind-an-exception-declares-whether-the-file-must-not-be-split-or-has-not-been-split-yet) — An exception declares whether the file must not be split, or has not been split yet.
- [§DF-005-exception-identity](DF-005-exception-identity.md#df-005-exception-identity-an-entry-is-identified-by-what-it-accepts-not-by-a-name-for-it) — An entry is identified by what it accepts, not by a name for it.
- [§DF-006-quantized-ceilings](DF-006-quantized-ceilings.md#df-006-quantized-ceilings-a-recorded-ceiling-is-a-round-number-not-a-measurement) — A recorded ceiling is a round number, not a measurement.
- [§DF-007-instructions-at-the-error-site](DF-007-instructions-at-the-error-site.md#df-007-instructions-at-the-error-site-an-instruction-lives-where-the-decision-is-made-not-in-the-always-loaded-block) — An instruction lives where the decision is made, not in the always-loaded block.
- [§DF-008-hard-severity-needs-a-terminal](DF-008-hard-severity-needs-a-terminal.md#df-008-hard-severity-needs-a-terminal-a-hard-exception-is-refused-off-a-terminal-and---force-is-the-way-past) — A hard exception is refused off a terminal, and `--force` is the way past.
- [§DF-009-one-file-agents-read](DF-009-one-file-agents-read.md#df-009-one-file-agents-read-agentsmd-holds-the-block-every-other-entrypoint-is-a-link-to-it) — `AGENTS.md` holds the block; every other entrypoint is a link to it.
- [§DF-010-stated-ceilings-are-exact](DF-010-stated-ceilings-are-exact.md#df-010-stated-ceilings-are-exact-a-ceiling-the-caller-states-is-written-as-stated-only-a-measurement-is-quantized) — A ceiling the caller states is written as stated; only a measurement is quantized.
- [§DF-011-rule-local-exclusions](DF-011-rule-local-exclusions.md#df-011-rule-local-exclusions-a-rule-opts-paths-out-with-its-own-exclusions-not-with-a-thresholdless-winner) — A rule opts paths out with its own exclusions, not with a thresholdless winner.
- [§DF-012-config-home](DF-012-config-home.md#df-012-config-home-tool-config-lives-in-agent-grounds-and-agents-stays-instructions) — Tool config lives in `.agent-grounds/`, and `.agents/` stays instructions.
- [§DF-013-bounded-soft-grace](DF-013-bounded-soft-grace.md#df-013-bounded-soft-grace-repeated-soft-debt-blocks-only-when-history-proves-the-grace-is-spent) — Repeated soft debt blocks only when history proves the grace is spent.
