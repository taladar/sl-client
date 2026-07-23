# Arabic bundle. A right-to-left locale (the direction is carried on `UiLocale`
# and flips the whole layout), and one whose plural rules have all six CLDR
# categories — the case a hardcoded English-style if-ladder gets most wrong.

ui-ellipsis = …

i18n-demo-title = التدويل

language-name = العربية

greeting = مرحبًا، { $name }!

items-selected =
    { $count ->
        [zero] لم يتم تحديد أي عنصر
        [one] تم تحديد عنصر واحد
        [two] تم تحديد عنصرين
        [few] تم تحديد { $count } عناصر
        [many] تم تحديد { $count } عنصرًا
       *[other] تم تحديد { $count } عنصر
    }

friend-status =
    { $gender ->
        [male] هو متصل الآن
        [female] هي متصلة الآن
       *[other] متصل الآن
    }

## The inventory window (viewer-inventory-*).

inventory-title = المخزون
inventory-tab-everything = الكل
inventory-tab-recent = الأحدث
inventory-tab-worn = المرتدى
inventory-expand-all = توسيع الكل
inventory-collapse-all = طي الكل

## The emoji-picker floater (viewer-emoji-picker-floater).

emoji-picker-title = رموز تعبيرية
## The inventory filters floater (viewer-inventory-advanced-filters).

inventory-filters-title = مرشحات المخزون
inventory-filter-animations = الحركات
inventory-filter-calling-cards = بطاقات الاتصال
inventory-filter-clothing = الملابس
inventory-filter-gestures = الإيماءات
inventory-filter-landmarks = المعالم
inventory-filter-materials = الخامات
inventory-filter-notecards = البطاقات
inventory-filter-objects = الكائنات
inventory-filter-scripts = البرامج النصية
inventory-filter-sounds = الأصوات
inventory-filter-textures = القوام
inventory-filter-snapshots = اللقطات
inventory-filter-settings = إعدادات البيئة
inventory-filter-all = الكل
inventory-filter-none = لا شيء
inventory-filter-worn = المرتدى فقط
inventory-filter-since-login = منذ تسجيل الدخول
inventory-filter-newer-than = أحدث من
inventory-filter-older-than = أقدم من
inventory-filter-hours-label = ساعات
inventory-filter-days-label = أيام
inventory-filter-reset = إعادة تعيين

## The avatar picker floater (viewer-inventory-share-picker).

avatar-picker-title = اختر مقيماً
avatar-picker-tab-search = بحث
avatar-picker-tab-friends = الأصدقاء
avatar-picker-tab-near-me = بالقرب مني
avatar-picker-go = بحث
avatar-picker-ok = موافق
avatar-picker-cancel = إلغاء
## The item properties floater + Open previews
## (viewer-inventory-open-and-properties).

item-properties-title = خصائص العنصر
item-properties-name = الاسم:
item-properties-description = الوصف:
item-properties-creator = المنشئ:
item-properties-owner = المالك:
item-properties-acquired = تاريخ الحصول:
item-properties-you-can = يمكنك:
item-properties-modify = تعديل
item-properties-copy = نسخ
item-properties-transfer = نقل
item-properties-group = المجموعة:
item-properties-share = مشاركة
item-properties-anyone = الجميع:
item-properties-next-owner = المالك التالي:
item-properties-for-sale = للبيع
item-properties-sale-original = الأصل
item-properties-sale-copy = نسخة
item-properties-sale-contents = المحتويات
landmark-teleport = الانتقال الآني
animation-play-inworld = تشغيل في العالم
animation-stop = إيقاف

## The inventory gallery (viewer-inventory-gallery).

inventory-gallery-title = معرض المخزون

## The minimap floater (minimap.rs).

minimap-floater-title = خريطة مصغّرة
# Compass labels around the map edge.
minimap-compass-north = ش
minimap-compass-north-east = ش‌ق
minimap-compass-east = ق
minimap-compass-south-east = ج‌ق
minimap-compass-south = ج
minimap-compass-south-west = ج‌غ
minimap-compass-west = غ
minimap-compass-north-west = ش‌غ
minimap-tooltip-avatar = { $name } ({ $distance } م)
minimap-tooltip-avatar-far = { $name } (> { $distance } م)
minimap-tooltip-region = المنطقة: { $name }
minimap-tooltip-parcel = قطعة الأرض: { $name }
minimap-tooltip-owner = المالك: { $name }
minimap-tooltip-sale = للبيع: L$ { $price } ({ $area } م²)
minimap-tooltip-hint-teleport = انقر نقرًا مزدوجًا للانتقال الآني
minimap-tooltip-hint-map = انقر نقرًا مزدوجًا لفتح خريطة العالم

## The world-map floater (world_map.rs).

worldmap-floater-title = خريطة العالم
worldmap-tooltip-region = المنطقة: { $name }
worldmap-tooltip-region-agents = { $count } أفاتار
worldmap-maturity-general = التصنيف: عام
worldmap-maturity-moderate = التصنيف: متوسط
worldmap-maturity-adult = التصنيف: للبالغين
worldmap-tooltip-agents = { $count } أفاتار هنا
worldmap-tooltip-telehub = تيليهَب: { $name }
worldmap-tooltip-infohub = إنفوهَب: { $name }
worldmap-tooltip-land-sale = للبيع: { $name } — L$ { $price } ({ $area } م²)
worldmap-tooltip-event = حدث: { $name }
worldmap-location-none = انقر على الخريطة لاختيار موقع
worldmap-button-teleport = انتقال آني
worldmap-button-copy-slurl = نسخ SLURL
worldmap-layer-people = الأشخاص
worldmap-layer-infohubs = تيليهَبات
worldmap-layer-land-sale = أراضٍ للبيع
worldmap-layer-events = الأحداث
worldmap-layer-mature-events = أحداث متوسطة
worldmap-layer-adult-events = أحداث للبالغين
worldmap-layer-region-names = أسماء المناطق

# Build tools (the object edit floater).
build-tools-floater-title = أدوات البناء
build-tool-move = تحريك
build-tool-rotate = تدوير
build-tool-stretch = تمديد
build-toggle-snap = محاذاة إلى الشبكة
build-toggle-local-frame = محاور محلية
build-toggle-edit-linked = تحرير الأجزاء المرتبطة
build-toggle-stretch-both = تمديد الجانبين
build-grid-unit-label = وحدة الشبكة (م)
build-position-label = الموضع
build-rotation-label = الدوران
build-size-label = الحجم
build-tab-object = الكائن
build-tab-features = الميزات
build-tab-texture = النسيج
build-tab-content = المحتوى
build-tab-placeholder = غير منفذ بعد
build-selection-none = لا شيء محدد
build-selection-count = { $count ->
    [zero] لا كائنات محددة
    [one] كائن واحد محدد
    [two] كائنان محددان
    [few] { $count } كائنات محددة
   *[other] { $count } كائنًا محددًا
}
build-selection-no-modify = غير قابل للتعديل
bottom-toolbar-build = بناء
