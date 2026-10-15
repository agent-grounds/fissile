# FS-009-exception-remove: fissile exception remove deletes an entry that accepts nothing

An exception is a record of debt, and debt gets paid. The file is split, the
rule's limit moves up, the path disappears — and what is left is an entry no
finding needs. `fissile` has named the remedy for that state since `retune`
shipped: `check` says "remove it" about an entry whose file is gone
(§FS-004-check-audit.1.3), `audit --stale-exceptions` calls a ceiling under its
rule's limit one that "silences nothing" (§FS-003-exceptions.7), and `retune`
refuses to follow a shrunk file below that limit and says to remove the entry
instead (§FS-008-exception-retune.2). None of those named a command, so the only
way to act on them was hand-edited registry TOML — which skips every check the
other exception commands apply, and which the tool's own validation can refuse
to read back.

`fissile exception remove` deletes one entry and writes nothing else. An entry
whose claim has changed is a `retune` or a fresh `add`, never a removal.

## 1. Command

```text
fissile exception remove <path> --severity soft|hard --rule <id>
                         [--config <path>] [--match exact|glob] [--dry-run]
```

`<path>`, `--severity`, `--rule` and `--match` address the entry with exactly the
fields that describe one to `exception add` and `exception retune`: an entry is
identified by the registry it lives in and the `(path matcher, rules, unit)`
condition it accepts (§DF-005-exception-identity). `--rule` may be repeated, and
the address matches an entry that covers any of the named rules at the selected
unit. When no entry answers to that address the command fails without writing,
and the error says there is nothing to remove and then lists the entries the
addressed registry does hold — path, matcher, rules and ceiling, one per line,
cut off with a count of the rest when the registry is long. It lists them itself
rather than sending the caller to `audit --stale-exceptions`: `remove` is the
one command that reads a registry the rule check rejects (§FS-009-exception-remove.2), so in the state
this command exists for it is the only one that can answer the question it just
raised (§DF-007-instructions-at-the-error-site).

The matcher is part of the address, so an address that merely *overlaps* an entry
is refused in both directions, each for its own reason. An exact path covered by
a glob entry cannot be removed as itself: the entry is a ceiling for a class, and
deleting it from under one member drops the ceiling for every other file the glob
names, so the error names the glob to address instead. A glob spanning an exact
entry cannot be removed either: the caller asked to delete a class-wide entry and
there is none, and deleting one file's entry under that spelling reports the
change against a path no entry carries. An address matching two entries is
refused as ambiguous and names both — two exact entries under one glob is a
registry §FS-003-exceptions.4 accepts, so the fault is the address, not the file.

`remove` takes no `--max` and no `--unit`. It states no ceiling, and the unit of
the address comes from the selected rules, as it does for `retune`.

## 2. Repairing a registry that blocks every command

Registry validation runs at load: every command reads both registries, holds each
entry against the configured rules, and aborts before measuring anything when one
fails (§FS-003-exceptions.4). An entry whose `max_accepted.value` is below its
rule's limit for its severity is such a failure — and a limit raised in the config
turns entries that were valid yesterday into exactly that. The abort is total:
`check`, `audit`, `measure` and the pre-commit hook all stop, and the entry that
has to go is reachable only by hand.

`remove` is the one command that loads both registries without holding them to
that check, because the entry it is about to delete is what fails it. Repairing
that state is what the command is for, and a repair tool that refuses to start in
the state it repairs is no tool at all.

What `remove` still requires is a document it can read, and that boundary is the
one the load already draws. Reading a registry is two stages: every entry is
built from the document first — it parses, it declares a supported version, and
each entry's own fields are present and consistent (§FS-003-exceptions.2) — and
only then is what was built held against the configured rules. `remove` loosens
the second stage and nothing of the first, with one repair-only exception: an
otherwise well-formed soft entry whose `shadows = "hard"` target is absent must
remain addressable by `exception remove --severity soft`. That command may carry
the unresolved entry only far enough to apply the usual address, removal, and
byte-preserving write (§FS-009-exception-remove.4); it deletes the entry, exits zero, and reports the
normal removal. This remains a repair when the path is above the soft limit:
the orphan cannot silence a finding because ordinary loading aborts before
evaluation, so §FS-009-exception-remove.3 does not preserve it.

The exception does not resolve the orphan or make it valid. `check`, `audit`,
the hook, every other command, and a removal from any registry other than the
soft registry still reject the same missing twin under the strict shadow rule
(§FS-003-exceptions.2.3). More than one possible twin remains an ambiguity, and
every failure in the orphan's own document or fields remains a build failure:
a blank `reason`, a blank `until`, an empty rule list, a ceiling of zero, a
`kind` that contradicts its `until` (§FS-003-exceptions.2.1), or a field that is
not there at all. Those failures stop `remove` exactly as they stop every other
command.

Any of those remaining failures at that stage is not confined to the entry that
carries it. Building stops at the first entry that fails, so the document goes
unread and every other entry in it is out of reach until the offending one is
edited by hand. An entry that will not build is therefore the thing to repair
first, and no command — this one included — will do it.

Two of the rule-check failures stay out of reach for a different reason: the
address takes its rule ids and its unit from the command line, and the unit from
a *configured* rule (§FS-009-exception-remove.1). So an entry whose only rule ids are ones the config no
longer configures, and an entry whose `max_accepted.unit` no configured rule it
names shares, answer to no address this command can be given — a retired id is
refused before the registry is read, and a live id addresses a different
condition. Those two are edited by hand, as every entry was before this command.
Widening the address to reach them would mean an entry addressed by something
other than the condition it accepts, which is the identity `add` and `retune`
are built on (§DF-005-exception-identity). Unlike an entry that will not build,
though, each costs only itself: the registry loads, and every other entry in it
answers to its address as usual.

No other command loosens. `check` and the hook keep failing on an invalid
registry, because a gate that measured against entries it could not validate
would be reporting under rules nobody wrote.

Removal cannot make the rule check worse: entries are validated one at a time, so
deleting one leaves the verdict on every other exactly as it stood. A registry
holding several failing entries is therefore repaired one command at a time,
which is why the guarantee on the write is stated as what it actually is — the
document written back holds precisely the entries that were read, less the one
addressed (§FS-009-exception-remove.5) — rather than as "the result validates".

## 3. An entry that still silences a finding is not removed

An exception exists to silence a finding. Deleting one that is doing that job
repairs nothing; it reports a file the repository decided to accept, and a caller
acting on a stale-entry report is not asking for that.

So `remove` measures before it writes. It takes the files in the scan scope the
entry's own matcher covers, evaluates each of them with the entry and again
without it, and refuses when a finding appears that does not stand today. The
refusal names the file, its measurement, and the rule and limit that would report
it, and says what to do instead (§DF-007-instructions-at-the-error-site): split
the file, or leave the entry where it is.

Three states pass, and they are the ones the removal remedy is named for:

- the file measures at or under the rule's limit for this severity, so nothing
  crosses it — the state `retune` will not follow a file down into
  (§FS-008-exception-retune.2) and `audit` calls "silences nothing"
  (§FS-003-exceptions.7);
- the file has grown past the entry's own ceiling, so the finding already stands
  and the entry is only the reason it is not larger;
- the entry's matcher covers no file in the scan scope — the stale entry `check`
  and `audit` report (§FS-004-check-audit.1.3), including the entry whose file
  was deleted.

Evaluating rather than comparing two numbers is what makes the harder cases
right. A glob entry is removable only when no member of its class is silenced by
it, so one member still under its ceiling refuses the whole class. A hard entry
is judged by the hard finding it holds back, and the two severities interact
(§FS-003-exceptions.3) — which the arithmetic on one rule's limit would miss and
the findings the run would actually report do not. And a file that cannot be
measured reports nothing under any registry, so it blocks nothing
(§FS-004-check-audit.5).

## 4. Result

The command deletes the addressed `[[exceptions]]` block together with the blank
line that separated it from what follows. Every other byte — the version line,
the comments that belong to another entry or to no entry, entry order, the
fields of the other entries, and the line endings the file is stored with — is
preserved, so the diff is one entry and nothing else. The tail is the one
exception, and only when the removed entry was the last block: the document is
then left ending with exactly one line terminator, in the endings it is stored
with, so blank lines trailing the last surviving line collapse into that one and
a registry stored without a final terminator gains one. Removing any other block
leaves the tail as it was.

A comment belongs to the entry it is written directly above. The block is
therefore the `[[exceptions]]` header, the lines under it, and the run of
comment lines immediately above the header with no blank line between: the note
that records why *this* entry is there goes with it, and the note above the
*next* entry stays with that entry rather than being cut away with this one. A
comment a blank line separates from any header belongs to no entry — a note
between two blocks, or one trailing the last block — and is left exactly where
it is, including when the block it trails is the one being removed. Reattaching
a detached comment would be a guess about what it is for, and a registry's notes
are as often about the file, or about the registry as a whole, as about one
entry.

Which block that is comes from reading the registry as TOML, not from matching
text. A `[[exceptions]]` header written inside a `reason` — in either multi-line
string form, or after a `#` — names no entry and shifts no index, exactly as for
`retune` (§FS-008-exception-retune.3).

```text
docs/file-size-agent-exceptions.toml: removed docs/functional-spec/FS-001-config.md (accepted up to 300 lines)
```

When the other registry also holds an entry for that path and rule, the result
names it and its ceiling. `remove` never writes to a registry the caller did not
select — twin consistency is a repository's policy, not the tool's — but a caller
who has just deleted one half of a twin should learn here that the other half is
still accepting the file, rather than from a later run
(§FS-008-exception-retune.3).

`--dry-run` prints that same line, says which registry it would update, and
writes nothing.

## 5. Validation

`remove` fails without writing when no entry answers the address, when the
address matches more than one, when the address only overlaps an entry in either
direction (§FS-009-exception-remove.1), and when the entry still silences a finding (§FS-009-exception-remove.3).

Before the write, the document about to be written is parsed and compared against
the entries the command read: it must hold precisely those, less the one
addressed. A rewrite that cut the wrong lines is therefore refused rather than
written, and the check holds on a registry that does not validate — which
re-validating the combined document, as `add` and `retune` do
(§FS-005-exception-add.4), could not, since the registry `remove` exists to
repair is one validation already rejects.
