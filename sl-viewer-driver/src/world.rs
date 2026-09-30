//! World handles: [`World`] to start from, and [`WorldHandle`] — a
//! [`WorldLocator`] bound to its viewer, acted on through the viewer's own
//! pick resolver.
//!
//! A world action waits for exactly one thing to match, finds a point where
//! a click lands on *that* thing — framing it with the camera when no point
//! does, unless told not to — and plays the gesture there through the real
//! input path.

use std::time::Duration;

use sl_automation_proto::{
    Locator, RequestBody, ResponseBody, UiNode, WorldAction, WorldKind, WorldLocator, WorldNode,
    WorldWaitCondition,
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
