//! The LSL probe corpus (`probes/`) and its per-grid results
//! (`probes/results/`), held to the format of [`sl_lsl_runtime::probe`].
//!
//! A probe is one `.lsl` file directly under an area directory
//! (`probes/<area>/<probe>.lsl`), or a directory of scripts that run together
//! (`probes/<area>/<probe>/*.lsl`, beside the `*.notecard`s it reads); its
//! id is its path below `probes/`
//! without the extension. Every script compiles here (but for the names
//! OpenSim's library adds, in a script whose header says `library: opensim`),
//! opens with a header naming its probe, and — in a probe of several — names
//! its inventory name and its place in the save order, each once. A script that names its
//! probe in a `string ID = "…"` global names its own.
//!
//! Every result file, `probes/results/<grid>/<probe id>.txt`, is for a known
//! grid and a known probe and is a well-formed run of it.

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};

    use pretty_assertions::assert_eq;
    use sl_lsl_runtime::probe::{Drive, GRIDS, Header, Line, ResultFile, Stimulus};
    use sl_lsl_runtime::{CompileErrorKind, compile};

    /// The corpus root.
    fn probes_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("probes")
    }

    /// The entries of a directory, sorted.
    fn entries(dir: &Path) -> Result<Vec<PathBuf>, String> {
        let mut paths = fs_err::read_dir(dir)
            .map_err(|error| error.to_string())?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
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

    /// One probe: its id and its scripts.
    struct Probe {
        /// The path below `probes/` without the extension.
        id: String,
        /// Its scripts, one for a single-file probe.
        scripts: Vec<PathBuf>,
        /// Whether it is a directory of scripts.
        multi: bool,
    }

    /// Every probe in the corpus.
    fn probes() -> Result<Vec<Probe>, String> {
        let mut found = Vec::new();
        for area in entries(&probes_root())? {
            if !area.is_dir() || name_of(&area, false) == "results" {
                continue;
            }
            let area_name = name_of(&area, false);
            for entry in entries(&area)? {
                let id = format!("{area_name}/{}", name_of(&entry, true));
                if entry.is_dir() {
                    let files = entries(&entry)?;
                    // Beside its scripts, a probe of several may hold the
                    // notecards the runner puts into prim A.
                    if let Some(stray) = files.iter().find(|path| {
                        !is_lsl(path)
                            && path.extension().and_then(|ext| ext.to_str()) != Some("notecard")
                    }) {
                        return Err(format!("{} is not a script or notecard", stray.display()));
                    }
                    let scripts: Vec<PathBuf> =
                        files.into_iter().filter(|path| is_lsl(path)).collect();
                    if scripts.is_empty() {
                        return Err(format!("{} holds no script", entry.display()));
                    }
                    found.push(Probe {
                        id,
                        scripts,
                        multi: true,
                    });
                } else if is_lsl(&entry) {
                    found.push(Probe {
                        id,
                        scripts: vec![entry],
                        multi: false,
                    });
                } else {
                    return Err(format!("{} is not a probe", entry.display()));
                }
            }
        }
        Ok(found)
    }

    /// The value of a `string ID = "…";` global, if the script has one.
    fn declared_id(source: &str) -> Option<&str> {
        source
            .lines()
            .find_map(|line| line.strip_prefix("string ID = \"")?.strip_suffix("\";"))
    }

    /// The problems with one probe's scripts.
    fn check_probe(probe: &Probe) -> Result<Vec<String>, String> {
        let mut problems = Vec::new();
        let mut names = BTreeSet::new();
        let mut orders = BTreeSet::new();
        for script in &probe.scripts {
            let shown = script
                .strip_prefix(probes_root())
                .unwrap_or(script)
                .display()
                .to_string();
            let source = fs_err::read_to_string(script).map_err(|error| error.to_string())?;
            // A script for OpenSim may name what OpenSim's library adds,
            // which the compiler here does not know; everything else must
            // compile as Second Life's library has it.
            let library = Header::parse(&source)
                .map(|header| header.library)
                .unwrap_or_default();
            if let Err(errors) = compile(&source) {
                for error in errors {
                    if error.kind == CompileErrorKind::Undefined
                        && source
                            .get(error.span.clone())
                            .is_some_and(|name| library.adds(name))
                    {
                        continue;
                    }
                    problems.push(format!(
                        "{shown}:{}:{} {}",
                        error.position.line,
                        error.position.column,
                        error.kind.message()
                    ));
                }
            }
            if let Some(id) = declared_id(&source)
                && id != probe.id
            {
                problems.push(format!("{shown}: `ID` is `{id}`, not `{}`", probe.id));
            }
            let header = match Header::parse(&source) {
                Ok(header) => header,
                Err(error) => {
                    problems.push(format!("{shown}: {error}"));
                    continue;
                }
            };
            if header.probe != probe.id {
                problems.push(format!("{shown}: header `probe` is not `{}`", probe.id));
            }
            if probe.multi {
                match header.script {
                    Some(name) if !names.insert(name.clone()) => {
                        problems.push(format!("{shown}: script name `{name}` is taken"));
                    }
                    Some(_) => {}
                    None => problems.push(format!("{shown}: header `script` is missing")),
                }
                match header.order {
                    Some(order) if !orders.insert(order) => {
                        problems.push(format!("{shown}: order {order} is taken"));
                    }
                    Some(_) => {}
                    None => problems.push(format!("{shown}: header `order` is missing")),
                }
            }
        }
        Ok(problems)
    }

    /// What is wrong with the `waiting` records of a run: one that is not a
    /// stimulus, or asks for a drive the probe's headers do not declare.
    fn stimulus_problems(file: &ResultFile, drives: &BTreeSet<Drive>) -> Vec<String> {
        file.body
            .iter()
            .filter_map(|text| Line::parse(text).ok())
            .filter(|line| line.rec == "waiting")
            .filter_map(|line| match Stimulus::from_waiting(&line) {
                Ok(stimulus) if drives.contains(&stimulus.action.drive()) => None,
                Ok(stimulus) => Some(format!(
                    "asks for {:?}, which no header declares",
                    stimulus.action.drive()
                )),
                Err(reason) => Some(reason),
            })
            .collect()
    }

    /// Every result file under a grid's directory, with the probe id its
    /// path names.
    fn result_files(grid_dir: &Path) -> Result<Vec<(PathBuf, String)>, String> {
        let mut found = Vec::new();
        let mut pending = vec![grid_dir.to_path_buf()];
        while let Some(dir) = pending.pop() {
            for path in entries(&dir)? {
                if path.is_dir() {
                    pending.push(path);
                    continue;
                }
                let id = path
                    .strip_prefix(grid_dir)
                    .ok()
                    .and_then(|relative| relative.to_str())
                    .and_then(|relative| relative.strip_suffix(".txt"))
                    .map(str::to_owned);
                match id {
                    Some(id) => found.push((path, id)),
                    None => return Err(format!("{} is not a result file", path.display())),
                }
            }
        }
        found.sort();
        Ok(found)
    }

    #[test]
    fn every_probe_compiles_and_names_itself() -> Result<(), String> {
        let probes = probes()?;
        if probes.is_empty() {
            return Err("the corpus holds no probe".to_owned());
        }
        let mut problems = Vec::new();
        for probe in &probes {
            problems.extend(check_probe(probe)?);
        }
        assert_eq!(problems, Vec::<String>::new());
        Ok(())
    }

    #[test]
    fn every_result_file_is_a_well_formed_run_of_a_known_probe() -> Result<(), String> {
        let mut known: BTreeMap<String, BTreeSet<Drive>> = BTreeMap::new();
        for probe in probes()? {
            let mut drives = BTreeSet::new();
            for script in &probe.scripts {
                let source = fs_err::read_to_string(script).map_err(|error| error.to_string())?;
                let header = Header::parse(&source).map_err(|error| error.to_string())?;
                drives.extend(header.drives);
            }
            known.insert(probe.id, drives);
        }
        let mut problems = Vec::new();
        for grid_dir in entries(&probes_root().join("results"))? {
            if !grid_dir.is_dir() {
                continue;
            }
            let grid = name_of(&grid_dir, false);
            if !GRIDS.contains(&grid.as_str()) {
                problems.push(format!("results/{grid}: not one of {GRIDS:?}"));
                continue;
            }
            for (path, id) in result_files(&grid_dir)? {
                let shown = format!("results/{grid}/{id}.txt");
                let Some(drives) = known.get(&id) else {
                    problems.push(format!("{shown}: no such probe"));
                    continue;
                };
                let text = fs_err::read_to_string(&path).map_err(|error| error.to_string())?;
                match ResultFile::parse(&text) {
                    Ok(file) => {
                        problems.extend(
                            file.problems(&grid, &id)
                                .into_iter()
                                .map(|problem| format!("{shown}: {problem}")),
                        );
                        problems.extend(
                            stimulus_problems(&file, drives)
                                .into_iter()
                                .map(|problem| format!("{shown}: {problem}")),
                        );
                    }
                    Err(error) => problems.push(format!("{shown}: {error}")),
                }
            }
        }
        assert_eq!(problems, Vec::<String>::new());
        Ok(())
    }
}
