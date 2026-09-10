//! Compile-time guards for the public shapes preserved by this release.

use fissile::cli::Loaded;
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

#[test]
fn public_shapes_remain_exhaustive() {
    let _ = rebuild_loaded;
    let _ = classify_config_error;
}
