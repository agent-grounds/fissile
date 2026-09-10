# FS-010-limits: fissile limits prints the configured rule inventory

A repository's budgets are configuration, and every command that reads them
declines to say what they are. `check` and `audit` speak only in findings, so a
tree that is passing tells a reader nothing; `measure` answers for paths the
caller already named, so it can report but never enumerate; and
`audit --rule-coverage` reports which rules matched files, not what any rule
declares (§FS-004-check-audit.2). The values live in one document and nowhere a
program can ask for them.

So they get copied. A README states a ceiling, an agent instruction file states
it again, and the copies become a second source of truth that nothing checks —
until a limit moves and the prose keeps the old number. `limits` is the one
surface that answers *what does this tree enforce*, completely and in a shape a
script can compare against, so a documented limit can be generated or verified
rather than maintained by hand.

## 1. Command

```text
fissile limits [--config <path>] [--format text|json] [--no-color]
```

There are no positional arguments and no `--staged`. The answer does not depend
on a file set: a rule that matches nothing in this tree today is still a
configured rule, and narrowing the inventory to what some path selected would
make it the one thing it must not be, partial. A positional argument is a usage
error in the shape §FS-006-cli.1 gives every command — `fissile limits:
unexpected argument \`<arg>\`` and the usage screen on stderr, exit `2` — and so
are an unknown option and a `--format` value that is neither `text` nor `json`.

`--config <path>` names the document to read instead of discovering one, on the
same terms as `audit` (§FS-004-check-audit.2). An unset `--format` takes
`[output].format` from the config being read (§FS-001-config.6).

`limits` is inspection, never a gate. It exits `0` whenever the config loads,
whatever the config turns out to say — the contract `measure` carries
(§FS-007-measure.3). Exit `2` is a usage error or a config that will not load,
reported as one `fissile limits:` diagnostic on stderr (§FS-004-check-audit.5).
Nothing printed is tinted, because nothing here is a verdict; `--no-color` is
accepted so the flag means the same thing on every command, and changes nothing.

## 2. What It Enumerates

Every rule of the effective rule set, in the order the config declares them, so
the output reads beside the document and diffs against it. Nothing is filtered:
a rule that currently matches no file prints like any other. That is the whole
difference from `audit --rule-coverage`, which reports what the rules *did*
against this tree's files. `limits` reports what they *say*, and the rule that
matched nothing is exactly the one a reader is most likely to be wrong about.

A tree with no config document of its own is not a tree with no budgets: the
built-in defaults apply (§FS-001-config.0), so `limits` prints those. The
command answers for the rules in force, not for the file that happens to hold
them.

A config that declares no rules at all prints one line, `no rules configured`,
rather than nothing — "this tree enforces nothing" is an answer and silence is
not, the same reason `measure` names a file no rule measures
(§FS-007-measure.2). In JSON that config is an empty `rules` array.

## 3. Text Output

One line per rule, no header and no banner (§GOAL-004-token-thrift.1):

```text
rust-library [src/**/*.rs] lines soft 700 soft-edit-limit 5 hard 900
entrypoints [README.md, AGENTS.md] exclude [docs/changelog/*.md] lines soft 250 soft-edit-limit 5 hard 500
config-toml [**/*.toml] bytes hard 262144
```

The fields are the rule id, its `include` patterns in declaration order inside
brackets, a non-empty `exclude` list in declaration order when the rule has one,
the unit it measures in, and the thresholds it declares, each named. The
exclusion is spelled `exclude [<patterns>]` immediately after the include list;
an omitted or empty list prints nothing, preserving the existing line exactly.
The threshold spelling is `measure`'s — `soft <N>` then `hard <M>`, the same
words in the same order — so a limit reads the same wherever fissile prints one
(§FS-007-measure.2). A soft threshold is immediately followed by
`soft-edit-limit <N>`, reporting the effective bounded-grace setting: the
configured value, or `5` when omitted (§FS-001-config.3). A rule declaring only
hard prints neither a soft threshold nor an edit limit; a placeholder would
invent a limit the config does not set.

The include list is bracketed because a rule may name several patterns and a
bare list of them would run into the unit. Every other field is separated by a
single space, so one line stays as readable to `awk` as to a person.

## 4. JSON Output

```text
{"rules":[…]}
```

An object with one key, not a bare array. This is an inventory, inventories
grow, and a top-level array cannot gain a sibling section later without breaking
every consumer that already reads it; `audit` is an object for that reason
(§FS-004-check-audit.2).

Each element carries, in this order: `id`, `include` (an array of strings),
`exclude` (an array of strings) when non-empty, `unit`, `soft`,
`soft_edit_limit`, `hard`, `priority`, `soft_message`, `hard_message`,
`count_blank_lines`, `count_comment_lines`. It carries more than the text form
because this is the agent surface (§GOAL-004-token-thrift.1): a generator
rendering a documentation table wants the message ids and the counting policy,
and a reader skimming a terminal does not.

A field that would describe nothing is omitted, never nulled, exactly as a
`measure` record omits a threshold that does not exist (§FS-007-measure.2):

- `soft` and `hard` appear only where the rule declares them.
- `soft_edit_limit` appears on every rule with `soft`, carrying its effective
  configured value or the default `5`; it is omitted on a hard-only rule.
- `exclude` appears only when the rule declares at least one pattern. An omitted
  list and `exclude = []` therefore preserve the existing JSON shape.
- `soft_message` and `hard_message` are message ids, and each appears only where
  the rule declares the threshold it belongs to. A severity a rule declares no
  threshold for borrows the other's template internally, so that a
  half-configured rule stays valid (§FS-001-config.3); that template is never
  rendered, and emitting it here would show a caller guidance their config never
  attached to a limit that does not exist.
- `count_blank_lines` and `count_comment_lines` appear only when `unit` is
  `lines`. They are the line-counting policy (§FS-001-config.3.1) and say
  nothing about a byte or token budget.

`priority` is always present. Every rule has one, `0` when the config omits it,
and it is what settles an overlap between two rules (§FS-001-config.3.2), so a
reader comparing two lines needs it on both.

The shape is published as `schema/limits.schema.json` and validated against
emitted output, on the same terms as `measure`'s.

## 5. It Reads The Config And Nothing Else

`limits` loads the config document and builds the rule set from it. It does not
read, parse, or validate the exception registries.

Nothing it prints comes from a registry — a ceiling accepted for one file is a
recorded exception, not a configured limit — so loading them could add nothing
to the answer and could only add ways for the command to fail. Registry
validation aborts a run on a schema error or an entry naming a rule that no
longer exists (§FS-003-exceptions.4), and that is precisely the state a
repository is in when someone most needs to read its configuration: `check` and
`audit` both refuse, and the reader is trying to establish what the tree
enforces before repairing it.

So a tree whose registries `check` and `audit` will not load still answers
`fissile limits` with exit `0` and its full inventory. The one command whose job
is to describe the configuration stays readable while the tree is broken.
