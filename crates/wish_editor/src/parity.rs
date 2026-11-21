//! Hand–agent parity (invariant 9, M8a WP9): for everything an agent can do
//! through the control plane, what a person does in the editor, and the
//! headless test that proves it.
//!
//! The `parity` test reads this table: every tool of the catalogue and every
//! operation of `edit_apply` must have a row, and every row's test must
//! exist. A new tool or operation without a row breaks the build, which is
//! the point: it has to be reachable by hand (or the row says why it is a
//! question the editor answers by showing, not an operation).

/// What an agent can do, and its counterpart in the editor.
#[derive(Debug)]
pub struct Capability {
    /// A tool (`asset_move`) or an operation of a tool (`edit_apply:spawn`,
    /// `edit_history:undo`).
    pub name: &'static str,
    /// Whether it changes the project or a running game (an operation), or
    /// only reads (a question).
    pub changes: bool,
    /// Where a person does it in the editor.
    pub by_hand: &'static str,
    /// The test that does it through the editor: `file::function` under
    /// `crates/wish_editor/tests/`.
    pub test: &'static str,
}

const fn op(name: &'static str, by_hand: &'static str, test: &'static str) -> Capability {
    Capability {
        name,
        changes: true,
        by_hand,
        test,
    }
}

const fn query(name: &'static str, by_hand: &'static str, test: &'static str) -> Capability {
    Capability {
        name,
        changes: false,
        by_hand,
        test,
    }
}

/// The table.
pub const PARITY: &[Capability] = &[
    // Scene edits (edit_apply), one row per operation.
    op(
        "edit_apply:spawn",
        "Hierarchy: + (an entity), Add child",
        "hierarchy::add_and_delete_are_single_undoable_calls_by_a_human",
    ),
    op(
        "edit_apply:instantiate",
        "Browser: double-click a prefab, Add to scene, or drag it onto the view",
        "assets::double_clicking_a_prefab_puts_an_instance_in_the_scene",
    ),
    op(
        "edit_apply:despawn",
        "Hierarchy: Delete (button, menu or key)",
        "hierarchy::the_delete_key_deletes_the_selection",
    ),
    op(
        "edit_apply:set",
        "Inspector: any field; the handles in the view",
        "inspector::typing_a_number_is_one_call_by_a_human",
    ),
    op(
        "edit_apply:replace",
        "Inspector: a component's menu, Paste values / Reset to defaults",
        "assets::component_values_are_copied_pasted_and_reset",
    ),
    op(
        "edit_apply:remove",
        "Inspector: Remove component",
        "inspector::choices_check_boxes_and_adding_and_removing_components",
    ),
    op(
        "edit_apply:reparent",
        "Hierarchy: drag a row onto another (or to the empty part: the root)",
        "hierarchy::dragging_a_row_onto_another_changes_its_parent",
    ),
    op(
        "edit_apply:rename",
        "Hierarchy: double-click a row, or Rename",
        "hierarchy::double_click_renames",
    ),
    op(
        "edit_apply:set_override",
        "Inspector, on a part of a prefab instance: any field",
        "inspector::a_part_of_a_prefab_instance_is_edited_as_an_override",
    ),
    op(
        "edit_apply:replace_override",
        "Inspector, on a part of a prefab instance: a component's menu, Paste values",
        "parity_ops::an_override_is_reset_and_reverted_to_the_prefab",
    ),
    op(
        "edit_apply:remove_override",
        "Inspector, on a part of a prefab instance: Revert to prefab",
        "parity_ops::an_override_is_reset_and_reverted_to_the_prefab",
    ),
    op(
        "edit_apply:set_notes",
        "Inspector: Notes",
        "parity_ops::notes_are_written_and_cleared",
    ),
    op(
        "edit_apply:set_resource",
        "Inspector with nothing selected (scene resources): any field, Add resource...",
        "parity_ops::scene_resources_are_edited_added_reset_and_removed",
    ),
    op(
        "edit_apply:replace_resource",
        "Scene resources: a resource's menu, Reset to defaults / Paste values",
        "parity_ops::scene_resources_are_edited_added_reset_and_removed",
    ),
    op(
        "edit_apply:remove_resource",
        "Scene resources: Remove resource",
        "parity_ops::scene_resources_are_edited_added_reset_and_removed",
    ),
    op(
        "edit_apply:apply_look",
        "Inspector with nothing selected (scene resources): Add resource..., then Apply a look: Plain, Natural or Vivid",
        "parity_ops::a_look_is_applied_by_hand",
    ),
    // The scene's history.
    op(
        "edit_history:undo",
        "Menu bar: Undo; Ctrl+Z; the History tab",
        "app::the_undo_button_is_a_control_plane_call_by_a_human",
    ),
    op(
        "edit_history:redo",
        "Menu bar: Redo; Ctrl+Y / Ctrl+Shift+Z; the History tab",
        "polish::ctrl_z_undoes_ctrl_y_redoes_and_ctrl_p_plays",
    ),
    op(
        "edit_history:revert",
        "The History tab: Undo this (one change on its own)",
        "parity_ops::one_change_is_taken_back_from_the_history_tab",
    ),
    query(
        "edit_history:list",
        "The History tab: who changed what, newest first",
        "parity_ops::the_history_tab_lists_who_changed_what",
    ),
    // Files.
    op(
        "asset_move",
        "Browser: a file's menu, Rename / move...",
        "assets::moving_a_file_updates_what_uses_it",
    ),
    op(
        "asset_create",
        "Browser: New... (a placeholder sound or picture, a material file, a sprite sheet of the selected picture); a material file's and a sprite sheet's fields and clips are changed where the browser shows the file (parity_ops::a_material_file_is_made_and_changed_by_hand, parity_ops::a_sprite_sheet_is_made_and_changed_by_hand)",
        "parity_ops::a_placeholder_sound_and_picture_are_made",
    ),
    op(
        "project_settings",
        "File > Settings... (name, default scene, input map)",
        "settings::a_new_action_is_added_and_the_project_renamed",
    ),
    op(
        "script_build",
        "Game > Build code (and on the card of a scene that needs it)",
        "panels::a_build_that_fails_shows_its_problems",
    ),
    op(
        "capture_frame",
        "File > Save picture (captures/)",
        "parity_ops::the_view_is_saved_as_a_picture",
    ),
    // Running the game.
    op(
        "sim_start",
        "Menu bar: Play; Ctrl+P",
        "play::play_runs_the_game_and_stop_brings_the_scene_back",
    ),
    op(
        "sim_step",
        "Menu bar: Step (paused), and real time while playing",
        "play::pause_holds_the_game_and_step_runs_one_tick",
    ),
    op(
        "sim_snapshot",
        "Taken every 2 s of play for the timeline",
        "play::the_timeline_goes_back_to_an_earlier_moment",
    ),
    op(
        "sim_restore",
        "The timeline slider: let go to go back",
        "play::the_timeline_goes_back_to_an_earlier_moment",
    ),
    op(
        "sim_stop",
        "Menu bar: Stop",
        "play::play_runs_the_game_and_stop_brings_the_scene_back",
    ),
    op(
        "edit_apply:session",
        "Inspector and hierarchy while playing: live changes",
        "play::a_change_made_while_playing_is_live_and_not_kept",
    ),
    op(
        "replay",
        "Toolbar: Record, then Play and Stop (saved); Game > Replays (play back)",
        "parity_ops::a_run_is_recorded_and_played_back",
    ),
    op(
        "checkpoint",
        "Edit > Checkpoints (save, go back, compare with now: the before/after window, delete)",
        "parity_ops::a_checkpoint_is_saved_compared_and_gone_back_to",
    ),
    op(
        "project_export",
        "File > Export game (exports/<name>/)",
        "parity_ops::the_game_is_exported_from_the_menu",
    ),
    op(
        "game_design",
        "AI mode: the game card, Decisions (add, remove), Plan (add, mark, remove), play-test notes (Take a note, Done), questions (choose an answer, Remember)",
        "ai_mode::the_design_is_made_by_hand",
    ),
    op(
        "packages",
        "File > Packages (install from a folder into the project or for yourself, remove, write the lock again)",
        "packages::packages_are_installed_and_removed_by_hand",
    ),
    op(
        "engines",
        "Hub: Engine versions (add an engine's folder, remove one); a recent project opens with the engine version it was made with",
        "welcome::engines_are_added_and_removed_in_the_hub_and_a_project_opens_with_its_own",
    ),
    op(
        "permissions",
        "AI > Permissions (Only suggest, Apply I review, Autonomous)",
        "parity_ops::an_agents_proposal_is_accepted_and_rejected_by_hand",
    ),
    op(
        "proposals",
        "The Activity tab: Waiting for you, Accept / Reject",
        "parity_ops::an_agents_proposal_is_accepted_and_rejected_by_hand",
    ),
    op(
        "test_run",
        "Game > Run tests (failures in the Problems tab), Update golden pictures",
        "parity_ops::the_tests_run_from_the_menu",
    ),
    // Questions: the editor answers them by showing.
    query(
        "project_info",
        "The project menu's name, the scene list, the status bar's counts",
        "app::the_hierarchy_and_the_scene_picture_show_the_project",
    ),
    query(
        "schema_get",
        "The inspector's widgets, made from the schemas (units, ranges, documentation on hover)",
        "inspector::every_engine_field_has_a_widget",
    ),
    query(
        "check",
        "Game > Check; the Problems tab",
        "panels::a_problem_is_listed_and_a_click_selects_its_entity",
    ),
    query(
        "scene_get",
        "The hierarchy",
        "app::the_hierarchy_and_the_scene_picture_show_the_project",
    ),
    query(
        "entity_query",
        "The hierarchy and a click in the view (what is where)",
        "hierarchy::clicking_in_the_view_picks_what_is_under_the_pointer",
    ),
    query(
        "entity_get",
        "The inspector (the running game's values while playing)",
        "inspector::every_engine_field_has_a_widget",
    ),
    query(
        "asset_list",
        "The browser",
        "assets::the_browser_lists_files_and_prefabs_and_filters_them",
    ),
    query(
        "asset_info",
        "The browser: select a file",
        "assets::selecting_a_file_shows_what_the_tools_know_about_it",
    ),
    query(
        "describe_view",
        "The view itself; the hierarchy",
        "app::the_hierarchy_and_the_scene_picture_show_the_project",
    ),
    query(
        "physics_query",
        "The view's Colliders view; a click in the view",
        "hierarchy::clicking_in_the_view_picks_what_is_under_the_pointer",
    ),
    query(
        "logs_get",
        "The Console tab",
        "panels::the_console_shows_what_was_logged",
    ),
    query(
        "activity_get",
        "The Activity tab: everyone's calls, newest first; Undo this on an edit",
        "parity_ops::the_activity_tab_shows_what_an_agent_did",
    ),
    query(
        "docs_search",
        "Each field's and component's documentation on hover; the guides are files (wish://docs) for any reader",
        "inspector::every_engine_field_has_a_widget",
    ),
];
