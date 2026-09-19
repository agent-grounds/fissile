//! Unit tests for evaluation against the registries (§FS-003-exceptions.3) and
//! for grouped text rendering (§FS-004-check-audit.1). Kept in a sibling file so
//! `report.rs` stays well under its own line budget.

use super::*;
use crate::config::{Bump, Config};
use crate::exceptions::RegistrySource;
use crate::{Budget, MessageTemplate, RenderedMessage, Rule, Selector, Unit};

const HARD_REGISTRY: &str = "docs/file-size-human-exceptions.toml";

const CONFIG: &str = r#"
fissile_config_version = 1
[[messages]]
id = "m"
text = "Split it."
[[rules]]
id = "rust"
include = ["src/**/*.rs"]
unit = "lines"
soft = 100
hard = 200
message = "m"
"#;

/// A hard registry accepting `src/big.rs` up to 250 lines, with `kind` written
/// verbatim so a pre-`kind` entry can be spelled as the empty string.
fn hard_registry(kind: &str, until: &str) -> String {
    format!(
        r#"
fissile_exceptions_version = 2
[[exceptions]]
path = "src/big.rs"
match = "exact"
rules = ["rust"]
{kind}
max_accepted = {{ value = 250, unit = "lines" }}
until = "{until}"
reason = "a reason"
"#
    )
}

/// Every outcome `evaluate_file` produced for a `lines`-long `src/big.rs`, as
/// `(severity, is_reported)` in emission order.
fn outcomes(registry: &str, lines: u64) -> Vec<(Severity, bool)> {
    let checker = Config::parse(CONFIG).unwrap().to_checker().unwrap();
    let registries =
        Registries::load(None, Some(RegistrySource::new(HARD_REGISTRY, registry))).unwrap();
    let file = FileMeasurement::new("src/big.rs", lines * 12).with_lines(lines);
    evaluate_file(&checker, &registries, &file)
        .unwrap()
        .iter()
        .map(|outcome| (outcome.overflow().severity, outcome.is_reported()))
        .collect()
}

/// §FS-003-exceptions.3: a `deferred` hard entry accepts the blocking finding and
/// leaves the soft one standing. An entry declaring no kind reads as deferred and
/// behaves the same (§FS-003-exceptions.2.1).
#[test]
fn a_deferred_hard_exception_leaves_the_soft_finding_standing() {
    let standing = vec![(Severity::Hard, false), (Severity::Soft, true)];
    let until = "the case-builder module lands";
    assert_eq!(
        outcomes(&hard_registry("kind = \"deferred\"", until), 250),
        standing
    );
    assert_eq!(outcomes(&hard_registry("", until), 250), standing);
}

/// §FS-003-exceptions.3: a `structural` hard entry silences the soft finding too.
/// Splitting is illegal, so the warning can never be cleared by doing the work.
#[test]
fn a_structural_hard_exception_silences_the_soft_finding_too() {
    assert_eq!(
        outcomes(&hard_registry("kind = \"structural\"", "indefinite"), 250),
        vec![(Severity::Hard, false)]
    );
}

/// §FS-003-exceptions.3: the rule reaches only as far as a hard finding does. A
/// file below the hard limit never consults the hard registry, so the soft
/// warning stands and the soft registry is what accepts it.
#[test]
fn a_file_below_the_hard_limit_warns_despite_a_structural_hard_entry() {
    let registry = hard_registry("kind = \"structural\"", "indefinite");
    assert_eq!(outcomes(&registry, 150), vec![(Severity::Soft, true)]);
}

/// §GOAL-006-graded-limits.1: a *standing* hard finding suppresses the soft one
/// whatever the registries say, so a file grown past its ceiling reports the hard
/// overflow alone — the kind never reaches that path.
#[test]
fn a_hard_overflow_past_its_ceiling_still_suppresses_the_soft_finding() {
    for (kind, until) in [
        ("kind = \"structural\"", "indefinite"),
        ("kind = \"deferred\"", "the case-builder module lands"),
    ] {
        assert_eq!(
            outcomes(&hard_registry(kind, until), 300),
            vec![(Severity::Hard, true)]
        );
    }
}

fn reported(path: &str, rule: &str, severity: Severity, actual: u64, text: &str) -> Outcome {
    Outcome::Reported(Overflow {
        path: path.into(),
        rule_id: rule.to_owned(),
        severity,
        unit: Unit::Lines,
        actual,
        limit: match severity {
            Severity::Soft => 350,
            Severity::Hard => 550,
        },
        message: RenderedMessage {
            id: format!("{severity}-guidance"),
            text: text.to_owned(),
        },
    })
}

/// Context as `contexts_for_file` would build it for the rule [`reported`]
/// describes — a 350/550-line budget under the default 100-line step.
fn context(outcome: &Outcome) -> FindingContext {
    context_with(outcome, Bump::default().step(Unit::Lines), Some(550))
}

/// The same, for a case that is about the step or the hard limit itself.
fn context_with(outcome: &Outcome, bump_step: u64, hard_limit: Option<u64>) -> FindingContext {
    let overflow = outcome.overflow();
    FindingContext {
        path: overflow.path.clone(),
        rule_id: overflow.rule_id.clone(),
        unit: overflow.unit,
        line_basis: Some("non-blank lines"),
        bump_step,
        hard_limit,
    }
}

/// Every staged failure describes this command's verdict, while hook bypass is
/// conditional unless the promoted-soft explanation has named the enforcing
/// hook (§FS-004-check-audit.1.2).
#[test]
fn issue_74_staged_epilogues_name_the_command_verdict_and_condition_hook_bypass() {
    let lead = "`fissile check --staged` rejected this staged snapshot.";
    for epilogue in [
        COMMIT_GATE,
        COMMIT_GATE_PROMOTED,
        COMMIT_GATE_STALE,
        COMMIT_GATE_UNMEASURED,
    ] {
        assert!(epilogue.starts_with(lead), "wrong staged lead: {epilogue}");
        assert!(!epilogue.contains("commit blocked by fissile"));
    }

    let promoted = COMMIT_GATE_PROMOTED.replace('\n', " ");
    assert!(
        promoted.contains("a hook that calls `fissile check <paths>` keeps soft findings advisory")
    );
    assert!(promoted.contains("Bypassing such a hook with `--no-verify`"));
    for epilogue in [COMMIT_GATE, COMMIT_GATE_STALE, COMMIT_GATE_UNMEASURED] {
        assert!(
            epilogue
                .replace('\n', " ")
                .contains("If this command is the commit hook, bypassing it with `--no-verify`")
        );
    }
}

#[test]
fn files_sharing_guidance_are_listed_under_one_copy_of_it() {
    let outcomes = [
        reported(
            "src/util.rs",
            "source",
            Severity::Soft,
            410,
            "Should split.",
        ),
        reported("src/big.rs", "source", Severity::Hard, 620, "Must split."),
        reported("src/tax.rs", "source", Severity::Soft, 502, "Should split."),
    ];

    let contexts: Vec<_> = outcomes.iter().map(context).collect();
    let blocks = finding_blocks_with_context(&outcomes, false, &contexts);

    // Hard first, and the soft guidance is written once for both its files.
    assert_eq!(
        blocks,
        vec![
            "hard: 1 file over the 550-line budget [rule: source, message: hard-guidance]\n  \
             Must split.\n    src/big.rs: 620 non-blank lines (budget 550; an exception here would accept 700)"
                .to_owned(),
            // src/tax.rs rounds to 600, on the 550-line hard limit, so its
            // ceiling is withheld (§FS-004-check-audit.1).
            "soft: 2 files over the 350-line budget [rule: source, message: soft-guidance]\n  \
             Should split.\n    src/tax.rs: 502 non-blank lines (budget 350)\n    src/util.rs: 410 non-blank lines (budget 350; an exception here would accept 500)"
                .to_owned(),
        ]
    );
}

/// §FS-003-exceptions.5: the attribution line names the file, the severity that
/// says which registry accepted it, and the ceiling. There is no entry id to
/// quote — an entry has none (§DF-005-exception-identity).
#[test]
fn a_silenced_overflow_is_attributed_by_path_and_ceiling() {
    let outcome = reported(
        "src/orders.rs",
        "source",
        Severity::Hard,
        620,
        "Must split.",
    );

    assert_eq!(
        silenced_line(outcome.overflow(), 620),
        "src/orders.rs: hard exception (accepted up to 620 lines)"
    );
}

/// §FS-001-config.4: guidance that names a file cannot stand for another file,
/// so a `{path}` template still renders one block per file.
#[test]
fn per_file_guidance_does_not_collapse() {
    let outcomes = [
        reported("src/a.rs", "source", Severity::Soft, 400, "Split src/a.rs."),
        reported("src/b.rs", "source", Severity::Soft, 380, "Split src/b.rs."),
    ];

    let contexts: Vec<_> = outcomes.iter().map(context).collect();
    let blocks = finding_blocks_with_context(&outcomes, false, &contexts);

    assert_eq!(blocks.len(), 2);
    assert!(blocks[0].ends_with(
        "Split src/a.rs.\n    src/a.rs: 400 non-blank lines (budget 350; an exception here would accept 400)"
    ));
    assert!(blocks[1].ends_with(
        "Split src/b.rs.\n    src/b.rs: 380 non-blank lines (budget 350; an exception here would accept 400)"
    ));
}

#[test]
fn guidance_wraps_at_a_fixed_width() {
    let long = "Move a cohesive group of items into a sibling module along a seam \
                that already exists, rather than cutting the file at the line count.";
    let outcomes = [reported("src/a.rs", "source", Severity::Soft, 400, long)];

    let contexts: Vec<_> = outcomes.iter().map(context).collect();
    let block = finding_blocks_with_context(&outcomes, false, &contexts).remove(0);
    let guidance: Vec<&str> = block
        .lines()
        .filter(|line| line.starts_with("  ") && !line.starts_with("    "))
        .collect();

    assert!(guidance.len() > 1, "the long guidance wrapped");
    for line in &guidance {
        assert!(line.chars().count() <= GUIDANCE_COLUMNS + 2, "{line}");
    }
    // Wrapping only inserts line breaks; no word is lost or broken.
    let rejoined: Vec<&str> = guidance
        .iter()
        .flat_map(|line| line.split_whitespace())
        .collect();
    assert_eq!(rejoined, long.split_whitespace().collect::<Vec<_>>());
}

/// Explicit newlines are the project's own paragraphing and survive wrapping.
#[test]
fn newlines_in_a_message_are_kept() {
    let outcomes = [reported(
        "src/a.rs",
        "source",
        Severity::Soft,
        400,
        "Should split.\nfissile exception add <path> --severity soft --rule source",
    )];

    let contexts: Vec<_> = outcomes.iter().map(context).collect();
    let block = finding_blocks_with_context(&outcomes, false, &contexts).remove(0);

    assert!(block.contains(
        "\n  Should split.\n  fissile exception add <path> --severity soft --rule source\n"
    ));
}

/// §FS-001-config.3.1, §FS-004-check-audit.1: each UTF-8 line policy is named
/// in the per-file detail, with the crossed limit carried beside the count.
#[test]
fn line_details_name_each_counting_policy() {
    for ((count_blank, count_comment), expected, actual) in [
        ((true, true), "physical lines", 4),
        ((false, true), "non-blank lines", 3),
        ((true, false), "non-comment lines", 3),
        ((false, false), "non-blank, non-comment lines", 2),
    ] {
        let rule = Rule::new(
            "source",
            Selector::All,
            Budget::new(Unit::Lines, Some(1), None),
            MessageTemplate::new("m", "Split it."),
        )
        .with_line_policy(count_blank, count_comment);
        let checker = Checker::new(vec![rule]).expect("valid checker");
        let file = crate::measure_text("src/policy.rs", "a\n\n// comment\nb\n");
        let hits = checker.evaluate(&file).expect("evaluation succeeds");
        let outcomes =
            evaluate_hits(&Registries::default(), &file, &hits).expect("reporting succeeds");
        let contexts = contexts_for_file(&file, &hits, true, &Bump::default());
        let block = finding_blocks_with_context(&outcomes, false, &contexts).remove(0);

        assert!(
            block.contains(&format!(
                "src/policy.rs: {actual} {expected} (budget 1; an exception here would accept 100)"
            )),
            "wrong detail for ({count_blank}, {count_comment}): {block}"
        );
    }
}

/// §FS-001-config.3.1: the raw-byte fallback is described as physical lines,
/// not as a UTF-8 policy that was never applied.
#[test]
fn non_utf8_line_details_name_physical_lines() {
    let rule = Rule::new(
        "source",
        Selector::All,
        Budget::new(Unit::Lines, Some(2), None),
        MessageTemplate::new("m", "Split it."),
    )
    .with_line_policy(false, false);
    let checker = Checker::new(vec![rule]).expect("valid checker");
    let file = FileMeasurement::new("src/binary.rs", 4).with_lines(3);
    let hits = checker.evaluate(&file).expect("evaluation succeeds");
    let outcomes = evaluate_hits(&Registries::default(), &file, &hits).expect("reporting succeeds");
    let contexts = contexts_for_file(&file, &hits, false, &Bump::default());
    let block = finding_blocks_with_context(&outcomes, false, &contexts).remove(0);

    assert!(block.contains(
        "src/binary.rs: 3 physical lines (budget 2; an exception here would accept 100)"
    ));
}

/// §FS-004-check-audit.1: non-line units lead with their historical detail
/// shape, and the ceiling opens a parenthesis of its own after it. Where the
/// ceiling is withheld there is no budget clause to keep the parenthesis open,
/// so the detail is the bare measurement — the one shape that prints none.
#[test]
fn byte_and_token_details_keep_their_unit_shape() {
    for (unit, hard, file, detail) in [
        (
            Unit::Bytes,
            None,
            FileMeasurement::new("src/data.bin", 3),
            "src/data.bin: 3 bytes (an exception here would accept 4096)",
        ),
        (
            Unit::Tokens,
            None,
            FileMeasurement::new("src/data.txt", 3).with_tokens(3),
            "src/data.txt: 3 tokens (an exception here would accept 1000)",
        ),
        // A soft ceiling landing on the rule's hard limit while the file is
        // still under it is refused, so nothing is named and nothing is opened
        // (§DF-010-stated-ceilings-are-exact.2).
        (
            Unit::Bytes,
            Some(4096),
            FileMeasurement::new("src/data.bin", 3),
            "src/data.bin: 3 bytes",
        ),
    ] {
        let rule = Rule::new(
            "size",
            Selector::All,
            Budget::new(unit, Some(2), hard),
            MessageTemplate::new("m", "Split it."),
        );
        let checker = Checker::new(vec![rule]).expect("valid checker");
        let hits = checker.evaluate(&file).expect("evaluation succeeds");
        let outcomes =
            evaluate_hits(&Registries::default(), &file, &hits).expect("reporting succeeds");
        let contexts = contexts_for_file(&file, &hits, true, &Bump::default());
        let block = finding_blocks_with_context(&outcomes, false, &contexts).remove(0);

        assert!(
            block.ends_with(&format!("    {detail}")),
            "wrong {unit} detail: {block}"
        );
    }
}

/// §FS-004-check-audit.1: the ceiling named is the measurement quantized to the
/// unit's step, and it is named exactly where a plain `fissile exception add`
/// would accept it. The end-to-end cases pin the two ends of that predicate;
/// every clause of it is here, including the steps that quantize nothing.
#[test]
fn a_ceiling_is_named_where_a_plain_exception_would_be_accepted() {
    for (severity, actual, step, hard_limit, expected) in [
        // Nothing binds a hard ceiling (§DF-010-stated-ceilings-are-exact.2).
        (Severity::Hard, 620, 100, Some(550), Some(700)),
        // A soft ceiling under the hard limit is written as it stands.
        (Severity::Soft, 410, 100, Some(550), Some(500)),
        // On that limit, for a file still under it, `add` refuses it.
        (Severity::Soft, 502, 100, Some(550), None),
        // Past the limit the soft entry is the record of the debt and stands.
        (Severity::Soft, 620, 100, Some(550), Some(700)),
        // A rule setting no hard limit binds nothing at either severity.
        (Severity::Soft, 502, 100, None, Some(600)),
        // A step of 0 or 1 quantizes nothing: the ceiling is the measurement,
        // which is still what a plain `add` writes (§FS-001-config.5).
        (Severity::Soft, 502, 1, None, Some(502)),
        (Severity::Soft, 502, 0, None, Some(502)),
    ] {
        let outcome = reported("src/a.rs", "source", severity, actual, "Split it.");
        let context = context_with(&outcome, step, hard_limit);

        assert_eq!(
            context.would_accept(outcome.overflow()),
            expected,
            "{severity} {actual} against step {step} and hard limit {hard_limit:?}"
        );
    }
}

/// §FS-001-config.5, §DF-006-quantized-ceilings.1: the step is the tree's own
/// `[exceptions.bump]` and each unit takes its own, so a line detail extends the
/// budget clause it already has and a byte or token detail opens one.
#[test]
fn the_ceiling_reads_the_configured_step_for_each_unit() {
    let bump = Bump {
        lines: 10,
        bytes: 100,
        tokens: 250,
    };
    for (unit, file, detail) in [
        (
            Unit::Lines,
            crate::measure_text("src/big.rs", "a\nb\nc\n"),
            "src/big.rs: 3 non-blank lines (budget 2; an exception here would accept 10)",
        ),
        (
            Unit::Bytes,
            FileMeasurement::new("src/data.bin", 3),
            "src/data.bin: 3 bytes (an exception here would accept 100)",
        ),
        (
            Unit::Tokens,
            FileMeasurement::new("src/data.txt", 3).with_tokens(3),
            "src/data.txt: 3 tokens (an exception here would accept 250)",
        ),
    ] {
        let rule = Rule::new(
            "size",
            Selector::All,
            Budget::new(unit, Some(2), None),
            MessageTemplate::new("m", "Split it."),
        );
        let checker = Checker::new(vec![rule]).expect("valid checker");
        let hits = checker.evaluate(&file).expect("evaluation succeeds");
        let outcomes =
            evaluate_hits(&Registries::default(), &file, &hits).expect("reporting succeeds");
        let contexts = contexts_for_file(&file, &hits, true, &bump);
        let block = finding_blocks_with_context(&outcomes, false, &contexts).remove(0);

        assert!(block.ends_with(&format!("    {detail}")), "{block}");
    }
}
