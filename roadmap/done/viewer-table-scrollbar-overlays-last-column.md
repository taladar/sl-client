---
id: viewer-table-scrollbar-overlays-last-column
title: A scrolling table's scrollbar is painted over its last column
topic: viewer
status: done
origin: seen on aditi fixing [[viewer-search-multi-packet-page]] (2026-09-12)
points: 2
refs: [viewer-search-multi-packet-page, viewer-ui-table-widget,
  viewer-table-spawns-two-scrollbars]
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

The bar reserves its width instead of overlaying it, on both sides of the
split.

`layout_virtual_lists` decides the reservation from the same geometry the bar's
own visibility comes from — `scrollbar_geometry(..).is_some()` — publishes it as
`VirtualList::scrollbar_inset()`, and writes it as the rows' inline-end inset in
the same pass. Same frame on purpose: a frame late and a list that has just
crossed the threshold shows one frame of the bar over its last column. It
cannot feed back, either, because rows are a fixed height and the count does
not depend on the width, so unlike a wrapping layout this settles in one pass.
The inset is direction-resolved (`RowInset`): the track is pinned by
`LogicalInset`, so under RTL it sits on the left and the gutter has to follow
it. Every pooled row is written, parked ones included — a parked row is
re-shown by `display` alone, so it must already be the right width when it
comes back.

The header is a **sibling** of the viewport and out of the bar's reach, so
`reserve_table_header_gutter` mirrors the reservation onto it as extra trailing
padding over the spec's row padding. Padding rather than a narrower box, so the
header's background still runs the full width and only its content stops where
the rows' does. Without this half the header's flex columns come out a
scrollbar wider than the rows' and every boundary after the first flex column
drifts — the fixed columns, being px, would have stayed put and hidden it.

`apply_table_row_node` no longer sets `left` / `right`. Those inline edges now
belong to `layout_virtual_lists` along with `top` and `display`; leaving them in
the consumer's amendment put a freshly-pooled row under the bar for the frame
before the next layout pass took it back. `amend_row_node`'s contract says so.

This is the reference's shape: `LLScrollListCtrl::updateLayout` narrows
`mItemListRect` by the scrollbar when the bar is visible
(`llscrolllistctrl.cpp:618`), and `updateColumns` lays both the headers and the
cells inside the narrowed rect, expanding the last header to its right edge
(`:864`).

Tests: the reservation appears only when the bar does — a list that fits leaves
the header on its plain row padding and the rows at `right: 0`, a list that
does not inset both by the bar's thickness — and the last column's header and
its cells end at the same x, clear of the track. A third pins that a table's
viewport has exactly one bar (see [[viewer-table-spawns-two-scrollbars]], fixed
alongside).

## How to verify

A widget test in `ui_table.rs`: a table whose content fits leaves the rows'
inline-end inset at zero, and one long enough to scroll insets both the header
and the rows by the bar's thickness — with the header's and a row's last cell
ending at the same x. Plus an eyeball in the gallery, and a live check that
Search → Places on aditi shows a whole `0` under **Traffic**.

Verified on aditi: Search → Places shows a whole `0` under **Traffic**, with
the bar beside it and the header lined up with the numbers.

The **gallery cannot check this** — worth knowing before reaching for it next
time. Nearly every entry in `crate::floaters::FLOATERS` is a
`FloaterContent::Stub` (chrome, no content), and even the `Specimen` ones are
static mocks: `spawn_debug_settings_specimen` builds "a static three-row list",
and the radar specimen's own comment says the live floater binds a virtualized
table while the specimen is flat. Nothing there spawns a real scrolling
`spawn_table`, so there is no scrollbar in the gallery to look at.
