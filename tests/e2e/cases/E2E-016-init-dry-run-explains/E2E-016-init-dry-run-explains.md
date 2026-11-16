# E2E-016-init-dry-run-explains: a dry run prints the block it would install

`fissile init --dry-run` prints the managed agent block on stdout while the
planned writes stay on stderr, and no file is created (§FS-002-init.4,
§FS-002-init.5). It is the one way to read the block without changing a file,
and it prints the same constant `init` writes — markers included — so what a
reader sees and what an entrypoint receives cannot drift.

The block is three sentences because the rest of the instructions live in the
surfaces that raise each question (§DF-007-instructions-at-the-error-site.1).
What the dry run answers is therefore what `init` would put in this repository,
not what an agent should do about a finding — the finding says that.

The block names which hook runs `fissile check --staged`: the one `init`
installs does, and a hook that passes file paths keeps soft findings advisory
(§FS-004-check-audit.1.4). It no longer says that any installed hook runs that
same check, which a path-passing hook manager does not.
