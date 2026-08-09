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

## The notecard viewer & editor floater (viewer-notecard-editor).

notecard-save = حفظ
notecard-view-preview = عرض العناصر
notecard-view-edit = تحرير النص
notecard-readonly-note = ليس لديك إذن لتعديل هذه الملاحظة.
notecard-status-loading = جارٍ التحميل…
notecard-status-saving = جارٍ الحفظ…
notecard-status-saved = تم الحفظ.
notecard-status-save-failed = فشل الحفظ.
notecard-status-decode-failed = تعذَّرت قراءة هذه الملاحظة.

## The LSL script editor floater (viewer-lsl-editor-save-compile).

script-save = حفظ وترجمة
script-readonly-note = ليس لديك إذن لتعديل هذا النص البرمجي.
script-running = قيد التشغيل
script-status-loading = جارٍ التحميل…
script-status-saving = جارٍ الحفظ والترجمة…
script-status-saved = تم الحفظ.
script-status-compile-failed = فشلت الترجمة.
script-status-save-failed = فشل الحفظ.
script-error-at = السطر { $line }، العمود { $column }: { $message }
script-error-nopos = { $message }

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
build-tab-general = عام
build-tab-object = الكائن
build-tab-features = الميزات
build-tab-texture = النسيج
build-tab-content = المحتوى
build-tab-placeholder = غير منفذ بعد

# علامة تبويب المحتوى + نافذة محتويات الكائن (viewer-prim-inventory-editing).
build-content-no-target = لم يتم تحديد أي كائن
build-content-loading = جارٍ تحميل المحتويات…
build-content-count = { $count ->
    [zero] لا عناصر
    [one] عنصر واحد
    [two] عنصران
    [few] { $count } عناصر
   *[other] { $count } عنصرًا
}
build-content-new-script = سكربت جديد
build-content-new-script-name = سكربت جديد
build-content-rename = إعادة تسمية
build-content-remove = إزالة
build-content-refresh = تحديث
build-content-no-modify = ليس لديك إذن لتعديل محتويات هذا الكائن.
build-content-item-no-modify = هذا العنصر غير قابل للتعديل.
build-content-state-adding = …جارٍ الإضافة
build-content-state-deleting = …جارٍ الحذف
build-content-state-refreshing = …جارٍ التحديث
object-contents-floater-title = محتويات الكائن
object-contents-none = لا شيء لعرضه
object-contents-copy = نسخ إلى المخزون
object-contents-copy-wear = نسخ وارتداء
object-contents-no-folder = مخزونك غير جاهز بعد — حاول مرة أخرى بعد لحظة.
object-contents-wear-note = تم النسخ إلى مخزونك؛ ارتدها من هناك.
build-selection-none = لا شيء محدد
build-selection-count = { $count ->
    [zero] لا كائنات محددة
    [one] كائن واحد محدد
    [two] كائنان محددان
    [few] { $count } كائنات محددة
   *[other] { $count } كائنًا محددًا
}
build-selection-no-modify = غير قابل للتعديل

# Build tools parameter tabs (viewer-prim-parameter-editing).
build-info-creator = المنشئ
build-info-owner = المالك
build-info-you-can = يمكنك
build-group-label = المجموعة
build-group-none = (لا شيء)
build-deed = تمليك للمجموعة
build-share-group = مشاركة مع المجموعة
build-next-owner-label = المالك التالي يمكنه
build-anyone-label = الجميع
build-perm-modify = تعديل
build-perm-copy = نسخ
build-perm-transfer = نقل
build-perm-move = تحريك
build-object-name-label = الاسم
build-object-desc-label = الوصف
build-flag-physical = فيزيائي
build-flag-temporary = مؤقت
build-flag-phantom = شبح
build-type-label = النوع
build-type-box = صندوق
build-type-cylinder = أسطوانة
build-type-prism = موشور
build-type-sphere = كرة
build-type-torus = حلقة دائرية
build-type-tube = أنبوب
build-type-ring = خاتم
build-type-sculpt = منحوت
build-type-mesh = شبكة
build-cut-label = قطع المسار (بداية/نهاية)
build-hollow-label = تجويف (%)
build-hole-default = افتراضي
build-hole-circle = دائرة
build-hole-square = مربع
build-hole-triangle = مثلث
build-twist-label = التواء (بداية/نهاية)
build-taper-label = استدقاق
build-hole-size-label = حجم الفتحة
build-shear-label = قص علوي
build-adv-profile-cut-label = قطع المقطع (بداية/نهاية)
build-adv-dimple-label = نقرة (بداية/نهاية)
build-adv-slice-label = شريحة (بداية/نهاية)
build-taper2-label = استدقاق المقطع
build-radius-offset-label = نصف القطر
build-revolutions-label = دورات
build-skew-label = انحراف
build-material-label = المادة
build-material-stone = حجر
build-material-metal = معدن
build-material-glass = زجاج
build-material-wood = خشب
build-material-flesh = لحم
build-material-plastic = بلاستيك
build-material-rubber = مطاط
build-material-light = ضوء (قديم)
build-feature-flexi = مسار مرن
build-flex-softness-label = نعومة
build-flex-gravity-label = جاذبية
build-flex-friction-label = مقاومة
build-flex-wind-label = رياح
build-flex-tension-label = شد
build-flex-force-label = قوة (X/Y/Z)
build-feature-light = ضوء
build-light-color-label = اللون (sRGB)
build-light-intensity-label = الشدة
build-light-radius-label = نصف القطر (م)
build-light-falloff-label = تلاشي
build-spot-label = كشاف (زاوية/تركيز/محيط)
bottom-toolbar-build = بناء
