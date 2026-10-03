# E2E-096-a-staged-duplicate-match-names-both-entries: a staged duplicate match names both entries

The same collision as the plain check, met by `check --staged` with
`src/big.rs` staged — the run the pre-commit hook makes. It refuses in the same
words, naming both colliding entries and the remedy (§FS-003-exceptions.3,
§FS-003-exceptions.4), because a hook is where a reader has the least context to
guess which entry is meant.
