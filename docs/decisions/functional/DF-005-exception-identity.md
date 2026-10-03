# DF-005-exception-identity: An entry is identified by what it accepts, not by a name for it.

Every registry entry used to carry an `id` — `EX-007-orders-rs` — and an optional
`replaces` pointing at another entry's id. Both are removed at
`fissile_exceptions_version = 2` (§FS-003-exceptions.2.2).

An entry was already identified by what it accepts. `path` alone is not a key:
the same path legitimately appears in both registries, and can appear twice in
one registry for rules with different units. But `(registry, path matcher, rules,
unit)` is, and `fissile` enforces exactly that — two entries matching one
overflow in one registry is a schema error (§FS-003-exceptions.3). So `id` was a
second name for a tuple the tool already computes, and the second name is the one
that can be wrong.

## 1. Decision

The registry schema has no `id` and no `replaces`, and `fissile exception add`
has no `--id` and no `--replaces` (§FS-005-exception-add.1). Wherever an entry
was named, it is now located: a diagnostic leads with the registry file and the
entry's `path`, because that pair is the line the reader has to edit.

```text
docs/file-size-human-exceptions.toml: src/orders.rs states no reason
```

The registry file is part of the identifier, not decoration: the same path in the
soft and the hard registry are two different entries making two different claims,
and a message naming only the path would be ambiguous between them.

The pair is an address, not a primary key — within one registry a path may still
appear twice, for rules of different units. That is enough. Every diagnostic that
depends on a rule already names the rule; the rest name a condition an eye can
check (`states no reason`) once the address has taken the reader to the entry.

Silenced attribution keeps the path and the ceiling and drops the name:

```text
src/orders.rs: hard exception (accepted up to 620 lines)
```

## 2. What the id bought, and why that was not enough

- **Output.** The audit line and the JSON `exception_id` carried the slug. The
  path was already on the same line and says more than a slug derived from it.
- **`replaces`.** A handle to point at when one entry is split into two. Rare,
  and the diff says the same thing at the moment anyone would read it.
- **Rename survival.** Move the file, edit `path`, keep the entry's identity in
  history. Also served by the diff, and only for a reader who was tracking the
  id in the first place.

Against that: `EX-NNN` allocation machinery in `exception add`, a cross-registry
uniqueness check, and — for anyone who edits a registry by hand, which is most
adopters after the first day — the burden of keeping a number unique for no
reader.

## 3. A break, not a tolerated leftover

Removing a required field is `fissile_exceptions_version = 2`
(§FS-003-exceptions.2.2). A version-1 registry is rejected, and because the
parser denies unknown keys, a version-2 registry that still carries `id` fails
naming that key. Both are errors an adopter reads once, on upgrade, and the
version error names the two edits that fix the file rather than stating which
version the build supports.

The cheaper option was available: accept `id` and `replaces` and ignore them,
keep version 1, and let every registry migrate on its own schedule. It was
rejected because it leaves the format saying one thing and meaning another.
A key the parser silently discards is still a key contributors copy into new
entries, still a field a reader has to ask about, and still a line the next
`grep` finds. Two edits per registry — one version line, one delete pass — is a
smaller total cost than a schema that permanently documents its own past.

Migrating a registry is mechanical enough to publish as one command, which the
release notes of the release that made this break did (§FS-003-exceptions.2.2).
The entries themselves do not change. Release notes are now the list of merged
pull requests (§AR-001-ci.8.3), so a later break carries its migration in the
pull request that makes it: the title names the break, and the body holds the
runnable upgrade steps under a **Migration** heading, which the notes reach
through the link.

## 4. Rejected alternatives

- **Accepting `id` and ignoring it, at version 1.** No migration, and no honesty
  either: the field stays in every file, meaning nothing, and the parser's
  silence is what makes it survive. A format that tolerates a dead key teaches
  the next contributor to write one.
- **Version 2 accepting `id` with a deprecation warning for one release.** The
  warning fires on every load, taxing the hook path §GOAL-001-fast-feedback
  protects, to say something the upgrade error already says once.
- **Keeping `id` as an optional field.** Optional identifiers are the worst of
  both: half the entries have one, no diagnostic can rely on it, and the tool
  still has to decide what a duplicate means.
- **Deriving a display name from `(path, rules, unit)`.** A synthesized name is
  still a name to read past. The registry file and the path are already the
  address, and they are the address a reader can act on.

## 5. Consequences

- The registry version becomes `2`, and every existing registry needs the two
  edits of §FS-003-exceptions.2.2 before the new binary will read it.
- `exception add` loses `--id` and `--replaces`; `resolve_id`, the `EX-NNN`
  allocator, the slug derivation, and the cross-registry uniqueness check are
  gone (§FS-005-exception-add.3).
- The JSON surface loses `exception_id` on silenced records and `id` on
  `audit --stale-exceptions` entries (§FS-004-check-audit.1). That is a breaking
  change for a consumer that read either.
- Entry-level diagnostics change shape: they lead with the registry path and the
  entry's `path` (§FS-003-exceptions.4).
