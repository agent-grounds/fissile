# FS-007-measure: fissile measure reports what fissile counts

`fissile measure` answers the one question `check` cannot: how large is this file
right now, in the arithmetic the budgets actually use, and how much room is left.
`check` reports a measurement only inside a finding, so a file that is passing —
under its limits, or under an exception ceiling — is a file whose size the tool
knows and will not say.

The counting rule is fissile's own (§FS-001-config.3.1): blank lines are free by
default and comments are not, so the number is not `wc -l` and nothing outside
`fissile` reproduces it. The decisions that depend on it — whether a new test
goes in this file or a new one, whether a change has room — are made before the
code is written, not after a finding appears.

## 1. Command

```text
fissile measure <paths>... [--staged] [--config <path>]
                [--format text|json] [--no-color]
```

Either explicit paths or `--staged` is required; there is no whole-repo default.
"These files" is `measure`; "the whole repository" is `audit --top`
(§FS-004-check-audit.2). Selection matches `check` exactly in the modes both have: `--staged`
takes the file set from git and applies `[scan].exclude`, explicit paths are
measured as passed (§FS-004-check-audit.1). `measure` has no `--base`: a branch's
verdict is `check --base` (§FS-004-check-audit.1.5).

`measure` is an inspection surface, not a gate. It exits `0` whatever it finds,
including a file over a hard limit. Only a run-level failure or an unmeasurable
path exits `2`, on the same terms as every other command
(§FS-004-check-audit.5).

## 2. Output

One line per `(file, rule)`. A file measured by both a byte rule and a line rule
reports both, because both can bite.

```text
src/domain/order.rs 612 lines [rust-source] soft 350 hard 550 hard-accepted 650 — 38 to hard-accepted
src/domain/tax.rs 289 lines [rust-source] soft 350 hard 550 — 61 to soft
src/domain/vat.rs 980 lines [rust-source] soft 350 hard 550 — 430 over hard
```

The fields are the path, the measured value and its unit, the rule that selected
the budget, the limits that rule declares, and the ceiling of every exception
accepting this file for this rule — labelled by the registry holding it, because
the soft and hard registries make two different claims about one path
(§DF-005-exception-identity).

The clause after the dash is the headroom: the room left before the lowest
threshold the value still clears, named. The thresholds are the rule's limits and
any accepted ceiling. When the value clears none of them, the clause reports how
far it has to come back to clear the highest one. That number is what the caller
came for — how much can I add before something changes — and it is the one number
that cannot be read off the registry, since it depends on a measurement the
registry does not carry.

It is room the caller can spend, so the arithmetic follows how each threshold
binds rather than treating them alike. A rule limit fires *above* the limit
(§GOAL-006-graded-limits), so a 349-line file under a 350-line soft limit has
`1 to soft` and a 350-line one has `0 to soft` while still passing; an accepted
ceiling silences *at* the ceiling (§FS-003-exceptions.3), so a file exactly at its
`hard-accepted` has `0 to hard-accepted` and `check` still calls it `ok`. A
headroom of `n` means `n` more units of the same kind leave every verdict where
it stands, and `0` means the next one does not.

A file matching no rule gets one line saying so, because "no budget applies here"
is an answer and silence is not:

```text
docs/notes.txt — no rule applies
```

JSON emits one record per `(file, rule)` carrying `path`, `unit`, `actual`,
`rule_id`, the `soft` and `hard` limits the rule declares, `soft_accepted` and
`hard_accepted` where an exception applies, and `headroom` with the `headroom_to`
threshold it is measured against. `headroom` is signed — positive is room left
below the named threshold, negative the distance past it — so one field carries
both directions and no consumer has to branch to learn which it got. A field for
a threshold that does not exist is omitted rather than nulled, and a file no rule
measures emits `{"path": …, "unruled": 1}`, because "not measured here" and "not
in the output" must not look alike. The shape is published as
`schema/measure.schema.json` and validated against emitted output.

## 3. Why This Is Not A Flag On check

`check` is the commit-time gate and its worth is that it is boring: findings in
one grouped shape on stdout, and an exit code meaning pass or fail
(§FS-004-check-audit.1). A measurement listing is a different shape under a
different exit contract, so carrying it there would give one command two output
contracts and force a JSON consumer to branch on a flag it did not pass. What the
two commands genuinely share — file-set selection — is shared plumbing, not a
reason to share a name (§FS-006-cli.1).
