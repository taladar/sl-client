//! Comparing two runs of one probe: what one grid printed against another,
//! or a run against the committed one.
//!
//! Two transcripts never agree byte for byte, and most of what differs is
//! not a finding. The tolerance model says what may legitimately differ:
//!
//! - **minted ids**: any key other than `NULL_KEY`, wherever it appears in a
//!   value, reads as one placeholder;
//! - **a float's last digit**: two decimals agree when they are within one
//!   unit of the last place the coarser of them prints (`5.0` against
//!   `5.000000`, `0.123456` against `0.123457`); an integer has to match;
//!   a vector or rotation agrees component by component;
//! - **measurements**: the fields a probe's header lists under `vary` (a
//!   duration, a rate, a clock) are set side by side, never counted as a
//!   difference;
//! - **context**: the `start` record's fields (region, channel, version)
//!   say where the run was made, not what the grid did;
//! - **order**: the lines printed before `start` (scripts saved before the
//!   leader) come in no defined order, nor does a run of the records a
//!   header lists under `unordered`; both are compared as sets.
//!
//! What is left after that is a [`Difference`]: a record one run printed and
//! the other did not, or a field whose values disagree. The `#` notes are
//! not compared: what they carry that matters (a run-time error's text) is
//! what a probe prints itself, and the rest names the run's own objects.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use super::{Header, Line, ResultFile};

/// The record whose fields are the run's context.
const START: &str = "start";

/// What [`compare`] lets differ, from a probe's headers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tolerance {
    /// The fields that are measurements (`vary`).
    pub vary: BTreeSet<String>,
    /// The records whose order among their own kind is not defined
    /// (`unordered`).
    pub unordered: BTreeSet<String>,
}

impl Tolerance {
    /// The union of what every script of a probe declares.
    pub fn of<'a>(headers: impl IntoIterator<Item = &'a Header>) -> Self {
        let mut tolerance = Self::default();
        for header in headers {
            tolerance.vary.extend(header.vary.iter().cloned());
            tolerance.unordered.extend(header.unordered.iter().cloned());
        }
        tolerance
    }
}

/// Which of the two runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The first, the baseline.
    Left,
    /// The second.
    Right,
}

/// One thing two runs disagree on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Difference {
    /// A record only one run printed.
    Only {
        /// The run that printed it.
        side: Side,
        /// The line.
        line: String,
    },
    /// A field of two matched records whose values disagree; `None` where
    /// a record lacks the field.
    Field {
        /// The record's name and the fields both runs agree on.
        record: String,
        /// The field.
        field: String,
        /// Its value in the left run.
        left: Option<String>,
        /// Its value in the right run.
        right: Option<String>,
    },
    /// A body line that is neither a `#` note nor a probe line.
    Unreadable {
        /// The run that holds it.
        side: Side,
        /// The line.
        line: String,
    },
}

/// A measurement of two matched records, set side by side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Measured {
    /// The record's name and the fields both runs agree on.
    pub record: String,
    /// The field.
    pub field: String,
    /// Its value in the left run.
    pub left: Option<String>,
    /// Its value in the right run.
    pub right: Option<String>,
}

/// What [`compare`] found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Comparison {
    /// The differences, in transcript order.
    pub differences: Vec<Difference>,
    /// The measurements that disagree, in transcript order.
    pub measured: Vec<Measured>,
    /// The runs that never printed their `end`.
    pub incomplete: Vec<Side>,
}

impl Comparison {
    /// Whether the two runs agree, measurements aside.
    #[must_use]
    pub const fn agrees(&self) -> bool {
        self.differences.is_empty() && self.incomplete.is_empty()
    }

    /// The report's lines, the runs named `left` and `right`.
    #[must_use]
    pub fn lines(&self, left: &str, right: &str) -> Vec<String> {
        let name = |side: Side| match side {
            Side::Left => left,
            Side::Right => right,
        };
        let value = |value: &Option<String>| value.as_deref().unwrap_or("(absent)").to_owned();
        let mut lines = Vec::new();
        for side in &self.incomplete {
            lines.push(format!("incomplete  {} never printed `end`", name(*side)));
        }
        for difference in &self.differences {
            lines.push(match difference {
                Difference::Only { side, line } => format!("only        {}: {line}", name(*side)),
                Difference::Field {
                    record,
                    field,
                    left: left_value,
                    right: right_value,
                } => format!(
                    "differs     {record}: {field} {} | {}",
                    value(left_value),
                    value(right_value)
                ),
                Difference::Unreadable { side, line } => {
                    format!("unreadable  {}: {line}", name(*side))
                }
            });
        }
        for measured in &self.measured {
            lines.push(format!(
                "measured    {}: {} {} | {}",
                measured.record,
                measured.field,
                value(&measured.left),
                value(&measured.right)
            ));
        }
        lines
    }
}

/// Compares two runs of one probe under `tolerance`.
#[must_use]
pub fn compare(left: &ResultFile, right: &ResultFile, tolerance: &Tolerance) -> Comparison {
    let mut comparison = Comparison::default();
    for (side, file) in [(Side::Left, left), (Side::Right, right)] {
        if !file.complete() {
            comparison.incomplete.push(side);
        }
    }
    let left_lines = arrange(Side::Left, left, tolerance, &mut comparison);
    let right_lines = arrange(Side::Right, right, tolerance, &mut comparison);
    let mut gap = Gap::default();
    for step in align(&left_lines, &right_lines, |left_line, right_line| {
        matches(left_line, right_line, tolerance)
    }) {
        match step {
            Step::Both(at_left, at_right) => {
                gap.close(&left_lines, &right_lines, tolerance, &mut comparison);
                if let (Some(left_line), Some(right_line)) =
                    (left_lines.get(at_left), right_lines.get(at_right))
                {
                    compare_fields(left_line, right_line, tolerance, &mut comparison);
                }
            }
            Step::Only(Side::Left, at) => gap.left.push(at),
            Step::Only(Side::Right, at) => gap.right.push(at),
        }
    }
    gap.close(&left_lines, &right_lines, tolerance, &mut comparison);
    comparison
}

/// The lines between two matched ones that matched nothing: a left line
/// and a right line of one record are the same record with a field that
/// differs; the rest are records only one run printed.
#[derive(Debug, Default)]
struct Gap {
    /// The left lines' indices.
    left: Vec<usize>,
    /// The right lines' indices.
    right: Vec<usize>,
}

impl Gap {
    /// Reports the gap's lines and empties it.
    fn close(
        &mut self,
        left: &[Line],
        right: &[Line],
        tolerance: &Tolerance,
        comparison: &mut Comparison,
    ) {
        let mut unpaired: Vec<Option<&Line>> =
            self.right.drain(..).map(|at| right.get(at)).collect();
        for left_line in self.left.drain(..).filter_map(|at| left.get(at)) {
            let partner = unpaired
                .iter_mut()
                .find(|slot| slot.is_some_and(|right_line| right_line.rec == left_line.rec))
                .and_then(Option::take);
            match partner {
                Some(right_line) => compare_fields(left_line, right_line, tolerance, comparison),
                None => comparison.differences.push(Difference::Only {
                    side: Side::Left,
                    line: render(left_line),
                }),
            }
        }
        for right_line in unpaired.into_iter().flatten() {
            comparison.differences.push(Difference::Only {
                side: Side::Right,
                line: render(right_line),
            });
        }
    }
}

/// Whether two lines are the same record with every field agreeing,
/// measurements aside.
fn matches(left: &Line, right: &Line, tolerance: &Tolerance) -> bool {
    let mut scratch = Comparison::default();
    left.rec == right.rec && {
        compare_fields(left, right, tolerance, &mut scratch);
        scratch.differences.is_empty()
    }
}

/// A run's probe lines in the order they are compared in: the lines before
/// `start` sorted, then the rest with each run of an `unordered` record
/// sorted. A line that does not parse goes to `comparison` instead.
fn arrange(
    side: Side,
    file: &ResultFile,
    tolerance: &Tolerance,
    comparison: &mut Comparison,
) -> Vec<Line> {
    let mut lines = Vec::new();
    for text in &file.body {
        if text.starts_with('#') {
            continue;
        }
        match Line::parse(text) {
            Ok(line) => lines.push(line),
            Err(_error) => comparison.differences.push(Difference::Unreadable {
                side,
                line: text.clone(),
            }),
        }
    }
    let key = |line: &Line| sort_key(line, tolerance);
    let mut arranged: Vec<Line> = Vec::with_capacity(lines.len());
    let mut lines = lines.into_iter().peekable();
    let mut before: Vec<Line> = Vec::new();
    while let Some(line) = lines.next_if(|line| line.rec != START) {
        before.push(line);
    }
    if lines.peek().is_none() {
        // No `start` at all: nothing is known to be unordered.
        arranged.extend(before);
        return arranged;
    }
    before.sort_by_key(key);
    arranged.extend(before);
    while let Some(line) = lines.next() {
        if !tolerance.unordered.contains(&line.rec) {
            arranged.push(line);
            continue;
        }
        let rec = line.rec.clone();
        let mut run = vec![line];
        while let Some(next) = lines.next_if(|next| next.rec == rec) {
            run.push(next);
        }
        run.sort_by_key(key);
        arranged.extend(run);
    }
    arranged
}

/// What a line sorts by among lines that come in no defined order: its
/// record and the fields that are not measurements, ids read as one.
fn sort_key(line: &Line, tolerance: &Tolerance) -> String {
    let mut key = line.rec.clone();
    for (field, value) in &line.fields {
        if tolerance.vary.contains(field) {
            continue;
        }
        key.push(' ');
        key.push_str(field);
        key.push('=');
        key.push_str(&mask_keys(value));
    }
    key
}

/// Compares the fields of two matched records.
fn compare_fields(left: &Line, right: &Line, tolerance: &Tolerance, comparison: &mut Comparison) {
    if left.rec == START {
        return;
    }
    let mut names: Vec<&str> = left
        .fields
        .iter()
        .map(|(field, _)| field.as_str())
        .collect();
    for (field, _) in &right.fields {
        if !names.contains(&field.as_str()) {
            names.push(field);
        }
    }
    let fields: Vec<FieldPair<'_>> = names
        .into_iter()
        .map(|field| {
            let (left_value, right_value) = (left.field(field), right.field(field));
            let measurement = tolerance.vary.contains(field);
            let agree = match (left_value, right_value) {
                (Some(left_value), Some(right_value)) if measurement => left_value == right_value,
                (Some(left_value), Some(right_value)) => same_value(left_value, right_value),
                (None, None) => true,
                _ => false,
            };
            FieldPair {
                field,
                left: left_value,
                right: right_value,
                measurement,
                agree,
            }
        })
        .collect();
    let mut record = left.rec.clone();
    for pair in &fields {
        if pair.agree
            && !pair.measurement
            && let Some(value) = pair.left
        {
            record.push(' ');
            record.push_str(pair.field);
            record.push('=');
            record.push_str(&render_value(value));
        }
    }
    for pair in fields.into_iter().filter(|pair| !pair.agree) {
        let (record, field, left, right) = (
            record.clone(),
            pair.field.to_owned(),
            pair.left.map(str::to_owned),
            pair.right.map(str::to_owned),
        );
        if pair.measurement {
            comparison.measured.push(Measured {
                record,
                field,
                left,
                right,
            });
        } else {
            comparison.differences.push(Difference::Field {
                record,
                field,
                left,
                right,
            });
        }
    }
}

/// One field of two matched records.
struct FieldPair<'a> {
    /// The field.
    field: &'a str,
    /// Its value in the left record.
    left: Option<&'a str>,
    /// Its value in the right record.
    right: Option<&'a str>,
    /// Whether it is a measurement.
    measurement: bool,
    /// Whether the two values agree.
    agree: bool,
}

/// Whether two values agree: equal once ids are masked, or equal numbers
/// (or vectors of them) to the last digit the coarser prints.
fn same_value(left: &str, right: &str) -> bool {
    let (left, right) = (mask_keys(left), mask_keys(right));
    if left == right {
        return true;
    }
    if let (Some(left), Some(right)) = (components(&left), components(&right)) {
        return left.len() == right.len()
            && left
                .iter()
                .zip(&right)
                .all(|(left, right)| same_number(left, right));
    }
    same_number(&left, &right)
}

/// The components of a `<x, y, z>` or `<x, y, z, s>` value.
fn components(value: &str) -> Option<Vec<&str>> {
    let inner = value.strip_prefix('<')?.strip_suffix('>')?;
    Some(inner.split(',').map(str::trim).collect())
}

/// Whether two numbers agree: integers exactly, decimals within one unit of
/// the last place the coarser prints.
fn same_number(left: &str, right: &str) -> bool {
    let (Ok(left_number), Ok(right_number)) = (left.parse::<f64>(), right.parse::<f64>()) else {
        return false;
    };
    if !left_number.is_finite() || !right_number.is_finite() {
        return false;
    }
    let places = match (decimals(left), decimals(right)) {
        (Some(left_places), Some(right_places)) => left_places.min(right_places),
        // An integer against anything has to match exactly.
        _ => return left_number.total_cmp(&right_number) == Ordering::Equal,
    };
    let unit = i32::try_from(places).map_or(0.0, |places| 10_f64.powi(places).recip());
    // The slack absorbs the binary rounding of the decimal unit itself.
    (left_number - right_number).abs() <= unit * (1.0 + 1e-9)
}

/// How many decimals a number prints, or `None` for an integer (or a form
/// with an exponent, which is compared as one).
fn decimals(number: &str) -> Option<usize> {
    if number.contains(['e', 'E']) {
        return None;
    }
    number.split_once('.').map(|(_, fraction)| fraction.len())
}

/// `value` with every key but `NULL_KEY` read as `<key>`. A key is 36
/// characters of hex in groups of 8, 4, 4, 4 and 12 with no hex digit on
/// either side.
fn mask_keys(value: &str) -> String {
    let mut masked = String::with_capacity(value.len());
    let mut rest = value;
    let mut after_hex = false;
    while let Some(next) = rest.chars().next() {
        if !after_hex
            && let Some(candidate) = rest.get(..KEY_LEN)
            && is_key(candidate)
            && !rest.get(KEY_LEN..).is_some_and(starts_hex)
        {
            masked.push_str(if candidate == NULL_KEY {
                NULL_KEY
            } else {
                "<key>"
            });
            rest = rest.get(KEY_LEN..).unwrap_or_default();
            after_hex = true;
            continue;
        }
        masked.push(next);
        after_hex = next.is_ascii_hexdigit();
        rest = rest.get(next.len_utf8()..).unwrap_or_default();
    }
    masked
}

/// The length of a key's text.
const KEY_LEN: usize = 36;

/// `NULL_KEY`'s text.
const NULL_KEY: &str = "00000000-0000-0000-0000-000000000000";

/// Whether `text` starts with a hex digit.
fn starts_hex(text: &str) -> bool {
    text.starts_with(|c: char| c.is_ascii_hexdigit())
}

/// Whether `text` is a key: hex in groups of 8, 4, 4, 4 and 12.
fn is_key(text: &str) -> bool {
    let groups: Vec<&str> = text.split('-').collect();
    groups.iter().map(|group| group.len()).eq([8, 4, 4, 4, 12])
        && groups
            .iter()
            .all(|group| group.chars().all(|c| c.is_ascii_hexdigit()))
}

/// A line as a probe printed it.
fn render(line: &Line) -> String {
    let mut text = format!("{}{} rec={}", super::PREFIX, line.id, line.rec);
    for (field, value) in &line.fields {
        text.push(' ');
        text.push_str(field);
        text.push('=');
        text.push_str(&render_value(value));
    }
    text
}

/// A value as a probe prints it: bare, or quoted when it has to be.
fn render_value(value: &str) -> String {
    if !value.is_empty() && !value.contains([' ', '"', '\\', '\n']) {
        return value.to_owned();
    }
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("\"{escaped}\"")
}

/// One step of an alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// The left line at the first index matches the right one at the second.
    Both(usize, usize),
    /// The line at the index is only in that side's run.
    Only(Side, usize),
}

/// The longest common subsequence of two line sequences under `same`, as
/// the steps that walk both.
fn align<T>(left: &[T], right: &[T], same: impl Fn(&T, &T) -> bool) -> Vec<Step> {
    // `table[i][j]`: the common length of `left[i..]` and `right[j..]`.
    let width = right.len().saturating_add(1);
    let mut table: Vec<Vec<usize>> = vec![vec![0; width]];
    for left_item in left.iter().rev() {
        let below = table.last().cloned().unwrap_or_default();
        let mut row = vec![0; width];
        for (at, right_item) in right.iter().enumerate().rev() {
            let next = at.saturating_add(1);
            let length = if same(left_item, right_item) {
                below.get(next).copied().unwrap_or(0).saturating_add(1)
            } else {
                let down = below.get(at).copied().unwrap_or(0);
                let across = row.get(next).copied().unwrap_or(0);
                down.max(across)
            };
            if let Some(cell) = row.get_mut(at) {
                *cell = length;
            }
        }
        table.push(row);
    }
    table.reverse();
    let cell = |at_left: usize, at_right: usize| {
        table
            .get(at_left)
            .and_then(|row| row.get(at_right))
            .copied()
            .unwrap_or(0)
    };
    let (mut at_left, mut at_right) = (0, 0);
    let mut steps = Vec::new();
    while at_left < left.len() || at_right < right.len() {
        let (next_left, next_right) = (at_left.saturating_add(1), at_right.saturating_add(1));
        match (left.get(at_left), right.get(at_right)) {
            (Some(left_item), Some(right_item)) if same(left_item, right_item) => {
                steps.push(Step::Both(at_left, at_right));
                (at_left, at_right) = (next_left, next_right);
            }
            (Some(_), Some(_)) if cell(next_left, at_right) >= cell(at_left, next_right) => {
                steps.push(Step::Only(Side::Left, at_left));
                at_left = next_left;
            }
            (Some(_) | None, Some(_)) => {
                steps.push(Step::Only(Side::Right, at_right));
                at_right = next_right;
            }
            (Some(_), None) => {
                steps.push(Step::Only(Side::Left, at_left));
                at_left = next_left;
            }
            (None, None) => break,
        }
    }
    steps
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{
        Comparison, Difference, Measured, Side, Tolerance, compare, mask_keys, same_value,
    };
    use crate::probe::ResultFile;

    /// A complete result file of `body` lines.
    fn run(body: &[&str]) -> Result<ResultFile, String> {
        ResultFile::parse(&format!(
            "grid: aditi\ndate: 2026-10-24\nrunner: r\nregion: r\nengine: e\n\
             settings: s\n\n{}\n",
            body.join("\n")
        ))
    }

    /// A tolerance of these `vary` fields and `unordered` records.
    fn tolerance(vary: &[&str], unordered: &[&str]) -> Tolerance {
        Tolerance {
            vary: vary.iter().map(|field| (*field).to_owned()).collect(),
            unordered: unordered.iter().map(|rec| (*rec).to_owned()).collect(),
        }
    }

    #[test]
    fn ids_floats_and_vectors_agree_within_the_tolerance() {
        assert!(same_value(
            "fab01e52-3517-4545-be1b-4cbf6d8ace3b",
            "5c041d94-690c-490e-a2aa-eaf65f61a10e"
        ));
        assert!(!same_value(
            "00000000-0000-0000-0000-000000000000",
            "5c041d94-690c-490e-a2aa-eaf65f61a10e"
        ));
        assert_eq!(
            mask_keys("primID: fab01e52-3517-4545-be1b-4cbf6d8ace3b at"),
            "primID: <key> at"
        );
        assert_eq!(mask_keys("deadbeef00"), "deadbeef00");
        assert!(same_value("5.0", "5.000000"));
        assert!(same_value("0.123456", "0.123457"));
        assert!(!same_value("0.123456", "0.123458"));
        assert!(same_value(
            "<1.0, 2.0, 3.0>",
            "<1.000000,2.000000,3.000000>"
        ));
        assert!(!same_value("<1.0, 2.0, 3.0>", "<1.0, 2.0>"));
        assert!(!same_value("64", "65"));
        assert!(!same_value("64", "64.4"));
        assert!(same_value("64", "64.0"));
    }

    #[test]
    fn runs_that_differ_only_where_tolerated_agree() -> Result<(), String> {
        let left = run(&[
            "PROBE a/b rec=listening channel=1",
            "PROBE a/b rec=listening channel=2",
            "PROBE a/b rec=start region=\"Mauve\"",
            "# touch",
            "PROBE a/b rec=heard dist=10.000000 id=fab01e52-3517-4545-be1b-4cbf6d8ace3b",
            "PROBE a/b rec=heard dist=50.000000 id=fab01e52-3517-4545-be1b-4cbf6d8ace3b",
            "PROBE a/b rec=loop seconds=1.97 per_second=505586",
            "PROBE a/b rec=end",
        ])?;
        let right = run(&[
            "PROBE a/b rec=listening channel=2",
            "PROBE a/b rec=start region=\"Default Region\"",
            "PROBE a/b rec=listening channel=1",
            "PROBE a/b rec=heard dist=50.0 id=5c041d94-690c-490e-a2aa-eaf65f61a10e",
            "PROBE a/b rec=heard dist=10.0 id=5c041d94-690c-490e-a2aa-eaf65f61a10e",
            "PROBE a/b rec=loop seconds=0.03 per_second=30303030",
            "PROBE a/b rec=end",
        ])?;
        let comparison = compare(
            &left,
            &right,
            &tolerance(&["seconds", "per_second"], &["heard"]),
        );
        // `listening channel=1` on the right came after `start`, where the
        // order is defined: it is a record only the right printed, and the
        // left's lone one before `start` is only the left's.
        assert_eq!(
            comparison,
            Comparison {
                differences: vec![
                    Difference::Only {
                        side: Side::Left,
                        line: "PROBE a/b rec=listening channel=1".to_owned()
                    },
                    Difference::Only {
                        side: Side::Right,
                        line: "PROBE a/b rec=listening channel=1".to_owned()
                    },
                ],
                measured: vec![
                    Measured {
                        record: "loop".to_owned(),
                        field: "seconds".to_owned(),
                        left: Some("1.97".to_owned()),
                        right: Some("0.03".to_owned())
                    },
                    Measured {
                        record: "loop".to_owned(),
                        field: "per_second".to_owned(),
                        left: Some("505586".to_owned()),
                        right: Some("30303030".to_owned())
                    },
                ],
                incomplete: Vec::new(),
            }
        );
        Ok(())
    }

    #[test]
    fn a_finding_is_a_field_that_disagrees_or_a_record_one_run_lacks() -> Result<(), String> {
        let left = run(&[
            "PROBE a/b rec=start",
            "PROBE a/b rec=received sent=100 got=64",
            "PROBE a/b rec=end",
        ])?;
        let right = ResultFile::parse(
            "grid: opensim-xengine\ndate: 2026-10-24\nrunner: r\nregion: r\nengine: e\n\
             settings: s\ncomplete: no\n\nPROBE a/b rec=start\n\
             PROBE a/b rec=received sent=100 got=100 extra=1\n\
             PROBE a/b rec=answer g=42\n",
        )?;
        let comparison = compare(&left, &right, &Tolerance::default());
        assert!(!comparison.agrees());
        assert_eq!(
            comparison.lines("aditi", "opensim-xengine"),
            vec![
                "incomplete  opensim-xengine never printed `end`",
                "differs     received sent=100: got 64 | 100",
                "differs     received sent=100: extra (absent) | 1",
                "only        aditi: PROBE a/b rec=end",
                "only        opensim-xengine: PROBE a/b rec=answer g=42",
            ]
        );
        Ok(())
    }
}
