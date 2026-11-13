//! The `exception` command family's command line (§FS-006-cli.1): what `add`,
//! `retune` and `remove` accept, and the dispatch between them. It sits beside
//! `main.rs` rather than in it because these three subcommands carry more flags
//! than every other command together, and none of that parsing is shared with
//! them (§FS-005-exception-add, §FS-008-exception-retune, §FS-009-exception-remove).

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use fissile::exception::{self, AddOptions, Rationale};
use fissile::exceptions::{Kind, MatchKind};
use fissile::invocation::Invocation;
use fissile::remove::{self, RemoveOptions};
use fissile::retune::{self, RetuneOptions};
use fissile::{Severity, Unit};

use crate::usage::{
    EXCEPTION_ADD_USAGE, EXCEPTION_REMOVE_USAGE, EXCEPTION_RETUNE_USAGE, EXCEPTION_USAGE,
};
use crate::{usage_fail, value};

pub fn run_exception(args: &[String]) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("add") => run_exception_add(&args[1..]),
        Some("retune") => run_exception_retune(&args[1..]),
        Some("remove") => run_exception_remove(&args[1..]),
        Some("--help" | "-h") | None => {
            println!("{EXCEPTION_USAGE}");
            ExitCode::SUCCESS
        }
        Some(other) => usage_fail(
            "exception",
            &format!("unknown subcommand `{other}`"),
            EXCEPTION_USAGE,
        ),
    }
}

fn run_exception_add(args: &[String]) -> ExitCode {
    let mut builder = AddBuilder::default();
    let mut iter = args.iter();

    while let Some(arg) = iter.next() {
        let result = match arg.as_str() {
            "--help" | "-h" => {
                println!("{EXCEPTION_ADD_USAGE}");
                return ExitCode::SUCCESS;
            }
            "--dry-run" => {
                builder.dry_run = true;
                Ok(())
            }
            "--force" => {
                builder.force = true;
                Ok(())
            }
            "--shadows-hard" => {
                builder.shadows_hard = true;
                Ok(())
            }
            "--rule" => value(&mut iter, "--rule").map(|v| builder.rules.push(v)),
            "--severity" => value(&mut iter, "--severity").and_then(|v| builder.set_severity(&v)),
            "--kind" => value(&mut iter, "--kind").and_then(|v| builder.set_kind(&v)),
            "--reason" => value(&mut iter, "--reason").map(|v| builder.reason = Some(v)),
            "--until" => value(&mut iter, "--until").map(|v| builder.until = Some(v)),
            "--config" => value(&mut iter, "--config").map(|v| builder.config = Some(v)),
            "--match" => value(&mut iter, "--match").and_then(|v| builder.set_match(&v)),
            "--title" => value(&mut iter, "--title").map(|v| builder.title = Some(v)),
            "--owner" => value(&mut iter, "--owner").map(|v| builder.owner = Some(v)),
            "--issue" => value(&mut iter, "--issue").map(|v| builder.issue = Some(v)),
            "--max" => value(&mut iter, "--max")
                .and_then(|v| parse_max(&v))
                .map(|v| builder.max = Some(v)),
            "--unit" => value(&mut iter, "--unit").and_then(|v| builder.set_unit(&v)),
            other if other.starts_with('-') => Err(format!("unknown option `{other}`")),
            other => builder.set_path(other),
        };
        if let Err(message) = result {
            return usage_fail("exception add", &message, EXCEPTION_ADD_USAGE);
        }
    }

    // A person adding an exception is at a terminal; an agent, a hook, and CI
    // are not (§DF-008-hard-severity-needs-a-terminal.1).
    let options = match builder.build(std::io::stdin().is_terminal(), &Invocation::resolve()) {
        Ok(options) => options,
        Err(message) => return usage_fail("exception add", &message, EXCEPTION_ADD_USAGE),
    };
    match exception::run(&options) {
        Ok(run) => {
            // Discovery's note is already a whole line addressed to the reader,
            // so it is not folded under this command's prefix (§FS-001-config.8.2).
            for note in &run.notes {
                eprintln!("{note}");
            }
            for warning in &run.warnings {
                eprintln!("fissile exception add: warning: {warning}");
            }
            println!("{}", run.output);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("fissile exception add: {error}");
            ExitCode::from(2)
        }
    }
}

fn parse_severity(raw: &str) -> Result<Severity, String> {
    match raw {
        "soft" => Ok(Severity::Soft),
        "hard" => Ok(Severity::Hard),
        other => Err(format!("unknown severity `{other}`")),
    }
}

fn parse_match(raw: &str) -> Result<MatchKind, String> {
    match raw {
        "exact" => Ok(MatchKind::Exact),
        "glob" => Ok(MatchKind::Glob),
        other => Err(format!("unknown match `{other}`")),
    }
}

fn parse_unit(raw: &str) -> Result<Unit, String> {
    match raw {
        "bytes" => Ok(Unit::Bytes),
        "lines" => Ok(Unit::Lines),
        "tokens" => Ok(Unit::Tokens),
        other => Err(format!("unknown unit `{other}`")),
    }
}

fn parse_max(raw: &str) -> Result<u64, String> {
    raw.parse()
        .map_err(|_| format!("--max `{raw}` is not an integer"))
}

/// The flags that address one existing entry — `<path>`, `--severity`,
/// `--rule`, `--match`, `--config`, `--dry-run` — parsed once so `retune` and
/// `remove` cannot drift on what names an entry (§DF-005-exception-identity).
/// `--max`/`--unit` belong to the one command that states a ceiling.
struct AddressArgs {
    path: Option<String>,
    severity: Option<Severity>,
    rules: Vec<String>,
    match_kind: MatchKind,
    config_path: Option<PathBuf>,
    dry_run: bool,
    max: Option<u64>,
    unit: Option<Unit>,
}

impl Default for AddressArgs {
    fn default() -> Self {
        Self {
            path: None,
            severity: None,
            rules: Vec::new(),
            // The same default `add` applies: a path with no metacharacter is
            // one file (§FS-005-exception-add.1).
            match_kind: MatchKind::Exact,
            config_path: None,
            dry_run: false,
            max: None,
            unit: None,
        }
    }
}

/// `Ok(None)` means `--help` was handled and the command is done. `sizing` says
/// whether this subcommand takes `--max`/`--unit`; where it does not, they are
/// unknown options rather than silently accepted ones.
fn parse_address(
    command: &str,
    usage: &str,
    args: &[String],
    sizing: bool,
) -> Result<Option<AddressArgs>, ExitCode> {
    let mut parsed = AddressArgs::default();
    let mut iter = args.iter();

    while let Some(arg) = iter.next() {
        let result =
            match arg.as_str() {
                "--help" | "-h" => {
                    println!("{usage}");
                    return Ok(None);
                }
                "--dry-run" => {
                    parsed.dry_run = true;
                    Ok(())
                }
                "--rule" => value(&mut iter, "--rule").map(|v| parsed.rules.push(v)),
                "--severity" => value(&mut iter, "--severity")
                    .and_then(|v| parse_severity(&v).map(|v| parsed.severity = Some(v))),
                "--match" => value(&mut iter, "--match")
                    .and_then(|v| parse_match(&v).map(|v| parsed.match_kind = v)),
                "--unit" if sizing => value(&mut iter, "--unit")
                    .and_then(|v| parse_unit(&v).map(|v| parsed.unit = Some(v))),
                "--max" if sizing => value(&mut iter, "--max")
                    .and_then(|v| parse_max(&v).map(|v| parsed.max = Some(v))),
                "--config" => value(&mut iter, "--config")
                    .map(|v| parsed.config_path = Some(PathBuf::from(v))),
                other if other.starts_with('-') => Err(format!("unknown option `{other}`")),
                other if parsed.path.is_some() => {
                    Err(format!("only one <path> is allowed, got `{other}`"))
                }
                other => {
                    parsed.path = Some(other.to_owned());
                    Ok(())
                }
            };
        if let Err(message) = result {
            return Err(usage_fail(command, &message, usage));
        }
    }
    Ok(Some(parsed))
}

/// The two fields an address cannot default: which registry holds the entry, and
/// which entry it is.
fn require_address(
    command: &str,
    usage: &str,
    parsed: &AddressArgs,
) -> Result<(String, Severity), ExitCode> {
    let Some(path) = parsed.path.clone() else {
        return Err(usage_fail(command, "a <path> is required", usage));
    };
    let Some(severity) = parsed.severity else {
        return Err(usage_fail(
            command,
            "--severity is required: it names the registry holding the entry",
            usage,
        ));
    };
    Ok((path, severity))
}

/// `fissile exception retune` (§FS-008-exception-retune): the flags that address
/// an entry, plus the value its ceiling should move to.
fn run_exception_retune(args: &[String]) -> ExitCode {
    let parsed = match parse_address("exception retune", EXCEPTION_RETUNE_USAGE, args, true) {
        Ok(Some(parsed)) => parsed,
        Ok(None) => return ExitCode::SUCCESS,
        Err(code) => return code,
    };
    let (path, severity) =
        match require_address("exception retune", EXCEPTION_RETUNE_USAGE, &parsed) {
            Ok(address) => address,
            Err(code) => return code,
        };
    // The root's registry, from whichever directory (§FS-001-config.5).
    let invocation = Invocation::resolve();
    let options = RetuneOptions {
        root: invocation.root().to_path_buf(),
        config_path: invocation.config_path(parsed.config_path),
        path,
        severity,
        rules: parsed.rules,
        match_kind: parsed.match_kind,
        max: parsed.max,
        unit: parsed.unit,
        dry_run: parsed.dry_run,
    };

    match retune::run(&options) {
        Ok(run) => {
            for note in &run.notes {
                eprintln!("{note}");
            }
            println!("{}", run.output);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("fissile exception retune: {error}");
            ExitCode::from(2)
        }
    }
}

/// `fissile exception remove` (§FS-009-exception-remove): the same address, and
/// no ceiling — the command states no number.
fn run_exception_remove(args: &[String]) -> ExitCode {
    let parsed = match parse_address("exception remove", EXCEPTION_REMOVE_USAGE, args, false) {
        Ok(Some(parsed)) => parsed,
        Ok(None) => return ExitCode::SUCCESS,
        Err(code) => return code,
    };
    let (path, severity) =
        match require_address("exception remove", EXCEPTION_REMOVE_USAGE, &parsed) {
            Ok(address) => address,
            Err(code) => return code,
        };
    let invocation = Invocation::resolve();
    let options = RemoveOptions {
        root: invocation.root().to_path_buf(),
        config_path: invocation.config_path(parsed.config_path),
        path,
        severity,
        rules: parsed.rules,
        match_kind: parsed.match_kind,
        dry_run: parsed.dry_run,
    };

    match remove::run(&options) {
        Ok(run) => {
            for note in &run.notes {
                eprintln!("{note}");
            }
            println!("{}", run.output);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("fissile exception remove: {error}");
            ExitCode::from(2)
        }
    }
}

/// Accumulates `exception add` flags before they are validated into [`AddOptions`].
#[derive(Default)]
struct AddBuilder {
    path: Option<String>,
    severity: Option<Severity>,
    kind: Option<Kind>,
    rules: Vec<String>,
    reason: Option<String>,
    until: Option<String>,
    match_kind: Option<MatchKind>,
    title: Option<String>,
    owner: Option<String>,
    issue: Option<String>,
    max: Option<u64>,
    unit: Option<Unit>,
    config: Option<String>,
    shadows_hard: bool,
    force: bool,
    dry_run: bool,
}

impl AddBuilder {
    fn set_path(&mut self, path: &str) -> Result<(), String> {
        if self.path.is_some() {
            return Err("only one <path> is allowed".to_owned());
        }
        self.path = Some(path.to_owned());
        Ok(())
    }

    fn set_severity(&mut self, raw: &str) -> Result<(), String> {
        self.severity = Some(parse_severity(raw)?);
        Ok(())
    }

    fn set_kind(&mut self, raw: &str) -> Result<(), String> {
        self.kind = Some(match raw {
            "structural" => Kind::Structural,
            "deferred" => Kind::Deferred,
            other => return Err(format!("unknown kind `{other}`")),
        });
        Ok(())
    }

    fn set_match(&mut self, raw: &str) -> Result<(), String> {
        self.match_kind = Some(parse_match(raw)?);
        Ok(())
    }

    fn set_unit(&mut self, raw: &str) -> Result<(), String> {
        self.unit = Some(parse_unit(raw)?);
        Ok(())
    }

    /// `interactive` is passed in rather than probed: this turns parsed flags
    /// into options and nothing else, so what it returns does not depend on how
    /// the process was launched (§DF-008-hard-severity-needs-a-terminal.1). The
    /// invocation is passed in for the same reason (§FS-001-config.8.1).
    fn build(self, interactive: bool, invocation: &Invocation) -> Result<AddOptions, String> {
        let severity = self.severity.ok_or("--severity is required")?;
        let rationale = self.rationale(severity)?;
        Ok(AddOptions {
            root: invocation.root().to_path_buf(),
            config_path: invocation.config_path(self.config.map(PathBuf::from)),
            path: self.path.ok_or("a <path> is required")?,
            severity,
            rules: self.rules,
            rationale,
            match_kind: self.match_kind.unwrap_or(MatchKind::Exact),
            title: self.title,
            owner: self.owner,
            issue: self.issue,
            max: self.max,
            unit: self.unit,
            interactive,
            force: self.force,
            dry_run: self.dry_run,
        })
    }
}

impl AddBuilder {
    /// What the entry will claim: the caller's own three flags, or the pointer
    /// that replaces all three (§FS-005-exception-add.1.1). A shadowing entry
    /// forbids them rather than ignoring them — a second copy of a rationale is
    /// exactly what the flag exists to remove (§FS-003-exceptions.2.3).
    fn rationale(&self, severity: Severity) -> Result<Rationale, String> {
        if !self.shadows_hard {
            return Ok(Rationale::Stated {
                // Required rather than defaulted: which of the two claims the
                // entry makes is the author's call, and the error is the moment
                // to say what each one means (§DF-004-exception-kind.1).
                kind: self.kind.ok_or(
                    "--kind is required: structural = an architectural constraint makes the \
                     split illegal; deferred = the boundary is simply missing and someone has \
                     to build it",
                )?,
                reason: self.reason.clone().ok_or("--reason is required")?,
                until: self.until.clone(),
            });
        }
        if severity != Severity::Soft {
            return Err(
                "--shadows-hard writes the soft twin of a hard entry, so it takes \
                 --severity soft; the hard entry is the one that carries the rationale"
                    .to_owned(),
            );
        }
        for (flag, given) in [
            ("--kind", self.kind.is_some()),
            ("--reason", self.reason.is_some()),
            ("--until", self.until.is_some()),
        ] {
            if given {
                return Err(format!(
                    "--shadows-hard takes the hard entry's kind, reason, and until, so \
                     {flag} has nothing to add here — drop {flag}, or drop --shadows-hard \
                     and state all three"
                ));
            }
        }
        Ok(Rationale::ShadowsHard)
    }
}
