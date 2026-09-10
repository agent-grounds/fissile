# DF-012-config-home: Tool config lives in `.agent-grounds/`, and `.agents/` stays instructions

`fissile` discovered its config at `.agents/fissile.toml`, and `fissile init`
wrote that path. `.agents/` is by convention where agent *instructions* live,
and agent runtimes have started protecting it accordingly: Codex's managed
permission profile mounts `.agents/` read-only inside a checkout, so an agent
working in the repository cannot edit anything under it.

That protection is right for instructions and wrong for tool config. It was hit
concretely on agent-grounds/rhei#157: three consecutive supervised fix rounds failed
to repair a tracked file under `.agents/`, each round reporting the same
finding, because the sandbox refused the write without saying so. The run cost a
full pass and shipped nothing. Nothing in the tool was broken — the directory
was simply the wrong home for a file the toolchain has to be able to write.

## 1. Decision

Two directories, separated by who writes them:

- `.agents/` — agent instructions, read-only to a sandboxed agent, as intended;
- `.agent-grounds/` — tool config and product files the toolchain maintains,
  writable.

`fissile` discovers its config at `.agent-grounds/fissile.toml` and `init`
writes it there (§FS-001-config.8, §FS-002-init.2). `.agents/fissile.toml` is
still read, one step behind it in the search order, and a run that reads it says
the path is deprecated and where to move it (§FS-001-config.8.2).

`.agents/skills` is out of scope: Codex scans that path, so it stays where it
is. Sibling changes move `grund`, `rhei` and `ephor` the same way, so one
directory answers for the toolchain rather than one per tool.

## 2. Why

The failure this prevents is silent. A sandboxed agent that cannot write the
config does not get an error it can act on; it gets a write that appears to
succeed and a file that did not change, and it re-derives the same finding on
every round until the run is spent. Separating the directories turns a
protection that happened to catch tool config into one that means what it says.

The deprecation path is a fallback rather than a flag day because a config is
the one file whose absence is not an error: a repository upgrading `fissile`
would silently fall back to the built-in defaults (§FS-001-config.0), pass every
run, and enforce none of its own rules. Reading the old path keeps that
impossible, and the warning is what stops the fallback from becoming permanent.

## 3. Consequences

- Discovery has an order and a source, where before it had one literal path.
  The source is what the deprecation warning names, so it has to be carried out
  of the load rather than recomputed by each command.
- `init` in a repository that already has `.agents/fissile.toml` writes no
  second config (§FS-002-init.2). A generated default at the new path would win
  discovery and replace the project's tuned rules with generic ones on the next
  run.
- An explicit `--config` never warns: it names a document rather than choosing
  one, and a caller who spelled the path out is not being surprised by it.
- `fissile` keeps its own config at `.agent-grounds/fissile.toml`, so the move
  is demonstrated in the repository that asks for it.

## 4. Rejected Alternatives

**Read only `.agent-grounds/fissile.toml`.** A repository that upgraded without
moving its file would fall back to the built-in defaults and keep passing, which
is a config loss with no diagnostic. The fallback costs one branch in discovery
and removes that outcome entirely.

**Keep `.agents/fissile.toml` and ask runtimes to make it writable.** The
read-only mount is correct behavior that other tools depend on, and it is not
ours to change. It would also have to be changed in every runtime, one at a
time, while the repository stays broken.

**Have `init` write the new path over an existing old one.** It reads as the
helpful migration and is the worst outcome available: two configs, the generic
one in force, and a project's rules still on disk and no longer enforced.
