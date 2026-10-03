# E2E-097-a-json-duplicate-match-names-both-entries: a JSON duplicate match names both entries

A JSON run meets the collision too. A schema error is not a finding: stdout
stays empty rather than carrying a findings array, the run exits 2, and stderr
names both colliding entries and the remedy, exactly as a text run does
(§FS-003-exceptions.3, §FS-003-exceptions.4, §FS-004-check-audit.5).
