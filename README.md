# fissile

[![crates.io](https://img.shields.io/crates/v/fissile.svg)](https://crates.io/crates/fissile)
[![license: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**A pre-commit guard that keeps source files small enough to stay cheap for
agents to read — without ever rewriting your code.**

Large files are an invisible tax on every AI-assisted workflow. A 4,000-line
module gets dragged into context whenever an agent needs one function inside it,
and you pay for that in tokens, latency, and attention — on every run, forever.
Reviewers see diffs, not totals, so a file that crossed a sensible size years
ago keeps gaining a line at a time and nothing pushes back at the moment the
bloat is introduced (§GND-001-fissile).

`fissile` is that feedback loop. At commit time it measures staged files against
per-repo, per-file-type budgets and flags the ones that have outgrown them. It
**only measures and reports** — it never edits your code, so *how* to split a
flagged file is always your call. Each overflow can carry a short, project-owned
message that names the local split: the destination module, the owner, the
extraction pattern.

It knows the difference between file types, because that is the whole point: a
2,000-line generated SQL dump is fine, a 2,000-line hand-written module is not,
and a checked-in PNG is usually a mistake.

## Why not an existing tool?

| Tool | What it protects |
| --- | --- |
| large-file hooks | the repo from accidental blobs |
| linter line rules | one language's local style |
| bundle-size checks | shipped artifacts |
| PR-size gates | reviewer throughput |
| **fissile** | **the source layout itself — its cost to read** |

The difference is the message. A generic "file too large" stops a bad commit;
`fissile` also names the architectural move that makes the next commit better.

## Install

```sh
cargo install fissile
```

One small static binary, no runtime, no Python/Node toolchain. Prebuilt
per-platform binaries (Linux x86_64/aarch64, macOS Intel/Apple silicon,
Windows x86_64/aarch64) ship with every
[GitHub release](https://github.com/agent-grounds/fissile/releases), each archive
with a `.sha256` beside it. To run the unreleased tip:

```sh
cargo install --git https://github.com/agent-grounds/fissile
```

## Quickstart

```sh
fissile init            # writes .agent-grounds/fissile.toml, AGENTS.md, and the git hook
```

```text
wrote ./.agent-grounds/fissile.toml
wrote ./AGENTS.md
wrote ./.git/hooks/pre-commit
next:
1. Review .agent-grounds/fissile.toml: the source rule budgets common code extensions; add this repo's languages or tune the limits.
2. Commit a change to see the pre-commit hook run fissile check --staged.
3. Run fissile audit once and add justified exceptions with fissile exception add.
see AGENTS.md for what agents are told; the findings carry the rest.
```

That is it. The installed hook runs `fissile check --staged` on every commit.
The starter config ships sensible defaults — a byte budget on everything, a
line budget on common source extensions (`.rs`, `.go`, `.py`, `.ts`, `.js`, …)
wherever they live, and a markdown budget — all editable in place.

## The two tiers

Every rule carries two limits, because one threshold is a false economy:

- **soft** — *warns, exit 0.* The signal for the agent that just grew the file:
  shrink it the way the message says, before claiming the task done.
- **hard** — *blocks, exit 1.* No override flag, no `# fissile: allow` comment.
  The only way past is a justified exception (below).

They also say different things. A rule carries a message per severity, so a
warning can ask for a split *next time you are in the file* while a block says
what has to happen *before more code lands* — and each names its own way out
when no honest split exists (§DF-003-severity-guidance).

## Example output

Findings are grouped: the guidance is printed once, and the files it applies to
are listed under it, largest first.

```text
$ fissile check
hard: 1 file over the 550-line budget [rule: source, message: split-source-hard]
  Must split before more code lands here: move cohesive groups of items into
  sibling modules along seams that already exist. Do not invent an abstraction,
  flatten a boundary, or shuffle lines to get under the limit — a worse design
  under budget is not the goal.
  If you cannot see a split that leaves the architecture intact, stop and ask a
  human. The only other way past this gate is a human-reviewed exception, whose
  reason names the constraint that makes splitting illegal (--kind structural)
  or the boundary that is missing and what must exist first (--kind deferred):
  fissile exception add <path> --severity hard --rule source --kind <kind>
    src/orders.rs: 620 non-blank lines (budget 550; an exception here would accept 700)

soft: 2 files over the 350-line budget [rule: source, message: split-source-soft]
  Should split, next time you are in one of these files: move a cohesive group
  of items into a sibling module. Split along a responsibility seam, never at
  the line count, and never break apart code that belongs together.
  If no split leaves the design better than it is now, record why instead of
  forcing one — not what the file contains, which is what this finding already
  said. Name the constraint that makes splitting illegal (--kind structural), or
  the boundary that is missing and what must exist first (--kind deferred,
  --until naming what retires it):
  fissile exception add <path> --severity soft --rule source --kind <kind>
    src/util.rs: 410 non-blank lines (budget 350; an exception here would accept 500)
    src/billing.rs: 372 non-blank lines (budget 350; an exception here would accept 400)

hint: fissile measure <path>... reports size and headroom for the files you split into.
# exit 1
```

The header states the severity, the crossed limit, and the rule and message that
own the budget; every file line leads with the path and, for line rules, names
the counting basis and budget so editors and agents know what to change. It also
names the ceiling an `exception add` with no `--max` would record for that file,
so a reader choosing to record the overflow rather than split reaches for a
number with headroom instead of copying the measurement. Twelve files under one
rule cost one copy of the guidance, not twelve. A passing run prints exactly
`ok`.

The shipped messages are deliberately generic and carry no citation — they know
nothing about your layout, and an ID from *fissile's* docs would resolve nowhere
in your repo. Rewriting them to name real destinations, and to cite your own
architecture, is the first edit worth making.

`--format json` is the agent surface — one flat record per finding, ungrouped:

```json
[{"path":"src/orders.rs","unit":"lines","actual":620,"limit":550,"severity":"hard","rule_id":"source","message_id":"split-source-hard","message":"Must split before more code lands here: ...","exception_would_accept":700}]
```

The schema is published under `schema/` and validated against emitted output.

## Audit an existing repo

Adopting against a large codebase? `audit` inventories the whole repo without
blocking anyone, so you can see the surface before turning the hook on:

```text
$ fissile audit --top 5
hard: 1 file over the 550-line budget [rule: source, message: split-source-hard]
  Must split before more code lands here: ...
    src/orders.rs: 620 non-blank lines (budget 550; an exception here would accept 700)

soft: 2 files over the 350-line budget [rule: source, message: split-source-soft]
  Should split, next time you are in one of these files: ...
    src/util.rs: 410 non-blank lines (budget 350; an exception here would accept 500)
    src/billing.rs: 372 non-blank lines (budget 350; an exception here would accept 400)

exceptions:
  structural (never expires): 3
  deferred (carrying debt): 32

top lines:
  620 src/orders.rs
  410 src/util.rs
```

The two exception counts are deliberately not one total: three files nobody will
ever split and thirty-two waiting on work are different facts about a codebase.

`check` already names an exact-path entry whose file the commit removes, so a
leftover entry surfaces in the diff that killed it. Add `--stale-exceptions` for
the rest of the inventory — globs matching nothing, ceilings that have drifted
far above the file they still accept, which is the ratchet slipping back, and
ceilings the file has grown up to exactly, which pass today and fail on the next
unrelated commit:

```text
loose ceilings:
  docs/file-size-agent-exceptions.toml: src/orders.rs accepts 700 lines, now 421 — retune to 500
  docs/file-size-agent-exceptions.toml: src/router.rs accepts 519 lines, now 519 — no headroom; retune to 600
```

Add `--rule-coverage` to find rules and messages that match nothing.

Both of those are read in a loop — edit the config, run again, look again — so
`--only` cuts the report down to the sections you are reading:

```text
$ fissile audit --only coverage
rule coverage:
  rules matching no file: legacy
  files only under catch-all: scripts/release.sh
  unused messages: none
```

The sections are the seven keys the JSON output publishes — `findings`,
`silenced`, `exceptions`, `top`, `stale`, `loose`, `coverage` — and they print
in that order however you name them. Naming one is what asks for it, so `--only
coverage` needs no `--rule-coverage`; `--only top` still needs `--top <N>`,
which carries the count. Selection reaches the screen and nothing else: a
repository with a hard overflow still exits non-zero with its findings off
screen (§FS-004-check-audit.2).

A script needs neither flag: `fissile audit --format json --rule-coverage | jq
.coverage` returns the same section, and JSON is the surface agents should
reach for (§GOAL-004-token-thrift).

## How big is this file?

`check` reports a measurement only when a file is over budget, and the count is
fissile's own — comments count, blank lines do not — so `wc -l` cannot answer
"is there room for this here?". `measure` answers it for any file, passing or
not, and never fails a build:

```text
$ fissile measure src/orders.rs src/util.rs
src/orders.rs 620 lines [source] soft 350 hard 550 hard-accepted 700 — 80 to hard-accepted
src/util.rs 410 lines [source] soft 350 hard 550 — 140 to hard
```

The clause after the dash is the room left before whichever threshold binds
first — a limit, or the ceiling an exception records. It is room you can
actually use: `0 to hard` means the file is at the limit and the next line is
the first one that fails the commit, because a limit fires above the limit. That
is the number you need *before* deciding whether the
new test goes in this file or a new one.

## What does this repo enforce?

`check` and `audit` speak in findings, so a passing tree tells you nothing about
its budgets. `limits` prints the rules themselves — every one the config
declares, in the order it declares them, with no file in hand:

```text
$ fissile limits
source [src/**/*.rs] lines soft 350 hard 550
config-toml [**/*.toml] lines soft 180 hard 300
```

`--format json` carries the rest of each rule — its priority, its message ids,
and how it counts a line — so a documented limit can be generated from the
config or compared against it in CI instead of copied into prose that nothing
checks:

```json
{"rules":[{"id":"source","include":["src/**/*.rs"],"unit":"lines","soft":350,"hard":550,"priority":0,"soft_message":"split-source-soft","hard_message":"split-source-hard","count_blank_lines":false,"count_comment_lines":true}]}
```

It reads the config and not the exception registries, so it still answers in a
tree whose registry `check` and `audit` refuse to load (§FS-010-limits).

## Justified exceptions

A file you have decided to keep large gets a written reason in a registry — not
a silent ignore comment. `exception add` appends the entry for you:

```sh
fissile exception add src/orders.rs --severity hard --rule source \
  --kind deferred --until "the pricing module exists" \
  --reason "Missing boundary: pricing has no module of its own, so the rate
table, the discount rules, and the invoice writer are all reachable only from
here. Splitting today just moves private helpers behind a new file."
```

A hard exception is the only way past a stop-the-line gate, so it is a person's
to record: off a terminal the command refuses and names the soft-severity route
instead, which leaves the finding standing. A script that adds one legitimately
passes `--force` (§DF-008-hard-severity-needs-a-terminal).

```toml
[[exceptions]]
path = "src/orders.rs"
match = "exact"
rules = ["source"]
kind = "deferred"
max_accepted = { value = 620, unit = "lines" }
until = "the pricing module exists"
reason = """
Missing boundary: pricing has no module of its own, so the rate table, the
discount rules, and the invoice writer are all reachable only from here.
Splitting today just moves private helpers behind a new file.
"""
```

The hard block is now silenced — but only up to `max_accepted`, which is the
measurement rounded up to the configured `[exceptions.bump]` step (100 lines by
default). A ceiling reads as a decision — *this file may run to 700 lines* —
rather than as whatever the file happened to measure the day someone wrote the
entry, and an ordinary edit no longer trips it. Grow the file past it and the
finding returns. The soft warning still nudges the agent, because
this entry is `deferred`: there is a split to keep asking for.

Silencing that warning takes a second entry, in the soft registry — and it holds
no argument of its own, since what to accept, why, and what retires it were all
decided above. `--shadows-hard` says where the argument lives instead of copying
it:

```sh
fissile exception add src/orders.rs --severity soft --rule source --shadows-hard
```

The twin it writes carries `shadows = "hard"`, the kind copied from the entry it
points at, and a `max_accepted` of its own — the one number the pair is allowed
to disagree about, so *hard debt to 620, warn again above 400* still works.
Delete the hard entry and the twin stops loading with it, which is what keeps
the two from drifting apart (§FS-003-exceptions.2.3).

`--kind` is the field that keeps the registry honest, because a reason answers
one of two questions and they are not the same question
(§DF-004-exception-kind):

| | the claim | `until` | retires when |
| --- | --- | --- | --- |
| `structural` | splitting is **illegal** — name the constraint | `indefinite` | never |
| `deferred` | a boundary is **missing** — name it and what must exist first | the condition | someone builds it |

Without the split, both collapse into "this file is large because …", which
describes the file and claims nothing a reviewer can disagree with. `audit`
counts the two separately, so *accepted permanently* and *carrying debt* never
show up as one number.

### Moving a ceiling

When the file outgrows its ceiling, the reason usually still holds and only the
number is wrong. `exception retune` moves it — in either direction, at either
severity, leaving the reason, kind, and `until` untouched:

```text
$ fissile exception retune src/orders.rs --severity hard --rule source
docs/file-size-human-exceptions.toml: src/orders.rs 700 -> 800 lines
```

It picks the number the same way `add` does, so a ceiling never becomes a fossil
of one commit, and the diff is the single line that changed. Lowering works the
same way and is how you retire a loose ceiling `audit` reports. State the number
yourself with `--max <N> --unit lines` and it is written as stated; the step
rounds only what the tool measured (§DF-010-stated-ceilings-are-exact).

### Removing an entry

When the file is split, the path is gone, or the rule's limit moves up past the
ceiling, there is no number left to move and the entry itself should go:

```text
$ fissile exception remove src/orders.rs --severity hard --rule source
docs/file-size-human-exceptions.toml: removed src/orders.rs (accepted up to 800 lines)
```

It addresses the entry exactly as `retune` does, and it will not delete one that
is still silencing a finding — the refusal names the file and the limit that
would report it. It is also the way out of a registry a raised limit has made
invalid, which every other command aborts on before it measures anything
(§FS-009-exception-remove).

The kind also decides what a hard entry silences. A `structural` one silences the
soft warning for the overflow it accepts as well: splitting is illegal, so the
warning names work nobody may do and no amount of work can clear it. One entry
makes a file over the hard limit quiet — no second entry in the soft registry
repeating the same rationale (§FS-003-exceptions.3).

## Use as a library

```rust
use fissile::{Budget, Checker, MessageTemplate, Rule, Selector, Unit, measure_text};

let checker = Checker::new(vec![Rule::new(
    "rust-modules",
    Selector::Extension("rs".into()),
    Budget::new(Unit::Lines, Some(200), Some(400)),
    MessageTemplate::new(
        "split-rust-module",
        "Move cohesive helpers from {path} into the nearest owned module.",
    ),
)])?;

let file = measure_text("src/lib.rs", "fn main() {}\n");
assert!(checker.check(&file)?.is_empty());
# Ok::<(), fissile::FissileError>(())
```

## Configuration

A single versioned TOML file at `.agent-grounds/fissile.toml` — data, not a plugin
surface. Budgets are set per extension, per glob, and per unit (`bytes`,
`lines`, or `tokens`); each rule names a message template. Full schema in
[`docs/functional-spec/FS-001-config.md`](docs/functional-spec/FS-001-config.md),
with a worked sample at [`examples/fissile.toml`](examples/fissile.toml).

A rule can subtract paths from only its own scope while every other applicable
unit and rule remains active (§FS-001-config.3.4):

```toml
[[rules]]
id = "citable-spec"
include = ["docs/**/*.md"]
exclude = ["docs/changelog/*.md"]
unit = "lines"
soft = 750
hard = 2000
message = "split-doc"
```

Use `[[rules]].exclude` when a path should leave one budget, `[scan].exclude`
when fissile should not scan or budget the path at all, and an exception when
the rule still applies but an overflow is accepted with a written reason and
ceiling. Omitting a rule exclusion, or writing `exclude = []`, is unchanged
from earlier version-1 configs.

## How it fits together

- **`init`** — config, exception registries, the managed `AGENTS.md` block, and
  the git hook (§FS-002-init).
- **`check`** — the commit-time gate over staged files or explicit paths
  (§FS-004-check-audit).
- **`measure`** — what fissile counts for a file, and the headroom left
  (§FS-007-measure).
- **`audit`** — the whole-repo inventory and migration surface
  (§FS-004-check-audit).
- **`exception add`** — append a justified oversized-file exception
  (§FS-005-exception-add).
- **`exception retune`** — move the ceiling an entry already records
  (§FS-008-exception-retune).
- **`exception remove`** — delete an entry that accepts nothing
  (§FS-009-exception-remove).

This repo is grounded with [`grund`](https://github.com/agent-grounds/grund): the
`§ID` markers above point at the specs and goals that justify each behavior.
