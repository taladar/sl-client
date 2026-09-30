//! The selector grammar: what prints, what parses, that the two agree for
//! every locator shape, and that a mistake is refused where it is.

use pretty_assertions::assert_eq;
use uuid::Uuid;

use crate::locator::{Locator, NameMatcher};
use crate::snapshot::Role;
use crate::world::{Anchor, WorldKind, WorldLocator};

/// Strings a name, key or test id may hold, the awkward ones included.
const TEXTS: [&str; 9] = [
    "OK",
    "floater:build",
    "prefs.ok",
    "Apply all",
    "",
    " padded ",
    "say \"hi\" \\ there",
    "tab\tline\nreturn\r",
    "bell\u{7}ünïcødé ✓ ] >> [",
];

/// Every UI locator shape: each criterion alone (each role, each text in
/// each text field, each state filter both ways, an index), all at once,
/// and scoped once and twice.
fn ui_shapes() -> Vec<Locator> {
    let mut shapes = vec![Locator::default()];
    shapes.extend(Role::ALL.into_iter().map(Locator::role));
    for text in TEXTS {
        shapes.push(Locator::default().named(text));
        shapes.push(Locator::default().name_containing(text));
        shapes.push(Locator::default().name_key(text));
        shapes.push(Locator::test_id(text));
    }
    for wanted in [true, false] {
        shapes.push(Locator::default().enabled(wanted));
        shapes.push(Locator::default().checked(wanted));
        shapes.push(Locator::default().selected(wanted));
        shapes.push(Locator::default().expanded(wanted));
        shapes.push(Locator::default().focused(wanted));
    }
    shapes.push(Locator::default().nth(0));
    shapes.push(Locator::default().nth(u32::MAX));
    let everything = Locator::role(Role::Checkbox)
        .named("Say \"yes\"")
        .name_key("prefs-yes")
        .enabled(true)
        .checked(false)
        .selected(true)
        .expanded(false)
        .focused(true)
        .nth(2);
    let everything = Locator {
        test_id: Some("prefs.yes".to_owned()),
        ..everything
    };
    shapes.push(everything.clone());
    let window = Locator::role(Role::Window).name_key("floater-build");
    shapes.push(everything.clone().within(window.clone()));
    shapes.push(Locator::default().within(window.clone()));
    shapes.push(
        Locator::role(Role::Button)
            .name_containing("Apply")
            .within(Locator::test_id("panel general").within(window)),
    );
    shapes
}

/// Every world locator shape: each criterion alone, both anchors with and
/// without a radius, and all at once.
fn world_shapes() -> Vec<WorldLocator> {
    let mut shapes = vec![WorldLocator::default(), WorldLocator::own_avatar()];
    shapes.extend(WorldKind::ALL.into_iter().map(WorldLocator::kind));
    for text in TEXTS {
        shapes.push(WorldLocator::default().named(text));
        shapes.push(WorldLocator::default().name_containing(text));
        shapes.push(WorldLocator {
            hover_text: Some(NameMatcher::Exact(text.to_owned())),
            ..WorldLocator::default()
        });
        shapes.push(WorldLocator::default().hover_text_containing(text));
    }
    shapes.push(WorldLocator::default().own(false));
    shapes.push(WorldLocator::full_id(Uuid::from_u128(0x1234_5678_9abc)));
    shapes.push(WorldLocator::default().local_id(u32::MAX));
    shapes.push(WorldLocator::default().owned_by(Uuid::max()));
    shapes.push(WorldLocator::default().pcode(255));
    for radius in [None, Some(5.0), Some(0.125)] {
        shapes.push(WorldLocator::default().near(Anchor::OwnAvatar, radius));
        shapes.push(WorldLocator::default().near(Anchor::Point([128.5, -0.25, 1e-3]), radius));
    }
    shapes.push(WorldLocator::default().nth(7));
    shapes.push(
        WorldLocator::kind(WorldKind::Attachment)
            .own(true)
            .named("Hat")
            .local_id(4)
            .owned_by(Uuid::from_u128(5))
            .pcode(9)
            .hover_text_containing("hi")
            .near(Anchor::Point([1.0, 2.0, 3.0]), Some(10.0))
            .nth(1),
    );
    shapes
}

#[test]
fn every_ui_locator_shape_round_trips() -> Result<(), crate::SelectorError> {
    for locator in ui_shapes() {
        let printed = locator.to_string();
        assert_eq!(printed.parse::<Locator>()?, locator, "{printed}");
    }
    Ok(())
}

#[test]
fn every_world_locator_shape_round_trips() -> Result<(), crate::SelectorError> {
    for locator in world_shapes() {
        let printed = locator.to_string();
        assert_eq!(printed.parse::<WorldLocator>()?, locator, "{printed}");
    }
    Ok(())
}

#[test]
fn ui_locators_print_scope_first_values_bare_when_plain() {
    let apply = Locator::role(Role::Button)
        .name_key("build-apply")
        .enabled(false)
        .within(Locator {
            role: Some(Role::Window),
            ..Locator::test_id("floater:build")
        });
    assert_eq!(
        apply.to_string(),
        "window[test_id=floater:build] >> button[name_key=build-apply][enabled=false]"
    );
    assert_eq!(Locator::default().to_string(), "*");
    assert_eq!(Locator::test_id("a.b").to_string(), "[test_id=a.b]");
    assert_eq!(
        Locator::role(Role::Button)
            .name_containing("Apply all")
            .nth(1)
            .to_string(),
        r#"button[name~="Apply all"][nth=1]"#
    );
    assert_eq!(
        Locator::default()
            .within(Locator::role(Role::Window))
            .to_string(),
        "window >> *"
    );
    assert_eq!(
        Locator::default().named("a\"b\u{1}").to_string(),
        r#"[name="a\"b\u{1}"]"#
    );
}

#[test]
fn world_locators_print_like_selectors() {
    assert_eq!(
        WorldLocator::kind(WorldKind::Object)
            .named("Door")
            .near(Anchor::OwnAvatar, Some(5.0))
            .nth(0)
            .to_string(),
        "object[name=Door][near=own_avatar][radius=5][nth=0]"
    );
    assert_eq!(WorldLocator::default().to_string(), "*");
    assert_eq!(
        WorldLocator::default()
            .near(Anchor::Point([1.5, 2.0, -3.0]), None)
            .to_string(),
        "[near=1.5,2,-3]"
    );
    assert_eq!(WorldLocator::own_avatar().to_string(), "avatar[own=true]");
}

#[test]
fn hand_written_selectors_may_space_and_quote_freely() -> Result<(), crate::SelectorError> {
    let written = r#"  window [ test_id = "floater:build" ]>>button[name_key=build-apply]  "#;
    assert_eq!(
        written.parse::<Locator>()?,
        Locator::role(Role::Button)
            .name_key("build-apply")
            .within(Locator {
                role: Some(Role::Window),
                ..Locator::test_id("floater:build")
            })
    );
    assert_eq!(
        "button[name=Apply all]".parse::<Locator>()?,
        Locator::role(Role::Button).named("Apply all"),
        "a bare value runs to the bracket"
    );
    assert_eq!(
        "*[nth=0]".parse::<Locator>()?,
        Locator::default().nth(0),
        "a star head with attributes"
    );
    assert_eq!(
        "object[near= 1 , 2 ,3 ]".parse::<WorldLocator>()?,
        WorldLocator::kind(WorldKind::Object).near(Anchor::Point([1.0, 2.0, 3.0]), None)
    );
    Ok(())
}

/// Where parsing `input` as a UI locator fails, and why.
fn ui_error(input: &str) -> (usize, String) {
    match input.parse::<Locator>() {
        Ok(locator) => (0, format!("parsed as {locator:?}")),
        Err(error) => (error.column, error.message),
    }
}

/// Where parsing `input` as a world locator fails, and why.
fn world_error(input: &str) -> (usize, String) {
    match input.parse::<WorldLocator>() {
        Ok(locator) => (0, format!("parsed as {locator:?}")),
        Err(error) => (error.column, error.message),
    }
}

#[test]
fn mistakes_are_refused_at_their_column() {
    let cases: [(&str, usize, &str); 14] = [
        ("", 1, "expected a role, `*` or `[`"),
        ("knob", 1, "unknown role `knob`"),
        ("button[label=OK]", 8, "unknown attribute `label`"),
        ("button[name=OK][name=Cancel]", 17, "`name` given twice"),
        ("button[name_key~=ok]", 8, "`name_key` takes `=`, not `~=`"),
        ("button[enabled=maybe]", 8, "`enabled` wants true or false"),
        ("button[nth=-1]", 8, "`nth` wants a zero-based index"),
        ("button[name=OK", 13, "unclosed `[`"),
        ("button[name=\"OK]", 13, "unclosed `\"`"),
        (
            "button[name=\"OK\" x]",
            18,
            "expected `]` after the quoted value",
        ),
        ("button[name]", 12, "expected `=` or `~=` after `name`"),
        ("button[name=]", 13, "expected a value for `name`"),
        (
            "button window",
            8,
            "expected `>>` or the end of the selector",
        ),
        (r#"button[name="\q"]"#, 14, "unknown escape"),
    ];
    for (input, column, message) in cases {
        let (found_column, found_message) = ui_error(input);
        assert!(
            found_message.starts_with(message),
            "{input:?}: {found_message:?} does not start with {message:?}"
        );
        assert_eq!(found_column, column, "{input:?}: {found_message}");
    }
}

#[test]
fn world_mistakes_are_refused_at_their_column() {
    let cases: [(&str, usize, &str); 6] = [
        ("thing", 1, "unknown kind `thing`"),
        ("object >> avatar", 1, "a world selector is one step"),
        ("object[radius=5]", 8, "`radius` needs `near`"),
        ("object[near=1,2]", 8, "`near` wants three coordinates"),
        ("object[near=here]", 8, "`near` wants own_avatar or x,y,z"),
        ("object[full_id=nope]", 8, "`full_id` wants a UUID"),
    ];
    for (input, column, message) in cases {
        let (found_column, found_message) = world_error(input);
        assert!(
            found_message.starts_with(message),
            "{input:?}: {found_message:?} does not start with {message:?}"
        );
        assert_eq!(found_column, column, "{input:?}: {found_message}");
    }
}

#[test]
fn an_error_names_the_selector_and_the_column() {
    let error = "button[label=OK]".parse::<Locator>().err();
    assert_eq!(
        error.map(|error| error.to_string()),
        Some(
            "selector \"button[label=OK]\", column 8: unknown attribute `label` (known: name, \
             name_key, test_id, enabled, checked, selected, expanded, focused, nth)"
                .to_owned()
        )
    );
}
