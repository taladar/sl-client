---
id: viewer-table-scrollbar-overlays-last-column
title: A scrolling table's scrollbar is painted over its last column
topic: viewer
status: bugs
origin: seen on aditi fixing [[viewer-search-multi-packet-page]] (2026-09-12)
points: 2
refs: [viewer-search-multi-packet-page, viewer-ui-table-widget]
---

Context: [context/viewer.md](../context/viewer.md).

Search → **Places** on Second Life, with enough results to scroll: the
scrollbar covers the right edge of the **Traffic** column, so most of each
`0` is hidden. It is not a Places problem — every table the widget spawns has
it, and it bites hardest on a right-aligned numeric last column, where the
hidden edge is the *last digit*.

`spawn_virtual_scrollbar` (`sl-viewer-ui-core/src/virtual_list.rs:133`) makes
the bar an **overlay**: `PositionType::Absolute`, `inline_end: 0`,
`width: SCROLLBAR_THICKNESS` (10 px), `ZIndex(1)` — explicitly "above the
pooled rows". Nothing narrows the rows underneath it, so the last column's
final 10 px are simply painted over.

The header makes it worse rather than better. In `spawn_table`
(`sl-viewer-ui-widgets/src/ui_table.rs`) the header is a *sibling* of the
viewport, and the bar is a child of the viewport — so the bar covers the rows
but not the header. The header's last column therefore gets the full width
while the rows' last column effectively gets 10 px less, and the two are
misaligned by the bar's thickness whenever it is visible.

## The reference

`LLScrollListCtrl` reserves the space instead of overlaying it
(`llscrolllistctrl.cpp:602` `updateLayout`):

```cpp
bool scrollbar_visible = mLineHeight * getItemCount() > mItemListRect.getHeight();
if (scrollbar_visible)
{
    // provide space on the right for scrollbar
    mItemListRect.mRight = getRect().getWidth() - mBorderThickness - scrollbar_size;
}
...
dirtyColumns();
```

`updateColumns` (`:811`) then lays **both** the headers and the cells inside
that narrowed `mItemListRect` — the last header is expanded to
`mItemListRect.mRight` (`:864`), the already-reserved edge. So the columns
narrow together and stay aligned, and the bar sits beside the content rather
than on it. The bar's own height covers the heading too
(`mItemListRect.getHeight() + mHeadingHeight`, `:627`).

## The fix

Reserve, don't overlay. When the bar is visible, inset the row content *and*
the header by `SCROLLBAR_THICKNESS` on the inline end; when it hides, give the
space back. The inset has to reach the header, which lives in
`sl-viewer-ui-widgets`, while the bar's visibility is decided in
`sl-viewer-ui-core` — so the table widget needs to read that visibility (or the
list needs to publish "the bar is showing") rather than each side guessing.

Watch the feedback loop the reference has too: narrowing the content can change
nothing about the row *count*, so unlike a wrapping layout this cannot
oscillate — but the inset must be applied from the same frame's visibility, not
a frame late, or a table that just crossed the threshold shows one frame of
overlap.

Persisted column widths (`apply_persisted_widths` / `encode_widths`) are
fractions of the header's width, so check a restored layout after the change:
the widths must still round-trip when the reserved edge is in play.

## How to verify

A widget test in `ui_table.rs`: a table whose content fits leaves the rows'
inline-end inset at zero, and one long enough to scroll insets both the header
and the rows by the bar's thickness — with the header's and a row's last cell
ending at the same x. Plus an eyeball in the gallery, and a live check that
Search → Places on aditi shows a whole `0` under **Traffic**.
