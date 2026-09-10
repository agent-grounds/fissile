# FS-004-check-audit: fissile check and audit enforce file budgets

`fissile check` and `fissile audit` are the user-visible enforcement surfaces
for the library core. `check` is the commit-time gate; `audit` is the whole-repo
inventory and migration tool. Both use the same effective config, rule
resolution, exclusions, messages, and exception registries.

## 1. Check

```text
fissile check [<paths>...] [--staged] [--config <path>] [--format text|json] [--no-color]
```

`check --staged` receives the file set from git and applies `[scan].exclude`.
Without `--staged`, `check` evaluates the paths passed by the caller or the
configured scan scope. A file strictly above a soft limit produces a finding
and exits `0` unless a matching soft exception applies; equality passes. On
`check --staged`, a history-proven continuous run of over-soft edits can promote
that soft finding into a commit block (§1.4). A file strictly above a hard limit
produces a finding and exits non-zero unless a matching hard exception applies;
equality passes. Severity and promotion are not invocation-time knobs.
This is the stable
CI/pre-commit contract: the same config must produce the same pass/fail result
locally and remotely (§GOAL-003-friendly-output).

Text findings are grouped, one block per `(severity, rule, rendered guidance)`.
The header names the severity, the file count, the crossed limit, the rule, and
the message ID; the guidance follows once, indented two spaces; the files follow
indented four. Guidance is never repeated per file — a repo-wide run says what
to do once and then lists what it applies to (§GOAL-003-friendly-output).

```text
hard: 2 files over the 550-line budget [rule: rust-source, message: split-rust-hard]
  Must split before more code lands here: move cohesive groups of items into
  sibling modules. If you cannot see a safe split, stop and ask a human.
    src/domain/order.rs: 612 non-blank lines (budget 550; an exception here would accept 700)
    src/domain/invoice.rs: 588 non-blank lines (budget 550; an exception here would accept 600)

soft: 1 file over the 350-line budget [rule: rust-source, message: split-rust-soft]
  Split now or record the debt now. If no split leaves the architecture cleaner,
  record it with `fissile exception add --severity soft`.
    src/domain/tax.rs: 402 non-blank lines (budget 350; an exception here would accept 500)
```

Blocks are ordered hard before soft, then by rule ID; files within a block are
ordered by measured value descending, then by path. Blocks are separated by a
blank line. A message that interpolates a per-file variable renders distinct
text per file, which by the grouping key puts each file in its own block
(§FS-001-config.4).

For a line rule, each file detail is `<path>: <actual> <counting basis> (budget
<limit>)`. UTF-8 measurements name `physical lines` when blank and comment lines
count, `non-blank lines` when only blank lines are excluded, `non-comment lines`
when only comment lines are excluded, and `non-blank, non-comment lines` when
both are excluded. A non-UTF-8 raw-line measurement names `physical lines`.
Byte and token findings retain `<path>: <actual> <unit>`.

Each detail also names the ceiling a `fissile exception add` with no `--max`
would write for that file: the measurement quantized up to the unit's
`[exceptions.bump]` step (§DF-006-quantized-ceilings.1, §FS-005-exception-add.2).
It is the number that command already computes, said at the moment the caller is
choosing between the plain form and `--max`, so a line rule reads

```text
    src/domain/order.rs: 612 non-blank lines (budget 550; an exception here would accept 700)
```

and a byte or token rule, which carries no budget clause, opens a parenthesis of
its own:

```text
    assets/atlas.bin: 5200 bytes (an exception here would accept 8192)
```

The number is what makes the plain form the obvious one to reach for. A ceiling
stated with `--max` is written exactly as stated
(§DF-010-stated-ceilings-are-exact.1), so a caller who copies the measurement
off this line into `--max` records a ceiling with no headroom and fails the gate
on the next unrelated edit; the ceiling named beside it is the entry they would
get by asking for nothing. Where an entry already stands at the address, `add`
refuses and names `fissile exception retune` (§FS-005-exception-add.4), which
writes this same number — the line says what the file would be accepted at, not
which of the two commands writes it. It is not the `next <step>-<unit> step: N`
that `add` and `retune` print on a result (§FS-005-exception-add.2): that one is
a round number a *stated* ceiling passed up and never applied, and this one is
the ceiling that would actually be recorded.

The ceiling is named only where that plain call would be accepted. For a soft
finding on a rule that also sets a hard limit, a ceiling at or above that limit
is refused while the file is still under it, because the hard finding fires
there and the soft entry would never match (§DF-010-stated-ceilings-are-exact.2);
the detail then names no ceiling at all rather than a number the command would
decline. A file already past the hard limit keeps its number, on the same terms
that accept the entry — it is the record of the debt (§FS-005-exception-add.4).
A hard finding never withholds it: nothing binds a hard ceiling, and the
quantized value is at or above a measurement already over the limit.

One case deliberately says less than it could. `add` also accepts a soft ceiling
above the hard limit when the hard registry holds a *deferred* entry at the same
address (§FS-005-exception-add.4), and a finding does not read the registries to
find out — it withholds there too. Withholding is the direction that cannot
mislead. The caller runs the plain form and gets the entry the command would
have written anyway; a number printed here that ended in a refusal would have
sent them somewhere with nothing to do.

Guidance is wrapped at a fixed 78 columns, and newlines written into the message
are kept, so a project that configures a paragraph gets a readable block. The
width is fixed rather than read from the terminal: the same finding must be
byte-identical in a narrow terminal and in CI (§GOAL-006-graded-limits.2).

### 1.1 The hint line

A `check` run that reports at least one finding adds one `hint:` line naming
`fissile measure`, directly beneath the findings it is about:

```text
hint: fissile measure <path>... reports size and headroom for the files you split into.
```

The finding already carries the offending file's measurement and the limit it
crossed, so the hint is not about that file. It is about the ones the split
moves code *into*, whose headroom decides where the seam can go and which no
other tool computes (§FS-007-measure). One line, once per run, never per file,
and only when a *finding* was reported — a run whose only block is the stale
inventory of §1.3 has no split to place, and a clean run stays exactly `ok`.

This line and §1.2 are the two things `check` prints that are not findings. Both
exist because the instructions they carry left the managed agent block
(§DF-007-instructions-at-the-error-site.2), and both are bounded to a single
line so that §GOAL-004-token-thrift still holds for a run that reports many
files.

### 1.2 The commit-gate epilogue

A `check --staged` run that exits non-zero closes by saying so. Every reason to
fail has an epilogue, and the two are decided together: an epilogue printed
without failing is a false alarm, and a failure printed without one aborts the
commit with output that reads as advisory.

A standing hard overflow:

```text
commit blocked by fissile. Split the file, or ask a human for a reviewed hard
exception. Bypassing with --no-verify leaves the overflow for review or CI.
```

A dead exception entry under `[exceptions].stale = "error"` (§1.3), where there
is no file to split and the fix is in the registry the block above names:

```text
commit blocked by fissile. Remove the exception entry above, or point it at the
path its file moved to. Bypassing with --no-verify leaves a dead entry in the
registry.
```

A staged file that could not be measured, which exits 2 (§5) with nothing above
accounting for it:

```text
commit blocked by fissile. A staged file could not be measured, so nothing above
accounts for it — fix the path the error names, or unstage it. Bypassing with
--no-verify commits a file fissile never checked.
```

A run blocked by more than one leads with the overflow, then the dead entry: the
split is the largest thing to do, and each block is on screen above the epilogue
either way.

Only `--staged` prints an epilogue, because only `--staged` is a commit: the
same findings from a CI run or a manual `fissile check src/` are not blocking
anything a caller is about to bypass. It says the one thing the finding's own
guidance cannot, since a project rewrites that guidance (§DF-003-severity-guidance.1)
and it is the wrong voice regardless — `--no-verify` is reached for by a caller
who has just decided the gate is in the way.

### 1.3 Stale exceptions

`check` reports every `match = "exact"` exception entry its own file set proves
has outlived its file, in a block of its own:

```text
stale: 1 exception accepts a file that is not there [registry: docs/file-size-agent-exceptions.toml]
  The file moved or was deleted, so the entry silences nothing. Remove it with
  `fissile exception remove`, or point it at the path the file moved to.
    src/domain/order.rs [soft, rule: rust-source]
```

Entries are reported under `[exceptions].stale`: `warn` reports them, `error`
also fails the run — and through the pre-commit hook, blocks the commit (§1.2) —
`ignore` says nothing (§FS-003-exceptions.4).

`check` reports only what its file set proves, and its three file sets prove
different things:

- **`--staged`** is a commit, and a commit proves what it removes: the entry is
  reported when the run stages the deletion of its path or a rename away from
  it — the moment it died, with the diff that killed it on screen.
- **The configured scan scope** — plain `fissile check` — is the whole
  inventory, so an entry matching nothing in it is stale by §2's comparison,
  provided no file stands at its path: one the scope excludes or git ignores is
  right where the entry says, and §2 reports it without blocking a build.
- **Caller-passed paths** are a window, not an inventory, and prove nothing
  about an entry naming some other file. Nothing is reported.

Absence from the working tree is deliberately *not* the test on its own. A path can be
missing because a build has not written it, or because someone deleted it
without staging the deletion; neither means the entry has outlived anything, and
under `error` each would stand between the author and every later commit over an
entry that is still correct.

Globs are not judged here: a glob matching no file today is a question about the
scan scope, which is `audit`'s inventory to answer (§2), not a fact a commit
hook can establish.

So this is the staleness §2 reports, under the same setting and by the same
comparison — one fact, not two (§FS-003-exceptions.4). `check` only ever says
less: a narrower file set, and never a file that is still there.

JSON output emits one record per overflow with at least:

- `path`
- `unit`
- `actual`
- `limit`
- `severity`
- `rule_id`
- `message_id`
- `message`
- `exception_would_accept`, when the finding names a ceiling
- `exception_max`, when applicable in audit's silenced output

A staged soft finding additionally carries `soft_edit_count`,
`soft_edit_limit`, and `soft_edit_history_complete`. A promoted one retains
`"severity":"soft"` and adds `"promotion":"soft_edit_limit"`; a true hard
size overflow carries none of those fields. This keeps the exception route and
the reason for the blocking exit machine-visible without misreporting a soft
overflow as a hard-size overflow (§1.4).

`exception_would_accept` carries the same number the text detail names and is
omitted wherever the text withholds it, so a consumer of `--format json` chooses
between the plain and the stated form on the same facts a reader of the text
does. It is absent from a silenced `audit` record, which carries `exception_max`
— the ceiling the entry that already accepts the file records — instead.

When no findings are emitted, text output prints exactly `ok`; JSON output emits
no success envelope.

A `check` run can exit non-zero for something that is not an overflow — a dead
exception entry under `[exceptions].stale = "error"` (§1.3). The findings array
is the stable machine contract and grows no second record shape for it: the
stale block goes to stderr, which already owns every diagnostic a JSON run emits
(§5). What is ruled out is the one shape a consumer cannot act on — an empty
array, a failing exit code, and nothing anywhere saying why.

### 1.4 Bounded grace for staged soft edits

The soft tier asks for a decision in the commit that encounters it: split now,
or record the soft-limit debt now. It is a bounded grace period rather than a
permanent consequence-free warning. For each staged regular file that is above
its effective rule's soft limit, below or equal to its hard limit, and not
accepted by a matching soft exception, `check --staged` derives a continuous
over-soft edit count for that rule (§FS-001-config.3):

1. The current staged edit counts. When it is the edit that crosses from a
   committed version at or below the soft limit, its count is `1`.
2. Walking every commit reachable from `HEAD` backwards, each commit that
   introduces a file version distinct from its parent while that resulting
   version remained above the soft limit counts. Commits that did not edit it do
   not count. A merge whose version equals either parent imports history but is
   not another edit; a merge resolution distinct from every parent counts once.
   The effective rule's include and exclude scope is applied to every historical
   name. A rename from outside that scope into it is the first governed edit;
   versions under the old out-of-scope name cannot consume its grace.
3. The run ends at the most recent committed version at or below the soft limit,
   or at an established absence of the file. That boundary resets the count, so
   a later staged crossing starts again at `1`.

Counts below the rule's `soft_edit_limit` remain advisory and exit `0`. A count
equal to or greater than the limit is a **promoted soft finding** and makes the
staged check exit non-zero. With the default limit `5`, edits 1 through 4 warn
and edit 5 blocks. The promotion is still soft-limit debt: text begins `soft
(promoted):`, its detail says `soft edits <count>/<limit>; promoted to
blocking`, and its commit-gate epilogue directs the caller to split or add a
soft exception. It never uses the hard-size heading, guidance, exception route,
or provenance.

From the first staged soft finding, its text detail says `soft edits
<count>/<limit>`. JSON carries `soft_edit_count`, `soft_edit_limit`, and
`soft_edit_history_complete`; only a blocking promotion carries
`"promotion":"soft_edit_limit"`. Files with different counts or limits may
remain in one guidance block because these values are per-file details.

History is evidence for a block, never a guess. The walk measures versions only
until it proves the most recent reset or absence; versions behind that boundary
do not invoke a configured token counter. If the repository is shallow, Git is
unavailable, the input is not in a repository, a rename or rule scope cannot be
followed far enough, a merge graph cannot prove one uninterrupted over-soft
run, or the available history ends while the file is still over soft, fissile
reports only the count it can establish with
`soft_edit_history_complete = false` and keeps the finding advisory even when
that visible count reaches the configured limit. Text adds `history incomplete;
promotion disabled` to that file's edit clause. Plain `check`, `audit`, and the
library checker are snapshot surfaces: they neither inspect history nor attach
edit metadata, and a soft overflow on them does not block.

Size and exceptions take precedence over edit promotion. A true hard-size
overflow reports only hard. If a hard exception exposes the soft tier under
§FS-003-exceptions.3, the edit rule applies to that remaining soft debt; a
structural hard exception continues to silence it. A matching soft exception is
applied before edit promotion and silences the finding both below and at or
above the edit limit. Thus the same repository decision retires repeated soft
debt before and after it becomes commit-blocking (§DF-013-bounded-soft-grace).

## 2. Audit

```text
fissile audit [--config <path>] [--format text|json] [--top <N>]
              [--stale-exceptions] [--rule-coverage]
              [--history <from>..<to>]
              [--only <section>[,<section>]]
```

`audit` walks the configured scan scope and reports the current repository
state. It is for adoption and maintenance, not just pass/fail.

- Default audit reports current soft and hard overflows.
- Default audit also counts the exception registries by kind
  (§FS-003-exceptions.2.1), both as registry entries and as distinct literal
  `path` expressions across the soft and hard registries. The entry totals say
  how many entries are accepted permanently versus how many carry debt someone
  has to retire; the path totals say how many distinct path expressions carry
  each kind. A path expression is structural when any entry with the same
  `Exception::path` is structural, and deferred otherwise, regardless of
  registry or entry order. A glob is one literal path expression and is counted
  once; audit does not expand it into the files it currently matches:

  ```text
  exceptions:
    structural (never expires): 3 entries across 3 paths
    deferred (carrying debt): 32 entries across 20 paths
  ```

  The section is omitted from text output when both registries are empty, so a
  repository with no exceptions pays nothing for it. JSON always carries the
  object with numeric `structural` and `deferred` entry totals plus numeric
  `structural_paths` and `deferred_paths` distinct path totals, because a
  consumer should not have to distinguish "no exceptions" from "this build does
  not report them".
- `--top <N>` reports the largest measured files per unit, after exclusions, even
  when they are under limit. Where a rule measures the file in that unit, the
  value is the one that rule counts — its line policy decides what a line is
  (§FS-001-config.3.1) — so a `--top` number and a finding never disagree about
  one file. Every other scanned file still ranks, under the default line policy:
  `--top` is the adoption surface, and a repository whose rules do not yet reach
  its largest file is the repository that most needs to be told about it.
- `--stale-exceptions` reports every exception entry whose path or glob matches
  no scanned file, each named by its registry and its `path` (§FS-003-exceptions.4)
  — the list spans both registries, and the same path can be stale in each. It
  reports **loose** entries in the same pass: an exact-path entry whose ceiling
  stands more than one bump step above the file it accepts
  (§FS-003-exceptions.7), with the ceiling `fissile exception retune` would write
  in its place. Stale means the entry accepts a file that is gone; loose means it
  accepts far more of a file that is still there. An exact-path entry whose
  ceiling sits *exactly* on its file is reported in the same section, in the same
  line shape, with the advice prefixed `no headroom` (§FS-003-exceptions.7): it
  accepts precisely what the file measures, so it silences the finding today and
  stops on the next unrelated commit.

  The advice on a line is the first of these that applies, and each one is a call
  the named command performs — `audit` never names a remedy the command would
  decline:

  1. **The file no longer crosses the limit at all.** The entry silences
     nothing, so removing it is the remedy rather than moving it, and the line
     names `fissile exception remove` (§FS-009-exception-remove). There is no
     `no headroom` prefix here: an entry the file has fallen below is finished,
     not short of room.
  2. **A soft ceiling would land on the hard limit.** `retune` refuses the
     measured form there (§DF-010-stated-ceilings-are-exact.2), so the line names
     the stated one and the range that keeps the ceiling under the limit. The
     twin that exempts a ceiling here is resolved the same way `retune` resolves
     it. For an entry without headroom the range starts one unit above the
     measurement, since a ceiling at the measurement is what it already has; when
     that leaves the range empty — the file measures one under the hard limit —
     no soft ceiling grants headroom at all, and the line says so and names the
     hard registry instead.
  3. **The measurement is already a multiple of the step.** The measured form of
     `retune` would write the number already recorded and report that it changed
     nothing, so the line names the stated form with the step's next multiple
     filled in. This can only arise for an entry without headroom.
  4. **Otherwise** the line names a `retune to` value: for a loose entry the
     ceiling the step writes from the measurement, for one without headroom the
     step's next multiple strictly above it.

  ```text
  loose ceilings:
    docs/file-size-agent-exceptions.toml: src/domain/order.rs accepts 650 lines, now 421 — retune to 500
    docs/file-size-agent-exceptions.toml: src/domain/model.rs accepts 700 lines, now 472 — retune with --max <N> --unit lines, 472 <= N < 500
    docs/file-size-human-exceptions.toml: README.md accepts 519 lines, now 519 — no headroom; retune to 600
    docs/file-size-human-exceptions.toml: src/domain/tax.rs accepts 500 lines, now 500 — no headroom; retune with --max 600 --unit lines
    docs/file-size-agent-exceptions.toml: src/domain/vat.rs accepts 460 lines, now 460 — no headroom; retune with --max <N> --unit lines, 461 <= N < 500
    docs/file-size-agent-exceptions.toml: src/domain/fee.rs accepts 499 lines, now 499 — no headroom; no soft ceiling under the 500-line hard limit grants any — accept the file in the hard registry with `fissile exception add --severity hard`
  ```

  Every `loose` JSON record carries `no_headroom` as `0` or `1`, so a consumer
  reads which half of §FS-003-exceptions.7 it is looking at without parsing the
  line. The advice keeps the two fields the record already has: `retune_to` for
  case 4, and `stated_range` for cases 2 and 3 — `{"min": N, "max_excluded": M}`
  for a range, `{"min": N}` alone for a stated value with nothing above it to
  exclude. Exactly one of the two is set on every record, except where no
  ceiling under the hard limit grants headroom and there is none to name: then
  both are null. Case 2's empty range is that form — the file measures one unit
  under the hard limit — and so is a rule whose soft and hard limits coincide,
  which reaches it from the loose half, where the entry silences nothing and the
  line is the removal line of case 1.
- `--rule-coverage` reports which rules matched zero files, which files matched
  only built-in catch-all rules, and which rule/message pairs are unused.
- `--only <section>[,<section>]` prints the named sections of the **text**
  report and nothing else. Coverage and registry maintenance are edit-run-edit
  loops, and every iteration of one currently reprints a findings block the
  reader is not looking at; the flag is how the text surface reaches one section
  the way `--format json` already does (§GOAL-004-token-thrift.1).

  The valid names are the eight top-level keys of `schema/audit.schema.json`,
  and their canonical order is the order that schema declares them in:
  `findings`, `silenced`, `exceptions`, `top`, `stale`, `loose`, `coverage`,
  `history`.
  That schema is where the vocabulary comes from, so the text and the JSON
  surface cannot drift into two names for one section: adding a section to the
  schema adds it here, and this flag has no vocabulary of its own to keep in
  step.

  Naming a section is a request to compute it. `--only coverage` reports rule
  coverage without `--rule-coverage`, and `--only stale` or `--only loose` runs
  the registry pass without `--stale-exceptions`: the flag that would otherwise
  ask for the section is what naming it already says. `findings` is the standing
  findings, or the success marker in their place — the marker is what an empty
  findings section prints (§1), so it appears when `findings` is named and not
  otherwise, and `--only coverage` in a repository with nothing to report prints
  coverage and no `ok`.

  Sections render in canonical order whatever order they were named in, and a
  name repeated selects its section once; a second `--only` adds to the
  selection rather than replacing it. The flag names a set, so the same set
  always prints the same bytes however the caller typed it
  (§GOAL-004-token-thrift.1). A named section is rendered exactly as it is
  without `--only` — including the `exceptions:` block omitting itself when both
  registries are empty — so a selection can legitimately print nothing. Without
  `--only`, `audit` prints every section it would print today, in canonical
  order, which is the order it prints them in today.

  `--only top` still requires `--top <N>`, and its absence is a usage error
  naming `--top <N>`. This is the one place where naming a section does not
  compute it, and the reason belongs here rather than in the reader's guess:
  `top` is the only section whose computation takes a **parameter**, and no
  default count is defensible — one repository's useful ranking is another's
  whole inventory. Naming the section says which ranking to print, not how far
  down it goes, so the rule above cannot reach it and the flag carrying the
  number is still required.

  Selection governs what is printed and nothing else. Exit status is computed
  from the whole run, so a repository with a standing hard overflow run with
  `--only coverage` still exits non-zero, and §5's exit `2` for a file that
  could not be measured is equally untouched — as is every stderr diagnostic
  either one carries. A flag that changed a gate's exit code by hiding its
  output would be a trap, and hiding is all this flag does.

  `--only` is a text-surface flag: passing it with `--format json` is a usage
  error, not a filter and not a silent no-op. `findings`, `silenced` and
  `exceptions` are `required` in the schema, so a filtered object would not
  validate against the contract it claims to satisfy, and the JSON surface
  already addresses its sections independently and already omits the ones
  nobody asked for. A consumer who wants one key has `jq`. Accepting the flag
  and ignoring it is the failure worth refusing outright: it is the one
  outcome where the caller cannot tell selection from a section that had
  nothing to say.

  An unknown or empty section name is a usage error naming the name that was
  not recognized and the valid set in canonical order, and exits `2` with its
  diagnostic on stderr (§5) — never a silent empty report, which reads exactly
  like a section that had nothing in it. The eight names are public API in a
  second place from here on: renaming one breaks a command line as well as a
  JSON consumer.

### 2.1 Historical debt direction and age

`--history <from>..<to>` adds an explicit comparison of two committed repository
states. Both endpoints are revision expressions that must resolve to commits,
and `from` must be an ancestor of `to`; the report records their resolved,
full-length commit SHAs rather than the expressions the caller typed. The
history section is last in canonical text and JSON order. `--only history`
isolates its text, but requires `--history`; JSON contains an optional `history`
object exactly when `--history` was requested.

The library exposes the same request as `AuditOptions.history: Option<String>`
and the same selectable/renderable section as `Section::History`, last in
`SECTIONS`. `AuditOptions::select("history")` selects it on the same terms as
the CLI. These are additive public Rust fields and variants in a pre-1.0 crate,
so callers using struct literals or exhaustive matches must update.

This mode belongs only to `audit`. `check` has no history option, and an `audit`
without `--history` does no revision resolution, Git walk, historical checkout,
or rename detection. Its stdout, stderr, and exit behavior are byte-for-byte the
same as before this section existed. The ordinary current-state sections of a
history run still describe the working tree under the ordinary audit contract;
the history object describes the committed `to` snapshot. A caller that wants
only the comparison uses `--only history`.

The history object contains `from`, `to`, a `counts` object, and these arrays in
this order: `added`, `retired`, `raised`, `lowered`, `renamed`,
`deferred_ages`, and `soft_finding_ages`. Every array is present, including when
empty. Counts carry the same seven names and equal the corresponding array
lengths. An exception address is the full
`(registry, severity, path, match, rules, unit)` tuple: `registry` is the
repo-relative registry path at that record's endpoint, `severity` is `soft` or
`hard`, `path` is the literal entry path expression, `match` is `exact` or
`glob`, `rules` is the entry's declared array in its declared order (including
`"*"`), and `unit` is `bytes`, `lines`, or `tokens`
(§DF-005-exception-identity). A finding address has `severity = "soft"`, its
exact file `path`, the one `rule` whose soft limit it crosses, and the rule's
`unit`; it has no registry or matcher because it is specifically unexceptioned.

- `added` and `retired` records are exception addresses plus `value`, the
  ceiling present at `to` or `from` respectively.
- `raised` and `lowered` records are the continuing exception's `to` address
  plus `old_value` and `new_value`. Unit changes are not ceiling movement: unit
  is part of identity, so they are a retirement and an addition.
- `renamed` records carry `old` and `new` exception addresses plus `kind`,
  either `exact-file` or `directory`. A rename can accompany a ceiling movement;
  in that case it appears in both arrays. It never also appears in `added` or
  `retired`.
- `deferred_ages` records carry the current deferred exception address and
  ceiling plus the first commit and date of its current continuous state and
  its age in whole days. Structural entries have no age record. A soft entry
  with `shadows = "hard"` takes the hard entry's effective kind, as it does in
  current-state evaluation (§FS-003-exceptions.2.3).
- `soft_finding_ages` records carry a current unexceptioned soft finding address,
  `actual`, `limit`, and the same first-seen fields. A hard overflow also has a
  soft age when it crosses a distinct soft limit and no soft or structural-hard
  entry silences that soft finding (§FS-003-exceptions.5).

Exception movement arrays sort by the current address where one exists, the
old address for retirements, then by old and new numeric values. Addresses sort
lexicographically by severity, registry, path, match, unit, and the rules array.
Renames sort by their new address and then old address. Age arrays sort by age
descending, then first-seen commit ascending, then their address. Text renders
the same array order, using one line per record and no narrative:

```text
history <full-from-sha>..<full-to-sha>:
  exceptions: +1 -1; ceilings: 1 raised, 1 lowered; renames: 2
  added: soft docs/file-size-agent-exceptions.toml: src/new.rs [match=exact; rules=rust; unit=lines] = 600
  retired: soft docs/file-size-agent-exceptions.toml: src/old.rs [match=exact; rules=rust; unit=lines] = 500
  raised: soft docs/file-size-agent-exceptions.toml: src/growing.rs [match=exact; rules=rust; unit=lines] 500 -> 700
  lowered: hard docs/file-size-human-exceptions.toml: src/shrinking.rs [match=exact; rules=rust; unit=lines] 900 -> 700
  renamed: exact-file soft docs/file-size-agent-exceptions.toml: src/before.rs -> src/after.rs [match=exact; rules=rust; unit=lines]
  deferred age: soft docs/file-size-agent-exceptions.toml: src/new.rs [match=exact; rules=rust; unit=lines] = 600; first <full-sha> 2026-09-01T12:00:00Z; 9 days
  soft finding age: src/standing.rs [rule=rust; unit=lines] 420 > 350; first <full-sha> 2026-08-01T12:00:00Z; 40 days
```

Only non-empty arrays contribute detail lines in text; the summary line always
reports all movement counts, including zeros. JSON uses the field names and
shapes in `schema/history.schema.json`, including `first_seen_commit`,
`first_seen_date`, and `age_days`. Numbers never absorb their unit into a
string. The schema's arrays, required fields, and `additionalProperties: false`
at its envelopes plus closed record shapes are the stable machine contract.

Age begins at the first commit of the current *continuous* state. For a deferred
entry, continuity means an entry at the same identity address, after applying
the rename rules below, remains deferred at every commit through `to`; changes
to rationale, owner, issue, title, `until`, or ceiling do not reset it. For a
soft finding, the same file and rule must continuously
cross that revision's soft limit without an exception that silences it. A state
that is absent for even one commit and later recurs starts again at the later
commit. `first_seen_date` is that commit's committer timestamp normalized to
RFC 3339 UTC. `age_days` is the non-negative elapsed seconds from it to the `to`
commit's committer timestamp divided by 86,400 and rounded down; wall-clock time
is never read. A first-seen timestamp later than `to` is unevaluable rather than
silently clamped.

Every historical snapshot uses the committed config, registry documents, file
content, rule matching and exclusions, line policy, token command, and unit
semantics at that revision. Today's checkout is not projected backward. A
config or policy change can therefore begin or end a finding, and an exception
address using a different unit or rules is a different entry even if its path
did not move.

Rename continuity is evidence-based. A Git-confirmed rename of one exact file
maps its exact-path exception and finding identities to the destination. An
unambiguous directory or test-home rename maps descendants only when every
tracked source descendant has one Git-confirmed destination under a single new
prefix, no source or destination has a competing mapping, and the config and
registry edits at that commit make the corresponding prefix substitution.
Those mappings preserve identity and age and produce `renamed` records instead
of added-plus-retired records. Globs preserve continuity only through that
unambiguous prefix substitution; similarity of their expanded file sets is not
rename evidence. A copy, a delete-plus-add without Git rename evidence, or any
ambiguous mapping is never guessed.

History is all-or-nothing. A malformed range, an endpoint that does not resolve
to one commit, non-ancestry, invocation outside a Git work tree, a shallow or
truncated walk that cannot prove the true first appearance, ambiguous rename
evidence, a historical config/registry/schema error, an unavailable external
token evaluation, a negative timestamp interval, or any other unevaluable
snapshot exits `2`. The stderr diagnostic starts `fissile audit: history
<requested-range>:` and names the offending revision when one is known plus a
stable cause. No history text or JSON object is emitted, and no partial history
result is substituted. Usage errors still include audit usage; run-time history
failures do not. Revision expressions are passed to Git as data, never parsed as
options.

`audit` exits non-zero for hard overflows and schema errors. Soft-only findings
exit `0`. Stale exceptions follow `[exceptions].stale`: `warn`, `error`, or
`ignore`.

## 3. Default Large-File Guard

The built-in config includes a simple byte-size guard over all non-excluded
files. It is intentionally boring: it catches accidental blobs and platform-host
problems before they reach review. Projects should tune or replace it with
named, project-specific rules once they know their layout.

This guard does not replace line or token budgets. A file may be checked by one
effective byte rule and one effective line rule at the same time (§FS-001-config.3.2).

## 4. Named Budget Entries

Findings always name the matched rule. The intended config style is a list of
named budget entries, similar to bundle-size tools but applied to source layout:

```toml
[[rules]]
id = "api-docs"
include = ["docs/api/**/*.md"]
unit = "lines"
soft = 500
hard = 900
message = "split-api-doc"
```

Names must be stable because exception entries, JSON consumers, and agent
guidance all key off them.

## 5. Errors

Failures split by scope. A **run-level** failure — an unreadable or invalid
config, an invalid exception registry, a failed `git diff --cached`, an
ambiguous rule overlap — aborts before findings and exits `2` with a single
`fissile <command>:` diagnostic on stderr. When the failing document is a file,
the diagnostic names it (`.agent-grounds/fissile.toml: config parse error: … at
line 100`), and a failed git invocation appends git's own first stderr line so
"not a git repository" is visible verbatim.

A **file-level** failure — one path that cannot be read or measured (missing,
unreadable, a directory) — does not abort the run: one odd path must not hide
every other finding. The path is skipped, every other file is still measured,
findings print normally on stdout, and each skipped path adds one stderr line
that names it:

```text
fissile check: cannot measure src/gone.rs: No such file or directory (os error 2)
```

A run with file-level failures exits `2` even when no finding stands — silently
passing an unmeasurable file would make the gate unsound — and the text success
marker is withheld. JSON output never carries error records: stdout keeps the
stable findings shape (§GOAL-003-friendly-output.1) and stderr owns diagnostics.

Non-UTF-8 content is not an error: line budgets measure physical lines from raw
bytes (§FS-001-config.3.1).
