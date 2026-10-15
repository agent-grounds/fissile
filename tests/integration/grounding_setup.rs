//! Repository grounding setup agrees with the CI contract (§AR-001-ci.2).
//! These checks read repository files and do not invoke an installed grund.

use std::{fs, path::Path};

fn read(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

fn grund_config() -> toml::Value {
    read("grund.toml").parse().expect("valid grund.toml")
}

fn assert_required_grund_policy(table: &str, setting: &str) {
    let config = grund_config();
    match config
        .get("grund_config_version")
        .and_then(toml::Value::as_integer)
    {
        Some(1) => assert_eq!(
            config
                .get(table)
                .and_then(|table| table.get(setting))
                .and_then(toml::Value::as_bool),
            Some(true),
            "grund.toml v1 must explicitly set [{table}] {setting} = true"
        ),
        // Grund 0.18.0 guarantees both policies in v2 without legacy keys.
        Some(2) => {}
        version => panic!("unsupported grund_config_version: {version:?}; expected 1 or 2"),
    }
}

fn assert_grund_install_version(relative: &str) {
    let text = read(relative);
    let installs: Vec<_> = text
        .lines()
        .filter(|line| line.contains("cargo install grund"))
        .collect();
    assert!(
        !installs.is_empty(),
        "{relative}: missing contributor/CI install command for grund 0.18.0"
    );
    for line in installs {
        let words: Vec<_> = line
            .split(|ch: char| ch.is_whitespace() || ch == '`')
            .filter(|word| !word.is_empty())
            .collect();
        assert!(
            words.windows(2).any(|pair| pair == ["--version", "0.18.0"]),
            "{relative}: grund install must pin --version 0.18.0; found: {}",
            line.trim()
        );
    }
}

#[test]
fn repository_grund_config_is_v2() {
    let config = grund_config();
    assert_eq!(
        config
            .get("grund_config_version")
            .and_then(toml::Value::as_integer),
        Some(2),
        "grund.toml must use grund_config_version = 2"
    );
}

/// Grounding requires marked citations (§AR-001-ci.2).
#[test]
fn repository_grounding_requires_marked_citations() {
    assert_required_grund_policy("reference", "strict");
}

/// Grounding recognizes named sections (§AR-001-ci.2).
#[test]
fn repository_grounding_recognizes_named_sections() {
    assert_required_grund_policy("id", "named_sections");
}

#[test]
fn ci_installs_grund_0_18_0() {
    assert_grund_install_version(".github/workflows/ci.yml");
}

#[test]
fn readme_installs_grund_0_18_0() {
    assert_grund_install_version("README.md");
}

#[test]
fn agents_installs_grund_0_18_0() {
    assert_grund_install_version("AGENTS.md");
}
