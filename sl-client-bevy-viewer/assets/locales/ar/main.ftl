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
