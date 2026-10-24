//! The **probe corpus** format: LSL scripts that print what a grid did as
//! machine-parsable lines, and the per-grid result files that keep what they
//! printed. The conventions are the book chapter `tools/lsl-probes.md`; the
//! corpus is `sl-lsl-runtime/probes/`.
//!
//! - A probe is one script, or several that run together in one or more
//!   prims; its id is its path below `probes/` without the extension
//!   (`events/queue-limit`, `errors/runtime-errors`).
//! - Every script opens with a [`Header`] of `// key: value` comment lines.
//! - What a probe prints is [`Line`]s:
//!   `PROBE <id> rec=<name> key=value…`: one `start`, from the script that
//!   leads it, and one `end`, last. In a probe of several scripts the ones
//!   saved before the leader may print first.
//! - A probe asks the runner for a stimulus — a touch, a line of chat, an
//!   answer to its permission question or its dialog, a sit, a stand, held
//!   controls, the owner leaving the region for a while — with a
//!   `waiting` record, read as a [`Stimulus`].
//! - A run's transcript is a [`ResultFile`]: a header of `key: value` lines,
//!   a blank line, then the `PROBE` lines with `#` notes between.
//! - Two runs are set side by side by [`compare`](compare::compare), under
//!   the tolerance its module describes.
//!
//! Like the rest of the crate this is I/O-free: callers read the files.

use std::collections::BTreeMap;

pub mod compare;

/// A stimulus a probe may ask the runner for, with a `waiting` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Drive {
    /// A touch of the probe's prim A.
    Touch,
    /// A line of chat on a channel.
    Chat,
    /// An answer to the permission question the probe's script puts.
    Answer,
    /// Sitting on the probe's prim A.
    Sit,
    /// Standing up.
    Stand,
    /// Holding movement controls down for a moment.
    Control,
    /// A reply to the dialog (`llDialog`, `llTextBox`) the probe's script
    /// puts.
    Dialog,
    /// The owner leaving the probe's region for a while.
    Away,
}

impl Drive {
    /// The drive a header or `waiting` record names.
    fn parse(name: &str) -> Option<Self> {
        match name {
            "touch" => Some(Self::Touch),
            "chat" => Some(Self::Chat),
            "answer" => Some(Self::Answer),
            "sit" => Some(Self::Sit),
            "stand" => Some(Self::Stand),
            "control" => Some(Self::Control),
            "dialog" => Some(Self::Dialog),
            "away" => Some(Self::Away),
            _ => None,
        }
    }
}

/// The `// key: value` lines that open a probe script.
#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    /// `probe`: the probe's id.
    pub probe: String,
    /// `drive`: `none`, or the [`Drive`]s the probe asks for, comma-separated.
    pub drives: Vec<Drive>,
    /// `script`: the inventory name the script must have — required in a
    /// probe of several scripts, which find each other by name.
    pub script: Option<String>,
    /// `prim`: which object holds the script (`A`, `B`, …); `A` when absent.
    pub prim: String,
    /// `order`: where the script comes in the order the scripts are saved —
    /// required in a probe of several scripts.
    pub order: Option<u32>,
    /// `distance`: how far east of prim A, in metres, this script's prim
    /// stands.
    pub distance: Option<f32>,
    /// `height`: how far above prim A, in metres, this script's prim
    /// stands — for a range no parcel is wide enough for, since a range is
    /// a distance in three dimensions.
    pub height: Option<f32>,
    /// `skip-on`: the [`GRIDS`] this script is left out on, comma-separated
    /// — where running it would take the region down rather than measure
    /// anything (the script's comment says why).
    pub skip_on: Vec<String>,
    /// `vary`: the fields that are measurements — a duration, a rate, a
    /// clock — which two runs need not agree on, comma-separated.
    pub vary: Vec<String>,
    /// `unordered`: the records whose order among a run of their own kind is
    /// not defined, comma-separated.
    pub unordered: Vec<String>,
}

/// Why a script's header was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HeaderError {
    /// A required key is absent.
    #[error("header `{0}` is missing")]
    Missing(&'static str),
    /// A key's value does not parse.
    #[error("header `{key}` is `{value}`, which does not parse")]
    Invalid {
        /// The key.
        key: String,
        /// Its value.
        value: String,
    },
    /// A key the format does not have.
    #[error("header `{0}` is not a probe header key")]
    Unknown(String),
}

impl Header {
    /// Reads the header of a script's source: the run of `// key: value`
    /// lines it opens with, up to the first line that is not one.
    ///
    /// # Errors
    ///
    /// A [`HeaderError`] when `probe` or `drive` is missing, a value does not
    /// parse, or a key is unknown.
    pub fn parse(source: &str) -> Result<Self, HeaderError> {
        let pairs: BTreeMap<&str, &str> = source
            .lines()
            .map_while(|line| line.strip_prefix("// "))
            .map_while(|line| line.split_once(": "))
            .take_while(|(key, _)| !key.contains(' '))
            .map(|(key, value)| (key, value.trim()))
            .collect();
        for key in pairs.keys() {
            if ![
                "probe",
                "drive",
                "script",
                "prim",
                "order",
                "distance",
                "height",
                "skip-on",
                "vary",
                "unordered",
            ]
            .contains(key)
            {
                return Err(HeaderError::Unknown((*key).to_owned()));
            }
        }
        let invalid = |key: &str, value: &str| HeaderError::Invalid {
            key: key.to_owned(),
            value: value.to_owned(),
        };
        let probe = (*pairs.get("probe").ok_or(HeaderError::Missing("probe"))?).to_owned();
        let drive = *pairs.get("drive").ok_or(HeaderError::Missing("drive"))?;
        let drives = if drive == "none" {
            Vec::new()
        } else {
            list(drive)
                .iter()
                .map(|name| Drive::parse(name))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| invalid("drive", drive))?
        };
        let order = pairs
            .get("order")
            .map(|value| value.parse().map_err(|_error| invalid("order", value)))
            .transpose()?;
        let metres = |key: &str| {
            pairs
                .get(key)
                .map(|value| {
                    value
                        .parse::<f32>()
                        .ok()
                        .filter(|metres| metres.is_finite() && *metres >= 0.0)
                        .ok_or_else(|| invalid(key, value))
                })
                .transpose()
        };
        let distance = metres("distance")?;
        let height = metres("height")?;
        let skip_on = match pairs.get("skip-on") {
            None => Vec::new(),
            Some(value) => {
                let grids = list(value);
                if !grids.iter().all(|grid| GRIDS.contains(&grid.as_str())) {
                    return Err(invalid("skip-on", value));
                }
                grids
            }
        };
        Ok(Self {
            probe,
            drives,
            script: pairs.get("script").map(|name| (*name).to_owned()),
            prim: pairs.get("prim").map_or("A", |prim| *prim).to_owned(),
            order,
            distance,
            height,
            skip_on,
            vary: pairs
                .get("vary")
                .map(|value| list(value))
                .unwrap_or_default(),
            unordered: pairs
                .get("unordered")
                .map(|value| list(value))
                .unwrap_or_default(),
        })
    }

    /// Whether the script is left out on `grid`.
    #[must_use]
    pub fn skipped_on(&self, grid: &str) -> bool {
        self.skip_on.iter().any(|skipped| skipped == grid)
    }
}

/// A header value's comma-separated items, trimmed.
fn list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|item| item.trim().to_owned())
        .collect()
}

/// One line a probe printed: `PROBE <id> rec=<name> key=value…`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// The probe's id.
    pub id: String,
    /// The record's name, the `rec` field, which comes first.
    pub rec: String,
    /// The other fields in order, quoted values unescaped.
    pub fields: Vec<(String, String)>,
}

/// Why a line is not a [`Line`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{reason}: {line}")]
pub struct LineError {
    /// What is wrong.
    pub reason: &'static str,
    /// The line.
    pub line: String,
}

/// The prefix every probe line starts with.
pub const PREFIX: &str = "PROBE ";

impl Line {
    /// Parses a line. A value is a bare token (no space, no `"`) or a `"…"`
    /// string with `\\`, `\"` and `\n` escapes — what the corpus's `q()`
    /// helper prints.
    ///
    /// # Errors
    ///
    /// A [`LineError`] when the line is not a probe line, `rec` is not its
    /// first field, or a field does not parse.
    pub fn parse(line: &str) -> Result<Self, LineError> {
        let refuse = |reason| LineError {
            reason,
            line: line.to_owned(),
        };
        let rest = line
            .strip_prefix(PREFIX)
            .ok_or_else(|| refuse("not a PROBE line"))?;
        let (id, mut rest) = rest.split_once(' ').ok_or_else(|| refuse("no fields"))?;
        let mut fields = Vec::new();
        while !rest.is_empty() {
            let (key, after) = rest
                .split_once('=')
                .ok_or_else(|| refuse("a field without `=`"))?;
            if key.is_empty() || key.contains([' ', '"']) {
                return Err(refuse("a bad key"));
            }
            let (value, after) = if let Some(quoted) = after.strip_prefix('"') {
                unquote(quoted).ok_or_else(|| refuse("a bad quoted value"))?
            } else {
                let (value, after) = after.split_at(after.find(' ').unwrap_or(after.len()));
                if value.contains('"') {
                    return Err(refuse("a quote inside a bare value"));
                }
                (value.to_owned(), after)
            };
            rest = match after.strip_prefix(' ') {
                Some(next) => next,
                None if after.is_empty() => after,
                None => return Err(refuse("no space after a value")),
            };
            fields.push((key.to_owned(), value));
        }
        let mut fields = fields.into_iter();
        let rec = match fields.next() {
            Some((key, rec)) if key == "rec" => rec,
            _ => return Err(refuse("the first field is not `rec`")),
        };
        Ok(Self {
            id: id.to_owned(),
            rec,
            fields: fields.collect(),
        })
    }

    /// The value of the field `key`, if the line has it.
    #[must_use]
    pub fn field(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }
}

/// Which avatar gives a [`Stimulus`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum By {
    /// The probe's owner, the runner's own avatar.
    Owner,
    /// A second avatar, which owns nothing of the probe's.
    Other,
}

/// What a stimulus is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Touch prim A.
    Touch,
    /// Say `text` on `channel`, at normal range.
    Chat {
        /// The channel.
        channel: i32,
        /// What is said.
        text: String,
    },
    /// Answer the permission question a probe's script puts to the avatar
    /// with the mask `grant`, as it is — `0` denies, and a mask beyond what
    /// was asked is sent too.
    Answer {
        /// The permissions granted.
        grant: i32,
    },
    /// Sit on prim A.
    Sit,
    /// Stand up.
    Stand,
    /// Hold the movement controls `keys` (the `CONTROL_*` bits, which are
    /// the agent's own control flags) for a moment, then let go.
    Control {
        /// The controls held.
        keys: u32,
    },
    /// Reply `reply` to the dialog a probe's script puts to the avatar: the
    /// label of the button pressed, which need not be one of the dialog's,
    /// or the text typed into an `llTextBox`.
    Dialog {
        /// The label or text sent back.
        reply: String,
    },
    /// Leave the probe's region for `seconds`: the owner teleports to
    /// another, stays, and comes back to where it stood.
    Away {
        /// How long the owner stays away.
        seconds: u32,
    },
}

impl Action {
    /// Its [`Drive`].
    #[must_use]
    pub const fn drive(&self) -> Drive {
        match self {
            Self::Touch => Drive::Touch,
            Self::Chat { .. } => Drive::Chat,
            Self::Answer { .. } => Drive::Answer,
            Self::Sit => Drive::Sit,
            Self::Stand => Drive::Stand,
            Self::Control { .. } => Drive::Control,
            Self::Dialog { .. } => Drive::Dialog,
            Self::Away { .. } => Drive::Away,
        }
    }
}

/// What a `waiting` record asks the runner for:
/// `rec=waiting drive=touch|chat|answer|sit|stand|control|dialog|away
/// [by=owner|other]`, and for chat `channel=<n> text="…"`, for an answer
/// `grant=<mask>`, for controls `keys=<mask>`, for a dialog `reply="…"`, for
/// away `seconds=<n>`. Without `by` it is the owner; only the owner goes
/// `away`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stimulus {
    /// Who gives it.
    pub by: By,
    /// What it is.
    pub action: Action,
}

impl Stimulus {
    /// Reads a `waiting` record.
    ///
    /// # Errors
    ///
    /// What is wrong with it: not a `waiting` record, an unknown drive or
    /// avatar, a chat without its channel or text, a dialog without its
    /// reply, an away without its seconds, or the other avatar sent away.
    pub fn from_waiting(line: &Line) -> Result<Self, String> {
        if line.rec != "waiting" {
            return Err(format!("`{}` is not a `waiting` record", line.rec));
        }
        let by = match line.field("by") {
            None | Some("owner") => By::Owner,
            Some("other") => By::Other,
            Some(other) => return Err(format!("`by={other}` is neither owner nor other")),
        };
        let drive = line
            .field("drive")
            .ok_or("a `waiting` record without `drive`")?;
        let action = match Drive::parse(drive) {
            Some(Drive::Touch) => Action::Touch,
            Some(Drive::Chat) => Action::Chat {
                channel: line
                    .field("channel")
                    .and_then(|channel| channel.parse().ok())
                    .ok_or("a chat drive without an integer `channel`")?,
                text: line
                    .field("text")
                    .ok_or("a chat drive without `text`")?
                    .to_owned(),
            },
            Some(Drive::Answer) => Action::Answer {
                grant: line
                    .field("grant")
                    .and_then(|grant| grant.parse().ok())
                    .ok_or("an answer drive without an integer `grant`")?,
            },
            Some(Drive::Sit) => Action::Sit,
            Some(Drive::Stand) => Action::Stand,
            Some(Drive::Control) => Action::Control {
                keys: line
                    .field("keys")
                    .and_then(|keys| keys.parse().ok())
                    .ok_or("a control drive without an unsigned integer `keys`")?,
            },
            Some(Drive::Dialog) => Action::Dialog {
                reply: line
                    .field("reply")
                    .ok_or("a dialog drive without `reply`")?
                    .to_owned(),
            },
            Some(Drive::Away) => Action::Away {
                seconds: line
                    .field("seconds")
                    .and_then(|seconds| seconds.parse().ok())
                    .ok_or("an away drive without an unsigned integer `seconds`")?,
            },
            None => return Err(format!("`drive={drive}` is not a drive")),
        };
        if by == By::Other && matches!(action, Action::Away { .. }) {
            return Err(format!("`drive={drive}` is the owner's alone"));
        }
        Ok(Self { by, action })
    }
}

/// Reads a quoted value from just after its opening quote: the value and
/// what follows its closing quote, or `None` for a bad escape or no closing
/// quote.
fn unquote(quoted: &str) -> Option<(String, &str)> {
    let mut value = String::new();
    let mut chars = quoted.char_indices();
    loop {
        match chars.next()? {
            (at, '"') => return Some((value, quoted.get(at.saturating_add(1)..)?)),
            (_, '\\') => match chars.next()? {
                (_, '\\') => value.push('\\'),
                (_, '"') => value.push('"'),
                (_, 'n') => value.push('\n'),
                _ => return None,
            },
            (_, other) => value.push(other),
        }
    }
}

/// The grids a result may be for: the live grids, then the fake grid's
/// flavours.
pub const GRIDS: &[&str] = &[
    "aditi",
    "opensim-yengine",
    "opensim-xengine",
    "fake-sl",
    "fake-opensim",
];

/// The header keys every result file carries, in the order they are
/// written.
pub const RESULT_KEYS: &[&str] = &["grid", "date", "runner", "region", "engine", "settings"];

/// One run's transcript: `probes/results/<grid>/<probe id>.txt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultFile {
    /// The header, in file order: the [`RESULT_KEYS`], and `complete: no`
    /// for a run that never printed its `end`.
    pub header: Vec<(String, String)>,
    /// The body: `PROBE` lines and `#` notes, in the order they came.
    pub body: Vec<String>,
}

impl ResultFile {
    /// Splits a result file's text into its header and body.
    ///
    /// # Errors
    ///
    /// The header line that is not `key: value`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut lines = text.lines();
        let mut header = Vec::new();
        for line in lines.by_ref() {
            if line.is_empty() {
                break;
            }
            match line.split_once(": ") {
                Some((key, value)) if !key.contains(' ') && !value.trim().is_empty() => {
                    header.push((key.to_owned(), value.trim().to_owned()));
                }
                _ => return Err(format!("a bad header line `{line}`")),
            }
        }
        let body = lines
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect();
        Ok(Self { header, body })
    }

    /// The header value of `key`.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.header
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    /// Whether the run printed its `end`, as the header says.
    #[must_use]
    pub fn complete(&self) -> bool {
        self.get("complete") != Some("no")
    }

    /// The file's text.
    #[must_use]
    pub fn render(&self) -> String {
        let mut text = String::new();
        for (key, value) in &self.header {
            text.push_str(key);
            text.push_str(": ");
            text.push_str(value);
            text.push('\n');
        }
        text.push('\n');
        for line in &self.body {
            text.push_str(line);
            text.push('\n');
        }
        text
    }

    /// What is wrong with this file as the run of probe `id` on `grid`: the
    /// header keys, the date, and the transcript — every line a probe line
    /// of `id` or a `#` note, exactly one `start` and, unless the header
    /// says `complete: no`, exactly one `end`, last.
    #[must_use]
    pub fn problems(&self, grid: &str, id: &str) -> Vec<String> {
        let mut problems = Vec::new();
        for key in RESULT_KEYS {
            if self.get(key).is_none() {
                problems.push(format!("header `{key}` is missing"));
            }
        }
        if self.get("grid").is_some_and(|value| value != grid) {
            problems.push(format!("header `grid` is not `{grid}`"));
        }
        if self.get("date").is_some_and(|date| !is_date(date)) {
            problems.push("header `date` is not YYYY-MM-DD".to_owned());
        }
        if !matches!(self.get("complete"), None | Some("yes" | "no")) {
            problems.push("header `complete` is neither yes nor no".to_owned());
        }
        let mut records = Vec::new();
        for line in &self.body {
            if line.starts_with('#') {
                continue;
            }
            match Line::parse(line) {
                Ok(parsed) if parsed.id != id => {
                    problems.push(format!("a line for `{}`", parsed.id));
                }
                Ok(parsed) => records.push(parsed.rec),
                Err(error) => problems.push(error.to_string()),
            }
        }
        if records.iter().filter(|rec| *rec == "start").count() != 1 {
            problems.push("the run does not hold one `start`".to_owned());
        }
        let ends = records.iter().filter(|rec| *rec == "end").count();
        if self.complete() {
            if ends != 1 || records.last().map(String::as_str) != Some("end") {
                problems.push("the run does not end with one `end`".to_owned());
            }
        } else if ends != 0 {
            problems.push("`complete: no`, yet the run printed `end`".to_owned());
        }
        problems
    }
}

/// Whether `text` reads as a `YYYY-MM-DD` date.
fn is_date(text: &str) -> bool {
    let parts: Vec<&str> = text.split('-').collect();
    parts.iter().map(|part| part.len()).eq([4, 2, 2])
        && parts
            .iter()
            .all(|part| part.chars().all(|c| c.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{Action, By, Drive, Header, HeaderError, Line, ResultFile, Stimulus};

    #[test]
    fn a_header_reads_up_to_the_first_other_line() -> Result<(), HeaderError> {
        let header = Header::parse(
            "// probe: errors/runtime-errors\n// script: Div int\n// prim: B\n\
             // order: 3\n// distance: 50\n// height: 99\n// drive: none\n\
             // skip-on: opensim-xengine, fake-sl\n//\n// probe: not/this\n",
        )?;
        assert_eq!(
            header,
            Header {
                probe: "errors/runtime-errors".to_owned(),
                drives: Vec::new(),
                script: Some("Div int".to_owned()),
                prim: "B".to_owned(),
                order: Some(3),
                distance: Some(50.0),
                height: Some(99.0),
                skip_on: vec!["opensim-xengine".to_owned(), "fake-sl".to_owned()],
                vary: Vec::new(),
                unordered: Vec::new(),
            }
        );
        assert!(header.skipped_on("fake-sl"));
        assert!(!header.skipped_on("aditi"));
        Ok(())
    }

    #[test]
    fn a_header_lists_its_drives_and_its_tolerances() -> Result<(), HeaderError> {
        let header = Header::parse(
            "// probe: a/b\n// drive: chat, touch, answer, sit, stand, control, dialog, away\n\
             // vary: seconds, rate\n// unordered: heard\n",
        )?;
        assert_eq!(
            header.drives,
            vec![
                Drive::Chat,
                Drive::Touch,
                Drive::Answer,
                Drive::Sit,
                Drive::Stand,
                Drive::Control,
                Drive::Dialog,
                Drive::Away
            ]
        );
        assert_eq!(header.vary, vec!["seconds".to_owned(), "rate".to_owned()]);
        assert_eq!(header.unordered, vec!["heard".to_owned()]);
        Ok(())
    }

    #[test]
    fn a_waiting_record_names_its_stimulus() -> Result<(), String> {
        let read = |line: &str| {
            Line::parse(line)
                .map_err(|error| error.to_string())
                .and_then(|line| Stimulus::from_waiting(&line))
        };
        assert_eq!(
            read("PROBE a/b rec=waiting drive=touch")?,
            Stimulus {
                by: By::Owner,
                action: Action::Touch
            }
        );
        assert_eq!(
            read(r#"PROBE a/b rec=waiting drive=chat by=other channel=-7 text="hi there""#)?,
            Stimulus {
                by: By::Other,
                action: Action::Chat {
                    channel: -7,
                    text: "hi there".to_owned()
                }
            }
        );
        assert_eq!(
            read("PROBE a/b rec=waiting drive=answer by=other grant=3092")?,
            Stimulus {
                by: By::Other,
                action: Action::Answer { grant: 3092 }
            }
        );
        assert_eq!(
            read("PROBE a/b rec=waiting drive=control keys=257")?,
            Stimulus {
                by: By::Owner,
                action: Action::Control { keys: 257 }
            }
        );
        assert_eq!(
            read("PROBE a/b rec=waiting drive=sit by=other")?.action,
            Action::Sit
        );
        assert_eq!(
            read("PROBE a/b rec=waiting drive=stand")?.action,
            Action::Stand
        );
        assert_eq!(
            read(r#"PROBE a/b rec=waiting drive=dialog by=other reply="Not a button""#)?,
            Stimulus {
                by: By::Other,
                action: Action::Dialog {
                    reply: "Not a button".to_owned()
                }
            }
        );
        assert_eq!(
            read("PROBE a/b rec=waiting drive=away by=owner seconds=20")?.action,
            Action::Away { seconds: 20 }
        );
        for refused in [
            "PROBE a/b rec=waiting drive=answer",
            "PROBE a/b rec=waiting drive=answer grant=all",
            "PROBE a/b rec=waiting drive=control keys=-1",
            "PROBE a/b rec=start",
            "PROBE a/b rec=waiting",
            "PROBE a/b rec=waiting drive=wave",
            "PROBE a/b rec=waiting drive=touch by=everyone",
            "PROBE a/b rec=waiting drive=chat text=hi",
            "PROBE a/b rec=waiting drive=chat channel=7",
            "PROBE a/b rec=waiting drive=dialog",
            "PROBE a/b rec=waiting drive=away",
            "PROBE a/b rec=waiting drive=away by=other seconds=20",
        ] {
            assert!(read(refused).is_err(), "{refused}");
        }
        Ok(())
    }

    #[test]
    fn a_header_without_its_drive_or_with_a_stray_key_is_refused() {
        assert_eq!(
            Header::parse("// probe: a/b\n"),
            Err(HeaderError::Missing("drive"))
        );
        assert_eq!(
            Header::parse("// probe: a/b\n// drive: sometimes\n"),
            Err(HeaderError::Invalid {
                key: "drive".to_owned(),
                value: "sometimes".to_owned()
            })
        );
        assert_eq!(
            Header::parse("// probe: a/b\n// drive: none\n// skip-on: mainland\n"),
            Err(HeaderError::Invalid {
                key: "skip-on".to_owned(),
                value: "mainland".to_owned()
            })
        );
        assert_eq!(
            Header::parse("// probe: a/b\n// drive: none\n// colour: red\n"),
            Err(HeaderError::Unknown("colour".to_owned()))
        );
    }

    #[test]
    fn a_probe_line_parses_bare_and_quoted_values() -> Result<(), super::LineError> {
        let parsed = Line::parse(
            r#"PROBE errors/runtime-errors rec=heard name="Object" dist=49.5 msg="a \"b\"\nc\\" len=7"#,
        )?;
        assert_eq!(
            parsed,
            Line {
                id: "errors/runtime-errors".to_owned(),
                rec: "heard".to_owned(),
                fields: vec![
                    ("name".to_owned(), "Object".to_owned()),
                    ("dist".to_owned(), "49.5".to_owned()),
                    ("msg".to_owned(), "a \"b\"\nc\\".to_owned()),
                    ("len".to_owned(), "7".to_owned()),
                ],
            }
        );
        assert_eq!(parsed.field("dist"), Some("49.5"));
        Ok(())
    }

    #[test]
    fn a_probe_line_without_rec_first_or_with_a_stray_quote_is_refused() {
        for line in [
            "PROBE a/b at=1 rec=start",
            "PROBE a/b rec=start x=\"open",
            "PROBE a/b rec=start x=a\"b",
            "PROBE a/b rec=start x=\"a\"b",
            "PROBE a/b rec=start x=\"bad \\t escape\"",
            "PROBE a/b",
            "chat PROBE a/b rec=start",
        ] {
            assert!(Line::parse(line).is_err(), "{line}");
        }
    }

    #[test]
    fn a_result_file_round_trips_and_holds_its_run_to_the_format() -> Result<(), String> {
        let text = "grid: aditi\ndate: 2026-10-23\nrunner: sl-conformance\n\
                    region: Somewhere\nengine: Mono\nsettings: Linden Lab's\n\n\
                    PROBE a/b rec=start\n# a note\nPROBE a/b rec=end\n";
        let file = ResultFile::parse(text)?;
        assert_eq!(file.render(), text);
        assert_eq!(file.problems("aditi", "a/b"), Vec::<String>::new());
        assert_eq!(
            file.problems("opensim-yengine", "a/c"),
            vec![
                "header `grid` is not `opensim-yengine`".to_owned(),
                "a line for `a/b`".to_owned(),
                "a line for `a/b`".to_owned(),
                "the run does not hold one `start`".to_owned(),
                "the run does not end with one `end`".to_owned(),
            ]
        );
        Ok(())
    }

    #[test]
    fn an_incomplete_run_says_so_and_prints_no_end() -> Result<(), String> {
        let file = ResultFile::parse(
            "grid: aditi\ndate: 2026-10-23\nrunner: r\nregion: r\nengine: Mono\n\
             settings: s\ncomplete: no\n\nPROBE a/b rec=start\n",
        )?;
        assert!(!file.complete());
        assert_eq!(file.problems("aditi", "a/b"), Vec::<String>::new());
        Ok(())
    }
}
