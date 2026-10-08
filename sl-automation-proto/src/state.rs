//! Waiting on the viewer's state rather than on one widget: the scene settling,
//! a probe's readout coming to hold a value, an entry reaching a log.
//!
//! A [`Probe`] names one of the readouts of [`crate::probe`]; a
//! [`StateCondition`] is a predicate over one of them (or over the event log),
//! evaluated in the viewer each frame; a [`StateObservation`] is what it saw
//! when the condition held — or, in a timeout, the last thing it saw.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::probe::{
    AgentReadout, ConversationReadout, EnvironmentReadout, InventoryFolderReadout, InventoryRoot,
    LogEntry, LogStream, NotificationReadout, QuiescenceReadout, SelectedObject, StatusReadout,
    WorldMapReadout,
};

/// One of the viewer's state readouts.
///
/// In JSON the readout is a tag, with its arguments beside it:
/// `{"probe":"agent"}`, `{"probe":"inventory","path":["Objects"]}`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "probe", rename_all = "snake_case", deny_unknown_fields)]
pub enum Probe {
    /// The own agent ([`AgentReadout`]).
    Agent,
    /// What the status bar shows ([`StatusReadout`]).
    Status,
    /// Every open conversation and its transcript
    /// ([`ConversationReadout`]s).
    Conversations,
    /// Every notification still in the viewer's history
    /// ([`NotificationReadout`]s).
    Notifications,
    /// The edit selection ([`SelectedObject`]s).
    Selection,
    /// One inventory folder by path ([`InventoryFolderReadout`]).
    Inventory {
        /// Which inventory the path starts in.
        #[serde(default)]
        root: InventoryRoot,
        /// Each segment a sub-folder's exact name; empty for the root itself.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        path: Vec<String>,
    },
    /// Every folder reachable from an inventory root, each with what the
    /// viewer knows of its contents ([`InventoryFolderReadout`]s) — the whole
    /// tree in one read, addressed by id rather than by name, so two sibling
    /// folders of one name are both there.
    InventoryTree {
        /// Which inventory to walk.
        #[serde(default)]
        root: InventoryRoot,
    },
    /// Whether the scene has settled ([`QuiescenceReadout`]).
    Quiescence,
    /// The environment being drawn ([`EnvironmentReadout`]).
    Environment,
    /// What the world map knows ([`WorldMapReadout`]).
    WorldMap,
}

impl fmt::Display for Probe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Agent => f.write_str("agent"),
            Self::Status => f.write_str("status"),
            Self::Conversations => f.write_str("conversations"),
            Self::Notifications => f.write_str("notifications"),
            Self::Selection => f.write_str("selection"),
            Self::Inventory { root, path } => {
                let root = match root {
                    InventoryRoot::Agent => "agent",
                    InventoryRoot::Library => "library",
                };
                write!(f, "inventory {root}:/{}", path.join("/"))
            }
            Self::InventoryTree { root } => match root {
                InventoryRoot::Agent => f.write_str("inventory tree agent"),
                InventoryRoot::Library => f.write_str("inventory tree library"),
            },
            Self::Quiescence => f.write_str("quiescence"),
            Self::Environment => f.write_str("environment"),
            Self::WorldMap => f.write_str("world map"),
        }
    }
}

/// What a [`Probe`] read.
///
/// In JSON the readout is beside the probe's tag:
/// `{"probe":"selection","readout":[]}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "probe", content = "readout", rename_all = "snake_case")]
pub enum ProbeReadout {
    /// The own agent.
    Agent(AgentReadout),
    /// The status bar.
    Status(StatusReadout),
    /// Every open conversation, Nearby first.
    Conversations(Vec<ConversationReadout>),
    /// Every notification in the history, oldest first.
    Notifications(Vec<NotificationReadout>),
    /// The edit selection in selection order, the primary last.
    Selection(Vec<SelectedObject>),
    /// The inventory folder, or nothing while no folder is at the path.
    Inventory(Option<InventoryFolderReadout>),
    /// Every folder of the tree, the root first and each folder before its
    /// sub-folders; empty while the root is not known.
    InventoryTree(Vec<InventoryFolderReadout>),
    /// Whether the scene has settled.
    Quiescence(QuiescenceReadout),
    /// The environment being drawn.
    Environment(EnvironmentReadout),
    /// What the world map knows.
    WorldMap(WorldMapReadout),
}

/// A test of the JSON value a [`StateCondition::Probe`]'s pointer selects in a
/// readout.
///
/// In JSON: `"present"`, `"absent"`, `{"equals":…}` or `{"includes":…}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueTest {
    /// The pointer selects something that is not `null`.
    Present,
    /// The pointer selects nothing, or `null`.
    Absent,
    /// The value is exactly this.
    Equals(serde_json::Value),
    /// The value includes this ([`includes`]): an object every key of this one
    /// with a value that includes this one's, an array holding an element that
    /// includes each of this one's elements (or this one itself, when it is
    /// not an array), a string containing this one, or otherwise an equal
    /// value.
    Includes(serde_json::Value),
}

impl ValueTest {
    /// Whether the value the pointer selected — `None` when it selects
    /// nothing — passes the test.
    #[must_use]
    pub fn holds(&self, value: Option<&serde_json::Value>) -> bool {
        let value = value.filter(|value| !value.is_null());
        match self {
            Self::Present => value.is_some(),
            Self::Absent => value.is_none(),
            Self::Equals(wanted) => value == Some(wanted),
            Self::Includes(wanted) => value.is_some_and(|value| includes(value, wanted)),
        }
    }
}

impl fmt::Display for ValueTest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Present => f.write_str("is present"),
            Self::Absent => f.write_str("is absent"),
            Self::Equals(value) => write!(f, "equals {value}"),
            Self::Includes(value) => write!(f, "includes {value}"),
        }
    }
}

/// Whether `actual` includes `wanted`: the structural subset a
/// [`ValueTest::Includes`] asks for.
///
/// - an object includes an object when each of the wanted keys is present
///   with a value that includes the wanted one;
/// - an array includes an array when each wanted element is included by some
///   element (not necessarily a different one each), and includes anything
///   else when some element includes it;
/// - a string includes a string it contains;
/// - anything else includes only an equal value.
#[must_use]
pub fn includes(actual: &serde_json::Value, wanted: &serde_json::Value) -> bool {
    use serde_json::Value as JsonValue;
    match (actual, wanted) {
        (JsonValue::Object(actual), JsonValue::Object(wanted)) => {
            wanted.iter().all(|(key, wanted)| {
                actual
                    .get(key)
                    .is_some_and(|actual| includes(actual, wanted))
            })
        }
        (JsonValue::Array(actual), JsonValue::Array(wanted)) => wanted
            .iter()
            .all(|wanted| actual.iter().any(|actual| includes(actual, wanted))),
        (JsonValue::Array(actual), wanted) => actual.iter().any(|actual| includes(actual, wanted)),
        (JsonValue::String(actual), JsonValue::String(wanted)) => actual.contains(wanted.as_str()),
        (actual, wanted) => actual == wanted,
    }
}

/// A predicate over the viewer's state, evaluated in the viewer each frame.
///
/// In JSON the kind is a tag beside its arguments: `{"kind":"quiet"}`,
/// `{"kind":"probe","probe":{"probe":"agent"},"pointer":"/region/name","test":{"equals":"Home"}}`,
/// `{"kind":"logged","cursor":12,"kind_is":"ChatReceived"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StateCondition {
    /// The scene has settled: a region is up, nothing the scene asked for is
    /// outstanding, and no render pipeline is compiling
    /// ([`QuiescenceReadout::is_quiet`]).
    Quiet,
    /// A probe's readout, as JSON, holds a value: the value the JSON Pointer
    /// (RFC 6901, `""` for the whole readout) selects passes the test.
    Probe {
        /// The readout.
        probe: Probe,
        /// Where in it, as a JSON Pointer.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        pointer: String,
        /// What the value there must pass.
        test: ValueTest,
    },
    /// An entry reached the event log at or after a cursor.
    Logged {
        /// Only entries from this sequence number on.
        cursor: u64,
        /// Only entries of these streams; every stream when empty.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        streams: Vec<LogStream>,
        /// Only entries of this kind, exactly (`ChatReceived`, `Chat`,
        /// `toolbar.inventory`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind_is: Option<String>,
        /// Only entries whose printed detail contains this.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail_contains: Option<String>,
    },
}

impl StateCondition {
    /// Whether a log entry satisfies a [`StateCondition::Logged`]'s filters;
    /// always `false` for the other conditions.
    #[must_use]
    pub fn accepts_entry(&self, entry: &LogEntry) -> bool {
        match self {
            Self::Logged {
                cursor,
                streams,
                kind_is,
                detail_contains,
            } => {
                entry.seq >= *cursor
                    && (streams.is_empty() || streams.contains(&entry.stream))
                    && kind_is.as_ref().is_none_or(|kind| entry.kind == *kind)
                    && detail_contains
                        .as_ref()
                        .is_none_or(|part| entry.detail.contains(part.as_str()))
            }
            Self::Quiet | Self::Probe { .. } => false,
        }
    }
}

impl fmt::Display for StateCondition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Quiet => f.write_str("the scene to be quiet"),
            Self::Probe {
                probe,
                pointer,
                test,
            } => write!(f, "the {probe} readout at {pointer:?} {test}"),
            Self::Logged {
                cursor,
                streams,
                kind_is,
                detail_contains,
            } => {
                f.write_str("a log entry")?;
                if !streams.is_empty() {
                    let names: Vec<&str> = streams.iter().map(|stream| stream.as_str()).collect();
                    write!(f, " of {}", names.join("|"))?;
                }
                if let Some(kind) = kind_is {
                    write!(f, " of kind {kind:?}")?;
                }
                if let Some(part) = detail_contains {
                    write!(f, " containing {part:?}")?;
                }
                write!(f, " from #{cursor}")
            }
        }
    }
}

/// What a state wait saw: when its condition held, what made it hold; in a
/// timeout, the last thing it saw.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StateObservation {
    /// The scene's quiescence.
    Quiet {
        /// The readout.
        readout: QuiescenceReadout,
    },
    /// A probe's readout.
    Probe {
        /// The whole readout.
        readout: ProbeReadout,
    },
    /// A log entry.
    Logged {
        /// The entry: the one that matched, or in a timeout the last one read.
        entry: LogEntry,
        /// The cursor after it.
        next: u64,
    },
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::{Probe, ProbeReadout, StateCondition, StateObservation, ValueTest, includes};
    use crate::probe::{InventoryRoot, LogEntry, LogStream, QuiescenceReadout};

    #[test]
    fn includes_is_a_structural_subset() {
        let transcript = json!([
            {"conversation": {"kind": "nearby"}, "lines": [
                {"speaker": "Door", "text": "Locked tight", "own": false},
                {"speaker": "Me", "text": "hi", "own": true},
            ]},
        ]);
        assert!(includes(
            &transcript,
            &json!([{"conversation": {"kind": "nearby"}, "lines": [{"text": "Locked"}]}])
        ));
        assert!(
            includes(&transcript, &json!({"lines": [{"own": true}]})),
            "an array includes a non-array one of its elements includes"
        );
        assert!(!includes(
            &transcript,
            &json!([{"lines": [{"text": "bye"}]}])
        ));
        assert!(!includes(&json!({"a": 1}), &json!({"b": 1})), "missing key");
        assert!(includes(&json!(3), &json!(3)));
        assert!(!includes(&json!(3), &json!("3")));
    }

    #[test]
    fn a_test_reads_a_missing_or_null_value_as_absent() {
        let value = json!("Home");
        assert!(ValueTest::Present.holds(Some(&value)));
        assert!(!ValueTest::Present.holds(Some(&json!(null))));
        assert!(ValueTest::Absent.holds(None));
        assert!(ValueTest::Absent.holds(Some(&json!(null))));
        assert!(ValueTest::Equals(json!("Home")).holds(Some(&value)));
        assert!(!ValueTest::Equals(json!("Hme")).holds(Some(&value)));
        assert!(ValueTest::Includes(json!("om")).holds(Some(&value)));
        assert!(!ValueTest::Includes(json!("om")).holds(None));
    }

    #[test]
    fn a_logged_condition_filters_entries() {
        let entry = LogEntry {
            seq: 5,
            stream: LogStream::Event,
            kind: "ChatReceived".to_owned(),
            detail: "ChatReceived(Locked)".to_owned(),
        };
        let condition =
            |cursor, streams: Vec<LogStream>, kind: Option<&str>, part: Option<&str>| {
                StateCondition::Logged {
                    cursor,
                    streams,
                    kind_is: kind.map(ToOwned::to_owned),
                    detail_contains: part.map(ToOwned::to_owned),
                }
            };
        assert!(condition(5, Vec::new(), None, None).accepts_entry(&entry));
        assert!(!condition(6, Vec::new(), None, None).accepts_entry(&entry));
        assert!(!condition(0, vec![LogStream::Command], None, None).accepts_entry(&entry));
        assert!(
            condition(
                0,
                vec![LogStream::Event],
                Some("ChatReceived"),
                Some("Locked")
            )
            .accepts_entry(&entry)
        );
        assert!(!condition(0, Vec::new(), Some("Chat"), None).accepts_entry(&entry));
        assert!(!condition(0, Vec::new(), None, Some("Open")).accepts_entry(&entry));
        assert!(!StateCondition::Quiet.accepts_entry(&entry));
    }

    #[test]
    fn conditions_and_observations_round_trip() -> Result<(), serde_json::Error> {
        let conditions = [
            StateCondition::Quiet,
            StateCondition::Probe {
                probe: Probe::Inventory {
                    root: InventoryRoot::Library,
                    path: vec!["Objects".to_owned()],
                },
                pointer: "/items".to_owned(),
                test: ValueTest::Includes(json!([{"name": "Box"}])),
            },
            StateCondition::Logged {
                cursor: 12,
                streams: vec![LogStream::UiAction],
                kind_is: Some("toolbar.inventory".to_owned()),
                detail_contains: None,
            },
        ];
        for condition in conditions {
            let json = serde_json::to_string(&condition)?;
            assert_eq!(
                serde_json::from_str::<StateCondition>(&json)?,
                condition,
                "{json}"
            );
        }
        assert_eq!(
            serde_json::to_string(&StateCondition::Probe {
                probe: Probe::Agent,
                pointer: "/region/name".to_owned(),
                test: ValueTest::Equals(json!("Home")),
            })?,
            r#"{"kind":"probe","probe":{"probe":"agent"},"pointer":"/region/name","test":{"equals":"Home"}}"#
        );
        let observation = StateObservation::Probe {
            readout: ProbeReadout::Selection(Vec::new()),
        };
        let json = serde_json::to_string(&observation)?;
        assert_eq!(
            json,
            r#"{"kind":"probe","readout":{"probe":"selection","readout":[]}}"#
        );
        assert_eq!(
            serde_json::from_str::<StateObservation>(&json)?,
            observation
        );
        let quiet = StateObservation::Quiet {
            readout: QuiescenceReadout::default(),
        };
        let json = serde_json::to_string(&quiet)?;
        assert_eq!(serde_json::from_str::<StateObservation>(&json)?, quiet);
        for json in [
            r#"{"kind":"probe","probe":{"probe":"agent"},"pointr":"","test":"present"}"#,
            r#"{"kind":"logged","cursor":1,"kindis":"Chat"}"#,
            r#"{"kind":"probe","probe":{"probe":"inventory","route":[]},"test":"present"}"#,
        ] {
            assert!(
                serde_json::from_str::<StateCondition>(json).is_err(),
                "a misspelt or stray field is refused: {json}"
            );
        }
        Ok(())
    }

    #[test]
    fn display_says_what_is_awaited() {
        assert_eq!(
            StateCondition::Probe {
                probe: Probe::Inventory {
                    root: InventoryRoot::Agent,
                    path: vec!["Objects".to_owned(), "Boxes".to_owned()],
                },
                pointer: "/loaded".to_owned(),
                test: ValueTest::Equals(json!(true)),
            }
            .to_string(),
            r#"the inventory agent:/Objects/Boxes readout at "/loaded" equals true"#
        );
        assert_eq!(
            StateCondition::Logged {
                cursor: 3,
                streams: vec![LogStream::Event, LogStream::Command],
                kind_is: Some("Chat".to_owned()),
                detail_contains: Some("hi".to_owned()),
            }
            .to_string(),
            r#"a log entry of event|command of kind "Chat" containing "hi" from #3"#
        );
    }
}
