//! The vendored static assets are the assets they claim to be.
//!
//! `viewer-assets/static_assets/` and `viewer-assets/fs_static_assets/` are
//! upstream Linden / Firestorm content, copied in verbatim (see
//! `viewer-assets/README.md`), and every asset store consults them ahead of the
//! network. Nothing else re-checks them: a truncated copy, a file that lost its
//! UUID name, or an upstream format this workspace's decoders do not read would
//! otherwise surface as an avatar that will not pose or a wearable that will
//! not bake, on a live grid, with a warning buried in the log.
//!
//! So this test opens every one of them with the decoder for its class — the
//! same decoders the viewer uses — and pins the inventory, so a partial
//! re-copy fails here instead.

#[cfg(test)]
mod test {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    use pretty_assertions::assert_eq;
    use sl_client_bevy::{AssetKey, StaticAssetLibrary, Uuid};

    /// A boxed test error, so the test can use `?`.
    type TestError = Box<dyn core::error::Error>;

    /// The two vendored directories, in the order the viewer layers them.
    fn directories() -> Vec<PathBuf> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        [
            "viewer-assets/static_assets",
            "viewer-assets/fs_static_assets",
        ]
        .into_iter()
        .map(|relative| root.join(relative))
        .collect()
    }

    /// Every vendored file, as `(path, extension)`.
    fn vendored_files() -> Result<Vec<(PathBuf, String)>, TestError> {
        let mut files = Vec::new();
        for dir in directories() {
            for entry in fs_err::read_dir(&dir)? {
                let path = entry?.path();
                let extension = path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .ok_or("a vendored file with no class extension")?
                    .to_owned();
                files.push((path, extension));
            }
        }
        Ok(files)
    }

    /// The inventory the workspace vendored, by class. Pinned so a partial
    /// re-copy is a failing test rather than a missing animation at run time;
    /// update this and `viewer-assets/README.md` together when re-copying from
    /// a newer viewer checkout.
    #[test]
    fn the_vendored_inventory_is_complete() -> Result<(), TestError> {
        let mut by_class: BTreeMap<String, usize> = BTreeMap::new();
        for (_path, extension) in vendored_files()? {
            *by_class.entry(extension).or_default() += 1;
        }
        assert_eq!(
            by_class,
            [
                ("animatn".to_owned(), 129),
                ("bodypart".to_owned(), 48),
                ("clothing".to_owned(), 49),
                ("gesture".to_owned(), 76),
            ]
            .into_iter()
            .collect::<BTreeMap<String, usize>>()
        );
        Ok(())
    }

    /// Every file is named for a UUID, no two share one across the two
    /// directories, and the library indexes all of them — which is what makes
    /// an asset reachable at all.
    #[test]
    fn every_file_is_indexed_under_its_own_uuid() -> Result<(), TestError> {
        let files = vendored_files()?;
        for (path, _extension) in &files {
            let stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .ok_or("a vendored file with no stem")?;
            let _id: Uuid = stem
                .parse()
                .map_err(|error| format!("{} is not named for a UUID: {error}", path.display()))?;
        }
        let library = StaticAssetLibrary::load(directories());
        assert_eq!(
            library.len(),
            files.len(),
            "the two directories share a UUID, so one asset shadows another"
        );
        Ok(())
    }

    /// Every keyframe motion decodes through `sl-anim` — the decoder the
    /// viewer's animation resolver runs — and animates at least one joint.
    #[test]
    fn every_animation_decodes() -> Result<(), TestError> {
        let mut decoded = 0_usize;
        for (path, extension) in vendored_files()? {
            if extension != "animatn" {
                continue;
            }
            let bytes = fs_err::read(&path)?;
            let motion = sl_anim::Motion::from_bytes(&bytes)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            assert!(
                !motion.joints.is_empty(),
                "{} animates no joint",
                path.display()
            );
            decoded += 1;
        }
        assert_eq!(decoded, 129);
        Ok(())
    }

    /// Every library body part and clothing item parses through `sl-avatar`'s
    /// `LLWearable` reader — the one the client-side baker feeds — and names a
    /// wearable type.
    #[test]
    fn every_wearable_parses() -> Result<(), TestError> {
        let mut parsed = 0_usize;
        for (path, extension) in vendored_files()? {
            if extension != "bodypart" && extension != "clothing" {
                continue;
            }
            let bytes = fs_err::read(&path)?;
            let text = String::from_utf8(bytes)
                .map_err(|error| format!("{} is not UTF-8: {error}", path.display()))?;
            let _wearable = sl_avatar::WearableAsset::parse(&text)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            parsed += 1;
        }
        assert_eq!(parsed, 97);
        Ok(())
    }

    /// The gestures have no decoder in this workspace yet (the gesture UI is
    /// still to come), so all this can say is that the bytes are there and
    /// carry the `LLMultiGesture` version line — enough to catch a truncated
    /// or empty copy.
    #[test]
    fn every_gesture_carries_its_version_line() -> Result<(), TestError> {
        let mut seen = 0_usize;
        for (path, extension) in vendored_files()? {
            if extension != "gesture" {
                continue;
            }
            let bytes = fs_err::read(&path)?;
            let first = bytes
                .split(|&byte| byte == b'\n')
                .next()
                .ok_or("an empty gesture")?;
            assert_eq!(
                first,
                b"2",
                "{} does not start with the gesture version line",
                path.display()
            );
            seen += 1;
        }
        assert_eq!(seen, 76);
        Ok(())
    }

    /// The library reads back the bytes on disk, so what a store serves for one
    /// of these ids is the vendored file itself.
    #[test]
    fn the_library_serves_the_vendored_bytes() -> Result<(), TestError> {
        let library = StaticAssetLibrary::load(directories());
        let (path, _extension) = vendored_files()?
            .into_iter()
            .find(|(_path, extension)| extension == "animatn")
            .ok_or("no vendored animation")?;
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or("no stem")?;
        let id = AssetKey::from(stem.parse::<Uuid>()?);
        assert!(library.contains(id));
        assert_eq!(
            library.read(id).as_deref(),
            Some(fs_err::read(&path)?.as_slice())
        );
        Ok(())
    }
}
