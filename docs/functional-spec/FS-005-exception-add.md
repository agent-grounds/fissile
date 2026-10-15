# FS-005-exception-add: fissile exception add writes structured exception entries

`fissile exception add` is the supported way to add entries to the soft and hard
exception registries. Users should not need to hand-edit registry TOML for the
common case of accepting a current overflow.

## 1. Command

```text
fissile exception add <path> --severity soft|hard --rule <id>
                      --kind structural|deferred --reason <text>
                      [--until <text>] [--shadows-hard] [--config <path>]
                      [--match exact|glob]
                      [--title <text>] [--owner <text>] [--issue <text>]
                      [--max <N> --unit bytes|lines|tokens]
                      [--force] [--dry-run]
```

`--severity` chooses the configured registry: `soft` writes to
`[exceptions].soft_registry`; `hard` writes to `[exceptions].hard_registry`.
`--rule` may be repeated to create one exception for multiple same-unit rules.

`--kind` and `--reason` are required so every accepted oversized file carries a
claim a reviewer can disagree with (§FS-003-exceptions.2.1,
§DF-004-exception-kind). The kind decides what the reason must establish and what
`--until` may say:

- `--kind structural` — the reason names the architectural constraint that makes
  splitting illegal, and what would break if the file were split anyway.
  `--until` is optional and defaults to `indefinite`; passing any other value is
  a usage error.
- `--kind deferred` — the reason names the boundary that is missing and what has
  to exist before the split is possible. `--until` is required and may not be
  `indefinite`.

Neither is answered by describing the file's contents; that is what the finding
already said. The flags the command requires make the two questions impossible
to conflate, and the error text names the distinction at the moment the entry is
written.

The command does not judge the prose, with one bounded exception. When a
`--reason` says nothing beyond the finding's own facts, it warns and still
writes the entry (§FS-005-exception-add.4). It never refuses on prose: the test can only catch a
reason that is *entirely* restatement, and a command that rejected a terse but
honest claim would teach callers to pad it.

`--force` is the way past the severity gate in §FS-005-exception-add.4. It has no other effect.

`--match` defaults to `exact`. `glob` is allowed only when `<path>` contains a
glob metacharacter. The command never creates `[scan].exclude` entries; accepted
oversized files remain under `fissile` measurement.

### 1.1 --shadows-hard Writes The Soft Twin Of A Hard Entry

A deferred hard entry leaves the soft finding standing (§FS-003-exceptions.3),
so silencing the file takes a second entry in the soft registry — one whose
rationale was already written, in the hard registry. `--shadows-hard` records
that relationship instead of restating it (§FS-003-exceptions.2.3):

```console
$ fissile exception add src/big.rs --severity soft --rule rust --shadows-hard
appended src/big.rs to docs/file-size-agent-exceptions.toml (accepted up to 100 lines)
```

The flag requires `--severity soft` and takes none of `--kind`, `--reason`, or
`--until`: all three belong to the hard entry at the same address, and passing
one is a usage error that names the flag to drop. Everything about the ceiling
is unchanged — the command still measures the file, still honors `--max`, and
still writes a `max_accepted` that is the twin's own to move
(§FS-003-exceptions.2.3).

The hard entry has to exist already. When no hard entry answers the address, or
more than one does, the command refuses without writing and names the registry
it looked in (§FS-005-exception-add.4): the shadow of a decision nobody recorded is not a shadow, and
guessing which of two entries it points at would decide the thing the flag
exists to state.

The hard-limit rule in §FS-005-exception-add.4 applies unchanged, and `--shadows-hard` does not buy
past it. The flag guarantees a hard entry at the address, not a **deferred**
one: shadowing a `deferred` entry accepts a ceiling at or above the rule's hard
limit, because that entry keeps the soft finding alive up there
(§FS-003-exceptions.3); shadowing a `structural` one is refused on the same
terms as any other soft entry, since evaluation ends at the hard entry and the
soft ceiling would never fire. A file already at or above the limit is spared by
§FS-005-exception-add.4's own exemption, whichever kind the twin declares.

The restatement check (§FS-005-exception-add.4) does not apply to a shadowing entry. There is no
reason of its own to judge, which is the point of writing it this way.

## 2. Accepted Size

When `--max` is omitted, `fissile` measures `<path>` using the selected rule
unit. When `--max` is present, `--unit` is required, the unit must match every
selected rule, and the value must be at least the selected soft or hard limit for
the chosen severity and at least the current measurement for exact-path entries.

Which of the two supplied the value decides what is written. A measurement is
quantized up to the unit's `[exceptions.bump]` step (§FS-001-config.5): the
smallest multiple of the step at or above it, so under the default 100-line step
a 488-line file is accepted at 500. The entry is still a ceiling and not an
open-ended waiver — the finding returns once the file passes the number — but
the number is a decision a reviewer can weigh rather than a reading taken on the
day the entry was written (§DF-006-quantized-ceilings). A `--max` is written as
stated: the caller has already made that decision, and rounding it would replace
their number with one nobody chose (§DF-010-stated-ceilings-are-exact.1).
`fissile exception retune` moves either afterwards (§FS-008-exception-retune).

A `check` or `audit` finding names the ceiling the measured form would write
for the file it reports, before either form is run (§FS-004-check-audit.1), so
the choice between them is made with both numbers on screen rather than with the
measurement alone.

A stated ceiling names the step it did not take. When `--max` is not a multiple
of the step, the result names the next multiple — the round number the measured
form would have chosen — as a suggestion, never applied, and omitted when that
number is one §FS-005-exception-add.4 would refuse:

```console
$ fissile exception add README.md --severity hard --rule entrypoints \
    --kind deferred --max 519 --unit lines --until 'the docs site exists' \
    --reason '...' --force
appended README.md to docs/file-size-human-exceptions.toml (accepted up to 519 lines; next 100-line step: 600)
```

A ceiling pinned to the day's measurement grants no headroom, so the first
unrelated growth fails the gate. Naming the round number one step away is what
makes that the caller's informed choice rather than a silent one
(§DF-010-stated-ceilings-are-exact.3). `exception retune` names it in the same
words (§FS-008-exception-retune.3), so a caller who has seen one command's
result can read the other's. The measured form names nothing: the step already
chose its number.

For `--match glob`, `--max` and `--unit` are required because there is no single
file measurement to infer — so a glob ceiling is always the number stated.

A `--severity soft` ceiling at or above the rule's hard limit is refused (§FS-005-exception-add.4):
the hard finding fires there and suppresses the soft one, so the entry would
never fire (§DF-010-stated-ceilings-are-exact.2). A glob is held to the same
rule — it measures nothing, so no class of files can claim the exemption a
single file already past the limit has.

## 3. Generated Entry

The command appends one `[[exceptions]]` table to the selected registry:

```toml
[[exceptions]]
title = "generated parser fixture"
path = "tests/fixtures/parser/large-corpus.json"
match = "exact"
rules = ["fixtures"]
kind = "deferred"
max_accepted = { value = 300000, unit = "bytes" }
until = "the fixture generator lands"
owner = "parser"
reason = """
Missing boundary: a generator that reproduces this corpus from the incident
descriptions. Until one exists the corpus can only be split by hand, and a hand
split loses the incident-to-case mapping the fixture exists to preserve.
"""
```

`kind` and `until` are always written, even for a `structural` entry that took
the `indefinite` default, so a registry entry never depends on a reader knowing
the command's defaults (§DF-002-explicit-config).

A `--shadows-hard` entry writes `shadows = "hard"` in place of `until` and
`reason`, and copies `kind` from the hard entry it shadows — the twin still
states its kind outright rather than making a reader follow the pointer for it
(§FS-003-exceptions.2.3):

```toml
[[exceptions]]
path = "src/big.rs"
match = "exact"
rules = ["rust"]
kind = "deferred"
shadows = "hard"
max_accepted = { value = 600, unit = "lines" }
```

The entry gets no name of its own: it is identified by the registry it is written
to and what it accepts (§FS-003-exceptions.2.2, §DF-005-exception-identity), and
the command never writes the removed `id` or `replaces` keys. The entry records
no date — the commit that adds it carries that — and optional flags are omitted
when absent.

If the target registry does not exist, `fissile` creates it with:

```toml
fissile_exceptions_version = 2
```

Existing registry comments and entry order are preserved. New entries append at
the end so reviews see exactly what changed.

## 4. Validation

Before writing, `fissile` validates the effective config, both exception
registries, and the new entry using §FS-003-exceptions. The command fails without
modifying files when:

- the selected rule does not exist;
- selected rules use different units;
- `--kind` is absent, or `--until` disagrees with it (§FS-005-exception-add.1);
- `--shadows-hard` was passed with `--severity hard`, or alongside `--kind`,
  `--reason`, or `--until` (§FS-005-exception-add.1.1). The refusal names the flag to drop: a
  shadowing entry that also carried one of them would be the second copy the
  field exists to remove (§FS-003-exceptions.2.3);
- `--shadows-hard` was passed and the hard registry holds no entry — or holds
  more than one — at the same `path` and `match`, in the same unit, covering
  every `--rule` given (§FS-003-exceptions.2.3). The refusal names the hard
  registry and the address it looked for, and both ways forward: record the
  hard acceptance first, or state this entry's own `--kind`, `--reason`, and
  `--until`;
- another exception in the same severity registry already answers to the same
  `(path, rule, unit)` address — the rejection names that registry and the
  entry's `path`, reports the ceiling it records against the file's current
  measurement, and names `fissile exception retune` as the command that moves it
  (§FS-008-exception-retune). "An entry exists here" and "the file is accepted"
  are different facts, and a refusal issued while `check` is reporting that very
  file must not assert the second;
- `--max` would make the exception invalid or smaller than the current exact-path
  measurement;
- the ceiling would be at or above the selected rule's hard limit for a
  `--severity soft` entry, and the hard registry holds no **deferred** entry at
  the same address (§DF-010-stated-ceilings-are-exact.2) — only a deferred twin
  keeps the soft finding alive above the limit (§FS-003-exceptions.3); a
  structural one ends evaluation there, leaving the soft ceiling as dead as one
  with no twin. A file already past the limit is the debt the soft route below
  records, and is accepted as before; a glob measures nothing and never claims
  that exemption. Whether the step produced that number or `--max` did, the
  refusal prints the form that succeeds — this call with `--max <N> --unit
  <unit>` and the range `N` may take, whose floor clears every selected rule's
  soft limit so the next call is not refused in turn — and, for a stated value,
  the hard-severity call carrying that same ceiling, as the other route
  (§DF-007-instructions-at-the-error-site);
- the registry contains unrelated schema errors;
- `--severity hard` was passed, standard input is not a terminal, and `--force`
  was not passed (§DF-008-hard-severity-needs-a-terminal.1). The refusal names
  the soft-severity route, which is the one an agent can take on its own, and
  names `--force` for the script that legitimately adds hard entries. That soft
  route carries the caller's `--max` unless the number is one a soft entry may
  not hold, in which case it asks for `--max <N>` instead: repeating a ceiling
  the rule above refuses would close a circle between the two refusals
  (§DF-007-instructions-at-the-error-site). `--dry-run`
  is refused on the same terms: a dry run that printed the entry would answer
  the question the gate exists to route to a person.

  The route is offered as a command, and it is this call with `--severity soft`:
  every other flag carried through, including the `--reason` the caller already
  wrote and the `--kind` they claimed. Both matter. A command missing a required
  flag ends in a second refusal, which teaches nothing and leaves hand-edited
  registry TOML as the way forward; and substituting a kind rewrites the
  caller's claim about the file into a different one — a structural constraint
  told to name what retires it has nothing to name (§DF-004-exception-kind).

After those checks pass, one condition warns without refusing. A `--reason` is a
**restatement** when, with the entry's own facts removed — the `<path>`, the
selected rule ids, the unit name, and every number — fewer than five words are
left. "src/big.rs is 612 lines, over the 550-line limit" reduces to four and
warns; a reason that names a constraint or a missing boundary does not come
close. The warning goes to stderr, the entry is written, and the exit code is
unchanged: what is wrong with a restatement is that a reviewer learns nothing
from it (§DF-004-exception-kind), which is a reason to say so, not to block a
commit on a word count.

`--dry-run` prints the TOML entry that would be appended and the registry path it
would update — with the same next-step suggestion §FS-005-exception-add.2 names, when `--max` supplied
a ceiling off the step:

```text
would update docs/file-size-human-exceptions.toml (next 100-line step: 600)
```

It does not modify the filesystem.
