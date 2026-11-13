//! `fissile measure` (§FS-007-measure): what the budgets count, for a passing
//! file as much as a failing one. The arithmetic is fissile's own and nothing
//! outside fissile reproduces it (§FS-001-config.3.1).

use std::path::PathBuf;

use crate::cli::{self, CommandError, Format, Loaded};
use crate::exceptions::Verdict;
use crate::json::Json;
use crate::report;
use crate::{FileMeasurement, Severity, Unit};

/// Inputs to a `measure` run.
#[derive(Clone, Debug)]
pub struct MeasureOptions {
    /// The root; one made only of `..` steps puts the caller below it ([`crate::invocation`]).
    pub root: PathBuf,
    pub config_path: Option<PathBuf>,
    pub staged: bool,
    pub format: Option<Format>,
    pub no_color: bool,
    pub paths: Vec<String>,
}

/// The rendered output plus the file-level errors that force exit 2 without
/// hiding the rest (§FS-004-check-audit.5). No `failed`: a file over a hard
/// limit is an answer, not a verdict (§FS-007-measure.1).
pub struct Run {
    pub output: String,
    /// What stderr owes beside the rows: today, the deprecated config home this
    /// run was governed by (§FS-001-config.8.2).
    pub notes: Vec<String>,
    pub errors: Vec<String>,
}

/// One `(file, rule)` pair, or a file no rule measures.
enum Row {
    Measured(Measured),
    Unruled { path: String },
}

struct Measured {
    path: String,
    unit: Unit,
    actual: u64,
    rule_id: String,
    soft: Option<u64>,
    hard: Option<u64>,
    soft_accepted: Option<u64>,
    hard_accepted: Option<u64>,
}

/// One threshold, with the largest measurement that still clears it: a limit
/// fires *above* the limit (§GOAL-006-graded-limits) and a ceiling silences *at*
/// the ceiling (§FS-003-exceptions.3), so the two cannot share one comparison.
struct Threshold {
    label: &'static str,
    value: u64,
    /// Signed: a limit of `0`, which nothing clears, is `-1` and not `0`.
    clears_up_to: i64,
}

/// The room left before the threshold that binds first, negative once the value
/// has passed it (§FS-007-measure.2). One signed quantity, so `0 to hard` means
/// the next unit fails rather than promising room `check` will not honor.
struct Headroom {
    room: i64,
    label: &'static str,
}

pub fn run(options: &MeasureOptions) -> Result<Run, CommandError> {
    let loaded = cli::load(&options.root, options.config_path.as_deref())?;
    // No whole-repo default: "these files" is `measure`, "the repository" is
    // `audit --top` (§FS-007-measure.1).
    let files = cli::selected_files(&loaded, options.staged, &options.paths)?.ok_or_else(|| {
        CommandError::Usage(
            "measure needs <paths>... or --staged; `fissile audit --top <N>` ranks the \
             whole repository"
                .to_owned(),
        )
    })?;
    let format = options
        .format
        .unwrap_or_else(|| loaded.config.output.format.into());

    // The same walk `check` makes, including how it treats a path it cannot
    // measure (§FS-004-check-audit.5).
    let (measurements, errors) = cli::measure_each(&loaded, options.staged, &files);
    let mut rows = Vec::with_capacity(measurements.len());
    for measurement in &measurements {
        rows.extend(rows_for(&loaded, measurement)?);
    }

    let output = match format {
        Format::Text => {
            let color = cli::use_color(loaded.config.output.color, options.no_color, format);
            rows.iter()
                .map(|row| render_row(row, color))
                .collect::<Vec<_>>()
                .join("\n")
        }
        Format::Json => Json::Array(rows.iter().map(row_json).collect()).render(),
    };
    Ok(Run {
        output,
        notes: cli::config_notes(&loaded.source, &loaded.root),
        errors,
    })
}

/// Every rule that measures this file, with the ceilings each registry records
/// for it. A rule and its exceptions are read together because the ceiling is
/// what the headroom is measured against once a file is over limit.
fn rows_for(loaded: &Loaded, file: &FileMeasurement) -> Result<Vec<Row>, CommandError> {
    let path = file.path.to_string_lossy().replace('\\', "/");
    let hits = loaded
        .checker
        .evaluate(file)
        .map_err(crate::report::EvalError::from)?;

    if hits.is_empty() {
        // "No budget applies here" is an answer; silence would read as zero.
        return Ok(vec![Row::Unruled { path }]);
    }

    let mut rows = Vec::with_capacity(hits.len());
    for hit in hits {
        let unit = hit.rule.budget.unit;
        rows.push(Row::Measured(Measured {
            soft_accepted: accepted(
                loaded,
                Severity::Soft,
                &path,
                &hit.rule.id,
                unit,
                hit.actual,
            )?,
            hard_accepted: accepted(
                loaded,
                Severity::Hard,
                &path,
                &hit.rule.id,
                unit,
                hit.actual,
            )?,
            path: path.clone(),
            unit,
            actual: hit.actual,
            rule_id: hit.rule.id.clone(),
            soft: hit.rule.budget.soft,
            hard: hit.rule.budget.hard,
        }));
    }
    Ok(rows)
}

/// The ceiling one registry records for this `(path, rule, unit)`, whether or
/// not the file is currently within it — a ceiling above the file is exactly the
/// number a caller sizing an edit needs.
fn accepted(
    loaded: &Loaded,
    severity: Severity,
    path: &str,
    rule_id: &str,
    unit: Unit,
    actual: u64,
) -> Result<Option<u64>, CommandError> {
    Ok(
        match loaded
            .registries
            .verdict(severity, path, rule_id, unit, actual)?
        {
            Verdict::Silenced(entry) | Verdict::Exceeded(entry) => Some(entry.max_value),
            Verdict::None => None,
        },
    )
}

/// Every threshold that applies to this row, in the order they are printed.
fn thresholds(row: &Measured) -> Vec<Threshold> {
    // Limits and ceilings both accept equality now, so each pair carries its own
    // "largest value that still clears it" rather than one shared rule in case
    // their semantics diverge again.
    [
        ("soft", row.soft, 0),
        ("hard", row.hard, 0),
        ("soft-accepted", row.soft_accepted, 0),
        ("hard-accepted", row.hard_accepted, 0),
    ]
    .into_iter()
    .filter_map(|(label, value, exclusive)| {
        value.map(|value| Threshold {
            label,
            value,
            clears_up_to: i64::try_from(value).unwrap_or(i64::MAX) - exclusive,
        })
    })
    .collect()
}

fn headroom(thresholds: &[Threshold], actual: u64) -> Option<Headroom> {
    let actual = i64::try_from(actual).unwrap_or(i64::MAX);
    if let Some(threshold) = thresholds
        .iter()
        .filter(|threshold| threshold.clears_up_to >= actual)
        .min_by_key(|threshold| threshold.clears_up_to)
    {
        return Some(Headroom {
            room: threshold.clears_up_to - actual,
            label: threshold.label,
        });
    }
    // Past everything: the distance back under the highest threshold, which is
    // the one a reader has to answer for.
    let threshold = thresholds.iter().max_by_key(|threshold| threshold.value)?;
    Some(Headroom {
        room: threshold.clears_up_to - actual,
        label: threshold.label,
    })
}

fn render_row(row: &Row, color: bool) -> String {
    let row = match row {
        Row::Unruled { path } => return format!("{path} — no rule applies"),
        Row::Measured(row) => row,
    };
    let mut line = format!("{} {} {} [{}]", row.path, row.actual, row.unit, row.rule_id);
    let thresholds = thresholds(row);
    for threshold in &thresholds {
        line.push_str(&format!(" {} {}", threshold.label, threshold.value));
    }
    if let Some(Headroom { room, label }) = headroom(&thresholds, row.actual) {
        // Only a value that has actually passed a threshold is tinted; standing
        // exactly at a limit or ceiling is room, not overflow, and `check` calls
        // it `ok` (§GOAL-006-graded-limits, §FS-003-exceptions.3).
        let clause = if room < 0 {
            let over = format!("{} over {label}", room.unsigned_abs());
            let tint = if label.starts_with("hard") {
                report::BOLD_RED
            } else {
                report::BOLD_YELLOW
            };
            report::paint(color, tint, &over)
        } else {
            format!("{room} to {label}")
        };
        line.push_str(&format!(" — {clause}"));
    }
    line
}

fn row_json(row: &Row) -> Json {
    let row = match row {
        Row::Unruled { path } => {
            return Json::Object(vec![
                ("path", Json::str(path.clone())),
                ("unruled", Json::UInt(1)),
            ]);
        }
        Row::Measured(row) => row,
    };
    let mut fields = vec![
        ("path", Json::str(row.path.clone())),
        ("unit", Json::str(row.unit.to_string())),
        ("actual", Json::UInt(row.actual)),
        ("rule_id", Json::str(row.rule_id.clone())),
    ];
    // A threshold that does not exist is omitted, not nulled (§FS-007-measure.2).
    for (key, value) in [
        ("soft", row.soft),
        ("hard", row.hard),
        ("soft_accepted", row.soft_accepted),
        ("hard_accepted", row.hard_accepted),
    ] {
        if let Some(value) = value {
            fields.push((key, Json::UInt(value)));
        }
    }
    if let Some(headroom) = headroom(&thresholds(row), row.actual) {
        // Signed: positive is room left below the threshold, negative is the
        // distance past it, so one field carries both directions.
        fields.push(("headroom", Json::Int(headroom.room)));
        fields.push(("headroom_to", Json::str(headroom.label)));
    }
    Json::Object(fields)
}
