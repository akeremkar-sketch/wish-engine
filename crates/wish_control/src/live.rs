//! Edits of a running game (M8a WP7, carried over from M5): `edit_apply` with
//! `session` changes the session's live world instead of the scene file.
//!
//! The same operations, the same all-or-nothing rule (the world is put back
//! as it was if one fails), the same answers. The scene file and its history
//! are untouched: a live edit lasts as long as the session (to keep it, make
//! the same edit to the scene). Operations that only mean something in a
//! file (placing a prefab, overrides, notes) are refused with a hint.
//!
//! A session that is recording a replay refuses live edits: an edit is not
//! input, so the replay would no longer play the same.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};
use wish_core::{Diagnostic, Entity, Result, suggest::unknown_name_hint};
use wish_ecs::World;
use wish_scene::{Actor, EditOp, SCENE_ID, is_valid_scene_id};

use crate::sessions::entity_names;
use crate::workspace::Workspace;

fn unknown_entity(id: &str, names: &BTreeMap<String, Entity>) -> Diagnostic {
    let ids: Vec<&str> = names.keys().map(String::as_str).collect();
    Diagnostic::error(
        "unknown_entity",
        format!("The running game has no entity '{id}'."),
        unknown_name_hint(id, &ids),
    )
    .with_candidates(ids.iter().copied())
}

fn op_name(op: &EditOp) -> &'static str {
    match op {
        EditOp::Spawn { .. } => "spawn",
        EditOp::Instantiate { .. } => "instantiate",
        EditOp::Despawn { .. } => "despawn",
        EditOp::Set { .. } => "set",
        EditOp::Replace { .. } => "replace",
        EditOp::Remove { .. } => "remove",
        EditOp::Reparent { .. } => "reparent",
        EditOp::Rename { .. } => "rename",
        EditOp::SetOverride { .. } => "set_override",
        EditOp::ReplaceOverride { .. } => "replace_override",
        EditOp::RemoveOverride { .. } => "remove_override",
        EditOp::SetNotes { .. } => "set_notes",
        EditOp::SetResource { .. } => "set_resource",
        EditOp::ReplaceResource { .. } => "replace_resource",
        EditOp::RemoveResource { .. } => "remove_resource",
    }
}

/// One operation on a live world; records the changed paths.
fn apply_live(world: &mut World, op: &EditOp, changes: &mut Vec<String>) -> Result<()> {
    let names = entity_names(world);
    let find = |id: &str| {
        names
            .get(id)
            .copied()
            .ok_or_else(|| unknown_entity(id, &names))
    };
    let free_id = |id: &str| -> Result<()> {
        if !is_valid_scene_id(id) {
            return Err(Diagnostic::error(
                "invalid_scene_id",
                format!("'{id}' is not a valid entity id."),
                "Use lowercase letters, digits and underscores, such as \"crate_2\".",
            ));
        }
        if names.contains_key(id) {
            return Err(Diagnostic::error(
                "entity_exists",
                format!("The running game already has an entity '{id}'."),
                "Choose another id, or change the one that is there with set.",
            ));
        }
        Ok(())
    };
    // What the engine writes itself cannot be set in a running game either
    // (a SceneId is how tools find an entity; GlobalTransform comes from
    // Transform).
    let engine_written = |component: &str| -> Result<()> {
        if component == SCENE_ID || component == "GlobalTransform" {
            return Err(Diagnostic::error(
                "managed_component",
                format!(
                    "'{component}' is written by the engine and cannot be set in a running game."
                ),
                if component == SCENE_ID {
                    "Rename an entity with the rename operation.".to_owned()
                } else {
                    "Set \"Transform\" instead: the engine computes GlobalTransform from it."
                        .to_owned()
                },
            ));
        }
        Ok(())
    };
    match op {
        EditOp::Set { component, .. }
        | EditOp::Replace { component, .. }
        | EditOp::Remove { component, .. } => engine_written(component)?,
        EditOp::Spawn { components, .. } => {
            for name in components.keys() {
                engine_written(name)?;
            }
        }
        _ => {}
    }
    let order_refused = || {
        Diagnostic::error(
            "live_edit_unsupported",
            "A sibling order cannot be given to a running game's entity.",
            "Leave out \"order\" (the entity goes last among its siblings), or set the order in the scene and play again.",
        )
    };
    match op {
        EditOp::Spawn { order, .. } if *order != 0 => return Err(order_refused()),
        EditOp::Reparent { order: Some(_), .. } => return Err(order_refused()),
        _ => {}
    }
    match op {
        EditOp::Spawn {
            id,
            parent,
            components,
            ..
        } => {
            free_id(id)?;
            let parent = parent.as_deref().map(find).transpose()?;
            let entity = world.spawn()?;
            world.set_json(entity, SCENE_ID, &json!({ "id": id }))?;
            for (name, value) in components {
                world.set_json(entity, name, value)?;
            }
            if let Some(parent) = parent {
                world.set_parent(entity, parent)?;
            }
            changes.push(format!("/entities/{id}"));
        }
        EditOp::Despawn { id } => {
            world.despawn(find(id)?)?;
            changes.push(format!("/entities/{id}"));
        }
        EditOp::Set {
            id,
            component,
            value,
        } => {
            world.set_json(find(id)?, component, value)?;
            changes.push(format!("/entities/{id}/components/{component}"));
        }
        EditOp::Replace {
            id,
            component,
            value,
        } => {
            let entity = find(id)?;
            world.remove_json(entity, component)?;
            world.set_json(entity, component, value)?;
            changes.push(format!("/entities/{id}/components/{component}"));
        }
        EditOp::Remove { id, component } => {
            if !world.remove_json(find(id)?, component)? {
                return Err(Diagnostic::error(
                    "component_not_found",
                    format!("'{id}' has no {component} in the running game."),
                    "entity_get with session shows what it has.",
                ));
            }
            changes.push(format!("/entities/{id}/components/{component}"));
        }
        EditOp::Reparent { id, parent, .. } => {
            let entity = find(id)?;
            match parent {
                Some(parent) => world.set_parent(entity, find(parent)?)?,
                None => world.remove_parent(entity)?,
            }
            changes.push(format!("/entities/{id}"));
        }
        EditOp::Rename { id, to } => {
            let entity = find(id)?;
            free_id(to)?;
            world.set_json(entity, SCENE_ID, &json!({ "id": to }))?;
            changes.push(format!("/entities/{id}"));
            changes.push(format!("/entities/{to}"));
        }
        EditOp::SetResource { name, value } => {
            world.set_resource_json(name, value)?;
            changes.push(format!("/resources/{name}"));
        }
        EditOp::ReplaceResource { name, value } => {
            world.remove_resource_json(name)?;
            world.set_resource_json(name, value)?;
            changes.push(format!("/resources/{name}"));
        }
        EditOp::RemoveResource { name } => {
            world.remove_resource_json(name)?;
            changes.push(format!("/resources/{name}"));
        }
        other => {
            let name = op_name(other);
            return Err(Diagnostic::error(
                "live_edit_unsupported",
                format!("'{name}' only means something in a scene file, not in a running game."),
                "Make this change to the scene (edit_apply without session) and start the game again; a running game takes spawn, despawn, set, replace, remove, reparent, rename and the resource operations.",
            ));
        }
    }
    Ok(())
}

impl Workspace {
    /// `edit_apply` on session `id`: the operations on its live world, all or
    /// none. A dry run reports the changes and puts the world back.
    pub(crate) fn edit_session(
        &mut self,
        id: &str,
        ops: &[EditOp],
        actor: &Actor,
        dry_run: bool,
    ) -> Result<Map<String, Value>> {
        let session = self.session_mut(id)?;
        if session.recording.is_some() {
            return Err(Diagnostic::error(
                "live_edit_while_recording",
                format!(
                    "Session {id} is recording a replay; a live edit would make it play differently."
                ),
                "Save the recording first (replay with action save), or edit the scene file instead.",
            ));
        }
        let before = session.app.snapshot();
        let mut changes = Vec::new();
        let mut failed = None;
        for (index, op) in ops.iter().enumerate() {
            if let Err(problem) = apply_live(session.app.world_mut(), op, &mut changes) {
                failed = Some(problem.at_op(u32::try_from(index).unwrap_or(u32::MAX)));
                break;
            }
        }
        if let Some(problem) = failed {
            session.app.restore(&before)?;
            return Err(problem);
        }
        if dry_run {
            session.app.restore(&before)?;
        } else {
            // What is drawn and asked about follows at once, not at the next
            // tick.
            wish_transform::update_global_transforms(session.app.world_mut())?;
        }
        let tick = session.app.tick();
        if !dry_run {
            let note = Diagnostic::info(
                "live_edit",
                format!(
                    "{} changed the running game ({} change(s)): {}",
                    actor.label(),
                    changes.len(),
                    changes.join(", ")
                ),
                "A live edit lasts as long as the session; make it in the scene to keep it.",
            );
            self.sessions.record(Some(id), Some(tick), "edit", &note);
        }
        let mut out = Map::new();
        out.insert("session".to_owned(), json!(id));
        out.insert("tick".to_owned(), json!(tick));
        out.insert(
            "changes".to_owned(),
            json!({ "count": changes.len(), "paths": changes }),
        );
        if dry_run {
            out.insert("dry_run".to_owned(), json!(true));
        }
        Ok(out)
    }
}
