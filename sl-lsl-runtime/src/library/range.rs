//! Index arithmetic shared by the string and list functions: taking a range
//! out, deleting it, inserting at an index and replacing a range, each in
//! Second Life's rules and OpenSim's.
//!
//! Second Life's are PyOptimizer's `InternalGetDeleteSubSequence`,
//! `llListInsertList` and `llListReplaceList`, which lean on Python's
//! slicing; [`py`] is that slicing's index rule. OpenSim's are ported from
//! `LSL_Types.list.GetSublist` / `DeleteSublist` and `LSL_Api`'s
//! `llGetSubString`, `llDeleteSubString`, `llListInsertList` and
//! `llListReplaceList`. The two agree inside the bounds and on the
//! documented wrap-around (a start after the end takes the outside); they
//! part on ranges past both ends (`llDeleteSubList(l, 1, -99)` is `[a]` on
//! Second Life and `[]` on OpenSim).

/// A length as a signed index.
pub(crate) fn signed(length: usize) -> i64 {
    i64::try_from(length).unwrap_or(i64::MAX)
}

/// Python's rule for a slice bound: a negative index counts from the end,
/// and either is then held to `0..=length`.
pub(crate) fn py(index: i64, length: usize) -> usize {
    let length_signed = signed(length);
    let resolved = if index < 0 {
        index.saturating_add(length_signed).max(0)
    } else {
        index.min(length_signed)
    };
    usize::try_from(resolved).unwrap_or(0)
}

/// Python's `items[from:to]`.
pub(crate) fn slice<T: Clone>(items: &[T], from: i64, to: i64) -> Vec<T> {
    let (from, to) = (py(from, items.len()), py(to, items.len()));
    items.get(from..to.max(from)).unwrap_or_default().to_vec()
}

/// Python's `items[from:]`.
fn tail<T: Clone>(items: &[T], from: i64) -> Vec<T> {
    slice(items, from, signed(items.len()))
}

/// Python's `items[:to]`.
fn head<T: Clone>(items: &[T], to: i64) -> Vec<T> {
    slice(items, 0, to)
}

/// Two pieces joined.
fn joined<T>(mut first: Vec<T>, second: Vec<T>) -> Vec<T> {
    first.extend(second);
    first
}

/// Whether Second Life reads `start..=end` as the outside of the range (a
/// start after the end, each counted from the end when negative).
fn wraps(start: i64, end: i64, length: usize) -> bool {
    let length = signed(length);
    let start = if start < 0 {
        start.saturating_add(length)
    } else {
        start
    };
    let end = if end < 0 {
        end.saturating_add(length)
    } else {
        end
    };
    start > end
}

/// Second Life's `llGetSubString` / `llList2List`: `start..=end`, or the
/// outside of it when the start comes after the end.
pub(crate) fn sl_get<T: Clone>(items: &[T], start: i32, end: i32) -> Vec<T> {
    match sl_get_pieces(items, start, end) {
        Ok(inside) => inside,
        Err((before, after)) => joined(before, after),
    }
}

/// [`sl_get`]'s answer in pieces: `Ok` with the range, or `Err` with the two
/// pieces of its outside — up to the end, and from the start — when the
/// start comes after the end.
pub(crate) fn sl_get_pieces<T: Clone>(
    items: &[T],
    start: i32,
    end: i32,
) -> Result<Vec<T>, (Vec<T>, Vec<T>)> {
    let (start, mut end) = (i64::from(start), i64::from(end));
    let wrap = wraps(start, end, items.len());
    if end == -1 {
        end = end.saturating_add(signed(items.len()));
    }
    if wrap {
        Err((head(items, end.saturating_add(1)), tail(items, start)))
    } else {
        Ok(slice(items, start, end.saturating_add(1)))
    }
}

/// Second Life's `llDeleteSubString` / `llDeleteSubList`: everything but
/// `start..=end`, or only the inside of the outside when the start comes
/// after the end.
pub(crate) fn sl_delete<T: Clone>(items: &[T], start: i32, end: i32) -> Vec<T> {
    let (start, mut end) = (i64::from(start), i64::from(end));
    let wrap = wraps(start, end, items.len());
    if end == -1 {
        end = end.saturating_add(signed(items.len()));
    }
    if wrap {
        slice(items, end.saturating_add(1), start)
    } else {
        joined(head(items, start), tail(items, end.saturating_add(1)))
    }
}

/// Second Life's `llListInsertList`: `insert` before `at`, a negative `at`
/// counting from the end (`llInsertString` clamps a negative index to the
/// start instead, and calls this with `0`).
pub(crate) fn sl_insert<T: Clone>(items: &[T], insert: &[T], at: i32) -> Vec<T> {
    let at = i64::from(at);
    joined(joined(head(items, at), insert.to_vec()), tail(items, at))
}

/// Second Life's `llListReplaceList`: `start..=end` replaced by `insert`; a
/// start after the end keeps only what lies between them and appends
/// `insert`; a start before the list's beginning drops `insert`.
pub(crate) fn sl_replace<T: Clone>(items: &[T], insert: &[T], start: i32, end: i32) -> Vec<T> {
    let (start, mut end) = (i64::from(start), i64::from(end));
    let length = signed(items.len());
    let insert = if start < length.saturating_neg() {
        Vec::new()
    } else {
        insert.to_vec()
    };
    let wrap = wraps(start, end, items.len());
    if end == -1 {
        end = end.saturating_add(length);
    }
    if wrap {
        joined(slice(items, end.saturating_add(1), start), insert)
    } else {
        joined(
            joined(head(items, start), insert),
            tail(items, end.saturating_add(1)),
        )
    }
}

/// `items[from..=to]` with both inside the bounds, or nothing.
fn inclusive<T: Clone>(items: &[T], from: i64, to: i64) -> Vec<T> {
    match (usize::try_from(from), usize::try_from(to)) {
        (Ok(from), Ok(to)) if from <= to => items
            .get(from..=to.min(items.len().saturating_sub(1)))
            .unwrap_or_default()
            .to_vec(),
        _ => Vec::new(),
    }
}

/// OpenSim's `GetSublist` (`llList2List`, and `llGetSubString` on UTF-16
/// units, whose code is the same case by case).
pub(crate) fn os_get<T: Clone>(items: &[T], start: i32, end: i32) -> Vec<T> {
    let length = signed(items.len());
    let mut start = i64::from(start);
    let mut end = i64::from(end);
    if start < 0 {
        start = start.saturating_add(length);
    }
    if end < 0 {
        end = end.saturating_add(length);
    }
    if start <= end {
        if start >= length || end < 0 {
            return Vec::new();
        }
        return inclusive(items, start.max(0), end.min(length.saturating_sub(1)));
    }
    let prefix = if end < 0 {
        if start < 0 {
            return items.to_vec();
        }
        Vec::new()
    } else {
        os_get(items, 0, i32::try_from(end).unwrap_or(i32::MAX))
    };
    if start >= length {
        return prefix;
    }
    joined(prefix, inclusive(items, start, length.saturating_sub(1)))
}

/// OpenSim's `DeleteSublist` (`llDeleteSubList`).
pub(crate) fn os_delete_list<T: Clone>(items: &[T], start: i32, end: i32) -> Vec<T> {
    if items.is_empty() {
        return Vec::new();
    }
    let length = signed(items.len());
    let mut start = i64::from(start);
    let mut end = i64::from(end);
    if start < 0 {
        start = start.saturating_add(length).max(0);
    }
    if end < 0 {
        end = end.saturating_add(length).max(0);
    }
    if start > end {
        let from = end.saturating_add(1);
        if from >= length {
            return Vec::new();
        }
        let to = start.saturating_sub(1).min(length.saturating_sub(1));
        return inclusive(items, from, to);
    }
    if start >= length {
        return items.to_vec();
    }
    let end = end.min(length.saturating_sub(1)).saturating_add(1);
    joined(head(items, start), tail(items, end))
}

/// OpenSim's `llDeleteSubString`, on UTF-16 units: unlike the list's, the
/// indices are not held to zero once counted from the end.
pub(crate) fn os_delete_string<T: Clone>(items: &[T], start: i32, end: i32) -> Vec<T> {
    let length = signed(items.len());
    let mut start = i64::from(start);
    let mut end = i64::from(end);
    if start < 0 {
        start = start.saturating_add(length);
    }
    if end < 0 {
        end = end.saturating_add(length);
    }
    if start <= end {
        if end < 0 || start >= length {
            return items.to_vec();
        }
        let start = start.max(0);
        let end = end.min(length.saturating_sub(1)).saturating_add(1);
        return joined(head(items, start), tail(items, end));
    }
    if start < 0 || end >= length {
        return Vec::new();
    }
    let kept = if start < length {
        head(items, start)
    } else {
        items.to_vec()
    };
    if end > 0 {
        tail(&kept, end.saturating_add(1))
    } else {
        kept
    }
}

/// OpenSim's `llListInsertList`.
pub(crate) fn os_insert<T: Clone>(items: &[T], insert: &[T], at: i32) -> Vec<T> {
    let length = signed(items.len());
    let mut at = i64::from(at);
    if at < 0 {
        at = at.saturating_add(length).max(0);
    }
    let suffix = if at < length {
        tail(items, at)
    } else {
        Vec::new()
    };
    joined(joined(head(items, at), insert.to_vec()), suffix)
}

/// OpenSim's `llListReplaceList`.
pub(crate) fn os_replace<T: Clone>(items: &[T], insert: &[T], start: i32, end: i32) -> Vec<T> {
    let length = signed(items.len());
    let mut start = i64::from(start);
    let mut end = i64::from(end);
    if start < 0 {
        start = start.saturating_add(length);
    }
    if end < 0 {
        end = end.saturating_add(length);
    }
    let after = |end: i64| {
        if end.saturating_add(1) < length {
            tail(items, end.saturating_add(1))
        } else {
            Vec::new()
        }
    };
    if start <= end {
        return match start {
            0.. => joined(joined(head(items, start), insert.to_vec()), after(end)),
            _ => after(end),
        };
    }
    let between = os_get(
        items,
        i32::try_from(end.saturating_add(1)).unwrap_or(i32::MAX),
        i32::try_from(start.saturating_sub(1)).unwrap_or(i32::MAX),
    );
    joined(between, insert.to_vec())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    /// `abcdef` as a list of characters, and the answer as a string.
    fn on(f: impl Fn(&[char]) -> Vec<char>) -> String {
        let items: Vec<char> = "abcdef".chars().collect();
        f(&items).into_iter().collect()
    }

    #[test]
    fn second_life_wraps_a_reversed_range() {
        // PyOptimizer's own cases (`unit_tests/expr.suite`) and the wiki's.
        assert_eq!(on(|s| sl_get(s, 1, 3)), "bcd");
        assert_eq!(on(|s| sl_get(s, -2, -1)), "ef");
        assert_eq!(on(|s| sl_get(s, 4, 1)), "abef");
        assert_eq!(on(|s| sl_get(s, -1, 0)), "af");
        assert_eq!(on(|s| sl_get(s, 99, 100)), "");
        assert_eq!(on(|s| sl_get(s, -99, 99)), "abcdef");
        assert_eq!(on(|s| sl_delete(s, 1, 3)), "aef");
        assert_eq!(on(|s| sl_delete(s, 4, 1)), "cd");
        assert_eq!(on(|s| sl_delete(s, 1, -99)), "a");
        assert_eq!(on(|s| sl_insert(s, &['x'], -2)), "abcdxef");
        assert_eq!(on(|s| sl_insert(s, &['x'], 99)), "abcdefx");
        assert_eq!(on(|s| sl_replace(s, &['x'], 1, 2)), "axdef");
        assert_eq!(on(|s| sl_replace(s, &['x'], 4, 1)), "cdx");
        assert_eq!(on(|s| sl_replace(s, &['x'], -7, -7)), "abcdef");
        assert_eq!(on(|s| sl_replace(s, &['x'], -7, -6)), "bcdef");
    }

    #[test]
    fn opensim_cuts_by_its_own_code() {
        assert_eq!(on(|s| os_get(s, 1, 3)), "bcd");
        assert_eq!(on(|s| os_get(s, 4, 1)), "abef");
        assert_eq!(on(|s| os_get(s, -99, 99)), "abcdef");
        // `errors/bounds`: the two grids part on a range past both ends.
        assert_eq!(on(|s| os_delete_list(s, 1, -99)), "");
        assert_eq!(on(|s| os_delete_list(s, 4, 1)), "cd");
        assert_eq!(on(|s| os_delete_string(s, 4, 1)), "cd");
        assert_eq!(on(|s| os_delete_string(s, 1, 3)), "aef");
        assert_eq!(on(|s| os_insert(s, &['x'], -2)), "abcdxef");
        assert_eq!(on(|s| os_replace(s, &['x'], 1, 2)), "axdef");
        assert_eq!(on(|s| os_replace(s, &['x'], 4, 1)), "cdx");
    }
}
