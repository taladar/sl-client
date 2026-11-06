//! One corpus case: a script, the header that says how to run it, and where
//! its expected transcripts live.
//!
//! The header is the script's leading `//` lines, before the first line of
//! code:
//!
//! ```text
//! // corpus: what the case pins, in one line
//! // flavours: secondlife yengine xengine
//! // ticks: 20
//! // at 3: touch
//! // at 4: chat 7 hello there
//! // at 5: link 42 some text 0a0a0a0a-0000-4000-8000-000000000001
//! // at 6: changed 1
//! // at 1: rez 99
//! ```
//!
//! `corpus` is required; `flavours` defaults to all three engines and `ticks`
//! to [`DEFAULT_TICKS`]. An `at` line posts a stimulus before the tick it
//! names runs. Any other lowercase key is a mistake and fails the case, so a
//! misspelt header cannot silently run the case some other way; a comment
//! that is not `key: value` — the case's notes, its `Oracle:` — is prose.

use std::path::{Path, PathBuf};

use sl_lsl_runtime::Dialect;
use sl_lsl_runtime::vm::EngineFlavour;

/// How many ticks a case runs when its header does not say: two seconds at
/// the corpus's tenth-of-a-second step.
const DEFAULT_TICKS: u64 = 20;

/// The engines a case can run under, by the name a header gives them.
pub(crate) const FLAVOURS: [(&str, EngineFlavour); 3] = [
    ("secondlife", EngineFlavour::SecondLife),
    ("yengine", EngineFlavour::YEngine),
    ("xengine", EngineFlavour::XEngine),
];

/// The compiler a flavour's grid compiles with.
pub(crate) const fn dialect(flavour: EngineFlavour) -> Dialect {
    if flavour.is_opensim() {
        Dialect::OpenSim
    } else {
        Dialect::SecondLife
    }
}

/// Something done to the script from outside, before a tick runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Stimulus {
    /// The owner touches the prim: `touch_start`, then `touch_end`.
    Touch,
    /// The owner says `message` on `channel`.
    Chat {
        /// The channel.
        channel: i32,
        /// What is said.
        message: String,
    },
    /// A link message from link 1: `link_message(1, num, text, id)`.
    Link {
        /// The number.
        num: i32,
        /// The string.
        text: String,
        /// The key.
        id: String,
    },
    /// `changed(bits)`.
    Changed(i32),
    /// The object is rezzed with a start parameter: `on_rez(param)`.
    Rez(i32),
}

impl Stimulus {
    /// The stimulus an `at` line's text after the colon names.
    fn parse(text: &str) -> Result<Self, String> {
        let (verb, rest) = text.split_once(' ').unwrap_or((text, ""));
        let integer = |word: &str| {
            word.parse::<i32>()
                .map_err(|error| format!("{word:?} is no integer: {error}"))
        };
        match verb {
            "touch" if rest.is_empty() => Ok(Self::Touch),
            "chat" => {
                let (channel, message) = rest.split_once(' ').unwrap_or((rest, ""));
                Ok(Self::Chat {
                    channel: integer(channel)?,
                    message: message.to_owned(),
                })
            }
            "link" => {
                let (num, rest) = rest.split_once(' ').unwrap_or((rest, ""));
                let (text, id) = rest.rsplit_once(' ').unwrap_or((rest, ""));
                Ok(Self::Link {
                    num: integer(num)?,
                    text: text.to_owned(),
                    id: id.to_owned(),
                })
            }
            "changed" => Ok(Self::Changed(integer(rest)?)),
            "rez" => Ok(Self::Rez(integer(rest)?)),
            _ => Err(format!("no stimulus {text:?}")),
        }
    }
}

/// Whether the text before a header comment's colon is a key: one lowercase
/// word, or `at` and a tick. Anything else — `Oracle:`, a sentence that
/// happens to hold a colon — is prose.
fn is_header_key(key: &str) -> bool {
    let key = key.trim();
    let word = |text: &str| !text.is_empty() && text.chars().all(|c| c.is_ascii_lowercase());
    key.strip_prefix("at ")
        .map_or_else(|| word(key), |tick| tick.trim().parse::<u64>().is_ok())
}

/// One case of the corpus.
#[derive(Debug, Clone)]
pub(crate) struct Case {
    /// Its path below the corpus root, without the extension
    /// (`values/casts`): what a failure names it by.
    pub(crate) id: String,
    /// The `.lsl` file.
    pub(crate) script: PathBuf,
    /// The script's source.
    pub(crate) source: String,
    /// The engines it runs under, by name.
    pub(crate) flavours: Vec<(&'static str, EngineFlavour)>,
    /// How many ticks it runs.
    pub(crate) ticks: u64,
    /// What is done to it, by the tick before which it is done.
    pub(crate) stimuli: Vec<(u64, Stimulus)>,
}

impl Case {
    /// Reads the case whose script is `script`, `id` below the corpus root.
    pub(crate) fn read(id: String, script: &Path) -> Result<Self, String> {
        let source = fs_err::read_to_string(script).map_err(|error| error.to_string())?;
        let mut described = false;
        let mut flavours = FLAVOURS.to_vec();
        let mut ticks = DEFAULT_TICKS;
        let mut stimuli = Vec::new();
        for line in source.lines() {
            let Some(comment) = line.strip_prefix("//") else {
                break;
            };
            let comment = comment.trim();
            // Prose between the header lines is the case's own notes.
            let Some((key, value)) = comment
                .split_once(':')
                .filter(|(key, _)| is_header_key(key))
            else {
                continue;
            };
            let value = value.trim();
            match key.trim() {
                "corpus" => described = !value.is_empty(),
                "flavours" => {
                    flavours = value
                        .split_whitespace()
                        .map(|name| {
                            FLAVOURS
                                .iter()
                                .find(|(known, _)| *known == name)
                                .copied()
                                .ok_or_else(|| format!("{id}: no flavour {name:?}"))
                        })
                        .collect::<Result<_, _>>()?;
                }
                "ticks" => {
                    ticks = value
                        .parse()
                        .map_err(|error| format!("{id}: ticks {value:?}: {error}"))?;
                }
                other => {
                    let Some(tick) = other.strip_prefix("at ") else {
                        return Err(format!("{id}: unknown header key {other:?}"));
                    };
                    let tick = tick
                        .trim()
                        .parse::<u64>()
                        .map_err(|error| format!("{id}: at {tick:?}: {error}"))?;
                    let stimulus =
                        Stimulus::parse(value).map_err(|error| format!("{id}: {error}"))?;
                    stimuli.push((tick, stimulus));
                }
            }
        }
        if !described {
            return Err(format!("{id}: the header has no `// corpus:` line"));
        }
        if flavours.is_empty() {
            return Err(format!("{id}: the header names no flavour"));
        }
        Ok(Self {
            id,
            script: script.to_owned(),
            source,
            flavours,
            ticks,
            stimuli,
        })
    }

    /// The expectation every flavour shares.
    pub(crate) fn shared_expectation(&self) -> PathBuf {
        self.script.with_extension("expected")
    }

    /// The expectation of one flavour, kept when the flavours disagree.
    pub(crate) fn flavour_expectation(&self, flavour: &str) -> PathBuf {
        self.script.with_extension(format!("{flavour}.expected"))
    }
}
