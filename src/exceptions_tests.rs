//! Tests for exception registry parsing, validation, and matching (§FS-003-exceptions).

use super::*;
use crate::config::Config;

#[path = "exceptions_validation_tests.rs"]
mod wildcard_validation;

const SOFT: &str = r#"
fissile_exceptions_version = 2

[[exceptions]]
path = "tests/fixtures/large.json"
match = "exact"
rules = ["fixtures"]
max_accepted = { value = 300000, unit = "bytes" }
until = "indefinite"
reason = "golden corpus copied from production incidents"
"#;

const SOFT_REGISTRY: &str = "docs/file-size-agent-exceptions.toml";
const HARD_REGISTRY: &str = "docs/file-size-human-exceptions.toml";

/// Load `text` as the soft registry, the shape most cases only need.
fn load_soft(text: &str) -> Result<Registries, ExceptionError> {
    Registries::load(Some(RegistrySource::new(SOFT_REGISTRY, text)), None)
}

fn load_both(soft: &str, hard: &str) -> Result<Registries, ExceptionError> {
    Registries::load(
        Some(RegistrySource::new(SOFT_REGISTRY, soft)),
        Some(RegistrySource::new(HARD_REGISTRY, hard)),
    )
}

fn rules() -> Vec<Rule> {
    let toml = r#"
fissile_config_version = 1
[[messages]]
id = "m"
text = "Split {path}."
[[rules]]
id = "fixtures"
include = ["tests/fixtures/**"]
unit = "bytes"
soft = 65536
hard = 262144
message = "m"
"#;
    Config::parse(toml)
        .unwrap()
        .to_checker()
        .unwrap()
        .rules()
        .to_vec()
}

#[test]
fn loads_and_validates_against_rules() {
    let registries = load_soft(SOFT).expect("loads");
    registries.validate_against(&rules()).expect("validates");
    assert_eq!(registries.soft.len(), 1);
    assert_eq!(registries.soft[0].severity, Severity::Soft);
}

#[test]
fn silences_within_ceiling_and_reports_when_exceeded() {
    let registries = load_soft(SOFT).expect("loads");
    let silenced = registries
        .verdict(
            Severity::Soft,
            "tests/fixtures/large.json",
            "fixtures",
            Unit::Bytes,
            250000,
        )
        .expect("verdict");
    assert!(matches!(silenced, Verdict::Silenced(_)));

    let grew = registries
        .verdict(
            Severity::Soft,
            "tests/fixtures/large.json",
            "fixtures",
            Unit::Bytes,
            400000,
        )
        .expect("verdict");
    assert!(matches!(grew, Verdict::Exceeded(_)));
}

#[test]
fn unmatched_path_is_none() {
    let registries = load_soft(SOFT).expect("loads");
    let verdict = registries
        .verdict(Severity::Soft, "src/lib.rs", "fixtures", Unit::Bytes, 1)
        .expect("verdict");
    assert_eq!(verdict, Verdict::None);
}

#[test]
fn rejects_empty_reason() {
    let toml = r#"
fissile_exceptions_version = 2
[[exceptions]]
path = "a"
match = "exact"
rules = ["*"]
max_accepted = { value = 1, unit = "bytes" }
until = "x"
reason = "   "
"#;
    let error = load_soft(toml).expect_err("empty reason");
    assert!(matches!(error, ExceptionError::EmptyReason { .. }));
    // The entry has no name, so the message locates it: the registry file and
    // the entry's own `path` are the line to edit (§FS-003-exceptions.4).
    assert_eq!(
        error.to_string(),
        format!(
            "{SOFT_REGISTRY}: a states no reason; write one, or declare shadows = \"hard\" to inherit the hard entry's"
        )
    );

    // Omitting the field is the same defect and reads the same way: `reason` is
    // optional in the schema only so a shadowing entry can leave it out
    // (§FS-003-exceptions.4).
    let omitted = toml.replace("reason = \"   \"\n", "");
    let error = load_soft(&omitted).expect_err("no reason at all");
    assert!(matches!(error, ExceptionError::EmptyReason { .. }));
    assert!(error.to_string().contains("a states no reason"), "{error}");

    // Nothing sits above the hard registry to inherit from, so the entry there
    // is told to write one and nothing else (§FS-003-exceptions.2.3).
    let error = Registries::load(None, Some(RegistrySource::new(HARD_REGISTRY, &omitted)))
        .expect_err("no reason at all");
    assert_eq!(
        error.to_string(),
        format!("{HARD_REGISTRY}: a states no reason")
    );
}

/// §FS-003-exceptions.2.1: an entry that omits the field still loads, and reads
/// as deferred — the reading that keeps `until` meaningful and asserts no
/// constraint the author never claimed.
#[test]
fn undeclared_kind_reads_as_deferred() {
    let registries = load_soft(SOFT).expect("loads");
    assert_eq!(registries.soft[0].kind, Kind::Deferred);
    // The kind/until agreement is not applied to it, so `until = "indefinite"`
    // on an entry that declares no kind is not retroactively an error.
    assert_eq!(registries.soft[0].until, "indefinite");
}

/// §FS-003-exceptions.2.1: a declared kind must agree with `until`. Structural
/// never expires; deferred has to name what retires it.
#[test]
fn rejects_kind_that_disagrees_with_until() {
    let structural = SOFT.replace(
        "until = \"indefinite\"",
        "kind = \"structural\"\nuntil = \"the generator lands\"",
    );
    let error = load_soft(&structural).expect_err("dated structural");
    assert!(matches!(
        error,
        ExceptionError::KindUntilMismatch {
            kind: Kind::Structural,
            ..
        }
    ));

    let deferred = SOFT.replace("until =", "kind = \"deferred\"\nuntil =");
    let error = load_soft(&deferred).expect_err("open-ended deferred");
    assert!(matches!(
        error,
        ExceptionError::KindUntilMismatch {
            kind: Kind::Deferred,
            ..
        }
    ));
}

#[test]
fn accepts_agreeing_kind_and_counts_by_kind() {
    let toml = SOFT.replace("until =", "kind = \"structural\"\nuntil =");
    let registries = load_soft(&toml).expect("loads");
    assert_eq!(registries.soft[0].kind, Kind::Structural);
    let counts = registries.kind_counts();
    assert_eq!(counts.structural, 1);
    assert_eq!(counts.deferred, 0);
}

/// §FS-004-check-audit.2: a soft/hard twin contributes two entry totals but one
/// distinct path-expression total.
#[test]
fn kind_path_counts_deduplicate_a_path_across_registries() {
    let registries = load_both(SOFT, SOFT).expect("one deferred path in both registries");

    assert_eq!(registries.kind_counts().deferred, 2);
    assert_eq!(
        registries.kind_path_counts(),
        KindPathCounts {
            structural: 0,
            deferred: 1,
        }
    );
}

/// §FS-004-check-audit.2: structural takes precedence for a path, regardless
/// of which registry supplies it first.
#[test]
fn structural_path_precedence_is_independent_of_registry_order() {
    let structural = SOFT.replace(
        "until = \"indefinite\"",
        "kind = \"structural\"\nuntil = \"indefinite\"",
    );
    let deferred_first = load_both(SOFT, &structural).expect("registries load");
    let structural_first = load_both(&structural, SOFT).expect("registries load");

    let expected = KindPathCounts {
        structural: 1,
        deferred: 0,
    };
    assert_eq!(deferred_first.kind_path_counts(), expected);
    assert_eq!(structural_first.kind_path_counts(), expected);
}

/// §FS-004-check-audit.2: a repeated glob is one literal expression, not the
/// number of files it happens to match.
#[test]
fn kind_path_counts_count_repeated_glob_once_without_expansion() {
    let glob = SOFT
        .replace("tests/fixtures/large.json", "tests/fixtures/**/*.json")
        .replace("match = \"exact\"", "match = \"glob\"");
    let registries = load_both(&glob, &glob).expect("one deferred glob in both registries");

    assert_eq!(registries.kind_counts().deferred, 2);
    assert_eq!(
        registries.kind_path_counts(),
        KindPathCounts {
            structural: 0,
            deferred: 1,
        }
    );
}

/// A hard entry carrying the judgment a soft twin points at
/// (§FS-003-exceptions.2.3). Its ceiling differs from the twin's on purpose:
/// `max_accepted` is the one field the pair is allowed to disagree about.
const HARD_TWIN: &str = r#"
fissile_exceptions_version = 2

[[exceptions]]
path = "tests/fixtures/large.json"
match = "exact"
rules = ["fixtures"]
kind = "deferred"
max_accepted = { value = 400000, unit = "bytes" }
until = "the fixture generator lands"
reason = "no generator reproduces this corpus from the incident descriptions yet"
"#;

/// The twin: a ceiling, a kind, and a pointer — no rationale of its own.
const SHADOWING_TWIN: &str = r#"
fissile_exceptions_version = 2

[[exceptions]]
path = "tests/fixtures/large.json"
match = "exact"
rules = ["fixtures"]
kind = "deferred"
shadows = "hard"
max_accepted = { value = 300000, unit = "bytes" }
"#;

/// §FS-003-exceptions.2.3: the twin states neither field and reads back with
/// both, so every reporting surface treats it like any other entry.
#[test]
fn a_shadowing_twin_inherits_its_hard_entrys_reason_and_until() {
    let registries = load_both(SHADOWING_TWIN, HARD_TWIN).expect("the twin resolves");
    registries.validate_against(&rules()).expect("validates");

    let twin = &registries.soft[0];
    assert_eq!(twin.reason, registries.hard[0].reason);
    assert_eq!(twin.until, "the fixture generator lands");
    // The one field a twin owns stays local, and the pair may differ.
    assert_eq!(twin.max_value, 300000);
    assert_eq!(registries.hard[0].max_value, 400000);
}

/// §FS-003-exceptions.2.3: the address is the same `path`, `match`, and unit,
/// covering every rule listed — so a hard entry for other rules is not a twin,
/// and pointing at nothing fails the load rather than resolving to nothing.
#[test]
fn an_orphan_shadowing_twin_is_a_schema_error() {
    let empty = "fissile_exceptions_version = 2\n";
    let error = load_both(SHADOWING_TWIN, empty).expect_err("nothing to shadow");
    assert!(matches!(error, ExceptionError::ShadowsWithoutTwin { .. }));
    let message = error.to_string();
    // The twin's own site leads, and the file the missing entry belongs in is
    // named (§DF-005-exception-identity).
    assert!(
        message.starts_with(&format!("{SOFT_REGISTRY}: tests/fixtures/large.json")),
        "{message}"
    );
    assert!(message.contains(HARD_REGISTRY), "{message}");

    let other_rule = HARD_TWIN.replace("\"fixtures\"", "\"other\"");
    let error = load_both(SHADOWING_TWIN, &other_rule).expect_err("a twin for other rules");
    assert!(matches!(error, ExceptionError::ShadowsWithoutTwin { .. }));

    // With no hard registry file on disk `load` is given no path for it, so the
    // caller that has one from the config supplies it: the reader's next edit is
    // in that file whether or not it exists yet (§FS-003-exceptions.2.3).
    let error = Registries::load(
        Some(RegistrySource::new(SOFT_REGISTRY, SHADOWING_TWIN)),
        None,
    )
    .expect_err("no hard registry at all")
    .naming_hard_registry(HARD_REGISTRY);
    assert!(
        error
            .to_string()
            .contains(&format!("{HARD_REGISTRY} holds no entry")),
        "{error}"
    );
}

/// §FS-009-exception-remove.2: soft removal gets an address-only view of an
/// orphan, while the ordinary registry view continues to reject exactly the
/// same documents under §FS-003-exceptions.2.3.
#[test]
fn soft_removal_alone_can_load_an_orphan_shadow_address() {
    let empty = "fissile_exceptions_version = 2\n";
    let strict = load_both(SHADOWING_TWIN, empty).expect_err("strict load rejects the orphan");
    assert!(matches!(strict, ExceptionError::ShadowsWithoutTwin { .. }));

    let (registries, entries) = Registries::load_for_soft_removal(
        Some(RegistrySource::new(SOFT_REGISTRY, SHADOWING_TWIN)),
        Some(RegistrySource::new(HARD_REGISTRY, empty)),
    )
    .expect("soft removal reaches the orphan address");
    assert!(registries.soft.is_empty(), "the orphan is not resolved");
    assert!(registries.hard.is_empty());
    assert_eq!(entries.len(), 1);
    assert!(entries[0].resolved().is_none());
    assert_eq!(entries[0].path(), "tests/fixtures/large.json");
    assert_eq!(entries[0].rules().len(), 1);
    assert_eq!(entries[0].rules()[0], "fixtures");
    assert_eq!(entries[0].max_value(), 300000);

    let invalid = SHADOWING_TWIN.replace("value = 300000", "value = 0");
    let error = Registries::load_for_soft_removal(
        Some(RegistrySource::new(SOFT_REGISTRY, &invalid)),
        Some(RegistrySource::new(HARD_REGISTRY, empty)),
    )
    .expect_err("an invalid field owned by the orphan stays strict");
    assert!(matches!(error, ExceptionError::NonPositiveMax { .. }));
}

/// §FS-009-exception-remove.5: the post-write comparison retains every field
/// owned by an orphan even though none of that metadata is needed to address
/// the entry for removal.
#[test]
fn soft_removal_written_comparison_retains_orphan_metadata() {
    let empty = "fissile_exceptions_version = 2\n";
    let original_text = SHADOWING_TWIN.replace(
        "kind = \"deferred\"",
        "kind = \"deferred\"\n\
         title = \"old title\"\n\
         owner = \"old owner\"\n\
         issue = \"old issue\"",
    );
    let (_, original_entries) = Registries::load_for_soft_removal(
        Some(RegistrySource::new(SOFT_REGISTRY, &original_text)),
        Some(RegistrySource::new(HARD_REGISTRY, empty)),
    )
    .expect("the original orphan loads for removal");
    for (field, changed) in [
        (
            "kind",
            original_text.replace("kind = \"deferred\"", "kind = \"structural\""),
        ),
        (
            "title",
            original_text.replace("title = \"old title\"", "title = \"new title\""),
        ),
        (
            "owner",
            original_text.replace("owner = \"old owner\"", "owner = \"new owner\""),
        ),
        (
            "issue",
            original_text.replace("issue = \"old issue\"", "issue = \"new issue\""),
        ),
    ] {
        let (_, changed_entries) = Registries::load_for_soft_removal(
            Some(RegistrySource::new(SOFT_REGISTRY, &changed)),
            Some(RegistrySource::new(HARD_REGISTRY, empty)),
        )
        .unwrap_or_else(|error| panic!("changed orphan {field} loads for removal: {error}"));
        assert!(
            !original_entries[0].same_written_entry(&changed_entries[0]),
            "the write guard must retain orphan {field}"
        );
    }
}

/// §FS-009-exception-remove.5: an inherited rationale may have the same
/// effective value as a standalone rationale, but changing the persisted
/// `shadows` identity is still a change to the surviving entry.
#[test]
fn soft_removal_written_comparison_retains_shadow_identity() {
    let standalone = SHADOWING_TWIN.replace(
        "shadows = \"hard\"",
        "until = \"the fixture generator lands\"\n\
         reason = \"no generator reproduces this corpus from the incident descriptions yet\"",
    );
    let (shadow_registries, shadow_entries) = Registries::load_for_soft_removal(
        Some(RegistrySource::new(SOFT_REGISTRY, SHADOWING_TWIN)),
        Some(RegistrySource::new(HARD_REGISTRY, HARD_TWIN)),
    )
    .expect("the shadow resolves");
    let (standalone_registries, standalone_entries) = Registries::load_for_soft_removal(
        Some(RegistrySource::new(SOFT_REGISTRY, &standalone)),
        Some(RegistrySource::new(HARD_REGISTRY, HARD_TWIN)),
    )
    .expect("the equivalent standalone entry loads");

    assert_eq!(
        shadow_registries.soft, standalone_registries.soft,
        "the effective entries deliberately agree"
    );
    assert!(
        !shadow_entries[0].same_written_entry(&standalone_entries[0]),
        "the write guard must retain the shadow pointer"
    );
}

/// §FS-009-exception-remove.5: removing an earlier resolved entry renumbers a
/// survivor in the filtered loader vector, but does not change its persisted
/// meaning.
#[test]
fn soft_removal_written_comparison_ignores_resolved_index() {
    let second = SOFT.replace("large.json", "other.json");
    let both = format!(
        "{}\n{}",
        SOFT,
        second.replace("fissile_exceptions_version = 2", "")
    );
    let (_, before) =
        Registries::load_for_soft_removal(Some(RegistrySource::new(SOFT_REGISTRY, &both)), None)
            .expect("both entries load");
    let (_, after) =
        Registries::load_for_soft_removal(Some(RegistrySource::new(SOFT_REGISTRY, &second)), None)
            .expect("the survivor loads");

    assert_eq!(before[1].resolved().map(|(index, _)| index), Some(1));
    assert_eq!(after[0].resolved().map(|(index, _)| index), Some(0));
    assert!(before[1].same_written_entry(&after[0]));
}

/// §FS-003-exceptions.2.3: a twin inherits one rationale, so two hard entries
/// answering its address is an ambiguity, not a pick.
#[test]
fn an_ambiguous_shadowing_twin_is_a_schema_error() {
    let two = format!(
        "{HARD_TWIN}{}",
        HARD_TWIN
            .replace("fissile_exceptions_version = 2", "")
            .replace("[\"fixtures\"]", "[\"*\"]")
    );
    let error = load_both(SHADOWING_TWIN, &two).expect_err("two entries answer the address");
    assert!(matches!(error, ExceptionError::ShadowsAmbiguousTwin { .. }));
    let message = error.to_string();
    // Their paths are the same string, so the rule lists are what tell them apart.
    assert!(
        message.contains("[fixtures]") && message.contains("[*]"),
        "{message}"
    );

    // True duplicates are told apart by nothing, and a message printing one rule
    // list twice would read as a contradiction: the duplicate is the defect
    // (§FS-003-exceptions.2.3).
    let twice = format!(
        "{HARD_TWIN}{}",
        HARD_TWIN.replace("fissile_exceptions_version = 2", "")
    );
    let error = load_both(SHADOWING_TWIN, &twice).expect_err("the same address twice");
    let message = error.to_string();
    assert!(
        message.contains(&format!(
            "{HARD_REGISTRY} holds more than one entry for that address, each listing rules [fixtures]"
        )) && message.contains("delete the duplicate entry there"),
        "{message}"
    );
    assert!(!message.contains("another"), "{message}");
}

/// §FS-003-exceptions.2.3: the hard registry is where a rationale lives, so
/// nothing in it has anything above it to point at.
#[test]
fn shadows_in_the_hard_registry_is_a_schema_error() {
    let error = load_both(
        SOFT,
        &HARD_TWIN.replace("kind =", "shadows = \"hard\"\nkind ="),
    )
    .expect_err("a hard entry may not shadow");
    assert!(matches!(
        error,
        ExceptionError::ShadowsInHardRegistry { .. }
    ));
    assert!(error.to_string().starts_with(HARD_REGISTRY), "{error}");
}

/// §FS-003-exceptions.2.3: `shadows` excludes both fields it replaces. A second
/// copy of either is the drift the pointer exists to remove, so it is named
/// rather than ignored.
#[test]
fn shadows_beside_a_stated_rationale_is_a_schema_error() {
    for field in ["reason", "until"] {
        let stated = format!("{SHADOWING_TWIN}{field} = \"a copy that can drift\"\n");
        let error = load_both(&stated, HARD_TWIN).expect_err("shadows plus {field}");
        assert!(
            matches!(error, ExceptionError::ShadowsWithOwnRationale { field: named, .. } if named == field),
            "{error}"
        );
        // Both edits are named: dropping the pointer is as often the fix.
        let message = error.to_string();
        assert!(message.contains(&format!("delete {field}")), "{message}");
        assert!(message.contains("delete shadows"), "{message}");
    }
}

/// §FS-003-exceptions.2.3: the §FS-003-exceptions.2.1 agreement is checked
/// against the *inherited* `until`, so a twin whose original flips kind stops
/// loading — the forced revisit, rather than a silent disagreement.
#[test]
fn a_declared_kind_is_checked_against_the_inherited_until() {
    let structural_original = HARD_TWIN
        .replace("kind = \"deferred\"", "kind = \"structural\"")
        .replace(
            "until = \"the fixture generator lands\"",
            "until = \"indefinite\"",
        );
    let error = load_both(SHADOWING_TWIN, &structural_original)
        .expect_err("a deferred twin inherits indefinite");
    assert!(matches!(
        error,
        ExceptionError::KindUntilMismatch {
            kind: Kind::Deferred,
            ..
        }
    ));
    assert!(error.to_string().starts_with(SOFT_REGISTRY), "{error}");

    // Following the original's kind loads again, on the same inherited value.
    let agreeing = SHADOWING_TWIN.replace("kind = \"deferred\"", "kind = \"structural\"");
    let registries = load_both(&agreeing, &structural_original).expect("the twin agrees");
    assert_eq!(registries.soft[0].until, "indefinite");
}

#[test]
fn rejects_empty_until() {
    let toml = SOFT.replace("until = \"indefinite\"", "until = \"  \"");
    let error = load_soft(&toml).expect_err("empty until");
    assert!(matches!(error, ExceptionError::EmptyUntil { .. }));
    // Blank and omitted are one defect here too, and the soft entry is offered
    // the pointer that would fill it (§FS-003-exceptions.4).
    assert_eq!(
        error.to_string(),
        format!(
            "{SOFT_REGISTRY}: tests/fixtures/large.json states no until; name what retires the entry, or declare shadows = \"hard\" to inherit the hard entry's"
        )
    );

    let omitted = SOFT.replace("until = \"indefinite\"\n", "");
    let error = load_soft(&omitted).expect_err("no until at all");
    assert!(matches!(error, ExceptionError::EmptyUntil { .. }));
    assert!(error.to_string().contains("states no until"), "{error}");
}

/// §FS-003-exceptions.2.2: version 2 removed `id`/`replaces` rather than
/// tolerating them. A version-1 registry is refused with both migration edits
/// named, and a version-2 entry that kept a name fails on the unknown key.
#[test]
fn version_1_is_refused_and_names_both_edits() {
    let v1 = SOFT.replace("_version = 2", "_version = 1");
    let error = load_soft(&v1).expect_err("version 1 is not supported");
    let message = error.to_string();
    assert!(message.starts_with(SOFT_REGISTRY), "{message}");
    for edit in [
        "set fissile_exceptions_version = 2",
        "delete every id and replaces line",
    ] {
        assert!(message.contains(edit), "migration edit missing: {message}");
    }
}

#[test]
fn a_leftover_id_is_an_unknown_key() {
    let named = SOFT.replace("path =", "id = \"EX-001-fixture\"\npath =");
    let error = load_soft(&named).expect_err("id is no longer a field");
    assert!(matches!(error, ExceptionError::Parse { .. }));
    assert!(error.to_string().contains("id"), "{error}");
}

/// The same path in both registries is two entries making two claims at two
/// severities, so a diagnostic naming only the path would be ambiguous
/// (§DF-005-exception-identity).
#[test]
fn the_same_path_in_both_registries_is_two_entries() {
    let broken = SOFT.replace(
        "reason = \"golden corpus copied from production incidents\"",
        "reason = \"  \"",
    );
    let registries = Registries::load(
        Some(RegistrySource::new(SOFT_REGISTRY, SOFT)),
        Some(RegistrySource::new(HARD_REGISTRY, SOFT)),
    )
    .expect("one path, two registries");
    assert_eq!(registries.soft.len(), 1);
    assert_eq!(registries.hard.len(), 1);

    let error = Registries::load(
        Some(RegistrySource::new(SOFT_REGISTRY, SOFT)),
        Some(RegistrySource::new(HARD_REGISTRY, &broken)),
    )
    .expect_err("the hard copy has an empty reason");
    assert!(error.to_string().starts_with(HARD_REGISTRY));
}

#[test]
fn rejects_unknown_rule() {
    let toml = SOFT.replace("\"fixtures\"", "\"nope\"");
    let registries = load_soft(&toml).expect("loads");
    let error = registries
        .validate_against(&rules())
        .expect_err("unknown rule");
    assert!(matches!(error, ExceptionError::UnknownRule { .. }));
}

#[test]
fn rejects_max_below_limit() {
    let toml = SOFT.replace("value = 300000", "value = 1000");
    let registries = load_soft(&toml).expect("loads");
    let error = registries
        .validate_against(&rules())
        .expect_err("below soft limit");
    assert!(matches!(error, ExceptionError::BelowLimit { .. }));
}

#[test]
fn rejects_unit_mismatch() {
    let toml = SOFT.replace("unit = \"bytes\"", "unit = \"lines\"");
    let registries = load_soft(&toml).expect("loads");
    let error = registries
        .validate_against(&rules())
        .expect_err("unit mismatch");
    assert!(matches!(error, ExceptionError::UnitMismatch { .. }));
}

#[test]
fn reports_multiple_matches_as_schema_error() {
    let toml = r#"
fissile_exceptions_version = 2
[[exceptions]]
path = "tests/**"
match = "glob"
rules = ["fixtures"]
max_accepted = { value = 300000, unit = "bytes" }
until = "x"
reason = "first"
[[exceptions]]
path = "tests/fixtures/**"
match = "glob"
rules = ["fixtures"]
max_accepted = { value = 300000, unit = "bytes" }
until = "x"
reason = "second"
"#;
    let registries = load_soft(toml).expect("loads");
    let error = registries
        .verdict(
            Severity::Soft,
            "tests/fixtures/large.json",
            "fixtures",
            Unit::Bytes,
            1,
        )
        .expect_err("multiple matches");
    assert!(matches!(error, ExceptionError::MultipleMatches { .. }));
}

#[test]
fn reports_stale_entries() {
    let registries = load_soft(SOFT).expect("loads");
    let stale = registries.stale(&["src/lib.rs".to_owned()]);
    assert_eq!(stale.len(), 1);
    let live = registries.stale(&["tests/fixtures/large.json".to_owned()]);
    assert!(live.is_empty());
}
