# E2E-095-a-duplicate-match-names-both-entries: a duplicate match names both entries

Two glob entries in the soft registry both match `src/big.rs` for the same rule
and unit, and neither spells that path. The refusal is correct — one accepted
condition at one severity has one rationale (§FS-003-exceptions.3) — but it has
to send the reader to the entries, not to the file: it names both entries'
`path` values in registry order and the edit that restores one rationale
(§FS-003-exceptions.4).

The file is 15 lines against a 10/20 budget. That is load-bearing: only a soft
overflow under the hard limit consults the soft registry, so a file at the hard
limit would never reach the collision this case is about.
