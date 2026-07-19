# Polish bundle. The plural case that motivates using Fluent at all: Polish has
# four CLDR categories — `one` (1), `few` (2-4, 22-24, …), `many` (0, 5-21, …)
# and `other` (fractions) — which the reference viewer's three-language
# `getCountString` cannot express. Fluent picks the branch from the numeric
# argument's CLDR rule for `pl`, so this is correct where the reference is not.

ui-ellipsis = …

i18n-demo-title = Internacjonalizacja

language-name = Polski

greeting = Cześć, { $name }!

items-selected =
    { $count ->
        [one] Zaznaczono { $count } element
        [few] Zaznaczono { $count } elementy
        [many] Zaznaczono { $count } elementów
       *[other] Zaznaczono { $count } elementu
    }

friend-status =
    { $gender ->
        [male] On jest online
        [female] Ona jest online
       *[other] Są online
    }

## The inventory window (viewer-inventory-*).

inventory-title = Ekwipunek
inventory-tab-everything = Wszystko
inventory-tab-recent = Ostatnie
inventory-tab-worn = Noszone
inventory-expand-all = Rozwiń wszystko
inventory-collapse-all = Zwiń wszystko
