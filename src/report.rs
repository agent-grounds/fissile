//! Shared evaluation and rendering for `check` and `audit` (§FS-004-check-audit).
//! Runs the checker over measurements, applies the exception registries and the
//! hard-implies-soft rule, and turns the result into text or JSON.

use std::error::Error;
use std::fmt;

use crate::config::Bump;
use crate::entry;
use crate::exceptions::{Exception, ExceptionError, Kind, Registries, Verdict};
use crate::json::Json;
use crate::staged_history::SoftEdit;
use crate::{
    Checker, FileMeasurement, FissileError, Overflow, RuleHit, Severity, Unit, render_overflow,
};

/// What evaluating one `(file, rule, severity)` produced.
#[derive(Clone, Debug)]
pub enum Outcome {
    /// A standing finding: no exception silenced it.
    Reported(Overflow),
    /// An overflow accepted by an exception. Carried for audit attribution
    /// (§FS-003-exceptions.5); never fails a build.
    Silenced {
        overflow: Overflow,
        exception_max: u64,
    },
}

impl Outcome {
    pub fn overflow(&self) -> &Overflow {
        match self {
            Outcome::Reported(overflow) => overflow,
            Outcome::Silenced { overflow, .. } => overflow,
        }
    }

    pub fn is_reported(&self) -> bool {
        matches!(self, Outcome::Reported(_))
    }
}

/// A failure while evaluating: a config/engine error or an exception schema error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvalError {
    Engine(FissileError),
    Exceptions(ExceptionError),
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EvalError::Engine(error) => write!(f, "{error}"),
            EvalError::Exceptions(error) => write!(f, "{error}"),
        }
    }
}

impl Error for EvalError {}

impl From<FissileError> for EvalError {
    fn from(error: FissileError) -> Self {
        EvalError::Engine(error)
    }
}

impl From<ExceptionError> for EvalError {
    fn from(error: ExceptionError) -> Self {
        EvalError::Exceptions(error)
    }
}

/// Evaluate one measured file against the checker and exception registries.
pub fn evaluate_file(
    checker: &Checker,
    registries: &Registries,
    file: &FileMeasurement,
) -> Result<Vec<Outcome>, EvalError> {
    evaluate_hits(registries, file, &checker.evaluate(file)?)
}

/// The same evaluation from rule hits a caller already has: `audit` reads them
/// for its inventory sections too, and evaluating one file twice is waste the
/// whole-repo walk cannot afford (§GOAL-001-fast-feedback).
pub fn evaluate_hits(
    registries: &Registries,
    file: &FileMeasurement,
    hits: &[RuleHit<'_>],
) -> Result<Vec<Outcome>, EvalError> {
    let path = file.path.to_string_lossy().replace('\\', "/");
    let mut outcomes = Vec::new();

    for hit in hits {
        let rule = hit.rule;
        let unit = rule.budget.unit;
        let actual = hit.actual;

        // Hard overflow: a standing hard finding suppresses the soft one
        // (§GOAL-006-graded-limits). A silenced hard leaves the soft finding to
        // the accepting entry's kind (§FS-003-exceptions.3).
        if let Some(hard) = rule.budget.hard.filter(|hard| actual > *hard) {
            match registries.verdict(Severity::Hard, &path, &rule.id, unit, actual)? {
                Verdict::None | Verdict::Exceeded(_) => {
                    outcomes.push(Outcome::Reported(render_overflow(
                        file,
                        rule,
                        Severity::Hard,
                        actual,
                        hard,
                    )));
                    continue;
                }
                Verdict::Silenced(entry) => {
                    outcomes.push(Outcome::Silenced {
                        overflow: render_overflow(file, rule, Severity::Hard, actual, hard),
                        exception_max: entry.max_value,
                    });
                    // Structural: the split is illegal, so there is nothing to
                    // minimize and the soft registry goes unconsulted. Deferred
                    // falls through and still warns (§FS-003-exceptions.3).
                    if entry.kind == Kind::Structural {
                        continue;
                    }
                }
            }
        }

        if let Some(soft) = rule.budget.soft.filter(|soft| actual > *soft) {
            match registries.verdict(Severity::Soft, &path, &rule.id, unit, actual)? {
                Verdict::None | Verdict::Exceeded(_) => outcomes.push(Outcome::Reported(
                    render_overflow(file, rule, Severity::Soft, actual, soft),
                )),
                Verdict::Silenced(entry) => outcomes.push(Outcome::Silenced {
                    overflow: render_overflow(file, rule, Severity::Soft, actual, soft),
                    exception_max: entry.max_value,
                }),
            }
        }
    }

    Ok(outcomes)
}

/// Whether any outcome is a standing hard finding — the build-failing condition.
pub fn has_hard_failure(outcomes: &[Outcome]) -> bool {
    outcomes
        .iter()
        .any(|outcome| outcome.is_reported() && outcome.overflow().severity == Severity::Hard)
}

/// Guidance wraps at a fixed width, not the terminal's, so a block is
/// byte-identical in a narrow terminal and in CI (§GOAL-006-graded-limits.2).
const GUIDANCE_COLUMNS: usize = 78;

/// Severity tints reused by other text surfaces (§FS-007-measure.2).
pub const BOLD_RED: &str = "\x1b[1;31m";
pub const BOLD_YELLOW: &str = "\x1b[1;33m";
const GREEN: &str = "\x1b[32m";
const RESET: &str = "\x1b[0m";

pub fn paint(color: bool, code: &str, text: &str) -> String {
    if color {
        format!("{code}{text}{RESET}")
    } else {
        text.to_owned()
    }
}

/// The standing findings as grouped text blocks: one per `(severity, rule,
/// rendered guidance)`, hard first, files largest first, and shared guidance
/// written once under a severity-tinted header (§FS-004-check-audit.1).
pub fn finding_blocks(outcomes: &[Outcome], color: bool) -> Vec<String> {
    finding_blocks_with_context(outcomes, color, &[])
}

/// The command-only context needed to explain a line measurement and to name
/// the ceiling a plain `fissile exception add` would write, without adding
/// fields to the public [`Overflow`] or changing the public renderer's
/// signature.
pub(crate) struct FindingContext {
    path: std::path::PathBuf,
    rule_id: String,
    unit: Unit,
    line_basis: Option<&'static str>,
    /// The unit's `[exceptions.bump]` step, which quantizes that ceiling.
    bump_step: u64,
    /// The hard limit of the rule this finding names, which is the rule a
    /// caller would pass to `exception add --rule` and so the one that binds
    /// their soft ceiling (§DF-010-stated-ceilings-are-exact.2).
    hard_limit: Option<u64>,
}

impl FindingContext {
    /// The ceiling a `fissile exception add` with no `--max` would record for
    /// this file: the measurement quantized up to the unit's `[exceptions.bump]`
    /// step (§FS-004-check-audit.1, §DF-006-quantized-ceilings.1). It is the
    /// number that command computes, so it is computed the same way here.
    ///
    /// `None` where that call would be refused, since a number ending in a
    /// refusal sends the caller somewhere with nothing to do. A soft ceiling at
    /// or above the rule's hard limit silences nothing while the file is still
    /// under that limit (§DF-010-stated-ceilings-are-exact.2); past the limit
    /// the soft entry is the record of the debt and `add` accepts it, and
    /// nothing binds a hard ceiling at all.
    ///
    /// `add` spares one more soft ceiling than this does: the one whose address
    /// carries a deferred hard twin (§FS-005-exception-add.4). Not reproducing
    /// that is a decision, not an oversight — it would mean reading the
    /// exception registries to render a finding, and withholding is the
    /// direction that cannot mislead (§FS-004-check-audit.1).
    fn would_accept(&self, overflow: &Overflow) -> Option<u64> {
        let ceiling = entry::quantize(overflow.actual, self.bump_step);
        let refused = overflow.severity == Severity::Soft
            && self
                .hard_limit
                .is_some_and(|hard| ceiling >= hard && overflow.actual < hard);
        (!refused).then_some(ceiling)
    }
}

/// Build rendering context from the effective rule hits for one measured file.
/// `utf8 = false` selects the raw-line fallback regardless of configured flags.
pub(crate) fn contexts_for_file(
    file: &FileMeasurement,
    hits: &[RuleHit<'_>],
    utf8: bool,
    bump: &Bump,
) -> Vec<FindingContext> {
    hits.iter()
        .map(|hit| FindingContext {
            path: file.path.clone(),
            rule_id: hit.rule.id.clone(),
            unit: hit.rule.budget.unit,
            line_basis: (hit.rule.budget.unit == Unit::Lines).then(|| {
                if utf8 {
                    line_basis(hit.rule.count_blank_lines, hit.rule.count_comment_lines)
                } else {
                    "physical lines"
                }
            }),
            bump_step: bump.step(hit.rule.budget.unit),
            hard_limit: hit.rule.budget.hard,
        })
        .collect()
}

/// The context built for the `(file, rule, unit)` this overflow came from.
fn context_for<'a>(
    contexts: &'a [FindingContext],
    overflow: &Overflow,
) -> Option<&'a FindingContext> {
    contexts.iter().find(|context| {
        context.path == overflow.path
            && context.rule_id == overflow.rule_id
            && context.unit == overflow.unit
    })
}

/// Render command findings with the line-counting context carried alongside
/// the public outcomes. Byte and token details deliberately keep their compact
/// historical spelling.
pub(crate) fn finding_blocks_with_context(
    outcomes: &[Outcome],
    color: bool,
    contexts: &[FindingContext],
) -> Vec<String> {
    finding_blocks_with_staged_context(outcomes, color, contexts, &[])
}

/// Render command findings with staged edit provenance where the commit-side
/// history proof supplied it (§FS-004-check-audit.1.4).
pub(crate) fn finding_blocks_with_staged_context(
    outcomes: &[Outcome],
    color: bool,
    contexts: &[FindingContext],
    soft_edits: &[SoftEdit],
) -> Vec<String> {
    let mut groups: Vec<Group<'_>> = Vec::new();

    for overflow in outcomes
        .iter()
        .filter(|outcome| outcome.is_reported())
        .map(Outcome::overflow)
    {
        let finding = Finding {
            overflow,
            context: context_for(contexts, overflow),
            soft_edit: soft_edit_for(soft_edits, overflow),
        };
        match groups.iter_mut().find(|group| group.accepts(&finding)) {
            Some(group) => group.overflows.push(finding),
            None => groups.push(Group {
                head: finding,
                overflows: vec![finding],
            }),
        }
    }

    groups.sort_by(|left, right| left.order().cmp(&right.order()));
    for group in &mut groups {
        // Worst first: the file that most needs splitting leads its block.
        group.overflows.sort_by(|left, right| {
            right
                .overflow
                .actual
                .cmp(&left.overflow.actual)
                .then(left.overflow.path.cmp(&right.overflow.path))
        });
    }

    groups.iter().map(|group| group.render(color)).collect()
}

fn line_basis(count_blank_lines: bool, count_comment_lines: bool) -> &'static str {
    match (count_blank_lines, count_comment_lines) {
        (true, true) => "physical lines",
        (false, true) => "non-blank lines",
        (true, false) => "non-comment lines",
        (false, false) => "non-blank, non-comment lines",
    }
}

/// The one line a run that reported something adds, naming the number no other
/// tool produces — the headroom of the files a split moves code *into*
/// (§FS-004-check-audit.1.1, §FS-007-measure).
pub const MEASURE_HINT: &str =
    "hint: fissile measure <path>... reports size and headroom for the files you split into.";

/// What `check --staged` says when a hard finding stands: the one context that
/// is a commit, and the one place `--no-verify` is a live temptation
/// (§FS-004-check-audit.1.2).
pub const COMMIT_GATE: &str = "\
commit blocked by fissile. Split the file, or ask a human for a reviewed hard
exception. Bypassing with --no-verify leaves the overflow for review or CI.";

/// The commit epilogue for edit-promoted soft debt: the exception remains in
/// the soft registry because promotion changes blocking, not severity.
pub const COMMIT_GATE_PROMOTED: &str = "\
commit blocked by fissile. Split the file now, or record the soft-limit debt
now with `fissile exception add <path> --severity soft --rule <rule> --kind
<kind>`. Bypassing with --no-verify leaves the overflow for review or CI.";

/// The same epilogue when a dead registry entry is the only thing blocking the
/// commit: there is no file to split, and the fix is in the registry the block
/// above names (§FS-004-check-audit.1.2, §FS-004-check-audit.1.3).
pub const COMMIT_GATE_STALE: &str = "\
commit blocked by fissile. Remove the exception entry above, or point it at the
path its file moved to. Bypassing with --no-verify leaves a dead entry in the
registry.";

/// The same epilogue when the commit is blocked by a staged file that could not
/// be measured: nothing above accounts for it, so the exit code is all the
/// caller would otherwise have (§FS-004-check-audit.1.2, §FS-004-check-audit.5).
pub const COMMIT_GATE_UNMEASURED: &str = "\
commit blocked by fissile. A staged file could not be measured, so nothing above
accounts for it — fix the path the error names, or unstage it. Bypassing with
--no-verify commits a file fissile never checked.";

/// The block naming entries that have outlived their file (§FS-004-check-audit.1.3).
/// One block per registry, so a reader opens the file the line names.
pub fn stale_blocks(entries: &[&Exception], color: bool) -> Vec<String> {
    let mut registries: Vec<&str> = Vec::new();
    for entry in entries {
        if !registries.contains(&entry.registry.as_str()) {
            registries.push(&entry.registry);
        }
    }

    registries
        .into_iter()
        .map(|registry| {
            let mine: Vec<&&Exception> = entries
                .iter()
                .filter(|entry| entry.registry == registry)
                .collect();
            // The whole clause agrees, not just the count: "files that is not
            // there" is the shape of a sentence assembled from two halves.
            let headline = if mine.len() == 1 {
                "1 exception accepts a file that is not there".to_owned()
            } else {
                format!("{} exceptions accept files that are not there", mine.len())
            };
            let mut block = paint(
                color,
                BOLD_YELLOW,
                &format!("stale: {headline} [registry: {registry}]"),
            );
            for line in wrap(STALE_GUIDANCE, GUIDANCE_COLUMNS) {
                block.push_str("\n  ");
                block.push_str(&line);
            }
            for entry in mine {
                block.push_str(&format!(
                    "\n    {} [{}, rule: {}]",
                    entry.path,
                    entry.severity,
                    entry.rules.join(", ")
                ));
            }
            block
        })
        .collect()
}

const STALE_GUIDANCE: &str = "The file moved or was deleted, so the entry silences nothing. Remove it with `fissile exception remove`, or point it at the path the file moved to.";

/// The findings that share a severity, a rule, and one rendered guidance string.
struct Group<'a> {
    head: Finding<'a>,
    overflows: Vec<Finding<'a>>,
}

#[derive(Clone, Copy)]
struct Finding<'a> {
    overflow: &'a Overflow,
    context: Option<&'a FindingContext>,
    soft_edit: Option<&'a SoftEdit>,
}

impl<'a> Group<'a> {
    /// Guidance is compared as rendered text, not by message ID: a template that
    /// interpolates `{path}` says something different about each file, so those
    /// findings must not be collapsed under one line (§FS-001-config.4).
    fn accepts(&self, finding: &Finding<'a>) -> bool {
        self.head.overflow.severity == finding.overflow.severity
            && self.head.overflow.rule_id == finding.overflow.rule_id
            && self.head.overflow.message.text == finding.overflow.message.text
            && self.head.soft_edit.is_some_and(SoftEdit::promoted)
                == finding.soft_edit.is_some_and(SoftEdit::promoted)
    }

    /// Hard before soft, then by rule ID, then by message ID.
    fn order(&self) -> (u8, &str, &str) {
        let severity = match self.head.overflow.severity {
            Severity::Hard => 0,
            Severity::Soft => 1,
        };
        (
            severity,
            &self.head.overflow.rule_id,
            &self.head.overflow.message.id,
        )
    }

    fn header(&self) -> String {
        let files = if self.overflows.len() == 1 {
            "1 file".to_owned()
        } else {
            format!("{} files", self.overflows.len())
        };
        let severity = if self.head.soft_edit.is_some_and(SoftEdit::promoted) {
            "soft (promoted)"
        } else {
            self.head.overflow.severity.as_str()
        };
        format!(
            "{severity}: {files} over the {}-{} budget [rule: {}, message: {}]",
            self.head.overflow.limit,
            self.head.overflow.unit.singular(),
            self.head.overflow.rule_id,
            self.head.overflow.message.id,
        )
    }

    fn render(&self, color: bool) -> String {
        let code = match self.head.overflow.severity {
            Severity::Hard => BOLD_RED,
            Severity::Soft => BOLD_YELLOW,
        };
        let mut block = paint(color, code, &self.header());

        for line in wrap(&self.head.overflow.message.text, GUIDANCE_COLUMNS) {
            block.push_str("\n  ");
            block.push_str(&line);
        }

        for finding in &self.overflows {
            let overflow = finding.overflow;
            let basis = finding.context.and_then(|context| context.line_basis);
            let measurement = match basis {
                Some(basis) => format!("{} {basis}", overflow.actual),
                None => format!("{} {}", overflow.actual, overflow.unit),
            };
            // A byte or token detail carries no budget clause, so the ceiling
            // opens a parenthesis of its own (§FS-004-check-audit.1).
            let clauses: Vec<String> = basis
                .map(|_| format!("budget {}", overflow.limit))
                .into_iter()
                .chain(finding.soft_edit.map(soft_edit_clause))
                .chain(
                    finding
                        .context
                        .and_then(|context| context.would_accept(overflow))
                        .map(|ceiling| format!("an exception here would accept {ceiling}")),
                )
                .collect();
            let path = overflow.path.display();
            let detail = if clauses.is_empty() {
                format!("{path}: {measurement}")
            } else {
                format!("{path}: {measurement} ({})", clauses.join("; "))
            };
            block.push_str(&format!("\n    {detail}"));
        }

        block
    }
}

/// Greedy word wrap at `columns`, measured in characters. Explicit newlines in
/// the message are kept as paragraph breaks; a word longer than `columns` (a
/// path, a URL) takes its own line rather than being broken.
fn wrap(text: &str, columns: usize) -> Vec<String> {
    let mut lines = Vec::new();

    for paragraph in text.lines() {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let width = line.chars().count();
            if !line.is_empty() && width + 1 + word.chars().count() > columns {
                lines.push(std::mem::take(&mut line));
            } else if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        lines.push(line);
    }

    lines
}

/// The success marker, tinted green when `color` is set (§FS-001-config.6).
pub fn success_marker(marker: &str, color: bool) -> String {
    paint(color, GREEN, marker)
}

/// The audit attribution line for a silenced overflow (§FS-003-exceptions.5).
/// Path plus severity locate the entry — the registry the severity names, under
/// this path — so there is no id to quote (§DF-005-exception-identity).
pub fn silenced_line(overflow: &Overflow, exception_max: u64) -> String {
    format!(
        "{}: {} exception (accepted up to {} {})",
        overflow.path.display(),
        overflow.severity,
        exception_max,
        overflow.unit,
    )
}

/// One JSON finding record (§FS-004-check-audit.1) without the command-only
/// context, so a caller holding bare outcomes still gets the stable fields.
pub fn overflow_json(outcome: &Outcome) -> Json {
    overflow_json_with_context(outcome, &[])
}

/// The same record carrying `exception_would_accept` wherever the text detail
/// names a ceiling, so a consumer of `--format json` chooses between the plain
/// and the stated form on the same facts a reader of the text does
/// (§FS-004-check-audit.1). A silenced record reports the accepting entry's own
/// ceiling as `exception_max` instead (§FS-003-exceptions.5).
pub(crate) fn overflow_json_with_context(outcome: &Outcome, contexts: &[FindingContext]) -> Json {
    overflow_json_with_staged_context(outcome, contexts, &[])
}

/// JSON finding plus staged edit provenance when this is the commit-time view.
pub(crate) fn overflow_json_with_staged_context(
    outcome: &Outcome,
    contexts: &[FindingContext],
    soft_edits: &[SoftEdit],
) -> Json {
    let overflow = outcome.overflow();
    let mut fields = vec![
        ("path", Json::str(overflow.path.to_string_lossy())),
        ("unit", Json::str(overflow.unit.to_string())),
        ("actual", Json::UInt(overflow.actual)),
        ("limit", Json::UInt(overflow.limit)),
        ("severity", Json::str(overflow.severity.as_str())),
        ("rule_id", Json::str(overflow.rule_id.clone())),
        ("message_id", Json::str(overflow.message.id.clone())),
        ("message", Json::str(overflow.message.text.clone())),
    ];
    if let Some(edit) = soft_edit_for(soft_edits, overflow) {
        fields.push(("soft_edit_count", Json::UInt(edit.count)));
        fields.push(("soft_edit_limit", Json::UInt(edit.limit)));
        fields.push((
            "soft_edit_history_complete",
            Json::Bool(edit.history_complete),
        ));
        if edit.promoted() {
            fields.push(("promotion", Json::str("soft_edit_limit")));
        }
    }
    match outcome {
        Outcome::Reported(_) => {
            if let Some(ceiling) =
                context_for(contexts, overflow).and_then(|context| context.would_accept(overflow))
            {
                fields.push(("exception_would_accept", Json::UInt(ceiling)));
            }
        }
        Outcome::Silenced { exception_max, .. } => {
            fields.push(("exception_max", Json::UInt(*exception_max)));
        }
    }
    Json::Object(fields)
}

fn soft_edit_for<'a>(soft_edits: &'a [SoftEdit], overflow: &Overflow) -> Option<&'a SoftEdit> {
    soft_edits.iter().find(|edit| {
        overflow.severity == Severity::Soft
            && edit.path == overflow.path
            && edit.rule_id == overflow.rule_id
            && edit.unit == overflow.unit
    })
}

fn soft_edit_clause(edit: &SoftEdit) -> String {
    if edit.promoted() {
        format!(
            "soft edits {}/{}; promoted to blocking",
            edit.count, edit.limit
        )
    } else if edit.history_complete {
        format!("soft edits {}/{}", edit.count, edit.limit)
    } else {
        format!(
            "soft edits {}/{}; history incomplete; promotion disabled",
            edit.count, edit.limit
        )
    }
}

pub(crate) fn has_soft_edit_promotion(soft_edits: &[SoftEdit]) -> bool {
    soft_edits.iter().any(SoftEdit::promoted)
}

#[cfg(test)]
#[path = "report_tests.rs"]
mod tests;
