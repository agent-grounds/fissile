//! Mixed-unit grouping, aggregation and stable ordering (§FS-004-check-audit.1).

use super::*;

fn finding(path: &str, unit: Unit, actual: u64, text: &str) -> Outcome {
    Outcome::Reported(Overflow {
        path: path.into(),
        rule_id: "source".into(),
        severity: Severity::Soft,
        unit,
        actual,
        limit: 10,
        message: RenderedMessage {
            id: "split".into(),
            text: text.into(),
        },
    })
}

#[test]
fn issue_99_shared_id_and_guidance_keep_all_three_units_separate() {
    let outcomes = [
        finding("src/a.rs", Unit::Tokens, 30, "Split it."),
        finding("src/a.rs", Unit::Lines, 20, "Split it."),
        finding("src/a.rs", Unit::Bytes, 100, "Split it."),
        finding("src/b.rs", Unit::Bytes, 80, "Split it."),
    ];
    let blocks = finding_blocks(&outcomes, false);
    assert_eq!(
        blocks.len(),
        3,
        "separate units, same-unit files aggregate: {blocks:#?}"
    );
    for (block, unit, count) in [
        (&blocks[0], "byte", "2 files"),
        (&blocks[1], "line", "1 file"),
        (&blocks[2], "token", "1 file"),
    ] {
        assert_eq!(
            block.lines().next().unwrap(),
            format!("soft: {count} over the 10-{unit} budget [rule: source, message: split]")
        );
        assert_eq!(block.matches("  Split it.").count(), 1);
        let details: Vec<_> = block
            .lines()
            .filter(|line| line.starts_with("    "))
            .collect();
        assert_eq!(details.len(), if unit == "byte" { 2 } else { 1 });
        assert!(
            details
                .iter()
                .all(|line| line.ends_with(&format!("{unit}s"))),
            "{block}"
        );
    }
}

/// Distinct rendered text forces separate blocks even before #99 is fixed,
/// isolating the ordering defect. Message IDs and every existing order key tie.
/// Reverse the actual outcomes passed to rendering, not merely config order.
#[test]
fn issue_99_unit_order_is_independent_of_finding_arrival() {
    let mut outcomes = vec![
        finding("src/a.rs", Unit::Bytes, 100, "Split bytes."),
        finding("src/a.rs", Unit::Lines, 20, "Split lines."),
        finding("src/a.rs", Unit::Tokens, 30, "Split tokens."),
    ];
    let forward = finding_blocks(&outcomes, false);
    assert_eq!(forward.len(), 3);
    for (block, unit) in forward.iter().zip(["byte", "line", "token"]) {
        assert!(
            block
                .lines()
                .next()
                .unwrap()
                .contains(&format!("10-{unit} budget")),
            "{block}"
        );
    }
    outcomes.reverse();
    let reverse = finding_blocks(&outcomes, false);
    assert_eq!(
        reverse, forward,
        "unit order must survive reversed finding arrival"
    );
}

/// Control: existing same-unit aggregation must survive the new grouping key.
#[test]
fn issue_99_same_unit_files_still_share_one_block() {
    let outcomes = [
        finding("src/b.rs", Unit::Bytes, 80, "Split it."),
        finding("src/a.rs", Unit::Bytes, 100, "Split it."),
    ];
    assert_eq!(
        finding_blocks(&outcomes, false),
        [
            "soft: 2 files over the 10-byte budget [rule: source, message: split]\n  Split it.\n    src/a.rs: 100 bytes\n    src/b.rs: 80 bytes"
        ]
    );
}
