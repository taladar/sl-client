# Japanese bundle. Two things it demonstrates that Latin cannot: the CJK
# truncation ellipsis (a centred six-dot form, not the Latin `…`), and a
# language with a single plural category, so `items-selected` needs no branches.

# CJK convention: a centred six-dot ellipsis rather than the Latin three dots.
ui-ellipsis = ……

i18n-demo-title = 国際化

language-name = 日本語

greeting = こんにちは、{ $name } さん！

# Japanese has one plural category (`other`), so there is nothing to branch on.
items-selected = { $count } 個を選択中

friend-status =
    { $gender ->
       *[other] オンライン中
    }

## The inventory window (viewer-inventory-*).

inventory-title = インベントリ
inventory-tab-everything = すべて
inventory-tab-recent = 最近
inventory-tab-worn = 着用中
inventory-expand-all = すべて展開
inventory-collapse-all = すべて折りたたむ

## The emoji-picker floater (viewer-emoji-picker-floater).

emoji-picker-title = 絵文字
