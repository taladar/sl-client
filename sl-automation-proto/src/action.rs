//! The arguments of the requests that act on the world: what a world action
//! does to its target, what a world wait waits for, and a drag of one of the
//! build tool's transform handles.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::locator::Locator;

/// What a world action does to the one thing its locator names.
///
/// In JSON: `"click"`, `"right_click"`, `"hover"`, `"select"`,
/// `"shift_select"`, `"place"`, or `{"drop_from":{…locator…}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldAction {
    /// A left click: a touch, outside build mode.
    Click,
    /// A right click: the thing's pie menu.
    RightClick,
    /// The pointer rests over it: its hover tip.
    Hover,
    /// A left click in build mode, which selects it. Waits for the build tool
    /// to be active on a tool that selects; outside it the same click would
    /// touch, and with the Create tool it would rez.
    Select,
    /// A left click with `Shift` held in build mode, which toggles it in the
    /// selection and keeps the rest selected — how a second object joins a
    /// selection (to link it, say). Waits as [`Select`](Self::Select) does.
    ShiftSelect,
    /// A left click with the build tool's Create tool, which rezzes the
    /// picked shape on the thing's surface. Waits for the Create tool to be
    /// the active one; with any other tool the same click would select.
    Place,
    /// Drag what the UI node this locator names carries (an inventory row)
    /// and drop it on the thing.
    DropFrom(Locator),
}

impl fmt::Display for WorldAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Click => f.write_str("click"),
            Self::RightClick => f.write_str("right_click"),
            Self::Hover => f.write_str("hover"),
            Self::Select => f.write_str("select"),
            Self::ShiftSelect => f.write_str("shift_select"),
            Self::Place => f.write_str("place"),
            Self::DropFrom(source) => write!(f, "drop_from({source})"),
        }
    }
}

/// What a wait on a world locator waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorldWaitCondition {
    /// At least one thing matches.
    Attached,
    /// Nothing matches.
    Detached,
}

impl WorldWaitCondition {
    /// The condition's serialized spelling, used for display too.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Attached => "attached",
            Self::Detached => "detached",
        }
    }
}

impl fmt::Display for WorldWaitCondition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How far a drag of a transform handle is to change the selection. Each
/// handle takes one kind.
///
/// In JSON: `{"distance":1.0}`, `{"offset":[1.0,0.5]}`, `{"angle":0.5}`,
/// `{"factor":2.0}`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DragAmount {
    /// Metres: along a move arrow, or the change of the primary object's
    /// extent on a stretch face.
    Distance(f32),
    /// Metres along the two other axes of a move pad, in x → y → z order.
    Offset([f32; 2]),
    /// Radians about a rotate ring's axis, right-handed.
    Angle(f32),
    /// The uniform scale factor of a stretch corner.
    Factor(f32),
}

/// Which side of the build tool's snap guide a handle drag ends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapSide {
    /// On the handle's line: the amount is applied as asked, snapping or not.
    #[default]
    Free,
    /// Past the snap guide, with snapping on: the result lands on the grid or
    /// a rotation detent nearest the amount.
    Grid,
}

/// The modifier keys held through a handle drag, which pick the rig.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DragModifiers {
    /// None: the build floater's own tool.
    #[default]
    None,
    /// `Shift`: on a move handle, leave a copy behind.
    Shift,
    /// `Ctrl`: the rotate rig.
    Ctrl,
    /// `Ctrl+Shift`: the stretch rig.
    CtrlShift,
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{DragAmount, DragModifiers, SnapSide, WorldAction, WorldWaitCondition};
    use crate::locator::Locator;

    #[test]
    fn arguments_round_trip_in_their_terse_spelling() -> Result<(), serde_json::Error> {
        for (action, json) in [
            (WorldAction::Click, r#""click""#),
            (WorldAction::RightClick, r#""right_click""#),
            (WorldAction::Hover, r#""hover""#),
            (WorldAction::Select, r#""select""#),
            (WorldAction::ShiftSelect, r#""shift_select""#),
            (WorldAction::Place, r#""place""#),
            (
                WorldAction::DropFrom(Locator::test_id("row")),
                r#"{"drop_from":{"test_id":"row"}}"#,
            ),
        ] {
            assert_eq!(serde_json::to_string(&action)?, json);
            assert_eq!(serde_json::from_str::<WorldAction>(json)?, action);
        }
        for amount in [
            DragAmount::Distance(1.5),
            DragAmount::Offset([1.0, -0.5]),
            DragAmount::Angle(0.25),
            DragAmount::Factor(2.0),
        ] {
            let json = serde_json::to_string(&amount)?;
            assert_eq!(serde_json::from_str::<DragAmount>(&json)?, amount, "{json}");
        }
        assert_eq!(
            serde_json::to_string(&DragAmount::Angle(0.5))?,
            r#"{"angle":0.5}"#
        );
        for side in [SnapSide::Free, SnapSide::Grid] {
            let json = serde_json::to_string(&side)?;
            assert_eq!(serde_json::from_str::<SnapSide>(&json)?, side);
        }
        for keys in [
            DragModifiers::None,
            DragModifiers::Shift,
            DragModifiers::Ctrl,
            DragModifiers::CtrlShift,
        ] {
            let json = serde_json::to_string(&keys)?;
            assert_eq!(serde_json::from_str::<DragModifiers>(&json)?, keys);
        }
        for condition in [WorldWaitCondition::Attached, WorldWaitCondition::Detached] {
            assert_eq!(
                serde_json::to_string(&condition)?,
                format!("\"{condition}\"")
            );
        }
        Ok(())
    }
}
