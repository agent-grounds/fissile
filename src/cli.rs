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
    Color, Config, ConfigError, ConfigSource, Format as ConfigFormat, SoftEditLimits,
};
use crate::exceptions::{ExceptionError, Registries, RegistrySource, RemovalEntry};
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
    pub(crate) soft_edit_limits: SoftEditLimits,
    pub registries: Registries,
    pub root: PathBuf,
    pub soft_registry: PathBuf,
    pub hard_registry: PathBuf,
}

/// Load and validate everything a `check`/`audit`/`exception` run needs.
pub fn load(root: &Path, config_path: Option<&Path>) -> Result<Loaded, CommandError> {
    let loaded = load_unvalidated(root, config_path)?;
    loaded.registries.validate_against(loaded.checker.rules())?;
    Ok(loaded)
}

/// The same load, stopping before the entries are held against the rules — the
/// check §FS-003-exceptions.4 aborts a run on. Only `exception remove` reads it:
/// the entry it deletes is what fails that check, so the one command that
/// repairs the registry has to start in the state it repairs
/// (§FS-009-exception-remove.2). Everything a command needs to *read* the
/// document still applies here — a registry that will not parse is refused.
pub fn load_unvalidated(root: &Path, config_path: Option<&Path>) -> Result<Loaded, CommandError> {
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

    Ok(Loaded {
        config,
        source,
        checker,
        soft_edit_limits,
        registries,
        root: root.to_path_buf(),
        soft_registry,
        hard_registry,
    })
}

/// Load the repair-only view for soft `exception remove`. Unlike
/// [`load_unvalidated`], this carries a missing-twin shadow separately from the
/// resolved registries so the command can address it without evaluating it
/// (§FS-009-exception-remove.2).
pub(crate) fn load_for_soft_removal(
    root: &Path,
    config_path: Option<&Path>,
) -> Result<(Loaded, Vec<RemovalEntry>), CommandError> {
    let (config, source, soft_edit_limits) =
        Config::discover_with_soft_edit_limits(root, config_path)?;
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
            soft_edit_limits,
            registries,
            root: root.to_path_buf(),
            soft_registry,
            hard_registry,
        },
        removal_entries,
    ))
}

/// What this run's stderr owes before it says anything it found: the one line
/// discovery leaves when the config came from the deprecated home, or passed one
/// over (§FS-001-config.8.2, §FS-001-config.8.3). A `Vec` because every command
/// already carries its stderr notes as one, and the deprecation joins them
/// rather than needing a stream of its own.
pub fn config_notes(source: &ConfigSource) -> Vec<String> {
    source
        .deprecation()
        .map(str::to_owned)
        .into_iter()
        .collect()
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
