//! The public parse surface has exactly one failure discipline.
//!
//! Every `pub fn parse_*` in this crate reports a malformed input the same way:
//! `Err(WireError)`. That was not always true — the surface once carried five
//! disciplines at once (`WireError`, bare `Option`, a leaked third-party XML
//! error, and two hand-rolled enums), so the same kind of fault was reported
//! four different ways depending on which function you happened to call, and one
//! shape — an infallible `Vec` — could not report it at all.
//!
//! This test reads the crate's own sources and refuses to let that come back.
//! The only permitted exception is a **route matcher**: a function whose
//! `Option` answers "this URL suffix is not the one this endpoint serves", which
//! is a decision, not a failure. Each one is pinned below with its reason, and
//! the list is checked in both directions — an unpinned `Option` fails, and so
//! does a pinned name that no longer exists.

#[cfg(test)]
mod test {
    use pretty_assertions::assert_eq;
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    /// The marker a public parser's signature starts with, and the prefix its
    /// name therefore lost by the time the scanner reads what follows.
    const MARKER: &str = "pub fn parse_";

    /// The `pub fn parse_*` that answer with `Option` on purpose: every one
    /// takes a **URL suffix or file name** and asks "is this mine?", so `None`
    /// means the caller should try the next route, not that anything was
    /// malformed. A body parser is never allowed in here.
    const PINNED_ROUTE_MATCHERS: &[(&str, &str)] = &[
        (
            "parse_ais_create_category_url",
            "AIS3 route: POST /category/<id>?tid=<id>",
        ),
        ("parse_ais_category_url", "AIS3 route: /category/<id>"),
        (
            "parse_ais_category_children_url",
            "AIS3 route: /category/<id>/children",
        ),
        (
            "parse_ais_category_links_url",
            "AIS3 route: /category/<id>/links",
        ),
        (
            "parse_ais_category_children_fetch_url",
            "AIS3 route: /category/<id>/children?depth=<n>",
        ),
        (
            "parse_ais_category_children_subset",
            "AIS3 route refinement: the optional &children=<id>,… subset",
        ),
        ("parse_ais_item_url", "AIS3 route: /item/<id>"),
        (
            "parse_avatar_picker_search_query",
            "AvatarPickerSearch route: ?names=…&page_size=…",
        ),
        (
            "parse_find_experience_query",
            "FindExperienceByName route: ?query=…&page=…",
        ),
        (
            "parse_group_experiences_query",
            "GroupExperiences route: the bare ?<group id> query",
        ),
        (
            "parse_forget_experience_query",
            "ForgetExperience route: the bare ?<experience id> query",
        ),
        (
            "parse_experience_id_query",
            "ExperienceQuery route: ?experience_id=<id>",
        ),
        (
            "parse_experience_query",
            "RegionExperiences route: ?parcelid=…&experiences=…",
        ),
        (
            "parse_file_name",
            "map-tile route: the map-<zoom>-<x>-<y>-objects.jpg tile file name",
        ),
    ];

    /// Collects every `.rs` file under `dir`, recursively — read at run time
    /// rather than embedded, so a module added in a later commit is covered
    /// without this test being touched.
    fn rust_sources(dir: &Path, into: &mut Vec<PathBuf>) -> Result<(), String> {
        let entries = fs_err::read_dir(dir).map_err(|error| error.to_string())?;
        for entry in entries {
            let path = entry.map_err(|error| error.to_string())?.path();
            if path.is_dir() {
                rust_sources(&path, into)?;
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                into.push(path);
            }
        }
        Ok(())
    }

    /// The declared return type of the signature `chunk` starts, with its
    /// whitespace collapsed, or `None` when `chunk` does not start one (the
    /// marker matched inside a doc comment or a string).
    ///
    /// The argument list is walked by *balancing* parentheses rather than
    /// searching for the next `)`, because an argument can itself be a callable
    /// (`decode: impl FnOnce(&Llsd) -> …`).
    fn return_type(chunk: &str) -> Option<String> {
        let mut depth = 0_usize;
        let mut tail = None;
        for (index, character) in chunk.char_indices() {
            match character {
                '(' => depth = depth.checked_add(1)?,
                ')' => {
                    depth = depth.checked_sub(1)?;
                    if depth == 0 {
                        tail = chunk.get(index..);
                        break;
                    }
                }
                // A body before any argument list: not a signature at all.
                '{' if depth == 0 => return None,
                _ => {}
            }
        }
        // Only this signature's own arrow: anything but the closing paren and
        // whitespace between them means the scan walked into the next item.
        let (gap, after_arrow) = tail?.split_once("->")?;
        if !gap.trim_start_matches(')').trim().is_empty() {
            return None;
        }
        // The body's `{`, or a `where` clause, ends the return type — whichever
        // comes first.
        let end = after_arrow
            .find('{')
            .into_iter()
            .chain(after_arrow.find(" where "))
            .min()?;
        let declared = after_arrow.get(..end)?;
        Some(declared.split_whitespace().collect::<Vec<_>>().join(" "))
    }

    /// Every `pub fn parse_*` in the crate's sources, as `(name, return type)`.
    fn public_parsers() -> Result<Vec<(String, String)>, String> {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        rust_sources(&src, &mut files)?;
        assert!(
            !files.is_empty(),
            "found no sources under {}",
            src.display()
        );

        let mut found = Vec::new();
        for file in files {
            let source = fs_err::read_to_string(&file).map_err(|error| error.to_string())?;
            for chunk in source.split(MARKER).skip(1) {
                let suffix: String = chunk
                    .chars()
                    .take_while(|character| character.is_alphanumeric() || *character == '_')
                    .collect();
                if let Some(returns) = return_type(chunk) {
                    found.push((format!("parse_{suffix}"), returns));
                }
            }
        }
        Ok(found)
    }

    /// The scanner reads what it thinks it reads. Without this, a regression in
    /// [`return_type`] that stopped matching anything would make the two tests
    /// below pass by finding nothing at all.
    #[test]
    fn the_scanner_finds_the_surface_it_is_checking() -> Result<(), String> {
        let parsers = public_parsers()?;
        assert!(
            parsers.len() > 80,
            "only found {} public parsers — the scanner is broken, not the crate",
            parsers.len()
        );
        let datagram = parsers
            .iter()
            .find(|(name, _)| name == "parse_datagram")
            .ok_or("parse_datagram is part of the public surface")?;
        assert_eq!(datagram.1, "Result<ParsedDatagram<'_>, WireError>");
        let route = parsers
            .iter()
            .find(|(name, _)| name == "parse_ais_item_url")
            .ok_or("parse_ais_item_url is part of the public surface")?;
        assert_eq!(route.1, "Option<InventoryKey>");
        Ok(())
    }

    /// Nothing on the public parse surface reports a fault as anything but a
    /// [`WireError`] — bar the pinned route matchers.
    #[test]
    fn every_public_parser_fails_into_wire_error() -> Result<(), String> {
        let pinned: BTreeSet<&str> = PINNED_ROUTE_MATCHERS
            .iter()
            .map(|(name, _reason)| *name)
            .collect();
        let offenders: Vec<String> = public_parsers()?
            .into_iter()
            .filter(|(name, returns)| {
                !returns.ends_with(", WireError>")
                    && !returns.ends_with(", crate::WireError>")
                    && !pinned.contains(name.as_str())
            })
            .map(|(name, returns)| format!("{name} -> {returns}"))
            .collect();
        assert_eq!(
            offenders,
            Vec::<String>::new(),
            "these public parsers do not fail into WireError; make them, or pin \
             them in PINNED_ROUTE_MATCHERS with the route they match"
        );
        Ok(())
    }

    /// The pin list does not outlive what it pins: a matcher that was renamed,
    /// removed, or converted to a `Result` has to leave the table too.
    #[test]
    fn the_pinned_route_matchers_all_still_exist() -> Result<(), String> {
        let parsers = public_parsers()?;
        let stale: Vec<&str> = PINNED_ROUTE_MATCHERS
            .iter()
            .filter(|(name, _reason)| {
                !parsers
                    .iter()
                    .any(|(found, returns)| found == name && returns.starts_with("Option<"))
            })
            .map(|(name, _reason)| *name)
            .collect();
        assert_eq!(
            stale,
            Vec::<&str>::new(),
            "these pinned route matchers no longer exist as `Option`-returning \
             parsers; drop them from PINNED_ROUTE_MATCHERS"
        );
        Ok(())
    }
}
