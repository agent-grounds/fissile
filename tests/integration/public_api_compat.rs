//! Compile-time guards for the public shapes preserved by this release.

use fissile::check::{self, BaseCheckOptions, CheckOptions, Run};
use fissile::cli::{CommandError, Loaded};
use fissile::config::ConfigError;

#[allow(dead_code)]
fn rebuild_loaded(loaded: Loaded) -> Loaded {
    let Loaded {
        config,
        source,
        checker,
        registries,
        root,
        soft_registry,
        hard_registry,
    } = loaded;
    Loaded {
        config,
        source,
        checker,
        registries,
        root,
        soft_registry,
        hard_registry,
    }
}

#[allow(dead_code)]
fn classify_config_error(error: ConfigError) -> &'static str {
    match error {
        ConfigError::Io { .. } => "io",
        ConfigError::Parse { .. } => "parse",
        ConfigError::UnsupportedVersion { .. } => "version",
        ConfigError::EmptyInclude { .. } => "include",
        ConfigError::UnknownMessage { .. } => "message",
        ConfigError::MissingMessage { .. } => "missing-message",
        ConfigError::Engine(_) => "engine",
        ConfigError::InFile { .. } => "file",
    }
}

/// The base mode is additive: `CheckOptions` and `check::run` keep their shape
/// beside `BaseCheckOptions` and `check::run_base` (§FS-004-check-audit.1.5).
#[allow(dead_code)]
fn run_both(options: CheckOptions, base: BaseCheckOptions) -> bool {
    let CheckOptions {
        root,
        config_path,
        staged,
        format,
        no_color,
        paths,
    } = options;
    let snapshot: Result<Run, CommandError> = check::run(&CheckOptions {
        root,
        config_path,
        staged,
        format,
        no_color,
        paths,
    });
    let BaseCheckOptions {
        root,
        config_path,
        base,
        format,
        no_color,
    } = base;
    let branch: Result<Run, CommandError> = check::run_base(&BaseCheckOptions {
        root,
        config_path,
        base,
        format,
        no_color,
    });
    snapshot.is_ok() && branch.is_ok()
}

#[test]
fn public_shapes_remain_exhaustive() {
    let _ = rebuild_loaded;
    let _ = classify_config_error;
    let _ = run_both;
}
