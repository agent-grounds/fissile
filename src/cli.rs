//! Shared command plumbing for `check`, `audit`, and `exception`
//! (§FS-004-check-audit, §FS-005-exception-add): load the effective config,
//! build the checker, and load+validate both exception registries.

use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use crate::Checker;
use crate::config::{
    CONFIG_HOME, Color, Config, ConfigError, ConfigSource, DEPRECATED_CONFIG_HOME,
    Format as ConfigFormat, SoftEditLimits,
};
use crate::exceptions::{ExceptionError, Registries, RegistrySource, RemovalEntry};
use crate::invocation;
use crate::report::EvalError;

/// Output format for a finding stream (§FS-001-config.6).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Format {
    #[default]
    Text,
    Json,
}

impl From<ConfigFormat> for Format {
    fn from(format: ConfigFormat) -> Self {
        match format {
            ConfigFormat::Text => Format::Text,
            ConfigFormat::Json => Format::Json,
        }
    }
}

/// Whether text output should be ANSI-colored (§FS-001-config.6). JSON is never
/// colored; `--no-color` and `color = "never"` force plain; `"auto"` colors only
/// when stdout is a terminal.
pub fn use_color(color: Color, no_color: bool, format: Format) -> bool {
    if no_color || format == Format::Json {
        return false;
    }
    match color {
        Color::Always => true,
        Color::Never => false,
        Color::Auto => std::io::stdout().is_terminal(),
    }
}

/// The effective config, checker, and registries for one command invocation.
pub struct Loaded {
    pub config: Config,
    /// Which document the config came from. Discovery decides it once; the
    /// command surface is what turns it into a line on stderr
    /// (§FS-001-config.8.2).
    pub source: ConfigSource,
    pub checker: Checker,
    pub registries: Registries,
    pub root: PathBuf,
    pub soft_registry: PathBuf,
    pub hard_registry: PathBuf,
}

/// Load and validate everything a `check`/`audit`/`exception` run needs,
/// retaining rule exclusions for wildcard validation (§FS-003-exceptions.4).
pub fn load(root: &Path, config_path: Option<&Path>) -> Result<Loaded, CommandError> {
    load_with_soft_edit_limits(root, config_path).map(|(loaded, _)| loaded)
}

pub(crate) fn load_with_soft_edit_limits(
    root: &Path,
    config_path: Option<&Path>,
) -> Result<(Loaded, SoftEditLimits), CommandError> {
    let (loaded, soft_edit_limits) = load_unvalidated_with_soft_edit_limits(root, config_path)?;
    // Validate complete scopes before measuring any files (§FS-003-exceptions.4).
    loaded.registries.validate_with_checker(&loaded.checker)?;
    Ok((loaded, soft_edit_limits))
}

/// The same load, stopping before the entries are held against the rules — the
/// check §FS-003-exceptions.4 aborts a run on. Only `exception remove` reads it:
/// the entry it deletes is what fails that check, so the one command that
/// repairs the registry has to start in the state it repairs
/// (§FS-009-exception-remove.2). Everything a command needs to *read* the
/// document still applies here — a registry that will not parse is refused.
pub fn load_unvalidated(root: &Path, config_path: Option<&Path>) -> Result<Loaded, CommandError> {
    load_unvalidated_with_soft_edit_limits(root, config_path).map(|(loaded, _)| loaded)
}

fn load_unvalidated_with_soft_edit_limits(
    root: &Path,
    config_path: Option<&Path>,
) -> Result<(Loaded, SoftEditLimits), CommandError> {
    let (config, source, soft_edit_limits) =
        Config::discover_with_soft_edit_limits(root, config_path)?;
    let checker = config.to_checker()?;

    let soft_registry = PathBuf::from(&config.exceptions.soft_registry);
    let hard_registry = PathBuf::from(&config.exceptions.hard_registry);
    let soft_text =
        read_optional(&root.join(&soft_registry)).map_err(|e| named(&soft_registry, e))?;
    let hard_text =
        read_optional(&root.join(&hard_registry)).map_err(|e| named(&hard_registry, e))?;
    // The configured path travels with the text: a registry diagnostic names the
    // file whose line the reader has to edit (§DF-005-exception-identity).
    let registries = Registries::load(
        soft_text
            .as_deref()
            .map(|text| RegistrySource::new(&config.exceptions.soft_registry, text)),
        hard_text
            .as_deref()
            .map(|text| RegistrySource::new(&config.exceptions.hard_registry, text)),
    )
    // An absent hard registry is one `Registries::load` was given no path for,
    // and a twin pointing into it still has to say which file to open
    // (§FS-003-exceptions.2.3).
    .map_err(|error| error.naming_hard_registry(&config.exceptions.hard_registry))?;

    Ok((
        Loaded {
            config,
            source,
            checker,
            registries,
            root: root.to_path_buf(),
            soft_registry,
            hard_registry,
        },
        soft_edit_limits,
    ))
}

/// Load the repair-only view for soft `exception remove`. Unlike
/// [`load_unvalidated`], this carries a missing-twin shadow separately from the
/// resolved registries so the command can address it without evaluating it
/// (§FS-009-exception-remove.2).
pub(crate) fn load_for_soft_removal(
    root: &Path,
    config_path: Option<&Path>,
) -> Result<(Loaded, Vec<RemovalEntry>), CommandError> {
    let (config, source) = Config::discover(root, config_path)?;
    let checker = config.to_checker()?;

    let soft_registry = PathBuf::from(&config.exceptions.soft_registry);
    let hard_registry = PathBuf::from(&config.exceptions.hard_registry);
    let soft_text =
        read_optional(&root.join(&soft_registry)).map_err(|e| named(&soft_registry, e))?;
    let hard_text =
        read_optional(&root.join(&hard_registry)).map_err(|e| named(&hard_registry, e))?;
    let (registries, removal_entries) = Registries::load_for_soft_removal(
        soft_text
            .as_deref()
            .map(|text| RegistrySource::new(&config.exceptions.soft_registry, text)),
        hard_text
            .as_deref()
            .map(|text| RegistrySource::new(&config.exceptions.hard_registry, text)),
    )
    .map_err(|error| error.naming_hard_registry(&config.exceptions.hard_registry))?;

    Ok((
        Loaded {
            config,
            source,
            checker,
            registries,
            root: root.to_path_buf(),
            soft_registry,
            hard_registry,
        },
        removal_entries,
    ))
}

/// What this run's stderr owes before it says anything it found: every line
/// discovery speaks, built here and nowhere else, from the source and the root
/// the run loaded (§FS-001-config.8). So every command says the same lines, in
/// the order of the points that define them. A `Vec` because every command
/// already carries its stderr notes as one, and these join them rather than
/// needing a stream of their own.
pub fn config_notes(source: &ConfigSource, root: &Path) -> Vec<String> {
    let built_in = matches!(source, ConfigSource::BuiltIn);
    discovery_lines(
        source,
        &invocation::document_prefix(root),
        invocation::passed_over(root),
        built_in.then(|| invocation::absolute(root)).as_deref(),
    )
}

/// The lines themselves, given what was looked up: `up` spells a config
/// document's path from where the caller stands (§FS-001-config.8.1), `passed`
/// is the config passed over in the invocation directory, and `root` is the
/// absolute root a run on the defaults names. An explicit `--config` is not
/// discovery and owes none of them.
fn discovery_lines(
    source: &ConfigSource,
    up: &str,
    passed: Option<&str>,
    root: Option<&Path>,
) -> Vec<String> {
    let in_effect = match source {
        ConfigSource::Explicit(_) => return Vec::new(),
        ConfigSource::Home { .. } => format!("{up}{CONFIG_HOME} is the config in effect"),
        ConfigSource::Deprecated => format!("{up}{DEPRECATED_CONFIG_HOME} is the config in effect"),
        ConfigSource::BuiltIn => "the built-in defaults are in effect".to_owned(),
    };
    let mut lines = Vec::new();
    match source {
        // §FS-001-config.8.2
        ConfigSource::Deprecated => lines.push(format!(
            "fissile: warning: {up}{DEPRECATED_CONFIG_HOME} is deprecated; move it to {up}{CONFIG_HOME}"
        )),
        // §FS-001-config.8.3
        ConfigSource::Home {
            shadows_deprecated: true,
        } => lines.push(format!(
            "fissile: warning: {up}{DEPRECATED_CONFIG_HOME} is ignored; {in_effect}"
        )),
        _ => {}
    }
    // §FS-001-config.8.4
    if let Some(passed) = passed {
        lines.push(format!(
            "fissile: warning: {passed} is ignored; discovery reads the repository root, \
             and {in_effect}"
        ));
    }
    // §FS-001-config.8.5
    if let (ConfigSource::BuiltIn, Some(root)) = (source, root) {
        lines.push(format!(
            "fissile: note: no {CONFIG_HOME} in {}; using the built-in defaults",
            root.display()
        ));
    }
    lines
}

/// The file set a caller named: git-staged, or explicit paths normalized to the
/// repo-relative spelling (§FS-004-check-audit.1). `None` means neither — `check`
/// falls back to the scan scope, `measure` to a usage error (§FS-007-measure.1).
pub fn selected_files(
    loaded: &Loaded,
    staged: bool,
    paths: &[String],
) -> Result<Option<Vec<String>>, CommandError> {
    if staged {
        return Ok(Some(crate::scan::staged_files(
            &loaded.root,
            &loaded.config.scan,
        )?));
    }
    if paths.is_empty() {
        return Ok(None);
    }
    // Caller-passed paths bypass scope/exclusion filtering, but still use the
    // repo-relative spelling consumed by rules and exceptions.
    paths
        .iter()
        .map(|path| crate::scan::normalize_repo_path(&loaded.root, path))
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
        .map_err(CommandError::Io)
}

/// Measure a selected file set — staged blobs under `--staged`, working-tree
/// bytes otherwise — collecting rather than raising the errors of paths that
/// cannot be measured (§FS-004-check-audit.5). Shared by `check` and `measure`.
pub fn measure_each(
    loaded: &Loaded,
    staged: bool,
    files: &[String],
) -> (Vec<crate::FileMeasurement>, Vec<String>) {
    let (measured, errors) = measure_each_with_context(loaded, staged, files);
    (
        measured.into_iter().map(|file| file.measurement).collect(),
        errors,
    )
}

/// The command rendering path also needs to know whether line classification
/// used UTF-8 policies or the raw-byte fallback. Keep that fact out of the
/// public measurement type and API.
pub(crate) fn measure_each_with_context(
    loaded: &Loaded,
    staged: bool,
    files: &[String],
) -> (Vec<crate::scan::MeasuredFile>, Vec<String>) {
    let mut measurements = Vec::with_capacity(files.len());
    let mut errors = Vec::new();
    for rel in files {
        let measured = if staged {
            crate::scan::measure_staged_file_with_context(&loaded.root, rel, &loaded.config.tokens)
        } else {
            crate::scan::measure_file_with_context(&loaded.root, rel, &loaded.config.tokens)
        };
        match measured {
            Ok(measurement) => measurements.push(measurement),
            Err(error) => errors.push(crate::scan::measure_error_line(rel, &error)),
        }
    }
    (measurements, errors)
}

/// Tag a registry read failure with the file it came from (§FS-004-check-audit.5).
fn named(path: &Path, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{}: {error}", path.display()))
}

/// Read a file, mapping a missing file to `None` (§FS-003-exceptions: an absent
/// registry is an empty registry).
pub fn read_optional(path: &Path) -> Result<Option<String>, io::Error> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Any failure that aborts a command before producing findings. These map to a
/// usage/schema exit code; standing findings are reported separately.
#[derive(Debug)]
pub enum CommandError {
    Config(ConfigError),
    Exceptions(ExceptionError),
    Eval(EvalError),
    Io(io::Error),
    Usage(String),
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandError::Config(error) => write!(f, "{error}"),
            CommandError::Exceptions(error) => write!(f, "{error}"),
            CommandError::Eval(error) => write!(f, "{error}"),
            CommandError::Io(error) => write!(f, "{error}"),
            CommandError::Usage(message) => write!(f, "{message}"),
        }
    }
}

impl Error for CommandError {}

impl From<ConfigError> for CommandError {
    fn from(error: ConfigError) -> Self {
        CommandError::Config(error)
    }
}

impl From<ExceptionError> for CommandError {
    fn from(error: ExceptionError) -> Self {
        CommandError::Exceptions(error)
    }
}

impl From<EvalError> for CommandError {
    fn from(error: EvalError) -> Self {
        CommandError::Eval(error)
    }
}

impl From<io::Error> for CommandError {
    fn from(error: io::Error) -> Self {
        CommandError::Io(error)
    }
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
