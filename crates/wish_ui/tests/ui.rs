//! The interface layout and buttons (M6 WP7).

#![allow(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wish_core::{Color, Entity};
use wish_ecs::World;
use wish_types::time::Time;
use wish_ui::{
    Pointer, UiAlign, UiButton, UiDirection, UiInteraction, UiJustify, UiLayout, UiLength, UiNode,
    UiPosition, UiSettings, UiText, UiUnit,
};

fn world() -> World {
    let mut world = World::new();
    wish_ui::register(&mut world).unwrap();
    world.register_resource::<Pointer>().unwrap();
    world.insert_resource(Time::at(0, 60)).unwrap();
    world
}

fn px(value: f32) -> UiLength {
    UiLength {
        unit: UiUnit::Px,
        value,
    }
}

fn boxed(world: &mut World, parent: Option<Entity>, node: UiNode) -> Entity {
    let entity = world.spawn().unwrap();
    world.insert(entity, node).unwrap();
    if let Some(parent) = parent {
        world.set_parent(entity, parent).unwrap();
    }
    entity
}

fn sized(width: f32, height: f32) -> UiNode {
    UiNode {
        width: px(width),
        height: px(height),
        ..UiNode::default()
    }
}

fn laid(world: &World, entity: Entity) -> UiLayout {
    *world.get::<UiLayout>(entity).unwrap()
}

#[test]
fn a_row_places_children_with_padding_and_gap() {
    let mut world = world();
    let row = boxed(
        &mut world,
        None,
        UiNode {
            direction: UiDirection::Row,
            padding: [5.0, 6.0, 7.0, 8.0],
            gap: 10.0,
            offset: [100.0, 50.0],
            position: UiPosition::Absolute,
            ..UiNode::default()
        },
    );
    let a = boxed(&mut world, Some(row), sized(100.0, 40.0));
    let b = boxed(&mut world, Some(row), sized(60.0, 20.0));
    wish_ui::layout(&mut world).unwrap();
    let (row, a, b) = (laid(&world, row), laid(&world, a), laid(&world, b));
    assert_eq!(row.position, [100.0, 50.0]);
    // Width: left 8 + 100 + gap 10 + 60 + right 6; height: 5 + 40 + 7.
    assert_eq!(row.size, [184.0, 52.0]);
    assert_eq!(a.position, [108.0, 55.0]);
    assert_eq!(b.position, [218.0, 55.0]);
    // Drawing order: the parent first.
    assert!(row.order < a.order && a.order < b.order);
    assert!(row.shown && a.shown && b.shown);
}

#[test]
fn percent_is_of_the_parent_and_of_the_screen_for_a_root() {
    let mut world = world();
    world
        .insert_resource(UiSettings {
            width: 1000.0,
            height: 500.0,
        })
        .unwrap();
    let root = boxed(
        &mut world,
        None,
        UiNode {
            width: UiLength {
                unit: UiUnit::Percent,
                value: 100.0,
            },
            height: UiLength {
                unit: UiUnit::Percent,
                value: 100.0,
            },
            justify: UiJustify::End,
            align: UiAlign::Center,
            ..UiNode::default()
        },
    );
    let half = boxed(
        &mut world,
        Some(root),
        UiNode {
            width: UiLength {
                unit: UiUnit::Percent,
                value: 50.0,
            },
            height: px(100.0),
            ..UiNode::default()
        },
    );
    wish_ui::layout(&mut world).unwrap();
    assert_eq!(laid(&world, root).size, [1000.0, 500.0]);
    let half = laid(&world, half);
    // Centred across, at the bottom along the column.
    assert_eq!(half.size, [500.0, 100.0]);
    assert_eq!(half.position, [250.0, 400.0]);
}

#[test]
fn text_sizes_its_box_and_wraps_in_a_narrow_one() {
    let mut world = world();
    let words = world.intern("Score: 12").unwrap();
    let label = boxed(&mut world, None, UiNode::default());
    world
        .insert(
            label,
            UiText {
                text: words,
                size: 32.0,
                ..UiText::default()
            },
        )
        .unwrap();
    wish_ui::layout(&mut world).unwrap();
    let font = wish_text::Font::default_font().unwrap();
    let expected = wish_text::layout(font, "Score: 12", 32.0, None, true, wish_text::Align::Left);
    let one = laid(&world, label);
    assert_eq!(one.size, [expected.width.ceil(), 32.0]);

    // In a column 60 px wide the two words take two lines.
    let column = boxed(&mut world, None, sized(60.0, 200.0));
    world.set_parent(label, column).unwrap();
    wish_ui::layout(&mut world).unwrap();
    let two = laid(&world, label);
    assert_eq!(two.size[0], 60.0, "stretched across the column");
    assert_eq!(two.size[1], 64.0, "two lines");
}

#[test]
fn hidden_boxes_hide_their_children_and_take_no_space() {
    let mut world = world();
    let column = boxed(&mut world, None, UiNode::default());
    let hidden = boxed(
        &mut world,
        Some(column),
        UiNode {
            visible: false,
            ..sized(50.0, 50.0)
        },
    );
    let inside = boxed(&mut world, Some(hidden), sized(10.0, 10.0));
    let after = boxed(&mut world, Some(column), sized(20.0, 20.0));
    wish_ui::layout(&mut world).unwrap();
    assert!(!laid(&world, hidden).shown && !laid(&world, inside).shown);
    assert!(laid(&world, after).shown);
    assert_eq!(laid(&world, after).position, [0.0, 0.0]);
}

#[test]
fn layout_is_written_only_when_it_changes() {
    let mut world = world();
    let a = boxed(&mut world, None, sized(10.0, 10.0));
    wish_ui::layout(&mut world).unwrap();
    let before = world.hash();
    wish_ui::layout(&mut world).unwrap();
    assert_eq!(world.hash(), before);
    world.insert(a, sized(20.0, 10.0)).unwrap();
    wish_ui::layout(&mut world).unwrap();
    assert_eq!(laid(&world, a).size, [20.0, 10.0]);
}

fn point(world: &mut World, at: [f32; 2], held: bool) {
    let before = world.resource::<Pointer>().copied().unwrap_or_default();
    world
        .insert_resource(Pointer {
            present: true,
            position: at,
            held,
            pressed: held && !before.held,
            released: !held && before.held,
            ..before
        })
        .unwrap();
}

fn tick(world: &mut World, n: u64) {
    world.insert_resource(Time::at(n, 60)).unwrap();
    wish_ui::step(world).unwrap();
}

fn clicked(world: &World) -> u64 {
    world.events_sent("ButtonClicked").unwrap()
}

#[test]
fn a_button_is_clicked_by_pressing_and_releasing_over_it() {
    let mut world = world();
    let button = boxed(
        &mut world,
        None,
        UiNode {
            position: UiPosition::Absolute,
            offset: [100.0, 100.0],
            ..sized(200.0, 50.0)
        },
    );
    world.insert(button, UiButton::default()).unwrap();
    // Text inside the button is part of it.
    let caption = boxed(&mut world, Some(button), sized(50.0, 20.0));

    point(&mut world, [10.0, 10.0], false);
    tick(&mut world, 1);
    let state = *world.get::<UiInteraction>(button).unwrap();
    assert!(!state.hovered && !state.pressed);

    point(&mut world, [120.0, 105.0], false);
    tick(&mut world, 2);
    assert!(world.get::<UiInteraction>(button).unwrap().hovered);
    assert_eq!(wish_ui::button_under(&world, [120.0, 105.0]), Some(button));
    let _ = caption;

    point(&mut world, [120.0, 105.0], true);
    tick(&mut world, 3);
    assert!(world.get::<UiInteraction>(button).unwrap().pressed);
    assert_eq!(clicked(&world), 0);

    point(&mut world, [130.0, 110.0], false);
    tick(&mut world, 4);
    let state = *world.get::<UiInteraction>(button).unwrap();
    assert_eq!((state.clicks, state.last_click_tick), (1, 4));
    assert!(!state.pressed);
    assert_eq!(clicked(&world), 1);

    // Pressed over it, released elsewhere: no click.
    point(&mut world, [130.0, 110.0], true);
    tick(&mut world, 5);
    point(&mut world, [5.0, 5.0], true);
    tick(&mut world, 6);
    point(&mut world, [5.0, 5.0], false);
    tick(&mut world, 7);
    assert_eq!(clicked(&world), 1);
    assert_eq!(world.get::<UiInteraction>(button).unwrap().clicks, 1);

    // Disabled: nothing.
    world.insert(button, UiButton { enabled: false }).unwrap();
    point(&mut world, [130.0, 110.0], true);
    tick(&mut world, 8);
    point(&mut world, [130.0, 110.0], false);
    tick(&mut world, 9);
    assert_eq!(clicked(&world), 1);
    assert!(!world.get::<UiInteraction>(button).unwrap().hovered);
}

#[test]
fn a_panel_in_front_blocks_the_button_behind_and_layers_decide() {
    let mut world = world();
    let button = boxed(&mut world, None, sized(100.0, 100.0));
    world.insert(button, UiButton::default()).unwrap();
    let glass = boxed(
        &mut world,
        None,
        UiNode {
            layer: 1,
            ..sized(100.0, 100.0)
        },
    );
    wish_ui::layout(&mut world).unwrap();
    // A transparent box lets the pointer through.
    assert_eq!(wish_ui::button_under(&world, [50.0, 50.0]), Some(button));
    let mut panel = *world.get::<UiNode>(glass).unwrap();
    panel.background = Color::rgba(0.0, 0.0, 0.0, 0.5);
    world.insert(glass, panel).unwrap();
    wish_ui::layout(&mut world).unwrap();
    assert_eq!(wish_ui::button_under(&world, [50.0, 50.0]), None);
    // Lower its layer: the button is on top again.
    panel.layer = -1;
    world.insert(glass, panel).unwrap();
    wish_ui::layout(&mut world).unwrap();
    assert_eq!(wish_ui::button_under(&world, [50.0, 50.0]), Some(button));
}

#[test]
fn the_virtual_screen_fits_any_picture() {
    let settings = UiSettings::default();
    let (scale, [left, top]) = settings.fit(640.0, 360.0);
    assert_eq!((scale, left, top), (0.5, 0.0, 0.0));
    // A square picture: letterboxed above and below.
    let (scale, [left, top]) = settings.fit(1280.0, 1280.0);
    assert_eq!((scale, left), (1.0, 0.0));
    assert_eq!(top, 280.0);
    assert_eq!(
        settings.from_picture(1280.0, 1280.0, 640.0, 280.0 + 360.0),
        [640.0, 360.0]
    );
}
