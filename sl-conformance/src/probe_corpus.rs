//! The LSL probe corpus on disk: `sl-lsl-runtime/probes/`, read into
//! [`Probe`]s for the `lsl-probes` cases to run, and its committed result
//! files compared grid against grid for `sl-conformance compare-probes`.
//!
//! The format is [`sl_lsl_runtime::probe`]; the conventions are the book's
//! `tools/lsl-probes.md`.

use std::path::{Path, PathBuf};

use sl_lsl_runtime::probe::compare::{Tolerance, compare};
use sl_lsl_runtime::probe::{Drive, GRIDS, Header, ResultFile};

/// Why the corpus or a result file could not be read.
#[derive(Debug, thiserror::Error)]
pub enum CorpusError {
    /// A file or directory could not be read.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// A script or result file is not in the probe format.
    #[error("{path}: {reason}")]
    Format {
        /// The file.
        path: PathBuf,
        /// What is wrong with it.
        reason: String,
    },
    /// A probe id the corpus does not hold.
    #[error("no probe `{0}` in the corpus")]
    UnknownProbe(String),
    /// A results grid that is not one of [`GRIDS`].
    #[error("`{0}` is not a results grid (one of {GRIDS:?})")]
    UnknownGrid(String),
}

/// The inventory name of a single-script probe's script.
const SINGLE_SCRIPT_NAME: &str = "Probe";

/// One script of a probe, read from the corpus.
#[derive(Debug)]
pub struct ProbeScript {
    /// Its header.
    pub header: Header,
    /// The inventory name it gets.
    pub name: String,
    /// Its source.
    pub source: String,
}

/// One probe, read from the corpus, its scripts in save order.
#[derive(Debug)]
pub struct Probe {
    /// Its id.
    pub id: String,
    /// Its scripts, in the order they are saved.
    pub scripts: Vec<ProbeScript>,
}

impl Probe {
    /// The stimuli its scripts declare they ask for.
    #[must_use]
    pub fn drives(&self) -> Vec<Drive> {
        let mut drives: Vec<Drive> = self
            .scripts
            .iter()
            .flat_map(|script| script.header.drives.iter().copied())
            .collect();
        drives.sort();
        drives.dedup();
        drives
    }

    /// Its prims, in the order a script first names each — its own prim,
    /// then the `bare` prims its header lists — with the distance east of
    /// prim A and the height above it each stands at.
    #[must_use]
    pub fn prims(&self) -> Vec<(String, f32, f32)> {
        let mut prims: Vec<(String, f32, f32)> = vec![("A".to_owned(), 0.0, 0.0)];
        let mut add = |label: &str, distance: f32, height: f32| {
            if !prims.iter().any(|(known, _, _)| known == label) {
                prims.push((label.to_owned(), distance, height));
            }
        };
        for script in &self.scripts {
            add(
                &script.header.prim,
                script.header.distance.unwrap_or(0.0),
                script.header.height.unwrap_or(0.0),
            );
            for bare in &script.header.bare {
                add(&bare.label, bare.distance, bare.height);
            }
        }
        prims
    }

    /// Whether a script asks for the region's active-script count to be
    /// noted (`stats: active-scripts`).
    #[must_use]
    pub fn notes_active_scripts(&self) -> bool {
        self.scripts
            .iter()
            .any(|script| script.header.active_scripts)
    }

    /// What a comparison of two of its runs lets differ.
    #[must_use]
    pub fn tolerance(&self) -> Tolerance {
        Tolerance::of(self.scripts.iter().map(|script| &script.header))
    }
}

/// The corpus directory.
#[must_use]
pub fn corpus_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../sl-lsl-runtime/probes")
}

/// The result file of probe `id` on results grid `grid` under `root`.
#[must_use]
pub fn result_path(root: &Path, grid: &str, id: &str) -> PathBuf {
    root.join("results").join(grid).join(format!("{id}.txt"))
}

/// The entries of a directory, sorted.
fn entries(dir: &Path) -> Result<Vec<PathBuf>, CorpusError> {
    let mut paths = fs_err::read_dir(dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.sort();
    Ok(paths)
}

/// Whether a path is an `.lsl` file.
fn is_lsl(path: &Path) -> bool {
    path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("lsl")
}

/// A path's final component, or its stem, as text.
fn name_of(path: &Path, stem: bool) -> String {
    let part = if stem {
        path.file_stem()
    } else {
        path.file_name()
    };
    part.map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Reads one script and its header.
fn read_script(path: &Path, multi: bool) -> Result<ProbeScript, CorpusError> {
    let source = fs_err::read_to_string(path)?;
    let format = |reason: String| CorpusError::Format {
        path: path.to_path_buf(),
        reason,
    };
    let header = Header::parse(&source).map_err(|error| format(error.to_string()))?;
    let name = if multi {
        header
            .script
            .clone()
            .ok_or_else(|| format("header `script` is missing".to_owned()))?
    } else {
        SINGLE_SCRIPT_NAME.to_owned()
    };
    Ok(ProbeScript {
        header,
        name,
        source,
    })
}

/// Every probe in the corpus under `root`, in id order.
///
/// # Errors
///
/// A [`CorpusError`] when a directory or script cannot be read or a header
/// is not in the format.
pub fn load_corpus(root: &Path) -> Result<Vec<Probe>, CorpusError> {
    let mut probes = Vec::new();
    for area in entries(root)? {
        if !area.is_dir() || name_of(&area, false) == "results" {
            continue;
        }
        let area_name = name_of(&area, false);
        for entry in entries(&area)? {
            let id = format!("{area_name}/{}", name_of(&entry, true));
            let mut scripts = if entry.is_dir() {
                entries(&entry)?
                    .iter()
                    .filter(|path| is_lsl(path))
                    .map(|path| read_script(path, true))
                    .collect::<Result<Vec<_>, _>>()?
            } else if is_lsl(&entry) {
                vec![read_script(&entry, false)?]
            } else {
                continue;
            };
            scripts.sort_by_key(|script| script.header.order);
            probes.push(Probe { id, scripts });
        }
    }
    probes.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(probes)
}

/// The probes `selected` names, in corpus order, or every probe when it
/// names none.
///
/// # Errors
///
/// [`CorpusError::UnknownProbe`] for a name the corpus does not hold.
pub fn select(probes: Vec<Probe>, selected: &[String]) -> Result<Vec<Probe>, CorpusError> {
    if let Some(unknown) = selected
        .iter()
        .find(|id| !probes.iter().any(|probe| probe.id == **id))
    {
        return Err(CorpusError::UnknownProbe(unknown.clone()));
    }
    Ok(probes
        .into_iter()
        .filter(|probe| selected.is_empty() || selected.contains(&probe.id))
        .collect())
}

/// Compares the committed runs of each probe in `selected` (every probe
/// when it names none): the first of `grids` that has a result is the
/// baseline, set against each other grid that has one. `grids` empty is
/// every results grid, in [`GRIDS`] order.
///
/// Returns the report's lines.
///
/// # Errors
///
/// A [`CorpusError`] when the corpus or a result file cannot be read, or a
/// probe or grid is unknown.
pub fn compare_results(
    root: &Path,
    selected: &[String],
    grids: &[String],
) -> Result<Vec<String>, CorpusError> {
    if let Some(unknown) = grids.iter().find(|grid| !GRIDS.contains(&grid.as_str())) {
        return Err(CorpusError::UnknownGrid(unknown.clone()));
    }
    let grids: Vec<String> = if grids.is_empty() {
        GRIDS.iter().map(|grid| (*grid).to_owned()).collect()
    } else {
        grids.to_vec()
    };
    let mut lines = Vec::new();
    for probe in select(load_corpus(root)?, selected)? {
        let mut runs = Vec::new();
        for grid in &grids {
            let path = result_path(root, grid, &probe.id);
            if !path.is_file() {
                continue;
            }
            let file = ResultFile::parse(&fs_err::read_to_string(&path)?)
                .map_err(|reason| CorpusError::Format { path, reason })?;
            runs.push((grid.as_str(), file));
        }
        let mut runs = runs.into_iter();
        let Some((baseline, left)) = runs.next() else {
            lines.push(format!("== {}: no results", probe.id));
            continue;
        };
        let tolerance = probe.tolerance();
        let mut others = 0_usize;
        for (grid, right) in runs {
            others = others.saturating_add(1);
            let comparison = compare(&left, &right, &tolerance);
            let differences = comparison.differences.len();
            lines.push(format!(
                "== {}: {baseline} | {grid} — {}",
                probe.id,
                if comparison.agrees() {
                    "agree".to_owned()
                } else {
                    format!("{differences} difference(s)")
                }
            ));
            lines.extend(
                comparison
                    .lines(baseline, grid)
                    .into_iter()
                    .map(|line| format!("  {line}")),
            );
        }
        if others == 0 {
            lines.push(format!("== {}: only {baseline} has a result", probe.id));
        }
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{CorpusError, corpus_root, load_corpus, select};

    #[test]
    fn the_corpus_loads_with_every_probe_in_save_order() -> Result<(), String> {
        let probes = load_corpus(&corpus_root()).map_err(|error| error.to_string())?;
        let errors = probes
            .iter()
            .find(|probe| probe.id == "errors/runtime-errors")
            .ok_or("no errors/runtime-errors probe")?;
        assert_eq!(
            errors
                .scripts
                .iter()
                .map(|script| script.name.as_str())
                .collect::<Vec<_>>(),
            [
                "Div int",
                "Mod int",
                "Div float",
                "Recurse",
                "Listener far",
                "Listener near",
                "Controller"
            ]
        );
        assert_eq!(
            errors.prims(),
            vec![
                ("A".to_owned(), 0.0, 0.0),
                ("B".to_owned(), 50.0, 0.0),
                ("C".to_owned(), 10.0, 0.0)
            ]
        );
        let chat = probes
            .iter()
            .find(|probe| probe.id == "comms/object-chat")
            .ok_or("no comms/object-chat")?;
        assert_eq!(
            chat.prims().last(),
            Some(&("G".to_owned(), 0.0, 101.0)),
            "the shout pair stands above prim A"
        );
        Ok(())
    }

    #[test]
    fn a_selection_keeps_corpus_order_and_refuses_an_unknown_probe() -> Result<(), String> {
        let load = || load_corpus(&corpus_root()).map_err(|error| error.to_string());
        let chosen = select(
            load()?,
            &["vm/throughput".to_owned(), "events/queue-limit".to_owned()],
        )
        .map_err(|error| error.to_string())?;
        assert_eq!(
            chosen
                .iter()
                .map(|probe| probe.id.as_str())
                .collect::<Vec<_>>(),
            ["events/queue-limit", "vm/throughput"]
        );
        assert!(matches!(
            select(load()?, &["vm/nothing".to_owned()]),
            Err(CorpusError::UnknownProbe(_))
        ));
        Ok(())
    }
}
