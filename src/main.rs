//! `fissile` CLI entry point (§FS-006-cli): hand-rolled parsing, tiny
//! dependency tree (§GOAL-002-tiny-footprint, §DA-003-single-static-binary);
//! dispatches the commands (§FS-002-init, §FS-004-check-audit, §FS-005-exception-add).

use std::path::PathBuf;
use std::process::ExitCode;
use std::slice::Iter;

use fissile::audit::{self, AuditOptions, Section};
use fissile::check::{self, CheckOptions};
use fissile::cli::Format;
use fissile::init::{self, AgentTargets, HookMode, InitOptions};
use fissile::limits::{self, LimitsOptions};
use fissile::measure::{self, MeasureOptions};

mod exception_cli;
mod usage;

use usage::{AUDIT_USAGE, CHECK_USAGE, INIT_USAGE, LIMITS_USAGE, MEASURE_USAGE, USAGE};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("init") => run_init(&args[1..]),
        Some("check") => run_check(&args[1..]),
        Some("measure") => run_measure(&args[1..]),
        Some("audit") => run_audit(&args[1..]),
        Some("limits") => run_limits(&args[1..]),
        Some("exception") => exception_cli::run_exception(&args[1..]),
        Some("--help" | "-h") | None => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        // One stable line, no banner (§FS-006-cli.3).
        Some("--version" | "-V") => {
            println!("fissile {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("fissile: unknown command `{other}`\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Pull the value of a flag that takes an argument, or report a usage error.
fn value(iter: &mut Iter<String>, flag: &str) -> Result<String, String> {
    iter.next()
        .cloned()
        .ok_or_else(|| format!("{flag} requires a value"))
}

fn parse_format(raw: &str) -> Result<Format, String> {
    match raw {
        "text" => Ok(Format::Text),
        "json" => Ok(Format::Json),
        other => Err(format!("unknown format `{other}` (expected text or json)")),
    }
}

fn run_init(args: &[String]) -> ExitCode {
    let mut options = InitOptions::new(".");
    let mut agents = AgentTargets::default();
    let mut iter = args.iter();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("{INIT_USAGE}");
                return ExitCode::SUCCESS;
            }
            "--name" => match value(&mut iter, "--name") {
                Ok(name) => options.name = Some(name),
                Err(message) => return usage_fail("init", &message, INIT_USAGE),
            },
            "--config" => match value(&mut iter, "--config") {
                Ok(path) => options.config_path = PathBuf::from(path),
                Err(message) => return usage_fail("init", &message, INIT_USAGE),
            },
            "--exceptions" => options.exceptions = true,
            "--hook" => {
                if options.hook == HookMode::Never {
                    return usage_fail("init", "--hook conflicts with --no-hook", INIT_USAGE);
                }
                options.hook = HookMode::Always;
            }
            "--no-hook" => {
                if options.hook == HookMode::Always {
                    return usage_fail("init", "--no-hook conflicts with --hook", INIT_USAGE);
                }
                options.hook = HookMode::Never;
            }
            "--force" => options.force = true,
            "--dry-run" => options.dry_run = true,
            "--agents-md" => agents.agents_md = true,
            "--claude" => agents.claude = true,
            "--gemini" => agents.gemini = true,
            "--copilot" => agents.copilot = true,
            "--cursor" => agents.cursor = true,
            "--windsurf" => agents.windsurf = true,
            "--zed" => agents.zed = true,
            other if other.starts_with('-') => {
                return usage_fail("init", &format!("unknown option `{other}`"), INIT_USAGE);
            }
            other => options.root = PathBuf::from(other),
        }
    }

    options.agents = agents;
    let dry_run = options.dry_run;
    match init::run(&options) {
        Ok(report) => {
            // `init` does not discover, so the deprecation is its own to say: a
            // repository whose config it found at the old home hears it from
            // nowhere else (§FS-001-config.8.2, §FS-002-init.2).
            if let Some(warning) = report.deprecation {
                eprintln!("{warning}");
            }
            eprintln!("{}", report.render());
            // A dry run is the one way to read the instructions without writing
            // a file, so it prints them — on stdout, keeping the planned writes
            // on stderr separable (§FS-002-init.4).
            if dry_run {
                println!("{}", init::MANAGED_BLOCK.trim_end());
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("fissile init: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The flags `check` and `measure` share: one file-set selection and one set of
/// output controls, parsed once so the two commands cannot drift on what
/// `--staged` or `--format` mean (§FS-007-measure.1, §FS-006-cli.1).
#[derive(Default)]
struct FileSetArgs {
    config_path: Option<PathBuf>,
    staged: bool,
    format: Option<Format>,
    no_color: bool,
    paths: Vec<String>,
}

/// `Ok(None)` means `--help` was handled and the command is done.
fn parse_file_set(
    command: &str,
    usage: &str,
    args: &[String],
) -> Result<Option<FileSetArgs>, ExitCode> {
    let mut parsed = FileSetArgs::default();
    let mut iter = args.iter();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("{usage}");
                return Ok(None);
            }
            "--staged" => parsed.staged = true,
            "--no-color" => parsed.no_color = true,
            "--config" => match value(&mut iter, "--config") {
                Ok(path) => parsed.config_path = Some(PathBuf::from(path)),
                Err(message) => return Err(usage_fail(command, &message, usage)),
            },
            "--format" => match value(&mut iter, "--format").and_then(|raw| parse_format(&raw)) {
                Ok(format) => parsed.format = Some(format),
                Err(message) => return Err(usage_fail(command, &message, usage)),
            },
            other if other.starts_with('-') => {
                return Err(usage_fail(
                    command,
                    &format!("unknown option `{other}`"),
                    usage,
                ));
            }
            other => parsed.paths.push(other.to_owned()),
        }
    }
    Ok(Some(parsed))
}

fn run_check(args: &[String]) -> ExitCode {
    let parsed = match parse_file_set("check", CHECK_USAGE, args) {
        Ok(Some(parsed)) => parsed,
        Ok(None) => return ExitCode::SUCCESS,
        Err(code) => return code,
    };
    let options = CheckOptions {
        root: PathBuf::from("."),
        config_path: parsed.config_path,
        staged: parsed.staged,
        format: parsed.format,
        no_color: parsed.no_color,
        paths: parsed.paths,
    };

    match check::run(&options) {
        // Under `--format json` stdout is holding a machine shape, so the run's
        // account of a registry it can no longer read comes out on stderr rather
        // than being lost (§FS-004-check-audit.5).
        Ok(run) => finish_run("check", &run.output, run.failed, &run.notes, &run.errors),
        Err(error) => {
            eprintln!("fissile check: {error}");
            ExitCode::from(2)
        }
    }
}

fn run_measure(args: &[String]) -> ExitCode {
    let parsed = match parse_file_set("measure", MEASURE_USAGE, args) {
        Ok(Some(parsed)) => parsed,
        Ok(None) => return ExitCode::SUCCESS,
        Err(code) => return code,
    };
    let options = MeasureOptions {
        root: PathBuf::from("."),
        config_path: parsed.config_path,
        staged: parsed.staged,
        format: parsed.format,
        no_color: parsed.no_color,
        paths: parsed.paths,
    };

    match measure::run(&options) {
        // Never `failed`: measuring is inspection, and a file over a hard limit
        // is an answer rather than a verdict (§FS-007-measure.1).
        Ok(run) => finish_run("measure", &run.output, false, &run.notes, &run.errors),
        Err(error) => {
            eprintln!("fissile measure: {error}");
            ExitCode::from(2)
        }
    }
}

/// Print a run's findings, its notes and its file-level errors, then map them to
/// the exit code: errors exit 2 even without a standing hard finding, because
/// silently passing an unmeasurable file would make the gate unsound
/// (§FS-004-check-audit.5). Notes change no exit code — a deprecated config path
/// is said, never charged (§FS-001-config.8.2).
fn finish_run(
    command: &str,
    output: &str,
    failed: bool,
    notes: &[String],
    errors: &[String],
) -> ExitCode {
    for note in notes {
        eprintln!("{note}");
    }
    if !output.is_empty() {
        println!("{output}");
    }
    for error in errors {
        eprintln!("fissile {command}: {error}");
    }
    if !errors.is_empty() {
        ExitCode::from(2)
    } else if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn run_audit(args: &[String]) -> ExitCode {
    let mut options = AuditOptions {
        root: PathBuf::from("."),
        ..AuditOptions::default()
    };
    let mut iter = args.iter();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("{AUDIT_USAGE}");
                return ExitCode::SUCCESS;
            }
            "--no-color" => options.no_color = true,
            "--stale-exceptions" => options.stale_exceptions = true,
            "--rule-coverage" => options.rule_coverage = true,
            "--history" => match value(&mut iter, "--history") {
                Ok(range) => options.history = Some(range),
                Err(message) => return usage_fail("audit", &message, AUDIT_USAGE),
            },
            "--config" => match value(&mut iter, "--config") {
                Ok(path) => options.config_path = Some(PathBuf::from(path)),
                Err(message) => return usage_fail("audit", &message, AUDIT_USAGE),
            },
            "--format" => match value(&mut iter, "--format").and_then(|raw| parse_format(&raw)) {
                Ok(format) => options.format = Some(format),
                Err(message) => return usage_fail("audit", &message, AUDIT_USAGE),
            },
            "--top" => match value(&mut iter, "--top").and_then(parse_count) {
                Ok(count) => options.top = Some(count),
                Err(message) => return usage_fail("audit", &message, AUDIT_USAGE),
            },
            // A set, so a repeated name and a second `--only` both add
            // (§FS-004-check-audit.2).
            "--only" => match value(&mut iter, "--only").and_then(|raw| options.select(&raw)) {
                Ok(()) => (),
                Err(message) => return usage_fail("audit", &message, AUDIT_USAGE),
            },
            other if other.starts_with('-') => {
                return usage_fail("audit", &format!("unknown option `{other}`"), AUDIT_USAGE);
            }
            other => {
                return usage_fail(
                    "audit",
                    &format!("unexpected argument `{other}`"),
                    AUDIT_USAGE,
                );
            }
        }
    }

    // The two things `--only` cannot mean, refused before any work is done
    // (§FS-004-check-audit.2). `top` is the one section whose computation takes
    // a parameter, so naming it does not supply the count.
    if options.selects(Section::Top) && options.top.is_none() {
        return usage_fail("audit", "--only top requires --top <N>", AUDIT_USAGE);
    }
    if options.selects(Section::History) && options.history.is_none() {
        return usage_fail(
            "audit",
            "--only history requires --history <from>..<to>",
            AUDIT_USAGE,
        );
    }
    if options.only.is_some() && options.format == Some(Format::Json) {
        return usage_fail(
            "audit",
            "--only selects sections of the text report and is not valid with --format json",
            AUDIT_USAGE,
        );
    }

    match audit::run(&options) {
        Ok(run) => finish_run("audit", &run.output, run.failed, &run.notes, &run.errors),
        Err(error) => {
            eprintln!("fissile audit: {error}");
            ExitCode::from(2)
        }
    }
}

/// `fissile limits` (§FS-010-limits.1): no positional argument and no
/// `--staged` — the answer does not depend on a file set, so an argument is a
/// usage error rather than a filter.
fn run_limits(args: &[String]) -> ExitCode {
    let mut options = LimitsOptions {
        root: PathBuf::from("."),
        ..LimitsOptions::default()
    };
    let mut iter = args.iter();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("{LIMITS_USAGE}");
                return ExitCode::SUCCESS;
            }
            "--no-color" => options.no_color = true,
            "--config" => match value(&mut iter, "--config") {
                Ok(path) => options.config_path = Some(PathBuf::from(path)),
                Err(message) => return usage_fail("limits", &message, LIMITS_USAGE),
            },
            "--format" => match value(&mut iter, "--format").and_then(|raw| parse_format(&raw)) {
                Ok(format) => options.format = Some(format),
                Err(message) => return usage_fail("limits", &message, LIMITS_USAGE),
            },
            other if other.starts_with('-') => {
                return usage_fail("limits", &format!("unknown option `{other}`"), LIMITS_USAGE);
            }
            other => {
                return usage_fail(
                    "limits",
                    &format!("unexpected argument `{other}`"),
                    LIMITS_USAGE,
                );
            }
        }
    }

    match limits::run(&options) {
        // Never a gate: the inventory is an answer, and the config loading is
        // the whole of what can go wrong (§FS-010-limits.1).
        Ok(run) => {
            for note in &run.notes {
                eprintln!("{note}");
            }
            println!("{}", run.output);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("fissile limits: {error}");
            ExitCode::from(2)
        }
    }
}

fn parse_count(raw: String) -> Result<usize, String> {
    raw.parse()
        .map_err(|_| format!("`{raw}` is not a non-negative integer"))
}

fn usage_fail(command: &str, message: &str, usage: &str) -> ExitCode {
    eprintln!("fissile {command}: {message}\n\n{usage}");
    ExitCode::from(2)
}
