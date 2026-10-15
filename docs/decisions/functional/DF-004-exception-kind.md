# DF-004-exception-kind: An exception declares whether the file must not be split, or has not been split yet.

Every registry entry answers a question in prose. Version 1 never said which
question, so `reason` collected answers to two different ones:

1. **This file must not be split.** An architectural constraint makes the split
   illegal — a shipped artifact asserted byte-identical by a test, a registry
   with no compiler-enforced seam. Nothing retires it.
2. **This file has not been split yet.** No such constraint exists; a boundary is
   simply missing. It retires when someone does the work.

The two are indistinguishable in a v1 entry and read identically in review. That
is how a registry fills up with entries that restate the finding — *"client
detection, arg parsing, and per-client writers share one file"* — a sentence a
reviewer cannot disagree with, because it does not claim anything.

`kind = "structural" | "deferred"` names which question the entry is answering,
and each kind fixes what its `reason` has to establish
(§GOAL-007-justified-exceptions).

## 1. Decision

An exception entry carries a `kind` (§FS-003-exceptions.2), and
`fissile exception add` requires `--kind` alongside `--severity`
(§FS-005-exception-add.1).

- **`structural`** — the reason must name the architectural constraint that makes
  splitting this file illegal, and what would break if it were split anyway. The
  entry never expires: `until` is `indefinite`, and writing anything else is an
  error, because a constraint with a retirement condition is not a constraint.
- **`deferred`** — the reason must name the boundary that is missing and what has
  to exist before the split is possible. `until` carries the retirement
  condition and may not be `indefinite`: debt with no trigger is a silent ignore
  with extra steps.

Neither kind accepts a description of the file's contents. That is what the
finding already said.

`kind` is optional in registry version 1 and an entry without one reads as
`deferred`, so registries written before the field existed keep loading. The
`kind`/`until` agreement is checked only on entries that declare a `kind`, for
the same reason.

## 2. Why not one free-text field

The distinction is real whether or not the format carries it, so the question is
only whether a tool can see it.

- **A reviewer can check a typed claim.** "Name the constraint" and "name the
  missing boundary" are falsifiable instructions; "explain why the file is large"
  is not. Splitting the field is what makes the instruction specific enough to
  fail.
- **Demanding one phrasing for both corrupts the other.** Asking every entry to
  argue that one file is *necessary* invites fabricating a justification for
  ordinary debt — the exact outcome the two-tier registry design exists to
  prevent (§GOAL-006-graded-limits).
- **`until` cannot carry it.** `"indefinite"` and `"#142 lands"` are different
  kinds of statement sharing one string. Nothing distinguishes a deliberate
  permanent acceptance from an `until` value nobody re-read.
- **Two numbers beat one.** `audit` can now report accepted-permanently and
  carrying-debt separately (§FS-004-check-audit.2). A total exception count mixes
  a design fact with a backlog and is actionable as neither.

## 3. Rejected alternatives

- **A prefix convention inside `reason`** (`STRUCTURAL — …` / `DEFERRED — …`).
  Works in one repository and nowhere else: no tool can check it, no `audit` can
  count it, and the next contributor writes a third prefix.
- **Inferring the kind from `until == "indefinite"`.** That reads a decision out
  of a string chosen for other reasons, and it silently reclassifies every
  pre-existing entry that used `indefinite` as shorthand for "no date yet".
- **Making `kind` required in version 1.** It would reject every registry already
  on disk on upgrade, for a field the tool can default safely. `deferred` is the
  safe default: it keeps `until` meaningful and never claims a constraint the
  author did not assert.
- **A third kind for "unclassified".** A registry that can say "we did not
  decide" collects entries that never get decided. Two kinds force the call at
  the moment the entry is written, which is the only moment the author knows.

## 4. Consequences

- `exception add` gains `--kind`, and `--until` becomes conditional: required for
  `deferred`, defaulted to `indefinite` for `structural`
  (§FS-005-exception-add.1).
- The shipped remediation messages and the managed agent block state what a
  reason must establish, not merely that one is required
  (§DF-003-severity-guidance.1, §FS-002-init.4). Saying "record the debt — a
  written reason and a revisit trigger" reads as satisfied by any sentence, which
  is how the descriptive reason gets written in the first place.
- `audit` reports the two counts whenever a registry holds entries
  (§FS-004-check-audit.2), and JSON carries them unconditionally.
- A silenced hard finding no longer always re-opens the soft one: a `structural`
  hard entry silences the soft finding for the same overflow, a `deferred` one
  leaves it standing (§FS-003-exceptions.3). The minimize loop
  (§GOAL-006-graded-limits.2) assumes shrinking is possible, and the kind is what
  made that knowable. Warning about a file nobody may split names work that does
  not exist, and it cannot be cleared by doing the work — only by writing a
  second entry duplicating the first: same file, same rationale twice, two
  `max_accepted` values free to drift apart.
- The `deferred` half of that rule is deliberate, and it does mean every
  deferred hard acceptance needs a soft entry beside it before the file goes
  quiet. That twin owns no judgment — the decision, its argument, and its
  retirement condition were all made in the hard registry — so it declares
  `shadows = "hard"` and inherits `reason` and `until` rather than storing a
  second copy free to drift (§FS-003-exceptions.2.3). The one field it does own,
  `max_accepted`, is the one the two entries are allowed to disagree about.
  `fissile exception add --shadows-hard` writes it (§FS-005-exception-add.1.1).
- Registry version stays `1`. An upgrade is not a migration: existing entries
  keep loading as `deferred`, and re-classifying them is a repository's own
  review pass, not something the tool forces at load time.
- **Superseded in part by §DF-005-exception-identity**. The format is now version
  `2` and a version-1 registry is refused (§FS-003-exceptions.2.2), so the
  no-migration property above no longer holds. What survives is the field's own
  design: `kind` stays optional, an entry that omits it still reads as
  `deferred`, and the `until` agreement is still checked only where a kind is
  declared.
