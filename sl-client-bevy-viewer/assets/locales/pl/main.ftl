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
## The item properties floater + Open previews
## (viewer-inventory-open-and-properties).

item-properties-title = Właściwości przedmiotu
item-properties-name = Nazwa:
item-properties-description = Opis:
item-properties-creator = Twórca:
item-properties-owner = Właściciel:
item-properties-acquired = Uzyskano:
item-properties-you-can = Możesz:
item-properties-modify = Modyfikować
item-properties-copy = Kopiować
item-properties-transfer = Przekazać
item-properties-group = Grupa:
item-properties-share = Udostępnij
item-properties-anyone = Wszyscy:
item-properties-next-owner = Następny właściciel:
item-properties-for-sale = Na sprzedaż
item-properties-sale-original = Oryginał
item-properties-sale-copy = Kopia
item-properties-sale-contents = Zawartość
landmark-teleport = Teleportuj
animation-play-inworld = Odtwórz w świecie
animation-stop = Zatrzymaj

## The inventory gallery (viewer-inventory-gallery).

inventory-gallery-title = Galeria ekwipunku

## The minimap floater (minimap.rs).

minimap-floater-title = Minimapa
# Compass labels around the map edge (international letters).
minimap-compass-north = N
minimap-compass-north-east = NE
minimap-compass-east = E
minimap-compass-south-east = SE
minimap-compass-south = S
minimap-compass-south-west = SW
minimap-compass-west = W
minimap-compass-north-west = NW
minimap-tooltip-avatar = { $name } ({ $distance } m)
minimap-tooltip-avatar-far = { $name } (> { $distance } m)
minimap-tooltip-region = Region: { $name }
minimap-tooltip-parcel = Działka: { $name }
minimap-tooltip-owner = Właściciel: { $name }
minimap-tooltip-sale = Na sprzedaż: L$ { $price } ({ $area } m²)
minimap-tooltip-hint-teleport = Kliknij dwukrotnie, aby się teleportować
minimap-tooltip-hint-map = Kliknij dwukrotnie, aby otworzyć mapę świata

## The world-map floater (world_map.rs).

worldmap-floater-title = Mapa świata
worldmap-tooltip-region = Region: { $name }
worldmap-tooltip-region-agents = Awatary: { $count }
worldmap-maturity-general = Kategoria: Ogólna
worldmap-maturity-moderate = Kategoria: Umiarkowana
worldmap-maturity-adult = Kategoria: Dla dorosłych
worldmap-tooltip-agents = Awatary tutaj: { $count }
worldmap-tooltip-telehub = Telehub: { $name }
worldmap-tooltip-infohub = Infohub: { $name }
worldmap-tooltip-land-sale = Na sprzedaż: { $name } — L$ { $price } ({ $area } m²)
worldmap-tooltip-event = Wydarzenie: { $name }
worldmap-location-none = Kliknij mapę, aby wybrać miejsce
worldmap-button-teleport = Teleportuj
worldmap-button-copy-slurl = Kopiuj SLURL
worldmap-layer-people = Ludzie
worldmap-layer-infohubs = Telehuby
worldmap-layer-land-sale = Ziemia na sprzedaż
worldmap-layer-events = Wydarzenia
worldmap-layer-mature-events = Wydarzenia umiarkowane
worldmap-layer-adult-events = Wydarzenia dla dorosłych
worldmap-layer-region-names = Nazwy regionów
