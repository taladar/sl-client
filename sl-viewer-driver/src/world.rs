//! World handles: [`World`] to start from, [`WorldHandle`] — a
//! [`WorldLocator`] bound to its viewer, acted on through the viewer's own
//! pick resolver — and [`GroundHandle`], a point of the ground bound the same
//! way.
//!
//! A world action waits for exactly one thing to match, finds a point where
//! a click lands on *that* thing — framing it with the camera when no point
//! does, unless told not to — and plays the gesture there through the real
//! input path.

use std::time::Duration;

use sl_automation_proto::{
    GroundPoint, Locator, RequestBody, ResponseBody, UiNode, WorldAction, WorldKind, WorldLocator,
    WorldNode, WorldWaitCondition,
};

use crate::artifacts::Subject;
use crate::error::DriverError;
use crate::ui::UiLocator;
use crate::viewer::Viewer;

/// The Fluent key of the object pie's sit slice.
const SIT_ON_OBJECT: &str = "pie-object-sit-here";

/// The Fluent key of the own avatar pie's sit slice: sit on the ground.
const SIT_DOWN: &str = "pie-avatar-sit-down";

/// The world of one viewer, to find things in.
#[derive(Debug, Clone)]
pub struct World {
    /// The viewer.
    viewer: Viewer,
}

impl World {
    /// The world of `viewer`.
    pub(crate) const fn new(viewer: Viewer) -> Self {
        Self { viewer }
    }

    /// The things `locator` names.
    #[must_use]
    pub fn locator(&self, locator: WorldLocator) -> WorldHandle {
        WorldHandle::new(self.viewer.clone(), locator)
    }

    /// The object nobody wears named `name` exactly.
    #[must_use]
    pub fn object_named(&self, name: &str) -> WorldHandle {
        self.locator(WorldLocator::kind(WorldKind::Object).named(name))
    }

    /// The avatar shown as `name` exactly.
    #[must_use]
    pub fn avatar(&self, name: &str) -> WorldHandle {
        self.locator(WorldLocator::kind(WorldKind::Avatar).named(name))
    }

    /// The own avatar.
    #[must_use]
    pub fn me(&self) -> WorldHandle {
        self.locator(WorldLocator::own_avatar())
    }

    /// The ground at `(x, y)` — metres from the south-west corner — of the
    /// region named `region`: the agent's own, or one the viewer sees across
    /// a border.
    #[must_use]
    pub fn ground(&self, region: &str, x: f32, y: f32) -> GroundHandle {
        GroundHandle {
            viewer: self.viewer.clone(),
            ground: GroundPoint::new(region, x, y),
            timeout: None,
            reveal: true,
        }
    }
}

/// A world locator bound to its viewer.
#[derive(Debug, Clone)]
pub struct WorldHandle {
    /// The viewer.
    viewer: Viewer,
    /// The query.
    locator: WorldLocator,
    /// How long its actions and reads wait; the viewer's default when
    /// `None`.
    timeout: Option<Duration>,
    /// Whether an action may frame the thing with the camera.
    reveal: bool,
}

impl WorldHandle {
    /// `locator` on `viewer`.
    pub(crate) const fn new(viewer: Viewer, locator: WorldLocator) -> Self {
        Self {
            viewer,
            locator,
            timeout: None,
            reveal: true,
        }
    }

    /// The query.
    #[must_use]
    pub const fn as_locator(&self) -> &WorldLocator {
        &self.locator
    }

    /// The viewer.
    #[must_use]
    pub const fn viewer(&self) -> &Viewer {
        &self.viewer
    }

    /// How long its actions and reads wait.
    #[must_use]
    pub fn wait(&self) -> Duration {
        self.timeout
            .unwrap_or_else(|| self.viewer.options().timeout)
    }

    /// Wait `timeout` instead of the viewer's default.
    #[must_use]
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Never move the camera to reach the thing: an action fails instead
    /// when no point of it on screen takes a click.
    #[must_use]
    pub const fn without_reveal(mut self) -> Self {
        self.reveal = false;
        self
    }

    /// Carry out `action` on the one thing this names; the thing and where
    /// the click landed on it.
    async fn act(&self, action: WorldAction) -> Result<WorldNode, DriverError> {
        let description = format!("{action} on {}", self.locator);
        match self
            .viewer
            .ask(
                RequestBody::WorldAction {
                    locator: self.locator.clone(),
                    action,
                    reveal: self.reveal,
                    deadline: Viewer::deadline(self.wait()),
                },
                self.wait(),
                &description,
                Subject::Viewer,
            )
            .await?
        {
            ResponseBody::WorldDone { node, .. } => Ok(*node),
            other => Err(self.viewer.unexpected(&description, &other)),
        }
    }

    /// Touch the one thing this names: a left click outside build mode.
    ///
    /// # Errors
    ///
    /// [`DriverError::Failed`] when it matches several things, never one, or
    /// no point of it takes a click; the other [`DriverError`]s when the
    /// viewer does not answer.
    pub async fn touch(&self) -> Result<WorldNode, DriverError> {
        self.act(WorldAction::Click).await
    }

    /// Double-click the one thing this names: with the double-click action
    /// set to teleport, a teleport onto it — onto an avatar, or an object
    /// that takes no click of its own.
    ///
    /// # Errors
    ///
    /// As [`touch`](Self::touch).
    pub async fn double_click(&self) -> Result<WorldNode, DriverError> {
        self.act(WorldAction::DoubleClick).await
    }

    /// Open the pie menu of the one thing this names: a right click.
    ///
    /// # Errors
    ///
    /// As [`touch`](Self::touch).
    pub async fn open_pie(&self) -> Result<WorldNode, DriverError> {
        self.act(WorldAction::RightClick).await
    }

    /// Rest the pointer on the one thing this names: its hover tip.
    ///
    /// # Errors
    ///
    /// As [`touch`](Self::touch).
    pub async fn hover(&self) -> Result<WorldNode, DriverError> {
        self.act(WorldAction::Hover).await
    }

    /// Select the one thing this names with a click in build mode, which
    /// the action waits for — on a tool that selects: the Build window opens
    /// on the Create tool when nothing is selected, and a click with it rezzes.
    ///
    /// # Errors
    ///
    /// As [`touch`](Self::touch), and when build mode on a selecting tool
    /// never comes.
    pub async fn select(&self) -> Result<WorldNode, DriverError> {
        self.act(WorldAction::Select).await
    }

    /// Toggle the one thing this names in the selection with a `Shift`-click
    /// in build mode, keeping the rest selected — how a second object joins
    /// the selection. Waits as [`select`](Self::select) does.
    ///
    /// # Errors
    ///
    /// As [`select`](Self::select).
    pub async fn shift_select(&self) -> Result<WorldNode, DriverError> {
        self.act(WorldAction::ShiftSelect).await
    }

    /// Rez the Build window's picked shape on the one thing this names: a
    /// click with the Create tool, which the action waits for.
    ///
    /// # Errors
    ///
    /// As [`touch`](Self::touch), and when the Create tool never comes.
    pub async fn place(&self) -> Result<WorldNode, DriverError> {
        self.act(WorldAction::Place).await
    }

    /// Sit: on the one object this names, by its pie's *Sit Here*; for the
    /// own avatar, on the ground by its pie's *Sit Down*.
    ///
    /// # Errors
    ///
    /// As [`open_pie`](Self::open_pie), and when the pie has no sit slice.
    pub async fn sit(&self) -> Result<UiNode, DriverError> {
        let _thing = self.open_pie().await?;
        let key = if self.locator.kind == Some(WorldKind::Avatar) {
            SIT_DOWN
        } else {
            SIT_ON_OBJECT
        };
        self.viewer
            .pie_slice(Locator::default().name_key(key))
            .await
    }

    /// Drag what the UI node `source` names carries (an inventory row) and
    /// drop it on the one thing this names.
    ///
    /// # Errors
    ///
    /// As [`touch`](Self::touch), and when the source is not actionable.
    pub async fn drop_from(&self, source: &UiLocator) -> Result<WorldNode, DriverError> {
        self.act(WorldAction::DropFrom(source.as_locator().clone()))
            .await
    }

    /// Every thing this names, once the names and owners it depends on have
    /// arrived.
    ///
    /// # Errors
    ///
    /// As any request; [`DriverError::Failed`] when the names never arrive.
    pub async fn nodes(&self) -> Result<Vec<WorldNode>, DriverError> {
        let action = format!("find {}", self.locator);
        match self
            .viewer
            .ask(
                RequestBody::FindWorld {
                    locator: self.locator.clone(),
                    deadline: Viewer::deadline(self.wait()),
                },
                self.wait(),
                &action,
                Subject::Viewer,
            )
            .await?
        {
            ResponseBody::FoundWorld { nodes } => Ok(nodes),
            other => Err(self.viewer.unexpected(&action, &other)),
        }
    }

    /// Wait for this to match something (`Attached`) or nothing
    /// (`Detached`), and return the matches then.
    pub(crate) async fn wait_for(
        &self,
        condition: WorldWaitCondition,
        verb: &str,
    ) -> Result<Vec<WorldNode>, DriverError> {
        let action = format!("{verb}: wait for {} to be {condition}", self.locator);
        match self
            .viewer
            .ask(
                RequestBody::WaitForWorld {
                    locator: self.locator.clone(),
                    condition,
                    deadline: Viewer::deadline(self.wait()),
                },
                self.wait(),
                &action,
                Subject::Viewer,
            )
            .await?
        {
            ResponseBody::WorldSatisfied { nodes } => Ok(nodes),
            other => Err(self.viewer.unexpected(&action, &other)),
        }
    }

    /// The one thing this names, once one is there.
    ///
    /// # Errors
    ///
    /// [`DriverError::Failed`] when none comes within the wait, or several
    /// match.
    pub async fn node(&self) -> Result<WorldNode, DriverError> {
        let mut found = self.wait_for(WorldWaitCondition::Attached, "read").await?;
        if found.len() == 1
            && let Some(node) = found.pop()
        {
            return Ok(node);
        }
        let action = format!("read {}", self.locator);
        Err(self
            .viewer
            .fail(
                &action,
                sl_automation_proto::AutomationError::WorldAmbiguous {
                    locator: self.locator.clone(),
                    candidates: found,
                },
                None,
                Subject::Viewer,
            )
            .await)
    }

    /// How many things this names now.
    ///
    /// # Errors
    ///
    /// As [`nodes`](Self::nodes).
    pub async fn count(&self) -> Result<usize, DriverError> {
        Ok(self.nodes().await?.len())
    }
}

/// A point of the ground bound to its viewer: a region, by name, and where
/// in it. The viewer's own pick resolver must say a click at the aim point
/// lands on that ground — not on an object or an avatar standing there, nor
/// on water over it.
#[derive(Debug, Clone)]
pub struct GroundHandle {
    /// The viewer.
    viewer: Viewer,
    /// The ground.
    ground: GroundPoint,
    /// How long its actions wait; the viewer's default when `None`.
    timeout: Option<Duration>,
    /// Whether an action may frame the point with the camera.
    reveal: bool,
}

impl GroundHandle {
    /// The ground.
    #[must_use]
    pub const fn as_ground(&self) -> &GroundPoint {
        &self.ground
    }

    /// How long its actions wait.
    #[must_use]
    pub fn wait(&self) -> Duration {
        self.timeout
            .unwrap_or_else(|| self.viewer.options().timeout)
    }

    /// Wait `timeout` instead of the viewer's default.
    #[must_use]
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Never move the camera to reach the point: an action fails instead
    /// when no click on screen lands on it.
    #[must_use]
    pub const fn without_reveal(mut self) -> Self {
        self.reveal = false;
        self
    }

    /// Carry out `action` on the ground; where the click landed, in the
    /// named region's metres.
    async fn act(&self, action: WorldAction) -> Result<[f32; 3], DriverError> {
        let description = format!("{action} on {}", self.ground);
        match self
            .viewer
            .ask(
                RequestBody::GroundAction {
                    ground: self.ground.clone(),
                    action,
                    reveal: self.reveal,
                    deadline: Viewer::deadline(self.wait()),
                },
                self.wait(),
                &description,
                Subject::Viewer,
            )
            .await?
        {
            ResponseBody::GroundDone { hit_point } => Ok(hit_point),
            other => Err(self.viewer.unexpected(&description, &other)),
        }
    }

    /// Left-click the ground.
    ///
    /// # Errors
    ///
    /// [`DriverError::Failed`] when the region or its ground is not known, or
    /// no click reaches the point; the other [`DriverError`]s when the viewer
    /// does not answer.
    pub async fn click(&self) -> Result<[f32; 3], DriverError> {
        self.act(WorldAction::Click).await
    }

    /// Double-click the ground: with the double-click action set to
    /// teleport, a teleport there — across a border too.
    ///
    /// # Errors
    ///
    /// As [`click`](Self::click).
    pub async fn double_click(&self) -> Result<[f32; 3], DriverError> {
        self.act(WorldAction::DoubleClick).await
    }

    /// Open the land pie: a right click on the ground.
    ///
    /// # Errors
    ///
    /// As [`click`](Self::click).
    pub async fn open_pie(&self) -> Result<[f32; 3], DriverError> {
        self.act(WorldAction::RightClick).await
    }

    /// Rest the pointer on the ground.
    ///
    /// # Errors
    ///
    /// As [`click`](Self::click).
    pub async fn hover(&self) -> Result<[f32; 3], DriverError> {
        self.act(WorldAction::Hover).await
    }

    /// Rez the Build window's picked shape on the ground: a click with the
    /// Create tool, which the action waits for.
    ///
    /// # Errors
    ///
    /// As [`click`](Self::click), and when the Create tool never comes.
    pub async fn place(&self) -> Result<[f32; 3], DriverError> {
        self.act(WorldAction::Place).await
    }

    /// Drag what the UI node `source` names carries (an inventory row) and
    /// drop it on the ground — rezzing an object there.
    ///
    /// # Errors
    ///
    /// As [`click`](Self::click), and when the source is not actionable.
    pub async fn drop_from(&self, source: &UiLocator) -> Result<[f32; 3], DriverError> {
        self.act(WorldAction::DropFrom(source.as_locator().clone()))
            .await
    }
}
