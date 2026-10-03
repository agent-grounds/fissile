//! `fissile exception retune` (§FS-008-exception-retune): move the ceiling of an
//! entry that already exists. It locates the entry, settles the new value, and
//! rewrites that one `max_accepted` line — leaving the rest of the registry alone.

use std::fs;
use std::path::PathBuf;

use crate::cli::{self, CommandError, Loaded};
use crate::entry::{self, Address, Sizing};
use crate::exception::shell_quote;
use crate::exceptions::{Exception, MatchKind};
use crate::toml_lines;
use crate::{Severity, Unit};

/// Inputs to `exception retune`.
#[derive(Clone, Debug)]
pub struct RetuneOptions {
    pub root: PathBuf,
    pub config_path: Option<PathBuf>,
    pub path: String,
    pub severity: Severity,
    pub rules: Vec<String>,
    pub match_kind: MatchKind,
    pub max: Option<u64>,
    pub unit: Option<Unit>,
    pub dry_run: bool,
}

#[derive(Clone, Debug)]
pub struct Run {
    pub output: String,
    /// What discovery owes the reader: the deprecated config home this run was
    /// governed by (§FS-001-config.8.2).
    pub notes: Vec<String>,
}

pub fn run(options: &RetuneOptions) -> Result<Run, CommandError> {
    let loaded = cli::load(&options.root, options.config_path.as_deref())?;
    let path = entry::match_path(&loaded, options.match_kind, &options.path)?;

    entry::validate_match(options.match_kind, &path)?;
    let rules = entry::resolve_rules(&loaded, &options.rules)?;
    let unit = rules[0].budget.unit;
    let registry_rel = entry::registry_path(&loaded, options.severity);

    let address = Address {
        severity: options.severity,
        path: &path,
        match_kind: options.match_kind,
        rules: &options.rules,
        unit,
    };
    // Retune edits; it never creates. An address with no entry behind it is a
    // call for the other command (§FS-008-exception-retune.1).
    let Some((index, existing)) = entry::locate(&loaded.registries, &address)? else {
        return Err(CommandError::Usage(format!(
            "{}: no entry accepts {path} for this rule and unit; \
             create one with `fissile exception add`",
            registry_rel.display()
        )));
    };
    // An address is a matcher, not just a path (§DF-005-exception-identity), so a
    // matcher that only *overlaps* the entry is the wrong address and each
    // direction is wrong for its own reason.
    match (existing.match_kind, options.match_kind) {
        // A glob's ceiling covers every file in its class, so deriving it from
        // one member's measurement could lower it under the others. The caller
        // has to address the glob and state the number (§FS-003-exceptions.7).
        (MatchKind::Glob, MatchKind::Exact) => {
            return Err(CommandError::Usage(format!(
                "{}: {path} is covered by the glob entry {}; retune it as \
                 `--match glob \"{}\" --max <N> --unit {unit}`, since one file's size \
                 cannot set a ceiling for the class",
                registry_rel.display(),
                existing.path,
                existing.path
            )));
        }
        // The reverse writes a class-wide number into a single file's entry: the
        // other files the glob names keep their old ceilings, and the result is
        // reported under a path no entry in the registry carries.
        (MatchKind::Exact, MatchKind::Glob) => {
            return Err(CommandError::Usage(format!(
                "{}: no glob entry accepts {path}; it spans the exact entry {}, which is \
                 retuned as `--match exact {}` — or `fissile exception add` creates the \
                 class-wide entry",
                registry_rel.display(),
                existing.path,
                existing.path
            )));
        }
        _ => {}
    }
    let recorded = existing.max_value;

    let sizing = Sizing {
        path: &path,
        match_kind: options.match_kind,
        max: options.max,
        unit: options.unit,
    };
    let base = entry::resolve_base(sizing, &loaded, unit, rules[0])?;
    // `audit --stale-exceptions` calls this state "silences nothing" and names
    // removal as the remedy, so the refusal names the same (§FS-003-exceptions.7)
    // — and the command that performs it (§FS-009-exception-remove).
    entry::check_min_limit(
        &rules,
        options.severity,
        unit,
        &base,
        &format!(
            "remove the entry rather than retuning it:\n  {}",
            remove_route(options, &path)
        ),
    )?;
    let step = loaded.config.exceptions.bump.step(unit);
    let ceiling = entry::ceiling(&base, step);
    let twin = twin(&loaded, options, &path, unit);
    // A soft ceiling on the hard limit is refused, and the refusal carries the
    // stated form that succeeds (§FS-008-exception-retune.4). The exemption is
    // the one `add` applies, so the two commands never disagree about an entry.
    let binding = entry::binding_hard_limit(
        &rules,
        options.severity,
        entry::has_deferred_hard_twin(
            &loaded.registries,
            &path,
            options.match_kind,
            &options.rules,
            unit,
        ),
    );
    entry::check_hard_limit(
        binding,
        &path,
        unit,
        &base,
        ceiling,
        step,
        &routes(options, &path, unit, ceiling),
    )?;
    let suggested = entry::suggested_step(&base, step, binding.map(|binding| binding.hard));
    let detail = ceiling_detail(&base, ceiling, step, unit, suggested);

    // A caller about to leave two registries disagreeing should learn it here
    // rather than from a later run (§FS-008-exception-retune.3).
    let note = twin_note(twin, unit, ceiling);

    if ceiling == recorded {
        // An edit that stayed inside the step is a normal outcome, not a failure.
        return Ok(Run {
            output: with_note(
                format!(
                    "{}: {path} already accepts {recorded} {unit}{detail}",
                    registry_rel.display(),
                ),
                note,
            ),
            notes: cli::config_notes(&loaded.source),
        });
    }

    let registry_path = loaded.root.join(&registry_rel);
    let text = cli::read_optional(&registry_path)?
        .ok_or_else(|| CommandError::Usage(format!("{} is missing", registry_rel.display())))?;
    let new_text = rewrite_ceiling(&text, index, ceiling, unit, &registry_rel, &path)?;
    entry::validate_combined(&loaded, options.severity, &new_text)?;

    let change = format!(
        "{}: {path} {recorded} -> {ceiling} {unit}{detail}",
        registry_rel.display(),
    );
    if options.dry_run {
        return Ok(Run {
            output: with_note(
                format!("{change}\nwould update {}", registry_rel.display()),
                note,
            ),
            notes: cli::config_notes(&loaded.source),
        });
    }

    fs::write(&registry_path, &new_text)?;
    Ok(Run {
        output: with_note(change, note),
        notes: cli::config_notes(&loaded.source),
    })
}

/// Explain the arithmetic between the value supplied or measured and the
/// ceiling written (§FS-008-exception-retune.3): a quantized measurement names
/// the measurement and step; a stated value names the step's next multiple.
fn ceiling_detail(
    base: &entry::Base<'_>,
    ceiling: u64,
    step: u64,
    unit: Unit,
    suggested: Option<u64>,
) -> String {
    match base.source {
        entry::BaseSource::Measured(_) if ceiling != base.value => format!(
            " (measured {} {unit}; quantized to {step}-{} step)",
            base.value,
            unit.singular()
        ),
        entry::BaseSource::Measured(_) => String::new(),
        entry::BaseSource::Max => entry::step_note(step, unit, suggested)
            .map(|note| format!(" ({note})"))
            .unwrap_or_default(),
    }
}

/// The commands a hard-limit refusal offers (§FS-008-exception-retune.4): this
/// address with `--max <N> --unit <unit>`, and the hard-severity `add` carrying
/// the ceiling, so the printed route runs as printed.
//
// Without --max the rerun would measure the file, find it under the hard limit,
// and be refused for needing no exception. The kind is the caller's claim to
// make, so both spellings are named rather than one chosen, since only the
// deferred kind takes --until.
fn routes(options: &RetuneOptions, path: &str, unit: Unit, ceiling: u64) -> entry::Routes {
    let path = shell_quote(path);
    let flags = address_flags(options);
    entry::Routes {
        stated: format!(
            "fissile exception retune {path} --severity soft{flags} --max <N> --unit {unit}"
        ),
        hard: Some(format!(
            "fissile exception add {path} --severity hard{flags} --max {ceiling} --unit {unit} \
             --kind structural --reason \"...\"\n  (or --kind deferred --until \
             '<what retires it>')"
        )),
    }
}

/// The `--config`, `--rule` and `--match` flags of the caller's own address, so
/// an offered command addresses the entry the caller just addressed.
fn address_flags(options: &RetuneOptions) -> String {
    let mut flags = String::new();
    if let Some(config) = &options.config_path {
        flags.push_str(&format!(
            " --config {}",
            shell_quote(&config.to_string_lossy())
        ));
    }
    for rule in &options.rules {
        flags.push_str(&format!(" --rule {}", shell_quote(rule)));
    }
    if options.match_kind == MatchKind::Glob {
        flags.push_str(" --match glob");
    }
    flags
}

/// The `exception remove` call for this address (§FS-009-exception-remove.1),
/// offered where a ceiling has fallen under the limit it exists to accept and
/// there is nothing left to retune.
fn remove_route(options: &RetuneOptions, path: &str) -> String {
    format!(
        "fissile exception remove {} --severity {}{}",
        shell_quote(path),
        options.severity,
        address_flags(options)
    )
}

fn with_note(head: String, note: Option<String>) -> String {
    match note {
        Some(note) => format!("{head}\n{note}"),
        None => head,
    }
}

/// The same address in the registry the caller did not select.
fn twin<'a>(
    loaded: &'a Loaded,
    options: &RetuneOptions,
    path: &str,
    unit: Unit,
) -> Option<&'a Exception> {
    let address = Address {
        severity: options.severity.other(),
        path,
        match_kind: options.match_kind,
        rules: &options.rules,
        unit,
    };
    let (_, twin) = entry::locate(&loaded.registries, &address).ok().flatten()?;
    Some(twin)
}

/// `retune` never writes to the twin's registry — twin consistency is a
/// repository's policy, not the tool's — but it reports a ceiling the edit is
/// about to contradict.
fn twin_note(twin: Option<&Exception>, unit: Unit, ceiling: u64) -> Option<String> {
    let twin = twin?;
    (twin.max_value != ceiling).then(|| {
        format!(
            "note: {} accepts {} up to {} {unit}",
            twin.registry, twin.path, twin.max_value
        )
    })
}

/// Rewrite the `max_accepted` line of the `index`-th `[[exceptions]]` block,
/// preserving every other byte, so the diff is the one decision that changed
/// (§FS-008-exception-retune.3).
fn rewrite_ceiling(
    text: &str,
    index: usize,
    value: u64,
    unit: Unit,
    registry: &std::path::Path,
    path: &str,
) -> Result<String, CommandError> {
    // Split rather than `lines()`: this round-trips the original bytes, including
    // the final newline and any `\r`, since only one line may change.
    let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    // Only a line that begins outside a string holds TOML structure. Inside one,
    // `[[exceptions]]` and `max_accepted` are prose someone wrote in a `reason`.
    let structural = toml_lines::structural_lines(&lines);
    let starts = toml_lines::block_starts(&lines);
    let target = starts.get(index).and_then(|start| {
        let end = starts.get(index + 1).copied().unwrap_or(lines.len());
        (*start..end).find(|number| structural[*number] && is_max_accepted(lines[*number].trim()))
    });

    // Reached only by a registry that spells the ceiling as a sub-table rather
    // than the inline form both commands write. Refusing beats guessing at a
    // rewrite that could silently corrupt the entry.
    let Some(number) = target else {
        return Err(CommandError::Usage(format!(
            "{}: cannot rewrite the ceiling for {path}; the entry does not spell \
             `max_accepted` as one inline table — edit it by hand",
            registry.display()
        )));
    };

    let indent: String = lines[number]
        .chars()
        .take_while(|character| character.is_whitespace())
        .collect();
    // A CRLF registry keeps its CRLF: the `\r` belongs to this line's bytes, and
    // dropping it would turn a one-line diff into whole-file churn on a checkout
    // that stores the file that way.
    let eol = if lines[number].ends_with('\r') {
        "\r"
    } else {
        ""
    };
    lines[number] = format!("{indent}{}{eol}", entry::max_accepted_line(value, unit));
    Ok(lines.join("\n"))
}

fn is_max_accepted(trimmed: &str) -> bool {
    trimmed
        .strip_prefix("max_accepted")
        .is_some_and(|rest| rest.trim_start().starts_with('='))
}

#[cfg(test)]
mod tests {
    use super::*;

    const REGISTRY: &str = "fissile_exceptions_version = 2\n\
        \n\
        [[exceptions]]\n\
        path = \"a.rs\"\n\
        max_accepted = { value = 400, unit = \"lines\" }\n\
        reason = \"\"\"\n\
        not this one: max_accepted = { value = 1, unit = \"lines\" }\n\
        \"\"\"\n\
        \n\
        [[exceptions]]\n\
        path = \"b.rs\"\n\
        max_accepted = { value = 500, unit = \"lines\" }\n";

    #[test]
    fn rewrites_only_the_addressed_block() {
        let out = rewrite_ceiling(
            REGISTRY,
            1,
            600,
            Unit::Lines,
            std::path::Path::new("r"),
            "b.rs",
        )
        .unwrap();
        assert!(out.contains("max_accepted = { value = 400, unit = \"lines\" }"));
        assert!(out.contains("max_accepted = { value = 600, unit = \"lines\" }"));
        assert!(!out.contains("value = 500"));
    }

    /// A `[[exceptions]]`-shaped line inside a reason must not shift the count,
    /// and a `max_accepted` written there is prose, not a field.
    #[test]
    fn a_reason_body_is_not_scanned() {
        let out = rewrite_ceiling(
            REGISTRY,
            0,
            700,
            Unit::Lines,
            std::path::Path::new("r"),
            "a.rs",
        )
        .unwrap();
        assert!(out.contains("not this one: max_accepted = { value = 1, unit = \"lines\" }"));
        assert!(out.contains("max_accepted = { value = 700, unit = \"lines\" }"));
    }

    /// A `'''` literal string closes on `'''` alone. Tracked as one toggle with
    /// `"""`, the reason below never closes, every block after it is miscounted,
    /// and the rewrite lands on prose while reporting success.
    #[test]
    fn a_literal_reason_body_is_not_scanned() {
        let registry = "[[exceptions]]\n\
            path = \"a.rs\"\n\
            max_accepted = { value = 400, unit = \"lines\" }\n\
            reason = '''\n\
            [[exceptions]]\n\
            max_accepted = { value = 1, unit = \"lines\" }\n\
            '''\n\
            \n\
            [[exceptions]]\n\
            path = \"b.rs\"\n\
            max_accepted = { value = 500, unit = \"lines\" }\n";
        let out = rewrite_ceiling(
            registry,
            1,
            600,
            Unit::Lines,
            std::path::Path::new("r"),
            "b.rs",
        )
        .unwrap();
        assert!(out.contains("max_accepted = { value = 1, unit = \"lines\" }"));
        assert!(out.contains("max_accepted = { value = 600, unit = \"lines\" }"));
        assert!(!out.contains("value = 500"));
    }

    /// Fence-shaped text outside a string opens nothing: a `#` comment is not
    /// TOML, and a quoted `"""` inside a single-line string is a value.
    #[test]
    fn a_comment_or_quoted_fence_opens_no_string() {
        let registry = "# a comment quoting \"\"\" and '''\n\
            [[exceptions]]\n\
            title = \"a \\\"\\\"\\\" b\"\n\
            max_accepted = { value = 400, unit = \"lines\" }\n";
        let out = rewrite_ceiling(
            registry,
            0,
            500,
            Unit::Lines,
            std::path::Path::new("r"),
            "a.rs",
        )
        .unwrap();
        assert!(out.contains("max_accepted = { value = 500, unit = \"lines\" }"));
    }

    /// The rewritten line keeps the bytes that end it, so a CRLF registry does
    /// not come back with one lone-LF line (§FS-008-exception-retune.3).
    #[test]
    fn a_crlf_registry_keeps_its_line_endings() {
        let registry = REGISTRY.replace('\n', "\r\n");
        let out = rewrite_ceiling(
            &registry,
            1,
            600,
            Unit::Lines,
            std::path::Path::new("r"),
            "b.rs",
        )
        .unwrap();
        assert!(out.contains("max_accepted = { value = 600, unit = \"lines\" }\r\n"));
        assert!(
            out.split("\r\n").all(|line| !line.contains('\n')),
            "every line ends CRLF: {out:?}"
        );
    }

    #[test]
    fn a_sub_table_ceiling_is_refused_not_guessed() {
        let registry = "[[exceptions]]\npath = \"a.rs\"\n[exceptions.max_accepted]\nvalue = 400\n";
        let error = rewrite_ceiling(
            registry,
            0,
            500,
            Unit::Lines,
            std::path::Path::new("r"),
            "a.rs",
        );
        assert!(error.is_err());
    }

    /// §FS-008-exception-retune.3: output exposes the measurement and configured
    /// step behind a quantized ceiling instead of making it resemble a budget.
    #[test]
    fn ceiling_detail_names_a_measurement_and_step() {
        let base = entry::Base {
            value: 436,
            measured: Some(436),
            source: entry::BaseSource::Measured("src/model.rs"),
        };
        let detail = ceiling_detail(&base, 500, 100, Unit::Lines, None);
        assert_eq!(detail, " (measured 436 lines; quantized to 100-line step)");
    }

    /// §DF-010-stated-ceilings-are-exact.1: a stated value is the ceiling, and
    /// the step is named as the round number it could have been — or not at
    /// all, when that number is one the command would refuse.
    #[test]
    fn a_stated_value_names_the_next_step_when_there_is_one() {
        let base = entry::Base {
            value: 436,
            measured: Some(430),
            source: entry::BaseSource::Max,
        };
        assert_eq!(
            ceiling_detail(&base, 436, 100, Unit::Lines, Some(500)),
            " (next 100-line step: 500)"
        );
        assert!(ceiling_detail(&base, 436, 100, Unit::Lines, None).is_empty());
    }

    #[test]
    fn exact_values_need_no_ceiling_detail() {
        let base = entry::Base {
            value: 500,
            measured: Some(500),
            source: entry::BaseSource::Measured("src/model.rs"),
        };
        assert!(ceiling_detail(&base, 500, 100, Unit::Lines, None).is_empty());
    }
}
