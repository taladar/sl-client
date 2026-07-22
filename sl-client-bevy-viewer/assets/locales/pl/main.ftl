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

## The emoji-picker floater (viewer-emoji-picker-floater).

emoji-picker-title = Emoji
## The inventory filters floater (viewer-inventory-advanced-filters).

inventory-filters-title = Filtry ekwipunku
inventory-filter-animations = Animacje
inventory-filter-calling-cards = Wizytówki
inventory-filter-clothing = Ubrania
inventory-filter-gestures = Gesty
inventory-filter-landmarks = Landmarki
inventory-filter-materials = Materiały
inventory-filter-notecards = Notki
inventory-filter-objects = Obiekty
inventory-filter-scripts = Skrypty
inventory-filter-sounds = Dźwięki
inventory-filter-textures = Tekstury
inventory-filter-snapshots = Zdjęcia
inventory-filter-settings = Ustawienia środowiska
inventory-filter-all = Wszystkie
inventory-filter-none = Żadne
inventory-filter-worn = Tylko noszone
inventory-filter-since-login = Od zalogowania
inventory-filter-newer-than = Nowsze niż
inventory-filter-older-than = Starsze niż
inventory-filter-hours-label = Godziny
inventory-filter-days-label = Dni
inventory-filter-reset = Resetuj

## The avatar picker floater (viewer-inventory-share-picker).

avatar-picker-title = Wybierz rezydenta
avatar-picker-tab-search = Szukaj
avatar-picker-tab-friends = Znajomi
avatar-picker-tab-near-me = W pobliżu
avatar-picker-go = Szukaj
avatar-picker-ok = OK
avatar-picker-cancel = Anuluj
