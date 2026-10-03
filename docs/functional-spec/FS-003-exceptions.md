# FS-003-exceptions: oversized files are accepted through configured registries

Exception registries are TOML documents that record every file or glob accepted
above a configured limit. Version 2 uses two registries with configurable paths:

- the soft registry, default `docs/file-size-agent-exceptions.toml`, accepts
  agent-facing soft-limit warning debt;
- the hard registry, default `docs/file-size-human-exceptions.toml`, accepts
  human-reviewed hard-limit blocking debt.

The hard registry is the only hard-limit escape hatch. Both registries are typed
data plus reviewable rationale: a reviewer or agent can read why the file is
large, and `fissile` can parse which path and rule are waived. The registry file,
not a field inside the entry, determines whether an entry waives soft or hard
findings. Each entry also records the largest accepted measurement, so an
exception starts reporting again if the file keeps growing.

## 1. File Shape

The file is a versioned TOML document:

```toml
fissile_exceptions_version = 2

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

`fissile_exceptions_version` is required and must be `2`; version 2 removed the
`id` and `replaces` keys version 1 carried (§2.2). Unknown keys are errors. An
entry has no name of its own: it is identified by the registry it lives in and
what it accepts (§DF-005-exception-identity). Exception files are parsed only
according to this functional spec.

## 2. Fields

Required fields:

- `path`: repo-relative path or glob being accepted;
- `match`: `exact` or `glob`;
- `rules`: array of rule IDs, or `["*"]` for every matching rule;
- `max_accepted.value`: largest measurement this exception accepts;
- `max_accepted.unit`: `bytes`, `lines`, or `tokens`;
- `until`: review condition, date, or `indefinite`;
- `reason`: non-empty rationale, establishing what §2.1 requires of the entry's
  kind.

A soft entry that declares `shadows = "hard"` states neither `until` nor
`reason`: it inherits both from the hard entry it shadows, and stating either
here is an error (§2.3).

Optional fields:

- `kind`: `structural` or `deferred` (§2.1); an entry without one reads as
  `deferred`;
- `shadows`: `"hard"` on a soft entry, whose `until` and `reason` then come from
  the hard registry (§2.3);
- `title`: short human-readable label;
- `owner`: team, person, or component responsible for retiring the exception;
- `issue`: tracker URL or ID.

There is no `created` field: the date an exception was added is recorded by the
commit that added it, so duplicating it in the entry would only invite drift.

Unknown fields are errors so typos cannot silently weaken the registry. That is
also what makes a leftover `id` a named error rather than a silent no-op (§2.2).

### 2.1 Kind, And What A Reason Must Establish

An accepted oversized file is one of two things, and the entry says which
(§DF-004-exception-kind). The kind decides what the `reason` has to claim and
what `until` may say; neither kind is answered by describing the file's contents,
which is what the finding already reported.

- `kind = "structural"` — an architectural constraint makes the split illegal.
  The reason names that constraint and what would break if the file were split
  anyway. Nothing retires the entry, so `until` is `indefinite`; any other value
  is a schema error.
- `kind = "deferred"` — no such constraint exists; a boundary is missing. The
  reason names the missing boundary and what has to exist before the split is
  possible. `until` carries the retirement condition and may not be `indefinite`.

An entry that omits `kind` is read as `deferred` for reporting, and the
`kind`/`until` agreement above is checked only on entries that declare a `kind`
— so an entry carried over from a registry written before the field existed
still loads. `fissile exception add` always writes the field explicitly
(§FS-005-exception-add.3).

`indefinite` is matched case-insensitively after trimming, so `Indefinite` and
`indefinite ` are the same value.

### 2.2 What Version 2 Removed, And How To Migrate

A version-1 entry carried a required `id` (`EX-NNN-slug`) and an optional
`replaces` naming another entry's id. Version 2 removes both: what identifies an
entry is the registry it lives in and the condition it accepts, and a second name
for that is a name that can be wrong (§DF-005-exception-identity).

The removal is a break, not a tolerated leftover. A registry declaring version 1
is rejected, and a version-2 registry that still carries `id` fails on the
unknown key (§2) — so no file is left holding a field that silently means
nothing. Migrating one registry is two edits:

1. set `fissile_exceptions_version = 2`;
2. delete every `id` and `replaces` line.

Nothing else about an entry changes. The version error names both edits, because
every adopter meets it once, on upgrade, and a message that only states the
version leaves the remedy to be guessed (§GOAL-003-friendly-output.1). An
unmigrated registry breaks both rules at once — it declares version 1 *and*
carries `id` keys — and the version error is the one reported, because it is the
one that names the whole fix.

`fissile exception add` writes version-2 registries and never writes either key
(§FS-005-exception-add.3).

### 2.3 A Soft Twin May Point At The Hard Entry Instead Of Restating It

A `deferred` hard entry silences the hard finding and leaves the soft one
standing (§3), so every deferred hard acceptance needs a soft entry beside it
before the file goes quiet. That soft entry owns no judgment. Whether to accept
the file, why it may be accepted, and what retires the acceptance were all
decided in the hard registry. Requiring the twin to state a `reason` and an
`until` of its own therefore stores one argument twice, in two files, with
nothing checking that the copies still agree.

`shadows = "hard"` says where the argument lives instead of repeating it:

```toml
[[exceptions]]
path = "crates/grund-core/src/api.rs"
match = "exact"
rules = ["core-source"]
kind = "deferred"
shadows = "hard"
max_accepted = { value = 1592, unit = "lines" }
```

The entry resolves to the hard entry at the same address and takes that entry's
`reason` and `until` as its own, so `audit`, verbose output, and stale reporting
read a shadowing entry exactly like any other (§5).

- `shadows` is legal only in the soft registry, and `"hard"` is its only value.
  A hard entry that declares it is a schema error: the hard registry is where
  the rationale lives, so there is nothing above it to point at.
- `shadows` excludes `reason` and `until`. An entry with a rationale of its own
  is not shadowing one, and allowing both would restore the second copy the
  field exists to remove. Declaring either alongside `shadows` is a schema error
  naming both edits.
- The **address** is the hard entry with the same `path` spelling, the same
  `match`, and the same `max_accepted.unit`, whose `rules` cover every rule the
  shadowing entry lists. Exactly one hard entry must answer it: none is a schema
  error and more than one is an ambiguity error, each naming the shadowing
  entry's site (§4). Both also name the hard registry's configured path, whether
  or not that file exists, since the reader's next edit is in it; and the
  ambiguity names what tells the two candidates apart, reporting a duplicate
  when their rule lists are equal and nothing does. Deleting the hard entry
  therefore fails the load until the twin goes too, which is what makes "retires
  with its twin" a fact rather than a convention.
- `kind` stays the shadowing entry's own field, and the §2.1 agreement is
  checked against the **inherited** `until`. So a `deferred` twin of a
  structural hard entry inherits `indefinite` and fails to load — the forced
  revisit when the original's kind flips.
- `max_accepted` stays required and stays local. It is the one thing a twin
  legitimately owns, and the pair is allowed to differ: accepting hard debt up
  to 620 lines while warning again above 400 is a working pair, not a mismatch.

`shadows` is opt-in. A soft entry that carries its own ceiling and its own
argument for the same path stays legal, and is the right shape whenever the soft
acceptance is a separate decision rather than the shadow of the hard one.

`fissile exception add --shadows-hard` writes the shape above
(§FS-005-exception-add.1.1).

## 3. Matching

`match = "exact"` compares `path` to the repo-relative normalized path.
An exact path names a file, so one that normalizes to the repository root
(`.`, `./`, `src/..`, the root's absolute path) is refused by every command that
takes an exact path — `exception add`, `exception retune` and `exception remove`
— with a message that says it is the repository root, and nothing is written.
`match = "glob"` uses the same glob engine as config rules. An exception applies
only when the path matcher, the `rules` field, the registry severity, and
`max_accepted` match the overflow. `max_accepted.unit` uses the matched rule's
unit and `max_accepted.value` must be greater than or equal to the rule limit for
the registry severity. A soft-registry entry silences only soft findings at or
below its accepted maximum. A hard-registry entry silences only hard findings at
or below its accepted maximum. If the measured value is higher than
`max_accepted.value`, `fissile` reports the overflow again.

A hard finding that still stands — no entry matched, or the file grew past its
ceiling — suppresses the soft finding on its own (§GOAL-006-graded-limits.1).
When a hard finding is *silenced*, the accepting entry's `kind` (§2.1) decides
what happens to the soft finding for the same overflow:

- `kind = "deferred"` — the soft finding is still emitted, so agents keep
  minimizing accepted human debt. An entry that declares no kind reads as
  `deferred` (§2.1) and behaves this way.
- `kind = "structural"` — the soft finding is silenced too. Splitting the file is
  illegal, so the warning asks for work nobody may do and no amount of work can
  clear it. A structural hard entry ends the evaluation of that overflow: the
  soft registry is not consulted, and a soft entry that would have matched the
  same overflow silences nothing while the hard entry is doing it.

The rule reads the hard entry's kind only. A `kind` in the soft registry says
what its own reason must establish (§2.1) and changes nothing about matching.

The rule reaches only as far as a hard finding does. A file below the hard limit
produces no hard finding, so the hard registry is never consulted for it and the
soft warning stands on its own, structural constraint or not. Accepting that
warning is the soft registry's job — with `kind = "structural"` there too, when
the same constraint is what makes it permanent. A soft entry paired with a
structural hard one is therefore dormant, not dead: it takes over exactly where
the hard entry stops applying.

When more than one exception in the same severity registry matches the same
overflow, `fissile` reports a schema error. One accepted oversized condition at
one severity should have one rationale. A single exception entry may list
multiple rules only when all listed rules use the same unit.

The refusal names the entries that collide, not only the file they collide on:
the measured path says which overflow is ambiguous, but what has to be edited is
an entry, and a glob entry need not spell that path at all. It names the `path`
of the first two matching entries in registry order, so the same registry gives
the same message on every run:

```text
docs/soft-exceptions.toml: more than one exception matches src/big.rs for lines rule rust-source; matching entries include path = "src/**" and path = "src/b*.rs"; remove or narrow overlapping entries so only one covers this file, rule and unit in this registry
```

Two names are enough to act on and stay bounded: with three or more matching
entries the message still names two, and says "include" so it does not claim
they are all. When both entries declare the same `path` the two names would read
as one, so the message says "at least two matching entries declare
path = ..." instead, naming it once — equal paths do not make the entries equal,
since they may differ in `match` or `rules`. The refusal is the same whatever the
measurement: a file above either entry's `max_accepted` is as ambiguous as one
below it, because which ceiling applies is the question one rationale answers.
The hard registry is refused in the same words; a soft and a hard entry for the
same overflow are two severities, not a collision.

## 4. Validation

`fissile` validates both registries before evaluating overflows:

- every required field is present once;
- every listed rule ID exists, unless `rules = ["*"]`;
- `max_accepted.value` is a positive integer;
- `max_accepted.unit` is `bytes`, `lines`, or `tokens`;
- `max_accepted.unit` matches every rule the entry can silence;
- `max_accepted.value` is at least the corresponding soft or hard rule limit;
- `reason` is stated and not empty after trimming whitespace — an omitted field
  and a blank one are one defect, the entry carries no rationale, and are
  reported as one;
- a declared `kind` agrees with `until` (§2.1): `structural` requires
  `indefinite`, `deferred` forbids it — for a shadowing entry, against the
  `until` it inherited (§2.3);
- `shadows` appears only in the soft registry, holds only `"hard"`, and never
  stands beside `reason` or `until` (§2.3);
- every `shadows = "hard"` entry resolves to exactly one hard entry at its
  address, whose `reason` and `until` it then carries (§2.3);
- every matched path is inside the scan scope unless stale handling is disabled;
- every stale entry follows `[exceptions].stale`: `warn`, `error`, or `ignore`.

Every diagnostic about a single entry leads with the registry file and the
entry's `path`, because that pair is the line the reader has to edit
(§DF-005-exception-identity):

```text
docs/file-size-human-exceptions.toml: src/orders.rs states no reason
```

The registry file is part of the identifier: the same path may appear in both
registries, making two different claims at two different severities.

A diagnostic about more than one entry keeps the measured file and the candidate
entries apart, because they are different things to the reader: the file is
where the condition arose, the entries are what to edit. It names each candidate
as `path = "<pattern>"`, the line as it appears in the registry, and ends with
the edit that restores one rationale — remove or narrow the overlapping entries
(§FS-003-exceptions.3) — rather than leaving the reader to infer it
(§DF-007-instructions-at-the-error-site.1).

The validator never requires the target file to exist: an entry is well-formed
whether or not the path it names is on disk today, so a file a build has not
written yet, or one a dirty working tree is missing, is not a schema error.

Which entries have outlived their file is a separate question, asked by both
enforcement surfaces against the file set each one has. `check` answers it for
exact paths its own run proves are gone (§FS-004-check-audit.1.3); whole-repo
`audit --stale-exceptions` answers it for every entry, globs included, against
the scan inventory (§FS-004-check-audit.2). Both report under `[exceptions].stale`
— `warn`, `error`, or `ignore` — so one setting governs the whole subject, and
under `error` it fails whichever run raised it.

## 5. Output

An overflow silenced by an exception emits no default finding for that severity.
In verbose audit output, `fissile` includes the severity and the accepted ceiling
so a reviewer can find the entry — in the registry the severity names, under the
path already on the line:

```text
tests/fixtures/parser/large-corpus.json: hard exception (accepted up to 300000 bytes)
```

JSON output carries the same ceiling as `exception_max`.

`audit` also counts the registries by kind (§FS-004-check-audit.2), so a reader
sees accepted-permanently and carrying-debt as two numbers rather than one
undifferentiated total.

## 6. Adding Entries

`fissile exception add` (§FS-005-exception-add) is the supported command for
adding entries. It measures exact-path files, chooses the configured soft or hard
registry, writes `max_accepted`, and validates the result before modifying the
registry.

## 7. Loose Ceilings

A ceiling above the file it accepts is slack, and some slack is deliberate:
measured ceilings are quantized, so a fresh entry normally accepts a little more
than the file measures today (§DF-006-quantized-ceilings). Slack wider than one
`[exceptions.bump]` step is not deliberate. It is the ratchet slipping —
re-granting budget that a split already paid back, to whoever edits the file
next.

An entry is **loose** when all of the following hold:

- `match = "exact"`. A glob's ceiling is a policy for a class of files, not a
  measurement of one, so lowering it to today's largest member would be a
  recommendation to break the next file that class acquires.
- Its path matches a scanned file that measures a value in the entry's unit under
  a rule the entry lists.
- `max_accepted.value` minus that measurement is greater than the unit's bump
  step.

Slack that has run out is the same subject from the other side. An entry has
**no headroom** when it clears the first two conditions above and
`max_accepted.value` *equals* the measurement: the ceiling sits exactly on the
file it accepts, so nothing at all stands between the entry and the finding it
silences. It passes today and fails on the next unrelated commit — the cost
§FS-005-exception-add.2 already names for a ceiling pinned to the day's
measurement, arrived at here by the file growing up to the ceiling rather than
by a caller stating it. `fissile measure` has always shown the condition as
`0 to soft-accepted` (§FS-007-measure.2). `audit` alone was silent, because the
one-step test that excuses a fresh quantized entry excused a spent one too.

The two conditions are exclusive — slack cannot be both wider than a step and
zero — so an entry is loose, or without headroom, or neither. A glob is out of
both, for the reason the first condition gives. A ceiling *below* its file is
neither: that entry has stopped silencing, and `check` reports the overflow
itself (§3).

`audit --stale-exceptions` reports both alongside stale ones
(§FS-004-check-audit.2), each named by its registry and `path`. A loose entry
carries the ceiling `fissile exception retune` would write
(§FS-008-exception-retune) — or, where the step would land a soft ceiling on the
hard limit, the stated form `retune` accepts instead
(§DF-010-stated-ceilings-are-exact.2). An entry without headroom carries the
step's next multiple *strictly above* the ceiling, the smallest round number
that grants any headroom at all; where the measured form of `retune` would write
the number already recorded, or would land on the hard limit, the report names
the stated form for the same reason. An entry whose file has fallen under the
limit altogether has no ceiling worth writing: it silences nothing, and the
report names `fissile exception remove`, the command that deletes it
(§FS-009-exception-remove).

Neither is a failure. `[exceptions].stale` governs stale entries only: a stale
entry accepts a file that is no longer there, while a loose entry — and one
without headroom — accepts everything it accepted yesterday and breaks nothing
today.
