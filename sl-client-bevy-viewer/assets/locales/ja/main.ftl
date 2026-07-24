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

## The minimap floater (minimap.rs).

minimap-floater-title = ミニマップ
# Compass labels around the map edge.
minimap-compass-north = 北
minimap-compass-north-east = 北東
minimap-compass-east = 東
minimap-compass-south-east = 南東
minimap-compass-south = 南
minimap-compass-south-west = 南西
minimap-compass-west = 西
minimap-compass-north-west = 北西
minimap-tooltip-avatar = { $name }（{ $distance } m）
minimap-tooltip-avatar-far = { $name }（> { $distance } m）
minimap-tooltip-region = リージョン: { $name }
minimap-tooltip-parcel = 区画: { $name }
minimap-tooltip-owner = 所有者: { $name }
minimap-tooltip-sale = 売り出し中: L$ { $price }（{ $area } m²）
minimap-tooltip-hint-teleport = ダブルクリックでテレポート
minimap-tooltip-hint-map = ダブルクリックで世界地図を開く

## The world-map floater (world_map.rs).

worldmap-floater-title = 世界地図
worldmap-tooltip-region = リージョン: { $name }
worldmap-tooltip-region-agents = アバター { $count } 人
worldmap-maturity-general = レーティング: 一般
worldmap-maturity-moderate = レーティング: 中程度
worldmap-maturity-adult = レーティング: アダルト
worldmap-tooltip-agents = ここにアバター { $count } 人
worldmap-tooltip-telehub = テレハブ: { $name }
worldmap-tooltip-infohub = インフォハブ: { $name }
worldmap-tooltip-land-sale = 売り出し中: { $name } — L$ { $price }（{ $area } m²）
worldmap-tooltip-event = イベント: { $name }
worldmap-location-none = 地図をクリックして場所を選択
worldmap-button-teleport = テレポート
worldmap-button-copy-slurl = SLURLをコピー
worldmap-layer-people = 人
worldmap-layer-infohubs = テレハブ
worldmap-layer-land-sale = 売地
worldmap-layer-events = イベント
worldmap-layer-mature-events = 中程度のイベント
worldmap-layer-adult-events = アダルトイベント
worldmap-layer-region-names = リージョン名

# Build tools (the object edit floater).
build-tools-floater-title = 制作ツール
build-tool-move = 移動
build-tool-rotate = 回転
build-tool-stretch = 拡縮
build-toggle-snap = グリッドにスナップ
build-toggle-local-frame = ローカル軸
build-toggle-edit-linked = リンク部分を編集
build-toggle-stretch-both = 両側に伸縮
build-grid-unit-label = グリッド単位 (m)
build-position-label = 位置
build-rotation-label = 回転
build-size-label = サイズ
build-tab-general = 一般
build-tab-object = オブジェクト
build-tab-features = 特徴
build-tab-texture = テクスチャ
build-tab-content = コンテンツ
build-tab-placeholder = 未実装
build-selection-none = 何も選択されていません
build-selection-count = { $count } 個のオブジェクトを選択中
build-selection-no-modify = 編集不可

# Build tools parameter tabs (viewer-prim-parameter-editing).
build-info-creator = 制作者
build-info-owner = 所有者
build-info-you-can = あなたの権限
build-group-label = グループ
build-group-none = （なし）
build-deed = 譲渡
build-share-group = グループと共有
build-next-owner-label = 次の所有者の権限
build-anyone-label = 全員
build-perm-modify = 編集
build-perm-copy = コピー
build-perm-transfer = 譲渡
build-perm-move = 移動
build-object-name-label = 名前
build-object-desc-label = 説明
build-flag-physical = 物理
build-flag-temporary = 一時的
build-flag-phantom = ファントム
build-type-label = タイプ
build-type-box = ボックス
build-type-cylinder = 円柱
build-type-prism = プリズム
build-type-sphere = 球
build-type-torus = トーラス
build-type-tube = チューブ
build-type-ring = リング
build-type-sculpt = スカルプト
build-type-mesh = メッシュ
build-cut-label = パスカット（始/終）
build-hollow-label = 中空（%）
build-hole-default = デフォルト
build-hole-circle = 円
build-hole-square = 四角
build-hole-triangle = 三角
build-twist-label = ねじれ（始/終）
build-taper-label = テーパー
build-hole-size-label = 穴のサイズ
build-shear-label = 上部シアー
build-adv-profile-cut-label = プロファイルカット（始/終）
build-adv-dimple-label = ディンプル（始/終）
build-adv-slice-label = スライス（始/終）
build-taper2-label = プロファイルテーパー
build-radius-offset-label = 半径
build-revolutions-label = 回転数
build-skew-label = スキュー
build-material-label = 素材
build-material-stone = 石
build-material-metal = 金属
build-material-glass = ガラス
build-material-wood = 木
build-material-flesh = 肉
build-material-plastic = プラスチック
build-material-rubber = ゴム
build-material-light = ライト（旧式）
build-feature-flexi = フレキシブルパス
build-flex-softness-label = 柔らかさ
build-flex-gravity-label = 重力
build-flex-friction-label = 抵抗
build-flex-wind-label = 風
build-flex-tension-label = 張力
build-flex-force-label = 力（X/Y/Z）
build-feature-light = ライト
build-light-color-label = 色（sRGB）
build-light-intensity-label = 強度
build-light-radius-label = 半径（m）
build-light-falloff-label = 減衰
build-spot-label = スポット（FOV/フォーカス/環境光）
bottom-toolbar-build = 制作
