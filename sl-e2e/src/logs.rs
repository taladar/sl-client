//! Where a stage's log lines go: each viewer's into its own `viewer.log`,
//! everything else (the grid, the test) into the stage's `grid.log`.
//!
//! A viewer process writes its own log. An in-process viewer logs through
//! this process's subscriber, inside its `viewer{name=…}` span
//! (`ViewerAppOptions::log_label`), which every task it spawns carries on —
//! so one [`tracing_subscriber::Layer`] can tell the viewers' lines apart and
//! file each where the process backend's would be.
//!
//! The subscriber is process-wide, installed once by the first stage. A
//! stage registers its files for as long as it runs ([`RouteGuard`]). Lines
//! outside every viewer span cannot be attributed to one stage, so they go to
//! every running stage's `grid.log` — under nextest, one test per process,
//! that is the one stage.

use core::fmt::Write as _;
use std::collections::HashMap;
use std::io::Write as _;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Instant;

use tracing_subscriber::layer::{Context, SubscriberExt as _};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt as _;

/// The files of every running stage, and when the process started logging.
struct Routes {
    /// Each running stage's files, by its route id.
    stages: Mutex<Vec<(u64, StageFiles)>>,
    /// The instant lines are stamped relative to.
    start: Instant,
}

/// One stage's files.
struct StageFiles {
    /// Lines outside every viewer span.
    general: fs_err::File,
    /// Each viewer's lines, by the `name` its span carries.
    viewers: HashMap<String, fs_err::File>,
}

/// The process's routes, once the subscriber is installed.
static ROUTES: OnceLock<Routes> = OnceLock::new();

/// The id the next registered stage gets.
static NEXT_ROUTE: AtomicU64 = AtomicU64::new(0);

/// Install the routing subscriber, once per process: the router, and a
/// formatted copy on standard error (which a test runner shows for a failed
/// test), both filtered by `RUST_LOG` (default `info`).
///
/// A process that already has a global subscriber keeps it, and an
/// in-process viewer's lines then go there instead of into its file, and a
/// warning through that subscriber says so.
fn routes() -> &'static Routes {
    ROUTES.get_or_init(|| {
        let filter = || {
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_unset| tracing_subscriber::EnvFilter::new("info"))
        };
        let installed = tracing_subscriber::registry()
            .with(tracing_subscriber::Layer::with_filter(Router, filter()))
            .with(tracing_subscriber::Layer::with_filter(
                tracing_subscriber::fmt::layer().with_writer(std::io::stderr),
                filter(),
            ))
            .try_init();
        if let Err(error) = installed {
            tracing::warn!(
                "a global subscriber was already installed ({error}); in-process viewer log lines \
                 go there, not into their viewer.log"
            );
        }
        Routes {
            stages: Mutex::new(Vec::new()),
            start: Instant::now(),
        }
    })
}

/// Install the routing subscriber now, if no stage has yet — so what a stage
/// logs before it registers (a skip) reaches standard error.
pub(crate) fn install() {
    let _routes = routes();
}

/// A stage's files, registered until this is dropped.
#[derive(Debug)]
pub(crate) struct RouteGuard {
    /// The stage's route id.
    id: u64,
}

impl RouteGuard {
    /// Route lines outside every viewer span to `general`, and each viewer's
    /// (by its span name) to its file, from now until the guard drops.
    ///
    /// # Errors
    ///
    /// The error from creating a file.
    pub(crate) fn register(
        general: &Path,
        viewers: &[(String, std::path::PathBuf)],
    ) -> Result<Self, std::io::Error> {
        let routes = routes();
        let files = StageFiles {
            general: fs_err::File::create(general)?,
            viewers: viewers
                .iter()
                .map(|(name, path)| Ok((name.clone(), fs_err::File::create(path)?)))
                .collect::<Result<_, std::io::Error>>()?,
        };
        let id = NEXT_ROUTE.fetch_add(1, Ordering::Relaxed);
        routes
            .stages
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((id, files));
        Ok(Self { id })
    }
}

impl Drop for RouteGuard {
    fn drop(&mut self) {
        if let Some(routes) = ROUTES.get() {
            routes
                .stages
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .retain(|(id, _files)| *id != self.id);
        }
    }
}

/// The layer that files each line.
struct Router;

/// The `name` of a `viewer` span, kept in the span's extensions.
struct ViewerName(String);

/// Reads a `viewer` span's `name`.
struct NameVisitor(Option<String>);

impl tracing::field::Visit for NameVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "name" {
            self.0 = Some(value.to_owned());
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn core::fmt::Debug) {
        if field.name() == "name" {
            self.0 = Some(format!("{value:?}"));
        }
    }
}

/// Prints an event's fields: the message first, then `key=value`.
struct LineVisitor {
    /// The message.
    message: String,
    /// The other fields.
    fields: String,
}

impl tracing::field::Visit for LineVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn core::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
        } else {
            // Writing into a `String` cannot fail.
            let _infallible = write!(self.fields, " {}={value:?}", field.name());
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            value.clone_into(&mut self.message);
        } else {
            let _infallible = write!(self.fields, " {}={value}", field.name());
        }
    }
}

impl<S: tracing::Subscriber + for<'a> LookupSpan<'a>> tracing_subscriber::Layer<S> for Router {
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        ctx: Context<'_, S>,
    ) {
        if attrs.metadata().name() != "viewer" {
            return;
        }
        let mut visitor = NameVisitor(None);
        attrs.record(&mut visitor);
        if let (Some(name), Some(span)) = (visitor.0, ctx.span(id)) {
            span.extensions_mut().insert(ViewerName(name));
        }
    }

    fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
        let Some(routes) = ROUTES.get() else {
            return;
        };
        let viewer = ctx.event_scope(event).and_then(|scope| {
            scope.from_root().find_map(|span| {
                span.extensions()
                    .get::<ViewerName>()
                    .map(|name| name.0.clone())
            })
        });
        let mut visitor = LineVisitor {
            message: String::new(),
            fields: String::new(),
        };
        event.record(&mut visitor);
        let metadata = event.metadata();
        let line = format!(
            "[{:>10.3}s] {:>5} {}: {}{}\n",
            routes.start.elapsed().as_secs_f64(),
            metadata.level(),
            metadata.target(),
            visitor.message,
            visitor.fields
        );
        let mut stages = routes.stages.lock().unwrap_or_else(PoisonError::into_inner);
        for (_id, files) in stages.iter_mut() {
            let file = match &viewer {
                Some(name) => match files.viewers.get_mut(name) {
                    Some(file) => file,
                    None => continue,
                },
                None => &mut files.general,
            };
            // A line that cannot be written is lost from the artifact, not from
            // the run: standard error still has it.
            let _written = file.write_all(line.as_bytes());
        }
    }
}
