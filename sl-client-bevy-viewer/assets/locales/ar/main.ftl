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
