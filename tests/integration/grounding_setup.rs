//! Repository grounding setup agrees with the CI contract (§AR-001-ci.2).
//! These checks read repository files and do not invoke an installed grund.

use std::{fs, path::Path};

fn read(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
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
    let config: toml::Value = read("grund.toml").parse().expect("valid grund.toml");
    assert_eq!(
        config
            .get("grund_config_version")
            .and_then(toml::Value::as_integer),
        Some(2),
        "grund.toml must use grund_config_version = 2"
    );
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
