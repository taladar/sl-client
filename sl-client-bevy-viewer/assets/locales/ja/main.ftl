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
## The inventory filters floater (viewer-inventory-advanced-filters).

inventory-filters-title = インベントリフィルター
inventory-filter-animations = アニメーション
inventory-filter-calling-cards = コーリングカード
inventory-filter-clothing = 衣類
inventory-filter-gestures = ジェスチャー
inventory-filter-landmarks = ランドマーク
inventory-filter-materials = マテリアル
inventory-filter-notecards = ノートカード
inventory-filter-objects = オブジェクト
inventory-filter-scripts = スクリプト
inventory-filter-sounds = サウンド
inventory-filter-textures = テクスチャ
inventory-filter-snapshots = スナップショット
inventory-filter-settings = 環境設定
inventory-filter-all = すべて
inventory-filter-none = なし
inventory-filter-worn = 着用中のみ
inventory-filter-since-login = ログイン以降
inventory-filter-newer-than = より新しい
inventory-filter-older-than = より古い
inventory-filter-hours-label = 時間
inventory-filter-days-label = 日
inventory-filter-reset = リセット

## The avatar picker floater (viewer-inventory-share-picker).

avatar-picker-title = 住人を選択
avatar-picker-tab-search = 検索
avatar-picker-tab-friends = フレンド
avatar-picker-tab-near-me = 近くの人
avatar-picker-go = 検索
avatar-picker-ok = OK
avatar-picker-cancel = キャンセル
## The item properties floater + Open previews
## (viewer-inventory-open-and-properties).

item-properties-title = アイテムのプロパティ
item-properties-name = 名前:
item-properties-description = 説明:
item-properties-creator = 制作者:
item-properties-owner = 所有者:
item-properties-acquired = 取得日:
item-properties-you-can = あなたの権限:
item-properties-modify = 編集
item-properties-copy = コピー
item-properties-transfer = 譲渡
item-properties-group = グループ:
item-properties-share = 共有
item-properties-anyone = 全員:
item-properties-next-owner = 次の所有者:
item-properties-for-sale = 販売中
item-properties-sale-original = オリジナル
item-properties-sale-copy = コピー
item-properties-sale-contents = 中身
landmark-teleport = テレポート
animation-play-inworld = ワールドで再生
animation-stop = 停止

## The inventory gallery (viewer-inventory-gallery).

inventory-gallery-title = インベントリギャラリー
