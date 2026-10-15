# FS-008-exception-retune: fissile exception retune moves a recorded ceiling

An exception's `reason` outlives its number. A file whose split is genuinely
illegal is still illegal two lines later, and a boundary that was missing is
still missing. What changes is `max_accepted` — and moving it is bookkeeping, not
a new claim. Until `retune`, the only way to move it was hand-edited registry
TOML, which skips every check `exception add` applies and, in a repository that
keeps twin registries, desynchronizes them silently.

`fissile exception retune` moves the ceiling of an entry that already exists. It
writes no `reason`, no `kind`, and no `until`: an entry whose claim has changed is
not a retune, it is a different entry.

## 1. Command

```text
fissile exception retune <path> --severity soft|hard --rule <id>
                         [--max <N> --unit bytes|lines|tokens]
                         [--config <path>] [--match exact|glob] [--dry-run]
```

`<path>`, `--severity`, `--rule` and `--match` address the entry with exactly the
fields that describe one to `exception add`: an entry is identified by the
registry it lives in and the `(path matcher, rules, unit)` condition it accepts
(§DF-005-exception-identity). `--rule` may be repeated, and the address matches an
entry that covers any of the named rules at the selected unit. When no entry
answers to that address the command fails without writing, and the error names
`fissile exception add` as the command that creates one.

The matcher is part of the address, so an address that merely *overlaps* an
entry is refused in both directions, each for its own reason. An exact path
covered by a glob entry cannot be retuned as itself: one member's measurement
must not set a ceiling for the class, so the error names the glob to address
instead. A glob spanning an exact entry cannot be retuned either: writing a
class-wide number into one file's entry leaves every other file the glob names
at its old ceiling and reports the change under a path no entry carries. An
address matching two entries is refused as ambiguous and names both — two exact
entries under one glob is a registry §FS-003-exceptions.4 accepts, so the fault
is the address, not the file.

With `--max` omitted, the new ceiling is the file's current measurement
quantized to `[exceptions.bump]` (§FS-005-exception-add.2,
§DF-006-quantized-ceilings): the caller states no number, so the step chooses
one. With `--max`, the ceiling is the value stated
(§DF-010-stated-ceilings-are-exact.1), and the result names the step's next
multiple as the round number the caller could have chosen instead.

For `--match glob` there is no single file to measure, so `--max` and `--unit`
are required, as they are for a glob `add`.

## 2. Direction

Both directions, one command. Raising accepts a file that grew; lowering follows
a file that shrank and is how a loose ceiling is retired
(§FS-003-exceptions.7). Neither direction asks for a new rationale, and neither is
gated by severity: the alternative to retuning a hard entry is a hand-edited hard
registry, which is the same change made worse. What reviews a moved ceiling is
the registry diff, exactly as before (§GOAL-007-justified-exceptions).

The new ceiling is never below the current measurement. Writing one would leave
the entry accepting less than the file it exists to accept, standing a finding
against an exception someone wrote on purpose.

A stated ceiling moves by exactly the amount stated, so `--max` is also how a
ceiling comes down by less than one step: the measured form can only land on a
multiple of the step, and a file that shrank from 500 to 472 lines is still
accepted at 500 by it.

Lowering stops at the rule's limit, and a file that has fallen under that limit
cannot be followed any further: an entry accepting less than the limit silences
nothing. That is the same state `audit --stale-exceptions` reports as "silences
nothing now" (§FS-003-exceptions.7), so the refusal names the same remedy — and
names the command that performs it, `fissile exception remove`
(§FS-009-exception-remove). It reports the measurement it read, never a `--max`
the caller did not pass.

## 3. Result

The command rewrites one `max_accepted` value in place. Every other byte of the
registry — comments, entry order, the other fields of the entry itself, and the
line endings the file is stored with — is preserved, so the diff is a single line
and a reviewer reads precisely the decision that changed.

Which line that is comes from reading the registry as TOML, not from matching
text. A `reason` is prose, and prose quoting `[[exceptions]]` or a
`max_accepted =` line — in either multi-line string form, or after a `#` — names
no entry and shifts no index. When the addressed entry does not spell its
ceiling as one inline table, the command refuses and says so rather than
guessing at a rewrite.

```text
docs/file-size-agent-exceptions.toml: src/order.rs 486 -> 500 lines (measured 488 lines; quantized to 100-line step)
```

When the step raises a measurement, the result names the measurement and the
configured step, so the ceiling cannot look like it came from a coincidentally
equal rule limit. A stated `--max` is written as it is; when it is not a multiple
of the step, the result names the next multiple — the round number the measured
form would have chosen — as a suggestion, omitted when that number is one the
command would refuse (§FS-008-exception-retune.4):

```text
docs/file-size-human-exceptions.toml: src/order.rs 500 -> 501 lines (next 100-line step: 600)
```

When the quantized ceiling equals the recorded one, nothing is written and the
command says so and exits `0`. An idempotent retune is the normal outcome of an
edit that stayed inside the step, and it must not read as a failure.

When the other registry also holds an entry for that path and rule, the result
names it and its ceiling. `retune` never writes to a registry the caller did not
select — twin consistency is a repository's policy, not the tool's — but a caller
about to leave two ceilings disagreeing should learn it here rather than from a
later run.

## 4. Validation

`retune` validates what `add` validates (§FS-005-exception-add.4): the effective
config, both registries as they stand, and the combined document before the
write. It fails additionally when the address matches no entry, when it matches
more than one, when `--max` is below the current measurement or below the
selected rule's limit for the chosen severity, and when a `--severity soft`
soft ceiling would be at or above the rule's hard limit, with no **deferred**
hard entry at the same address (§DF-010-stated-ceilings-are-exact.2) — a
structural twin ends evaluation above the limit (§FS-003-exceptions.3) and so
exempts nothing, and a file already past the limit is left alone. That last
refusal is the instruction (§DF-007-instructions-at-the-error-site): whether the
step produced the number or `--max` did, it prints this call with `--max <N>
--unit <unit>` and the range `N` may take, and for a stated value also the
hard-severity `exception add` — carrying the ceiling and both `--kind`
spellings, so the offered command runs as printed rather than being refused for
accepting a file that needs no exception.

`--dry-run` prints the ceiling change and the registry it would update, and
modifies nothing.
