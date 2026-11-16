//! The in-game interface in the simulation (design note M6 §7).
//!
//! Every tick the `ui` system lays the interface out ([`layout`]: boxes are
//! entities with a `UiNode`, nested by the entity hierarchy, placed like CSS
//! flexbox by `taffy`, text measured by `wish_text`) and writes each box's
//! place into its `UiLayout`; then it lets the pointer act on buttons
//! ([`interact`]): `UiInteraction` and `ButtonClicked`.
//!
//! Everything happens on the virtual screen of `UiSettings`, never on the
//! window, and depends only on the world: the same world lays out and
//! clicks the same way on every machine, in a replay and in a test.

use std::collections::{BTreeMap, BTreeSet};

use taffy::prelude::{
    AlignItems, AvailableSpace, Dimension, Display, FlexDirection, JustifyContent,
    LengthPercentage, LengthPercentageAuto, NodeId, Position, Rect, Size, Style, TaffyTree,
};
use wish_core::{Diagnostic, Entity, Result};
use wish_ecs::World;
use wish_text::{Align, Fonts};
use wish_types::time::Time;
pub use wish_types::ui::{
    ButtonClicked, Pointer, UiAlign, UiButton, UiDirection, UiImage, UiInteraction, UiJustify,
    UiLayout, UiLength, UiNode, UiPosition, UiSettings, UiText, UiTextAlign, UiUnit,
};

/// The name of the interface system in an app.
pub const SYSTEM: &str = "ui";

/// The deepest nesting of boxes laid out; deeper boxes are not shown.
pub const MAX_DEPTH: usize = 256;

/// Registers the interface's components, event and settings.
pub fn register(world: &mut World) -> Result<()> {
    world.register_component::<UiNode>()?;
    world.register_component::<UiText>()?;
    world.register_component::<UiImage>()?;
    world.register_component::<UiButton>()?;
    world.register_component::<UiInteraction>()?;
    world.register_component::<UiLayout>()?;
    world.register_event::<ButtonClicked>()?;
    world.register_resource::<UiSettings>()?;
    Ok(())
}

/// The interface system, for `App::add_system`.
pub fn system() -> wish_app::System {
    wish_app::System::new(SYSTEM, |world, _| step(world))
}

/// One interface tick: layout, then the pointer on the buttons.
pub fn step(world: &mut World) -> Result<()> {
    layout(world)?;
    interact(world)
}

fn internal(what: &str) -> Diagnostic {
    Diagnostic::error(
        "internal_error",
        format!("The interface layout failed: {what}."),
        "This is a bug in the engine. Please report it.",
    )
}

fn clean(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

fn length(value: UiLength) -> Dimension {
    match value.unit {
        UiUnit::Auto => Dimension::auto(),
        UiUnit::Px => Dimension::length(clean(value.value).max(0.0)),
        UiUnit::Percent => Dimension::percent(clean(value.value).max(0.0) / 100.0),
    }
}

/// CSS order: top, right, bottom, left.
fn sides(values: [f32; 4]) -> Rect<f32> {
    let [top, right, bottom, left] = values.map(clean);
    Rect {
        left,
        right,
        top,
        bottom,
    }
}

fn style_of(node: &UiNode) -> Style {
    let padding = sides(node.padding);
    let margin = sides(node.margin);
    let [x, y] = node.offset.map(clean);
    Style {
        display: if node.visible {
            Display::Flex
        } else {
            Display::None
        },
        position: match node.position {
            UiPosition::Relative => Position::Relative,
            UiPosition::Absolute => Position::Absolute,
        },
        inset: Rect {
            left: LengthPercentageAuto::length(x),
            top: LengthPercentageAuto::length(y),
            right: LengthPercentageAuto::auto(),
            bottom: LengthPercentageAuto::auto(),
        },
        size: Size {
            width: length(node.width),
            height: length(node.height),
        },
        padding: Rect {
            left: LengthPercentage::length(padding.left.max(0.0)),
            right: LengthPercentage::length(padding.right.max(0.0)),
            top: LengthPercentage::length(padding.top.max(0.0)),
            bottom: LengthPercentage::length(padding.bottom.max(0.0)),
        },
        margin: Rect {
            left: LengthPercentageAuto::length(margin.left),
            right: LengthPercentageAuto::length(margin.right),
            top: LengthPercentageAuto::length(margin.top),
            bottom: LengthPercentageAuto::length(margin.bottom),
        },
        gap: Size {
            width: LengthPercentage::length(clean(node.gap).max(0.0)),
            height: LengthPercentage::length(clean(node.gap).max(0.0)),
        },
        flex_direction: match node.direction {
            UiDirection::Column => FlexDirection::Column,
            UiDirection::Row => FlexDirection::Row,
        },
        justify_content: match node.justify {
            UiJustify::Start => JustifyContent::FLEX_START,
            UiJustify::Center => JustifyContent::CENTER,
            UiJustify::End => JustifyContent::FLEX_END,
            UiJustify::SpaceBetween => JustifyContent::SPACE_BETWEEN,
            UiJustify::SpaceAround => JustifyContent::SPACE_AROUND,
        },
        align_items: match node.align {
            UiAlign::Stretch => AlignItems::STRETCH,
            UiAlign::Start => AlignItems::FLEX_START,
            UiAlign::Center => AlignItems::CENTER,
            UiAlign::End => AlignItems::FLEX_END,
        },
        flex_grow: clean(node.grow).max(0.0),
        // Boxes keep their size rather than squeeze below it.
        flex_shrink: 0.0,
        ..Style::default()
    }
}

/// What measuring a text needs, copied out of the world.
struct TextSpec {
    font: String,
    text: String,
    size: f32,
    wrap: bool,
}

/// The text of `entity`'s `UiText`, its font path, and how to lay it out;
/// the same for the layout and for drawing.
pub fn text_of(world: &World, entity: Entity) -> Option<(UiText, String, String)> {
    let text = world.get::<UiText>(entity).copied()?;
    let string = world.text(text.text).unwrap_or_default().to_owned();
    let font = world.text(text.font.text()).unwrap_or_default().to_owned();
    Some((text, string, font))
}

/// The `wish_text` alignment of a `UiTextAlign`.
pub fn align_of(align: UiTextAlign) -> Align {
    match align {
        UiTextAlign::Left => Align::Left,
        UiTextAlign::Center => Align::Center,
        UiTextAlign::Right => Align::Right,
    }
}

/// The interface boxes and their children (boxes only), in hierarchy order.
struct Tree {
    roots: Vec<Entity>,
    children: BTreeMap<Entity, Vec<Entity>>,
}

fn tree_of(world: &World) -> Tree {
    let nodes: Vec<Entity> = world
        .entities()
        .filter(|entity| world.has::<UiNode>(*entity))
        .collect();
    let set: BTreeSet<Entity> = nodes.iter().copied().collect();
    let mut roots: Vec<(i32, Entity)> = Vec::new();
    let mut children = BTreeMap::new();
    for &entity in &nodes {
        match world.parent(entity) {
            Some(parent) if set.contains(&parent) => {}
            _ => roots.push((
                world.get::<UiNode>(entity).map_or(0, |node| node.layer),
                entity,
            )),
        }
        let kids: Vec<Entity> = world
            .children(entity)
            .into_iter()
            .filter(|child| set.contains(child))
            .collect();
        if !kids.is_empty() {
            children.insert(entity, kids);
        }
    }
    roots.sort_unstable();
    Tree {
        roots: roots.into_iter().map(|(_, entity)| entity).collect(),
        children,
    }
}

/// Works the interface out: writes every box's `UiLayout` (only where it
/// changed). Called by the `ui` system every tick; call it yourself on a
/// world that does not tick (a scene being drawn).
pub fn layout(world: &mut World) -> Result<()> {
    let tree = tree_of(world);
    if tree.roots.is_empty() {
        return Ok(());
    }
    let settings = world.resource::<UiSettings>().copied().unwrap_or_default();
    let [screen_w, screen_h] = settings.size();
    let fonts = world.extension::<Fonts>().cloned().unwrap_or_default();

    let mut taffy: TaffyTree<Entity> = TaffyTree::new();
    let mut texts: BTreeMap<Entity, TextSpec> = BTreeMap::new();
    let mut placed: Vec<(Entity, UiLayout)> = Vec::new();
    let mut order: u32 = 0;
    for &root in &tree.roots {
        // Build this root's boxes, depth first, keeping the taffy node of each.
        let mut stack: Vec<(Entity, usize)> = vec![(root, 0)];
        let mut built: BTreeMap<Entity, NodeId> = BTreeMap::new();
        let mut visit: Vec<(Entity, usize)> = Vec::new();
        while let Some((entity, depth)) = stack.pop() {
            visit.push((entity, depth));
            if depth + 1 < MAX_DEPTH
                && let Some(kids) = tree.children.get(&entity)
            {
                for kid in kids.iter().rev() {
                    stack.push((*kid, depth + 1));
                }
            }
        }
        // Children before parents, so that a parent can list them.
        for &(entity, depth) in visit.iter().rev() {
            let Some(node) = world.get::<UiNode>(entity).copied() else {
                continue;
            };
            let style = style_of(&node);
            let kids: Vec<NodeId> = if depth + 1 < MAX_DEPTH {
                tree.children
                    .get(&entity)
                    .map(|kids| kids.iter().filter_map(|k| built.get(k).copied()).collect())
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            let id = if kids.is_empty() {
                if let Some((text, string, font)) = text_of(world, entity) {
                    texts.insert(
                        entity,
                        TextSpec {
                            font,
                            text: string,
                            size: text.size,
                            wrap: text.wrap,
                        },
                    );
                }
                taffy.new_leaf_with_context(style, entity)
            } else {
                taffy.new_with_children(style, &kids)
            }
            .map_err(|e| internal(&e.to_string()))?;
            built.insert(entity, id);
        }
        let Some(&root_id) = built.get(&root) else {
            continue;
        };
        // The screen: a column the size of the virtual screen, its box at
        // the top left, sized by its content unless it says otherwise.
        let screen = taffy
            .new_with_children(
                Style {
                    size: Size {
                        width: Dimension::length(screen_w),
                        height: Dimension::length(screen_h),
                    },
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::FLEX_START,
                    justify_content: JustifyContent::FLEX_START,
                    ..Style::default()
                },
                &[root_id],
            )
            .map_err(|e| internal(&e.to_string()))?;
        taffy
            .compute_layout_with_measure(
                screen,
                Size {
                    width: AvailableSpace::Definite(screen_w),
                    height: AvailableSpace::Definite(screen_h),
                },
                |inputs, _, context, style| {
                    taffy::compute_leaf_layout(
                        inputs,
                        style,
                        |_, _| 0.0,
                        |known, available| {
                            let spec = context.and_then(|entity| texts.get(entity));
                            let Some(spec) = spec else {
                                return Size::ZERO;
                            };
                            if let (Some(width), Some(height)) = (known.width, known.height) {
                                return Size { width, height };
                            }
                            let Ok(font) = fonts.get(&spec.font) else {
                                return Size::ZERO;
                            };
                            let width = known.width.or(match available.width {
                                AvailableSpace::Definite(width) => Some(width),
                                AvailableSpace::MinContent => Some(0.0),
                                AvailableSpace::MaxContent => None,
                            });
                            let laid = wish_text::layout(
                                font,
                                &spec.text,
                                spec.size,
                                if spec.wrap { width } else { None },
                                spec.wrap,
                                Align::Left,
                            );
                            // Rounded up, so that the box (rounded to whole
                            // pixels) always holds the text without a new break.
                            Size {
                                width: known.width.unwrap_or(laid.width.ceil()),
                                height: known.height.unwrap_or(laid.height.ceil()),
                            }
                        },
                    )
                },
            )
            .map_err(|e| internal(&e.to_string()))?;
        // Positions are relative to the parent: add them up, top down.
        let mut absolute: BTreeMap<Entity, ([f32; 2], bool)> = BTreeMap::new();
        for &(entity, depth) in &visit {
            let Some(&id) = built.get(&entity) else {
                continue;
            };
            let laid = taffy.layout(id).map_err(|e| internal(&e.to_string()))?;
            let parent = if depth == 0 {
                None
            } else {
                world.parent(entity).and_then(|p| absolute.get(&p).copied())
            };
            let ([px, py], parent_shown) = parent.unwrap_or(([0.0, 0.0], true));
            let visible = world.get::<UiNode>(entity).is_some_and(|n| n.visible);
            let position = [px + laid.location.x, py + laid.location.y];
            let shown = parent_shown && visible;
            absolute.insert(entity, (position, shown));
            placed.push((
                entity,
                UiLayout {
                    position,
                    size: [laid.size.width, laid.size.height],
                    shown,
                    order,
                },
            ));
            order = order.saturating_add(1);
        }
        taffy.clear();
        texts.clear();
    }
    for (entity, laid) in placed {
        if world.get::<UiLayout>(entity) != Some(&laid) {
            world.insert(entity, laid)?;
        }
    }
    Ok(())
}

/// Whether `layout` holds interface point (`x`, `y`).
fn contains(layout: &UiLayout, [x, y]: [f32; 2]) -> bool {
    let [left, top] = layout.position;
    let [width, height] = layout.size;
    x >= left && y >= top && x < left + width && y < top + height
}

/// The button `entity` belongs to: itself or the nearest box above it that
/// is a button.
fn button_of(world: &World, entity: Entity) -> Option<Entity> {
    let mut at = Some(entity);
    let mut steps = 0;
    while let Some(entity) = at {
        if world.has::<UiButton>(entity) {
            return Some(entity);
        }
        if !world.has::<UiNode>(entity) || steps > MAX_DEPTH {
            return None;
        }
        at = world.parent(entity);
        steps += 1;
    }
    None
}

/// The enabled button under the pointer, if any: the topmost box under it
/// decides. A box inside a button counts as the button; a box with a
/// background or a picture in front of a button blocks it.
pub fn button_under(world: &World, point: [f32; 2]) -> Option<Entity> {
    let mut hits: Vec<(u32, Entity)> = world
        .entities()
        .filter_map(|entity| {
            let layout = world.get::<UiLayout>(entity)?;
            (layout.shown && world.has::<UiNode>(entity) && contains(layout, point))
                .then_some((layout.order, entity))
        })
        .collect();
    hits.sort_unstable_by(|a, b| b.cmp(a));
    for (_, entity) in hits {
        if let Some(button) = button_of(world, entity) {
            let enabled = world.get::<UiButton>(button).is_some_and(|b| b.enabled);
            return enabled.then_some(button);
        }
        let solid = world
            .get::<UiNode>(entity)
            .is_some_and(|node| node.background.a > 0.0)
            || world.has::<UiImage>(entity);
        if solid {
            return None;
        }
    }
    None
}

/// Lets the pointer act on the buttons: writes each button's
/// `UiInteraction` (where it changed) and sends `ButtonClicked` for a button
/// pressed and released over it.
pub fn interact(world: &mut World) -> Result<()> {
    let mut buttons: Vec<Entity> = world
        .query::<(Entity, &UiButton)>()?
        .map(|(entity, _)| entity)
        .collect();
    if buttons.is_empty() {
        return Ok(());
    }
    buttons.sort_unstable();
    let pointer = world.resource::<Pointer>().copied().unwrap_or_default();
    let tick = world.resource::<Time>().map_or(0, |time| time.tick);
    let target = if pointer.present && pointer.position.iter().all(|v| v.is_finite()) {
        button_under(world, pointer.position)
    } else {
        None
    };
    for button in buttons {
        let before = world.get::<UiInteraction>(button).copied();
        let mut state = before.unwrap_or_default();
        let enabled = world.get::<UiButton>(button).is_some_and(|b| b.enabled)
            && world.get::<UiLayout>(button).is_some_and(|l| l.shown);
        let mut clicked = false;
        if enabled {
            state.hovered = target == Some(button);
            if pointer.pressed && state.hovered {
                state.pressed = true;
            }
            if pointer.released && state.pressed && state.hovered {
                clicked = true;
                state.clicks = state.clicks.saturating_add(1);
                state.last_click_tick = tick;
            }
            if !pointer.held {
                state.pressed = false;
            }
        } else {
            state.hovered = false;
            state.pressed = false;
        }
        if before != Some(state) {
            world.insert(button, state)?;
        }
        if clicked {
            world.send(ButtonClicked { entity: button })?;
        }
    }
    Ok(())
}
