//! Tests for what a same-severity collision says (§FS-003-exceptions.3): the
//! refusal names the entries to edit, not only the file they collide on
//! (§FS-003-exceptions.4).

use super::*;

const REGISTRY: &str = "docs/soft-exceptions.toml";
const REMEDY: &str = "remove or narrow overlapping entries so only one covers this file, rule and unit in this registry";

/// One soft entry for `rust-source` with a 100-line ceiling, matching by `matcher`.
fn entry(path: &str, matcher: &str) -> String {
    format!(
        r#"
[[exceptions]]
path = "{path}"
match = "{matcher}"
rules = ["rust-source"]
kind = "structural"
max_accepted = {{ value = 100, unit = "lines" }}
until = "indefinite"
reason = "An entry that collides."
"#
    )
}

/// The refusal `src/big.rs` meets at `actual` lines under a registry of `entries`.
fn refusal(entries: &[String], actual: u64) -> String {
    let text = format!("fissile_exceptions_version = 2\n{}", entries.concat());
    let registries =
        Registries::load(Some(RegistrySource::new(REGISTRY, &text)), None).expect("loads");
    let error = registries
        .verdict(
            Severity::Soft,
            "src/big.rs",
            "rust-source",
            Unit::Lines,
            actual,
        )
        .expect_err("two entries match");
    assert!(matches!(error, ExceptionError::MultipleMatches { .. }));
    error.to_string()
}

#[test]
fn names_both_colliding_patterns_in_registry_order() {
    let message = refusal(&[entry("src/**", "glob"), entry("src/b*.rs", "glob")], 15);
    assert_eq!(
        message,
        format!(
            "{REGISTRY}: more than one exception matches src/big.rs for lines rule rust-source; matching entries include path = \"src/**\" and path = \"src/b*.rs\"; {REMEDY}"
        )
    );
}

#[test]
fn equal_paths_are_named_once() {
    let message = refusal(
        &[entry("src/big.rs", "exact"), entry("src/big.rs", "glob")],
        15,
    );
    assert_eq!(
        message,
        format!(
            "{REGISTRY}: more than one exception matches src/big.rs for lines rule rust-source; at least two matching entries declare path = \"src/big.rs\"; {REMEDY}"
        )
    );
}

#[test]
fn three_matches_name_the_first_two() {
    let message = refusal(
        &[
            entry("src/**", "glob"),
            entry("src/b*.rs", "glob"),
            entry("src/big.rs", "exact"),
        ],
        15,
    );
    assert!(
        message.contains("matching entries include path = \"src/**\" and path = \"src/b*.rs\";"),
        "{message}"
    );
    assert!(!message.contains("path = \"src/big.rs\""), "{message}");
}

#[test]
fn the_refusal_holds_above_both_ceilings() {
    let message = refusal(&[entry("src/**", "glob"), entry("src/b*.rs", "glob")], 1000);
    assert!(
        message.contains("matching entries include path = \"src/**\" and path = \"src/b*.rs\""),
        "{message}"
    );
    assert!(message.ends_with(REMEDY), "{message}");
}

/// A library caller reads the two entries from the variant itself, not only from
/// its wording (§FS-003-exceptions.3).
#[test]
fn the_variant_carries_both_patterns() {
    let text = format!(
        "fissile_exceptions_version = 2\n{}{}",
        entry("src/**", "glob"),
        entry("src/b*.rs", "glob")
    );
    let registries =
        Registries::load(Some(RegistrySource::new(REGISTRY, &text)), None).expect("loads");
    let error = registries
        .verdict(Severity::Soft, "src/big.rs", "rust-source", Unit::Lines, 15)
        .expect_err("two entries match");
    assert_eq!(
        error,
        ExceptionError::MultipleMatches {
            registry: REGISTRY.to_owned(),
            path: "src/big.rs".to_owned(),
            rule: "rust-source".to_owned(),
            unit: Unit::Lines,
            patterns: Box::new(["src/**".to_owned(), "src/b*.rs".to_owned()]),
        }
    );
}
