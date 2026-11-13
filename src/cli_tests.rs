//! Unit tests for the lines discovery speaks (§FS-001-config.8): which source
//! and which root give which lines, spelled from where the caller stands, in the
//! order of the points that define them.

use super::*;
use crate::config::{DEPRECATED_WARNING, IGNORED_WARNING};

fn home(shadows_deprecated: bool) -> ConfigSource {
    ConfigSource::Home { shadows_deprecated }
}

const PASSED: &str = "fissile: warning: .agent-grounds/fissile.toml is ignored; \
                      discovery reads the repository root, and ";

#[test]
fn at_the_root_the_home_warnings_keep_their_bytes() {
    // §FS-001-config.8.2, §FS-001-config.8.3
    let lines = |source| discovery_lines(&source, "", None, None);
    assert_eq!(lines(ConfigSource::Deprecated), [DEPRECATED_WARNING]);
    assert_eq!(lines(home(true)), [IGNORED_WARNING]);
    assert!(lines(home(false)).is_empty());
}

#[test]
fn below_the_root_the_home_warnings_are_spelled_from_the_caller() {
    assert_eq!(
        discovery_lines(&ConfigSource::Deprecated, "../../", None, None),
        [
            "fissile: warning: ../../.agents/fissile.toml is deprecated; \
          move it to ../../.agent-grounds/fissile.toml"
        ]
    );
    assert_eq!(
        discovery_lines(&home(true), "../", None, None),
        ["fissile: warning: ../.agents/fissile.toml is ignored; \
          ../.agent-grounds/fissile.toml is the config in effect"]
    );
}

#[test]
fn a_passed_over_config_names_what_is_in_effect_after_the_home_warning() {
    // §FS-001-config.8.4, after §FS-001-config.8.2 in section order.
    let passed = Some(".agent-grounds/fissile.toml");
    assert_eq!(
        discovery_lines(&home(false), "../", passed, None),
        [format!(
            "{PASSED}../.agent-grounds/fissile.toml is the config in effect"
        )]
    );
    assert_eq!(
        discovery_lines(&ConfigSource::Deprecated, "../", passed, None),
        [
            "fissile: warning: ../.agents/fissile.toml is deprecated; \
             move it to ../.agent-grounds/fissile.toml"
                .to_owned(),
            format!("{PASSED}../.agents/fissile.toml is the config in effect"),
        ]
    );
    assert_eq!(
        discovery_lines(&home(false), "../", Some(".agents/fissile.toml"), None),
        [
            "fissile: warning: .agents/fissile.toml is ignored; discovery reads the \
          repository root, and ../.agent-grounds/fissile.toml is the config in effect"
        ]
    );
}

#[test]
fn the_built_in_defaults_are_announced_last_with_the_absolute_root() {
    // §FS-001-config.8.5, after §FS-001-config.8.4.
    let root = Path::new("/work/repo");
    let note = "fissile: note: no .agent-grounds/fissile.toml in /work/repo; \
                using the built-in defaults";
    assert_eq!(
        discovery_lines(&ConfigSource::BuiltIn, "", None, Some(root)),
        [note]
    );
    assert_eq!(
        discovery_lines(
            &ConfigSource::BuiltIn,
            "../",
            Some(".agent-grounds/fissile.toml"),
            Some(root)
        ),
        [
            format!("{PASSED}the built-in defaults are in effect"),
            note.to_owned()
        ]
    );
}

#[test]
fn an_explicit_config_owes_no_line() {
    let explicit = ConfigSource::Explicit(PathBuf::from(".agents/fissile.toml"));
    assert!(
        discovery_lines(
            &explicit,
            "../",
            Some(".agent-grounds/fissile.toml"),
            Some(Path::new("/work/repo"))
        )
        .is_empty()
    );
    assert!(config_notes(&explicit, Path::new("..")).is_empty());
}

#[test]
fn config_notes_reads_the_loaded_root() {
    // At the root: no passed-over config is looked for, and the note names the
    // root as the filesystem resolves it.
    assert!(config_notes(&home(false), Path::new(".")).is_empty());
    let here = Path::new(".").canonicalize().unwrap();
    assert_eq!(
        config_notes(&ConfigSource::BuiltIn, Path::new(".")),
        [format!(
            "fissile: note: no .agent-grounds/fissile.toml in {}; using the built-in defaults",
            here.display()
        )]
    );
    assert_eq!(
        config_notes(&ConfigSource::Deprecated, Path::new(".")),
        [DEPRECATED_WARNING]
    );
}
