//! The `fissile` help screens (§FS-006-cli.2). One `--help` body per command,
//! each under the one-screen bound §GOAL-003-friendly-output.1 sets and pinned
//! by `tests/integration/help.rs`. They live beside the dispatch rather than in
//! it: they are a surface a reader arrives at, edited far more often than the
//! parsing under them.

pub const USAGE: &str = "\
usage: fissile <command> [options]

fissile keeps files small so readers — human and agent — spend less to
understand them, without letting a budget damage the design. Every rule has two
limits: soft warns, hard fails the commit. Run `fissile check --staged` before
calling work done, and split what it names along a seam the code already has.
Its findings carry the rest: what to split, how, and how to record a file that
cannot be split without making the design worse.

commands:
  init [<path>]        install config, registries, and agent instructions
  check [<paths>...]   enforce file budgets on a file set or the scan scope
  measure <paths>...   report what fissile counts, and the headroom left
  audit                inventory the whole repo against its budgets
  limits               print every rule this tree configures
  exception add        record a justified oversized-file exception
  exception retune     move the ceiling an exception already records
  exception remove     delete an entry that accepts nothing

run `fissile <command> --help` for command options
`--version`/`-V` prints the version";

pub const INIT_USAGE: &str = "\
usage: fissile init [<path>] [--name <name>] [--config <path>] [--exceptions]
                    [--hook] [--no-hook] [--force] [--dry-run] [--agents-md]
                    [--claude] [--gemini] [--copilot] [--cursor] [--windsurf] [--zed]

--config defaults to .agent-grounds/fissile.toml; a repository whose config is
still at the deprecated .agents/fissile.toml keeps it there and gets no second
one.

--dry-run reports the planned writes and prints the managed agent block, which
is the one way to read what agents are told without writing a file.

examples:
  fissile init --exceptions
  fissile init . --agents-md --claude";

pub const CHECK_USAGE: &str = "\
usage: fissile check [<paths>...] [--staged] [--config <path>]
                     [--format text|json] [--no-color]

examples:
  fissile check --staged
  fissile check src/lib.rs --format json";

pub const MEASURE_USAGE: &str = "\
usage: fissile measure <paths>... [--staged] [--config <path>]
                       [--format text|json] [--no-color]

Reports each file's measured size, the limits that apply, any accepted ceiling,
and the distance to whichever of those binds first. Unlike `check` it answers
for files that are passing, and it never fails a build.

examples:
  fissile measure src/lib.rs
  fissile measure --staged --format json";

pub const AUDIT_USAGE: &str = "\
usage: fissile audit [--config <path>] [--format text|json] [--top <N>]
                     [--stale-exceptions] [--rule-coverage] [--no-color]
                     [--only <section>[,<section>]]

--only prints the sections it names and nothing else: findings, silenced,
exceptions, top, stale, loose, coverage — always in that order, whatever order
you name them. Naming a section is what asks for it, so --only coverage needs
no --rule-coverage; --only top still needs --top <N>, which carries the count.
It selects what is printed and nothing else: the exit code is the run's.

--format json is the agent surface. It returns the same sections as separate
keys, so a script reaches one with jq and needs no selector.

examples:
  fissile audit --top 10
  fissile audit --stale-exceptions --rule-coverage
  fissile audit --only coverage
  fissile audit --format json --rule-coverage | jq .coverage";

pub const LIMITS_USAGE: &str = "\
usage: fissile limits [--config <path>] [--format text|json] [--no-color]

Prints every configured rule — id, include patterns, unit, soft and hard limits,
and the effective soft edit limit — in declaration order, whether or not any
file matches. It measures nothing and never fails a build, so it answers even
where a broken exception registry stops `check` and `audit`. Use the JSON form
to generate or verify a documented limit instead of copying it by hand.

examples:
  fissile limits
  fissile limits --format json";

pub const EXCEPTION_USAGE: &str = "\
usage: fissile exception <add|retune|remove> <path> [options]

  add     record a justified oversized-file exception
  retune  move the ceiling an entry already records, up or down
  remove  delete an entry that accepts nothing

--kind says what an added entry's --reason has to establish. Describing the
file does not:
  structural  splitting is illegal — name the constraint. Never expires.
  deferred    a boundary is missing — name it and what must exist first, and
              give --until the condition that retires the entry.
A soft entry whose rationale is already in the hard registry takes
--shadows-hard instead of all three.

examples:
  fissile exception add src/big.rs --severity hard --rule source --kind deferred --reason \"...\" --until \"the parser module lands\"
  fissile exception retune src/big.rs --severity hard --rule source
  fissile exception remove src/big.rs --severity soft --rule source

run `fissile exception <add|retune|remove> --help` for the full options";

pub const EXCEPTION_ADD_USAGE: &str = "\
usage: fissile exception add <path> --severity soft|hard --rule <id>
                 --kind structural|deferred --reason <text> [--until <text>]
                 [--shadows-hard]
                 [--config <path>] [--match exact|glob] [--title <text>]
                 [--owner <text>] [--issue <text>] [--force] [--dry-run]
                 [--max <N> --unit bytes|lines|tokens]

--kind says what --reason has to establish. Describing the file does not:
  structural  splitting is illegal — name the constraint. Never expires.
  deferred    a boundary is missing — name it and what must exist first, and
              give --until the condition that retires the entry.
--shadows-hard is the soft twin of a hard entry: it takes that entry's kind,
reason, and until instead of restating them, so it needs none of the three.
Soft severity only, and the hard entry has to exist already.
--max states the ceiling and is written as given; without it the file's
measurement is rounded up to the configured [exceptions.bump] step.

examples:
  fissile exception add src/big.rs --severity hard --rule source --kind deferred --reason \"...\" --until \"the parser module lands\"
  fissile exception add src/big.rs --severity soft --rule source --shadows-hard
  fissile exception add \"tests/fixtures/**\" --match glob --severity soft --rule fixtures --max 300000 --unit bytes --kind structural --reason \"...\"";

pub const EXCEPTION_RETUNE_USAGE: &str = "\
usage: fissile exception retune <path> --severity soft|hard --rule <id>
                 [--max <N> --unit bytes|lines|tokens]
                 [--config <path>] [--match exact|glob] [--dry-run]

Moves an existing entry's ceiling, up or down, leaving its reason, kind, and
until untouched. Without --max the new ceiling is the file's measurement rounded
up to the configured [exceptions.bump] step, so it reads as a decision rather
than as whatever the file happened to measure today. With --max the ceiling is
exactly the number given.

examples:
  fissile exception retune src/big.rs --severity soft --rule source
  fissile exception retune src/big.rs --severity hard --rule source --max 900 --unit lines";

pub const EXCEPTION_REMOVE_USAGE: &str = "\
usage: fissile exception remove <path> --severity soft|hard --rule <id>
                 [--config <path>] [--match exact|glob] [--dry-run]

Deletes one entry — the whole block, and nothing else in the registry. It is
also the way out of a registry whose own entries abort every other command: a
rule limit raised past an entry's ceiling leaves a document `check` and `audit`
refuse to read, and `remove` still runs there.

An entry that is still silencing a finding is not deleted; the refusal names the
file and the limit that would report it.

examples:
  fissile exception remove src/big.rs --severity soft --rule source
  fissile exception remove \"tests/fixtures/**\" --match glob --severity soft --rule fixtures";
