//! Load-time wildcard validation boundaries (§FS-003-exceptions.4). No target
//! files are created: selectors concern possible paths, not a scan inventory.

use super::*;

const CONFIG: &str = r#"
fissile_config_version = 1
[[messages]]
id = "m"
text = "Split."
[[rules]]
id = "source"
include = ["src/**/*.rs"]
exclude = ["src/vendor/**"]
unit = "lines"
soft = 500
hard = 2500
message = "m"
[[rules]]
id = "large-file-default"
include = ["**/*"]
unit = "bytes"
soft = 524288
hard = 1048576
message = "m"
"#;

fn configured(text: &str) -> Vec<Rule> {
    Config::parse(text)
        .unwrap()
        .to_checker()
        .unwrap()
        .rules()
        .to_vec()
}

fn validate(
    config: &str,
    severity: Severity,
    path: &str,
    matcher: &str,
    rule: &str,
    value: u64,
    unit: &str,
) -> Result<(), ExceptionError> {
    let text = format!(
        r#"fissile_exceptions_version = 2
[[exceptions]]
path = "{path}"
match = "{matcher}"
rules = ["{rule}"]
max_accepted = {{ value = {value}, unit = "{unit}" }}
until = "seam-exists"
reason = "the suite splits once its fixture seam exists"
"#
    );
    let (soft, hard) = match severity {
        Severity::Soft => (Some(RegistrySource::new(SOFT_REGISTRY, &text)), None),
        Severity::Hard => (None, Some(RegistrySource::new(HARD_REGISTRY, &text))),
    };
    Registries::load(soft, hard)
        .unwrap()
        .validate_against(&configured(config))
}

fn wildcard(config: &str, path: &str, matcher: &str, max: u64) -> Result<(), ExceptionError> {
    validate(config, Severity::Soft, path, matcher, "*", max, "lines")
}

fn below(result: Result<(), ExceptionError>, severity: Severity, path: &str, max: u64, limit: u64) {
    let registry = match severity {
        Severity::Soft => SOFT_REGISTRY,
        Severity::Hard => HARD_REGISTRY,
    };
    assert_eq!(
        result,
        Err(ExceptionError::BelowLimit {
            site: site(registry, path),
            rule: "source".into(),
            max,
            limit,
        })
    );
}

#[test]
fn soft_ceiling_below_the_applicable_limit_is_refused() {
    below(
        wildcard(CONFIG, "src/suite.rs", "exact", 300),
        Severity::Soft,
        "src/suite.rs",
        300,
        500,
    );
}

#[test]
fn hard_ceiling_uses_the_hard_limit() {
    below(
        validate(
            CONFIG,
            Severity::Hard,
            "src/suite.rs",
            "exact",
            "*",
            2499,
            "lines",
        ),
        Severity::Hard,
        "src/suite.rs",
        2499,
        2500,
    );
}

#[test]
fn equal_and_larger_ceilings_pass_with_the_bytes_catch_all() {
    for (severity, values) in [(Severity::Soft, [500, 600]), (Severity::Hard, [2500, 2600])] {
        for value in values {
            validate(
                CONFIG,
                severity,
                "src/suite.rs",
                "exact",
                "*",
                value,
                "lines",
            )
            .unwrap();
        }
    }
}

#[test]
fn bytes_wildcard_checks_the_bytes_rule_instead_of_the_source_unit() {
    let error = validate(
        CONFIG,
        Severity::Soft,
        "src/suite.rs",
        "exact",
        "*",
        30000,
        "bytes",
    );
    assert_eq!(
        error,
        Err(ExceptionError::BelowLimit {
            site: site(SOFT_REGISTRY, "src/suite.rs"),
            rule: "large-file-default".into(),
            max: 30000,
            limit: 524288,
        })
    );
}

#[test]
fn every_applicable_rule_is_checked_despite_precedence() {
    let config = CONFIG.replace(
        "[[rules]]\nid = \"source\"",
        "[[rules]]\nid = \"lenient\"\ninclude = [\"src/suite.rs\"]\nunit = \"lines\"\nsoft = 100\nhard = 2500\npriority = 10\nmessage = \"m\"\n[[rules]]\nid = \"source\"",
    );
    below(
        wildcard(&config, "src/suite.rs", "exact", 300),
        Severity::Soft,
        "src/suite.rs",
        300,
        500,
    );
}

#[test]
fn missing_soft_limit_is_refused() {
    let config = CONFIG.replace("soft = 500\n", "");
    assert_eq!(
        wildcard(&config, "src/suite.rs", "exact", 3000),
        Err(ExceptionError::NoSeverityLimit {
            site: site(SOFT_REGISTRY, "src/suite.rs"),
            rule: "source".into(),
            severity: Severity::Soft,
        })
    );
}

#[test]
fn missing_hard_limit_is_refused() {
    let config = CONFIG.replace("hard = 2500\n", "");
    assert_eq!(
        validate(
            &config,
            Severity::Hard,
            "src/suite.rs",
            "exact",
            "*",
            3000,
            "lines"
        ),
        Err(ExceptionError::NoSeverityLimit {
            site: site(HARD_REGISTRY, "src/suite.rs"),
            rule: "source".into(),
            severity: Severity::Hard,
        })
    );
}

#[test]
fn unrelated_and_excluded_exact_paths_remain_inert() {
    for path in ["docs/suite.rs", "src/vendor/suite.rs", "src/suite.txt"] {
        wildcard(CONFIG, path, "exact", 1).unwrap();
    }
    let no_lines = CONFIG.replace("unit = \"lines\"", "unit = \"bytes\"");
    wildcard(&no_lines, "src/suite.rs", "exact", 1).unwrap();
}

#[test]
fn overlapping_glob_is_checked_without_any_target_files() {
    let config = CONFIG.replace("src/**/*.rs", "src/future/?uite.rs");
    below(
        wildcard(&config, "src/**/s*.rs", "glob", 300),
        Severity::Soft,
        "src/**/s*.rs",
        300,
        500,
    );
}

#[test]
fn glob_overlap_at_zero_double_star_segments_is_checked() {
    let config = CONFIG.replace("src/**/*.rs", "src/suite.rs");
    below(
        wildcard(&config, "src/**/suite.rs", "glob", 300),
        Severity::Soft,
        "src/**/suite.rs",
        300,
        500,
    );
}

#[test]
fn partially_excluded_glob_still_checks_its_remaining_paths() {
    below(
        wildcard(CONFIG, "src/**/suite.rs", "glob", 300),
        Severity::Soft,
        "src/**/suite.rs",
        300,
        500,
    );
}

#[test]
fn disjoint_and_fully_excluded_globs_remain_inert() {
    for pattern in ["docs/**/*.rs", "src/vendor/**/*.rs", "src/**/*.txt"] {
        wildcard(CONFIG, pattern, "glob", 1).unwrap();
    }
    // These are literal braces in the shipped engine, not alternation syntax.
    let literal = CONFIG.replace("src/**/*.rs", "src/{suite,other}.rs");
    wildcard(&literal, "src/suite*.rs", "glob", 1).unwrap();
}

#[test]
fn excludes_may_cover_the_shared_paths_without_covering_either_glob() {
    let config = CONFIG
        .replace("src/**/*.rs", "src/suite.*")
        .replace("src/vendor/**", "src/suite.rs");
    wildcard(&config, "src/*.rs", "glob", 1).unwrap();
}

#[test]
fn union_of_excludes_can_cover_all_shared_paths() {
    let config = CONFIG
        .replace(
            "include = [\"src/**/*.rs\"]",
            "include = [\"src/a.rs\", \"src/b.rs\"]",
        )
        .replace(
            "exclude = [\"src/vendor/**\"]",
            "exclude = [\"src/a*.rs\", \"src/b*.rs\"]",
        );
    wildcard(&config, "src/?.rs", "glob", 1).unwrap();
}

#[test]
fn any_include_can_make_a_glob_applicable() {
    let config = CONFIG.replace(
        "include = [\"src/**/*.rs\"]",
        "include = [\"docs/**\", \"src/future/suite.rs\"]",
    );
    below(
        wildcard(&config, "src/**/s*.rs", "glob", 300),
        Severity::Soft,
        "src/**/s*.rs",
        300,
        500,
    );
}

#[test]
fn valid_glob_ceiling_passes() {
    wildcard(CONFIG, "src/**/s*.rs", "glob", 500).unwrap();
}

#[test]
fn explicit_rules_stay_strict_even_when_selectors_do_not_match() {
    let path = "docs/suite.rs";
    let check = |config: &str, rule, max, unit| {
        validate(config, Severity::Soft, path, "exact", rule, max, unit)
    };
    assert!(matches!(
        check(CONFIG, "missing", 500, "lines"),
        Err(ExceptionError::UnknownRule { .. })
    ));
    assert!(matches!(
        check(CONFIG, "source", 500, "bytes"),
        Err(ExceptionError::UnitMismatch { .. })
    ));
    below(
        check(CONFIG, "source", 300, "lines"),
        Severity::Soft,
        path,
        300,
        500,
    );
    assert!(matches!(
        check(&CONFIG.replace("soft = 500\n", ""), "source", 3000, "lines"),
        Err(ExceptionError::NoSeverityLimit { .. })
    ));
}
