//! What a partial asset answer says of the whole asset.
//!
//! The asset capabilities answer a ranged fetch with `206` and a
//! `Content-Range: bytes <first>-<last>/<total>`. The total is the only way a
//! fetcher learns where an asset ends short of running off it — and running
//! off it is expensive: Second Life and OpenSim both answer a range that
//! starts at or past the end of an asset with `200` and the **whole asset**
//! (measured 2026-10-12, `book/src/gridspec/assets.md`), so a store that asks
//! for "the rest" of an asset it already has downloads it a second time.

use reqwest::header::{CONTENT_RANGE, HeaderMap as ReqwestHeaderMap};

/// The whole asset's length as an answer's `Content-Range` states it, or
/// `None` where the answer has none, or one that states no total
/// (`bytes 0-9/*`).
#[must_use]
pub fn stated_total(headers: &ReqwestHeaderMap) -> Option<usize> {
    total_of(headers.get(CONTENT_RANGE)?.to_str().ok()?)
}

/// The total a `Content-Range` value states.
fn total_of(value: &str) -> Option<usize> {
    let (_span, total) = value.trim().strip_prefix("bytes")?.split_once('/')?;
    total.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::{stated_total, total_of};
    use pretty_assertions::assert_eq;
    use reqwest::header::{
        CONTENT_RANGE, HeaderMap as ReqwestHeaderMap, HeaderValue as ReqwestHeaderValue,
    };

    #[test]
    fn a_content_range_states_the_total() {
        assert_eq!(total_of("bytes 0-599/4096"), Some(4096));
        assert_eq!(total_of("bytes */4096"), Some(4096));
        assert_eq!(total_of("bytes 0-599/*"), None);
        assert_eq!(total_of("items 0-599/4096"), None);
        assert_eq!(total_of("bytes 0-599"), None);
    }

    #[test]
    fn an_answer_without_one_states_none() {
        let mut headers = ReqwestHeaderMap::new();
        assert_eq!(stated_total(&headers), None);
        let _previous = headers.insert(
            CONTENT_RANGE,
            ReqwestHeaderValue::from_static("bytes 0-9/20"),
        );
        assert_eq!(stated_total(&headers), Some(20));
    }
}
