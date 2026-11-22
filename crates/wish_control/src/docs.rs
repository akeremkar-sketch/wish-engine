//! The engine's documentation as the tools and the MCP server serve it: a
//! short guide (`wish://docs/guide`), a page per component, resource and
//! event type made from its schema (`wish://docs/components/<Name>`), and a
//! page per diagnostic code (`wish://docs/diagnostics/<code>`), which every
//! error's `docs` field points to.

use serde_json::{Value, json};
use wish_ecs::{Registry, TypeInfo};

use crate::tool::catalog;

/// Every diagnostic code with what it means and what usually causes it,
/// sorted by code. A test checks that every code the source creates is here.
pub const DIAGNOSTICS: &[(&str, &str)] = &[
    (
        "already_reverted",
        "That change was taken back already (edit_history revert). Undo the revert to bring it back.",
    ),
    (
        "ambient_color_unused",
        "A scene's Ambient sets color or intensity, but its light comes from the sky (Ambient.source Sky, the default since scene format 2), which uses neither: the setting changes nothing. Set Ambient.source to \"Color\" for a flat ambient light (or apply the look plain), or remove the two fields and use sky_strength. Given by check (at the field in the scene file) and by capture_frame.",
    ),
    (
        "archive_failed",
        "An archive could not be written or read (the engine's release archives, `wen package`): a link or a name that is not UTF-8 in the folder, a file too large for the format, a damaged archive, or a checksum list that is not one. The message names the file.",
    ),
    (
        "asset_exists",
        "A move would put an asset where a file already exists. Moves never overwrite: pick another destination or remove the other file first.",
    ),
    (
        "asset_not_found",
        "No asset file has that path. Paths are project-relative with '/' between folders; asset_list shows what exists.",
    ),
    (
        "audio_clip_unknown",
        "A sound source wants a clip that is not a readable .wav or .ogg file of the project (wrong path, missing file, or unreadable). The source is switched off. Check the path, and start a new session so the engine reads the project's clips again.",
    ),
    (
        "audio_decode",
        "A sound file could not be decoded (damaged, an unsupported format, or too long). Use .wav (8, 16, 24 or 32-bit integer, or 32-bit float) or .ogg (Vorbis) of at most an hour and 128 MiB.",
    ),
    (
        "audio_device",
        "No sound device could be opened for `wen play`, so the game runs without sound. Check that speakers or headphones are connected and not held by an exclusive-mode program.",
    ),
    (
        "audio_ended",
        "Log line (logs_get kind audio): a sound stopped because its clip finished, it was switched off, or its entity is gone.",
    ),
    (
        "audio_started",
        "Log line (logs_get kind audio): a sound started, with how loud it is at the listener (dB, 0 dB is the clip as recorded) and where (left, centre, right). Sounds are described in a session, not played.",
    ),
    (
        "bench_failed",
        "`wen bench --script` could not prepare its measurement (a temporary folder, or the measured game code did not build). Check that `wen sdk` was run with this engine.",
    ),
    (
        "capture_failed",
        "A scenario test's capture step could not take its picture (no camera, or the camera named does not exist); the message says which.",
    ),
    (
        "character_not_kinematic",
        "An entity has a CharacterController but its RigidBody is not Kinematic (or it has none); the controller only moves kinematic bodies. Set RigidBody.kind to Kinematic.",
    ),
    (
        "checkpoint_damaged",
        "A checkpoint's description in .wish/checkpoints cannot be read or names a path that is not a document of the project, so nothing is brought back. Delete it and save a new one.",
    ),
    (
        "checkpoint_exists",
        "A checkpoint of that name exists already. Choose another name or pass overwrite.",
    ),
    (
        "checkpoint_lacks_document",
        "The checkpoint was made before that scene existed, so it has no copy of it. Pick a later checkpoint or another scene.",
    ),
    (
        "checkpoint_not_found",
        "There is no checkpoint of that name. checkpoint with action list shows them.",
    ),
    (
        "checkpoint_restore_incomplete",
        "Going back to a checkpoint stopped half-way (a file could not be written). The message lists what was brought back; scenes and prefabs can be undone in their history, and the files written over are in the trash folder it names. Fix the problem and restore again.",
    ),
    (
        "checkpoint_textures_current",
        "A checkpoint's picture is drawn with the textures as they are now: checkpoints keep scenes, prefabs and material files, not pictures. To compare a change of a picture, save the new picture under another name.",
    ),
    (
        "component_not_found",
        "The entity does not have the component the operation needs. Add it first (set adds a missing component) or check the name with schema_get.",
    ),
    (
        "conflicting_query_access",
        "A system asked for the same component both to read and to write in one query. Split the query or read the value first.",
    ),
    (
        "default_scene_moved",
        "The project's default scene was moved, and wish.json was updated to name its new path (a note, not a problem).",
    ),
    (
        "defaults_record_invalid",
        "The engine's record of the defaults an older file format was written against could not be read, so the file cannot be migrated. It is part of the engine: reinstall it.",
    ),
    (
        "degenerate_transform",
        "An entity was not drawn because its scale is zero or not a finite number. Give its Transform (and its parents') a non-zero, finite scale.",
    ),
    (
        "design_file_invalid",
        "design/game.json, the game's design (card, decisions, plan, notes, questions), is not valid JSON of format 1. It is never overwritten while broken: fix it, or delete it to start the design again.",
    ),
    (
        "design_item_not_found",
        "game_design was given the id of a decision, plan step, note or question that does not exist. game_design get lists them with their ids.",
    ),
    (
        "duplicate_system",
        "Two systems have the same name. System names must be unique within an app.",
    ),
    (
        "empty_transaction",
        "An edit had no operations. Send at least one operation such as spawn, set or despawn.",
    ),
    (
        "engine_folder_invalid",
        "A folder given as an engine is not one: an engine folder has wen and wish-engine.json (a packaged engine; `wen package` writes both). In a listing, an added folder that is no longer an engine is a warning; remove it to forget it.",
    ),
    (
        "engine_not_found",
        "No engine of this computer is in the folder given. `engines` with action list (or the Hub's Engine versions tab) shows their folders.",
    ),
    (
        "engine_registry_invalid",
        "The person's list of added engines (engines.json in the settings folder) cannot be read, so it is read as empty. Adding an engine again writes it anew.",
    ),
    (
        "engine_remove_failed",
        "An installed engine could not be moved to the trash, usually because it is running or a file of it is open. Close it and try again.",
    ),
    (
        "engine_running",
        "The engine that is running cannot remove itself. Remove it from another engine, or delete its folder after closing it.",
    ),
    (
        "engine_start_failed",
        "The engine version a project was made with is installed, but could not be started. Check that its folder is complete (wen, sdk, toolchain), or open the project with this engine (`wen edit --root <project>`).",
    ),
    (
        "engine_too_many",
        "A person can keep at most 64 engines side by side. Remove the ones no longer used.",
    ),
    (
        "entity_exists",
        "An entity with that id already exists in the scene. Ids are unique: pick another id, or change the existing entity with set.",
    ),
    (
        "entity_not_found",
        "No entity has that id. Ids are the names in the scene file; scene_get and entity_query list them.",
    ),
    (
        "entity_out_of_bounds",
        "An entity in a running session went far out of the world (|y| over 1000 m); it probably fell. Add a floor under it or remove entities that fall too far.",
    ),
    (
        "export_exists",
        "An export of that name exists already; pass overwrite to replace it.",
    ),
    (
        "export_has_errors",
        "The project has errors (check), so it was not exported. Fix them first.",
    ),
    (
        "export_needs_build",
        "The project's game code is not built, so it was not exported. Build it first (script_build).",
    ),
    (
        "export_needs_c_runtime",
        "The player needs the Visual C++ runtime DLL, which a clean Windows may lack.",
    ),
    (
        "export_outside_project",
        "The exports folder is a link or lies outside the project.",
    ),
    (
        "expr_syntax",
        "A where or until condition could not be read. The column says where; write paths like Transform.translation.y, comparisons with ==, and join conditions with && or ||.",
    ),
    (
        "expr_unknown_name",
        "A condition names a component, resource or field that does not exist. The hint gives the closest real name; schema_get lists them all.",
    ),
    (
        "external_change_rejected",
        "A scene file was changed outside the engine and is now invalid. The last valid scene is kept and changes are refused until the file is fixed.",
    ),
    (
        "extra_directional_lights",
        "The scene has more than one DirectionalLight; only the first (in entity order; in a scene file: parents before their children, siblings by `order`, then by id; a prefab's entities follow their instance) lights the picture and casts shadows, the others are not drawn. The message names the one that is used and the ones that are not. Keep one as the sun and remove the rest, or turn them into PointLight or SpotLight entities.",
    ),
    (
        "file_changed_during_edit",
        "A file was saved by someone else at the very moment a change was being written, so nothing was written (their save is kept). Call again: the change is applied on top of their version.",
    ),
    (
        "file_not_found",
        "A file path given to the tool is not a document of the project. Paths are project-relative; the hint names the closest one.",
    ),
    (
        "file_unreadable",
        "A file could not be read: it is missing, locked, or not UTF-8 text.",
    ),
    (
        "file_unwritable",
        "A file could not be written. Check that the folder exists, can be written to, and that no other program has the file locked.",
    ),
    (
        "fill_too_large",
        "A tilemap_edit fill would reach more than 65,536 cells. Close the region with walls first, or paint rectangles.",
    ),
    (
        "fill_unbounded",
        "A tilemap_edit fill starts outside the painted part of the map (or the map is empty), where nothing would stop it. Fill inside the map's painted part, or paint a rectangle with rect.",
    ),
    (
        "font_invalid",
        "A font file of the project cannot be read (not a TrueType or OpenType font, damaged, or larger than 32 MiB). Text asking for it is drawn with the engine's font (Roboto). Re-export or replace the file.",
    ),
    (
        "gltf_file_missing",
        "A .gltf file refers to separate .bin or image files that are not next to it. Keep them together, or use a single .glb file.",
    ),
    (
        "gltf_image",
        "A model's texture could not be decoded. Textures must be 8-bit PNG or JPEG.",
    ),
    (
        "gltf_invalid",
        "A model file breaks the glTF 2.0 rules (or is damaged in a way the reader could not handle). Check it in the Khronos glTF validator or re-export it.",
    ),
    (
        "gltf_parse",
        "A file is not valid glTF 2.0 at all. Check that it is a .gltf (JSON) or .glb file and not damaged.",
    ),
    (
        "gltf_texture_too_large",
        "A texture has more pixels than the engine accepts (16384 x 16384). Scale it down in the modelling tool.",
    ),
    (
        "gpu_device_failed",
        "The graphics adapter was found but could not open a device. Update the driver, or use the software adapter.",
    ),
    (
        "gpu_error",
        "The GPU reported an error while drawing. The message says what; if it repeats, try the software adapter and report it.",
    ),
    (
        "gpu_lost",
        "The GPU device was lost (driver reset or crash) before the work could finish. Restart the run; if it repeats, update the driver or switch adapter.",
    ),
    (
        "gpu_out_of_memory",
        "The GPU ran out of memory. Draw a smaller picture, close other programs that use the GPU, or use the software adapter.",
    ),
    (
        "gpu_readback_failed",
        "A drawn picture could not be read back from the GPU. Try again; if it repeats, use the software adapter.",
    ),
    (
        "hidden_destination",
        "A move would put a file into a hidden folder (one starting with '.'), where the project does not look. Choose a visible folder.",
    ),
    (
        "hierarchy_cycle",
        "An entity would become its own ancestor. Choose a parent that is not below the entity in the hierarchy.",
    ),
    (
        "hierarchy_too_deep",
        "Game code tried to make the hierarchy deeper than 256 levels; the set_parent command was skipped. A chain this deep is usually a parent set every tick by mistake.",
    ),
    (
        "history_reset",
        "A scene file changed outside the engine in a way that could not be recorded as separate edits. It is recorded as one change by the file (revisions keep counting), and earlier changes can no longer be undone.",
    ),
    (
        "http_needs_token",
        "wen mcp --http was asked to listen beyond this machine without a token. Set WISH_MCP_TOKEN (clients send it as Authorization: Bearer <token>) or listen on 127.0.0.1.",
    ),
    (
        "image_invalid",
        "A picture for the interface cannot be read: it is not a PNG, is damaged, or is larger than 4096x4096 pixels. Re-export it as an 8- or 16-bit PNG.",
    ),
    (
        "internal_error",
        "Something inside the engine went wrong that should not be possible. Nothing was changed; please report the call that caused it.",
    ),
    (
        "invalid_arguments",
        "The arguments of a call are not in the expected form (for example not a JSON object). The hint shows the expected form.",
    ),
    (
        "invalid_asset_ref",
        "An asset reference is not a valid project path: it must be relative, use '/', and not leave the project.",
    ),
    (
        "invalid_bytes",
        "Stored data did not match its type's layout. This points to a bug or a damaged snapshot.",
    ),
    (
        "invalid_call",
        "An entry in a wen do list is not a call object. Write each call as { \"tool\": \"<name>\", \"args\": { ... } }.",
    ),
    (
        "invalid_camera",
        "A camera cannot be used: near must be positive and smaller than far, and its placement must be finite and invertible (non-zero scale).",
    ),
    (
        "invalid_capture_size",
        "A picture size is outside what can be drawn. Use a width and height within the allowed range, such as 512 x 288.",
    ),
    (
        "invalid_checkpoint_name",
        "A checkpoint's name is 1 to 40 of a-z, 0-9, _ and -.",
    ),
    (
        "invalid_cursor",
        "A paging cursor does not belong to the query. Pass the previous answer's next_cursor unchanged, with the same other parameters.",
    ),
    (
        "invalid_design",
        "A game_design call would put something the design file does not hold: an unknown card field, a list where text belongs, text that is too long, a question with fewer than two or more than six choices. The message says which.",
    ),
    (
        "invalid_editor_script",
        "An editor input script (wen edit --headless-frame) is malformed: an event or step the editor does not know, or a size out of range. The hint shows the format.",
    ),
    (
        "invalid_entity_id",
        "A runtime entity id is malformed or refers to an entity that no longer exists.",
    ),
    (
        "invalid_export_name",
        "An export's name is 1 to 40 of a-z, 0-9, _ and -.",
    ),
    (
        "invalid_hash",
        "A hash string is not 32 hexadecimal digits.",
    ),
    (
        "invalid_http_address",
        "wen mcp --http got an address it cannot listen on: write a port (7878) or an address and port (127.0.0.1:7878; port 0 picks a free one).",
    ),
    (
        "invalid_input_file",
        "input.json, the project's map from keys to actions, is not valid. The message says where; a mistyped key name comes with the closest real one.",
    ),
    (
        "invalid_op",
        "An operation in edit_apply is malformed: unknown op name, missing or misspelled key, or a value of the wrong kind. location.op says which operation.",
    ),
    (
        "invalid_output_path",
        "A tool may only write inside the project, not to scenes, prefabs, meta or project files, hidden folders or outside paths. Use something like captures/shot.png.",
    ),
    (
        "invalid_parameter",
        "A parameter has the wrong type or is out of range. The hint shows a correct example and what the parameter means.",
    ),
    (
        "invalid_project_file",
        "wish.json is malformed: an unknown setting, a wrong value (a kind that is not \"2d\" or \"3d\"), or a missing name, format or scene. The location points at the setting.",
    ),
    (
        "invalid_project_name",
        "A project name must be 1 to 80 characters without control characters.",
    ),
    (
        "invalid_replay_file",
        "A replay file is damaged or not a replay (the message gives the line). Replays are written by `replay save`; record the run again rather than editing the file.",
    ),
    (
        "invalid_scene_id",
        "An entity id is not valid: use letters, digits, '_' and '-', start with a letter or '_', at most 64 characters.",
    ),
    (
        "invalid_scene_path",
        "A scene path must be project-relative, end in .scene.json and stay inside the project.",
    ),
    (
        "invalid_schema",
        "A type's schema is malformed (a bug in a component definition).",
    ),
    (
        "invalid_system_name",
        "A system name is empty or contains characters that are not allowed.",
    ),
    (
        "invalid_template",
        "A template given to wen new or the welcome screen is not a project folder.",
    ),
    (
        "invalid_test_file",
        "A scenario test file (tests/*.test.json) is not valid; the message names the step and what is wrong. A test has format 1, a scene, a seed and steps: frames, until, expect or capture.",
    ),
    (
        "invalid_text_handle",
        "A stored text handle does not refer to any text; the data is damaged or comes from another world.",
    ),
    (
        "invalid_type_description",
        "A game module described one of its types in a way the engine cannot read (malformed or impossible layout). Build game code with the engine's SDK; type descriptions are generated by its macros.",
    ),
    (
        "invalid_version",
        "A version is not three numbers (major.minor.patch, such as 1.2.0). check reports it as a warning for the engine key of wish.json: until it is a version, no engine can be chosen for the project.",
    ),
    (
        "invalid_version_requirement",
        "A version requirement is not comparators joined with commas (>=, >, <=, <, =, ^, ~), such as \">=0.9, <2.0\" or \"^1.2\".",
    ),
    (
        "json_duplicate_key",
        "A JSON object has the same key twice. Keep one of them.",
    ),
    (
        "json_number_out_of_range",
        "A number in a file is too large or too precise to be read exactly.",
    ),
    (
        "json_syntax",
        "A file is not valid JSON at the position given. Common causes: a missing comma or quote, a trailing comma, or comments.",
    ),
    (
        "json_too_deep",
        "A JSON value nests deeper than the engine reads. Flatten the structure.",
    ),
    (
        "kind_change",
        "A move would turn a file into a folder or the other way round. Move files to files and folders to folders.",
    ),
    (
        "layer_not_found",
        "A tilemap_edit operation names a layer the map does not have; the hint lists its layers. add_layer makes one.",
    ),
    (
        "layout_mismatch",
        "Stored data was made by a different version of a type. Re-create it with the current version.",
    ),
    (
        "light_2d_without_lighting",
        "A scene has Light2D components while 2D lighting is off, so they light nothing. Set Lighting2D.enabled to true (and its ambient to how dark the scene is without lights).",
    ),
    (
        "light_clusters_full",
        "Too many point and spot lights reach the same part of the picture (more than 128); some were left out there, so it is darker than it should be. Spread lights out or give them a smaller range.",
    ),
    (
        "live_edit",
        "A note in a session's log: someone changed the running game with edit_apply and session. It lasts while the session runs; the scene file is not changed.",
    ),
    (
        "live_edit_unsupported",
        "An operation that only means something in a scene file (instantiate, overrides, notes) was sent to a running game. Make it in the scene and start the game again.",
    ),
    (
        "live_edit_while_recording",
        "A running game that is recording a replay refuses live edits (they are not input, so the replay would not play the same). Save the recording first.",
    ),
    (
        "lobby_full",
        "Every seat of the online lobby is taken (16 at most, the host's included), so the host refused the player. Wait for a player to leave.",
    ),
    (
        "lobby_host_left",
        "The online session ended: the host closed it, or the connection to the host was lost. Join another match; a player who lost the connection comes back into its seat within the grace period.",
    ),
    (
        "lobby_join_timeout",
        "A peer connected to the host but did not send its join in time, so the host disconnected it. A client joins as soon as it connects: one that does not is broken, of another version, or hostile.",
    ),
    (
        "lobby_match_running",
        "A new player asked to join while a match was running, so the host refused it. Join when the host returns to the lobby; a player who dropped comes back with its rejoin token.",
    ),
    (
        "lobby_not_host",
        "Game code on a player's machine asked the lobby to start, end or reopen the match; only the host does that. Let the host's game code send that LobbyRequest.",
    ),
    (
        "lobby_not_ready",
        "The match cannot start yet: fewer players than min_players are here, or someone is not ready (the hint names them). Wait, or lower min_players.",
    ),
    (
        "lobby_refused",
        "The host refused this player: the lobby was full, a match was running, the rejoin came too late, or the versions differ. The message says which; join when the lobby is open.",
    ),
    (
        "lobby_rejoin_expired",
        "A player who dropped did not come back within rejoin_grace_ticks, so its seat is left (Seat.left) and its rejoin token no longer works. It can join the next match from the lobby.",
    ),
    (
        "lobby_wrong_phase",
        "A lobby request does not fit where the lobby is: names, colours and ready change in the open lobby, a match starts from it, ends while running, and the lobby reopens after the match ended.",
    ),
    (
        "lock_file_invalid",
        "wish.lock, the exact packages the project uses, is not valid. Write it again from what is installed: packages with action lock.",
    ),
    (
        "looks_invalid",
        "The engine's own list of looks (for apply_look) could not be read. It is part of the engine: reinstall it.",
    ),
    (
        "managed_component",
        "The engine keeps this component itself (such as GlobalTransform or Hierarchy). Change the component it is derived from instead, such as Transform or the parent.",
    ),
    (
        "material_fields_unused",
        "Check warning: an entity's Material names a material file (file) and also writes its numbers. With a file the file is the whole look; the numbers do nothing. Remove them, change them in the material file instead (every entity that uses it changes), or clear file for a plain look of the entity's own.",
    ),
    (
        "material_file_too_large",
        "A *.mat.json file is larger than 256 KiB and was not read. A material file holds a few numbers and the paths of its textures; pictures stay in their own PNG files.",
    ),
    (
        "material_invisible",
        "Check warning: a scene or prefab gives an entity's plain Material (no file) a base_color alpha of 0 or below, so nothing of it is drawn: an alpha below 1 makes a surface see-through. Set the alpha to 1 (solid) or between 0 and 1 (see-through); to hide an entity on purpose remove its Primitive or Model, and to fade it let game code lower the alpha while the game runs.",
    ),
    (
        "material_needs_uvs",
        "Render warning: a material file with textures is on a Model that has parts without texture coordinates (UVs), so its textures show as one flat colour there. Export the model with UVs, or use a material file without textures on it.",
    ),
    (
        "material_not_loaded",
        "Render warning: an entity's Material.file names a material file that could not be used (it does not exist, is not a *.mat.json, or has a mistake: the message says which). The entity is drawn with the numbers of its own Material (a model: with its own materials). Fix the file or the path; check lists everything wrong with the project's material files.",
    ),
    (
        "material_unused_on_model",
        "Check warning: an entity has a Model and a Material without a file. A model is drawn with the materials of its own file; only a material file (Material.file) replaces them, all of them. Remove the Material, or save the look as a material file (asset_create with a .mat.json path) and name it in Material.file.",
    ),
    (
        "material_uv_unusable",
        "Check warning: a material file's uv_scale has a 0 (the whole picture would shrink to one point, so it is drawn as 1), or a uv_scale or uv_offset number is beyond 4096 either way (it is drawn as 4096). Use a number other than 0 between -4096 and 4096: [4, 4] tiles the picture 4 by 4, a negative number mirrors it.",
    ),
    (
        "mesh_too_large_for_gpu",
        "A mesh has more vertex data than the GPU allows in one buffer. Split it or reduce its detail in the modelling tool.",
    ),
    (
        "migration_engine_not_set",
        "wen migrate migrated files but could not read wish.json, so its engine key still names the older engine, which cannot read the migrated files. Fix wish.json and set engine to the version of the engine that migrated (the hint names it).",
    ),
    (
        "migration_prefab_unresolved",
        "wen migrate could not tell what a prefab gives an instance (the prefab is missing, or nested too deep), so it wrote the old default into the instance as if the prefab gave none. Check the result; migrating the whole project with the prefab present avoids it.",
    ),
    (
        "migration_refused",
        "wen migrate does not migrate a file whose entities or resources are not a JSON object: fix the file first (wen check), then migrate it.",
    ),
    (
        "missing_format",
        "A document has no \"format\" number. Add the number the hint gives (a scene: the engine's scene format, wish_scene::FORMAT).",
    ),
    (
        "missing_parameter",
        "A required parameter is missing. The hint names it with an example value.",
    ),
    (
        "model_not_loaded",
        "A Model entity's file could not be loaded, so it was not drawn; the rest of the scene still was. The message says why.",
    ),
    (
        "model_scale_unusual",
        "A model is very large (over 100 m) or very small (under 1 cm). glTF units are meters: rescale it or scale the entity's Transform.",
    ),
    (
        "move_into_itself",
        "A folder cannot be moved into itself or below itself.",
    ),
    (
        "nested_prefab",
        "Prefabs cannot contain prefab instances yet. Place the parts directly in the prefab.",
    ),
    (
        "net_ack_invalid",
        "A client of online play acknowledged a state message the host never sent it. The acknowledgement was ignored; a modified or broken client sends these.",
    ),
    (
        "net_bind_failed",
        "A UDP port for online play (or a simulated address in a test) could not be opened: another program uses it, or the address is not this machine's. Pick another port, or 0 to let the system choose one.",
    ),
    (
        "net_closed",
        "A connection of online play ended normally: the other side, or this one, closed it. Nothing went wrong; join or host again to play on.",
    ),
    (
        "net_component_not_replicable",
        "A component listed for replication cannot be sent: the engine keeps it itself (Hierarchy), its name is longer than 128 bytes, or its schema is not well formed. Replicate the components the game writes.",
    ),
    (
        "net_component_unknown",
        "A component listed for replication is not registered in the host's world. Name it as scene files do, and make sure the game module (or the engine) registers it before the match starts.",
    ),
    (
        "net_config_invalid",
        "A setting of online play is out of its range (players, message size, queue size, timeouts, rate, or a discovery game id or match name longer than 64 bytes). The location names the setting and the hint gives its range.",
    ),
    (
        "net_connect_timed_out",
        "A client trying to join a match got no answer from the host in time. Check the address and port, that the host is running and that a firewall lets UDP through; across the internet, use a relay.",
    ),
    (
        "net_crypto_failed",
        "A packet of online play could not be sealed. This should never happen (packets are far below the cipher's limit): it is an engine bug, please report it.",
    ),
    (
        "net_entropy_failed",
        "The operating system gave no random numbers, so online play could not make its keys. Restart the program; report it if it stays.",
    ),
    (
        "net_host_key_mismatch",
        "A client pinned the host's public key (from discovery or given by hand) and the host answered with another one: someone may stand between them, or the host started a new match. Get the host's key again and join again.",
    ),
    (
        "net_input_out_of_window",
        "A client sent input stamped further ahead of the host's tick than the input window (120 ticks by default). The frame was dropped: the client's clock is broken, or it was modified.",
    ),
    (
        "net_input_wrong_player",
        "A client sent input for a player id that is not its own. The message was dropped: a client only ever sends its own player's input, so a modified client tried to move someone else.",
    ),
    (
        "net_join_failed",
        "A player could not join an online match: the address is not one (write host:port, such as 192.168.1.20:47600), discovery found no host of this game on the network, or the session has no host to join. Check that the host runs and is reachable, or join by its address.",
    ),
    (
        "net_key_rejected",
        "The other side of an online connection sent a public key that gives no shared secret (a point of low order). Nothing was sent to it: the other program is broken or hostile.",
    ),
    (
        "net_message_flood",
        "A player sent more network messages than messages_per_second allows (a burst of message_burst), so the extra ones were dropped. Send state that changes every tick through replication, not messages.",
    ),
    (
        "net_message_invalid",
        "A client sent a replication message that does not parse or breaks a limit (larger than 2 KiB, more than 8 input frames or 32 actions in a frame). It was dropped and nothing was applied: the other program is broken, of another version, or hostile.",
    ),
    (
        "net_message_oversized",
        "A network message is larger than message_bytes (1024 by default), so it was dropped (or not sent). Keep messages small: an id, a choice, a short text.",
    ),
    (
        "net_message_target_refused",
        "Game code on a player's machine sent a network message to someone other than the host; players send only to the host. Send it with to_host and let the host's game code pass it on.",
    ),
    (
        "net_message_too_large",
        "An online message is larger than max_message_size (64 KiB by default, at most 1 MiB). Send less (a delta instead of the whole state), split it, or raise the limit on both sides.",
    ),
    (
        "net_not_connected",
        "A message was sent before the connection was open. Wait for the Connected event first.",
    ),
    (
        "net_not_networked",
        "A net action (or player on entity_query, entity_get, capture_frame or describe_view) names a session that is not a networked match. Host one with net (action host); its world is the host's, and join and bots add players.",
    ),
    (
        "net_player_unknown",
        "A player seat named on a networked session has no player machine in this process (not seated yet, or a player on another machine). Step the session until net status lists the seat; only machines in this process can be looked at.",
    ),
    (
        "net_players_full",
        "A peer joined a match whose player ids are all taken (16: the host's own and 15 others), so it was disconnected. Wait for a player to leave.",
    ),
    (
        "net_protocol_violation",
        "A peer that holds the connection's keys sent frames that break the protocol (malformed frames, fragment counts above the limit, messages out of shape), so the host closed that connection. The other program is broken or hostile.",
    ),
    (
        "net_replaced",
        "The same address joined a match again with a new key (a player's game restarted), so the old connection was closed. Nothing needs doing.",
    ),
    (
        "net_room_code_invalid",
        "A room code is 4 to 12 of the letters and digits ABCDEFGHJKLMNPQRSTUVWXYZ23456789 (no I, O, 0 or 1); spaces and dashes are ignored and small letters count as capitals.",
    ),
    (
        "net_room_code_wrong",
        "The host refused the room code a client gave. Type the code the host shows again.",
    ),
    (
        "net_room_full",
        "The match already has as many players as it takes. Wait for a player to leave, or ask the host to allow more players.",
    ),
    (
        "net_send_queue_full",
        "Too many bytes wait to be sent to (or acknowledged by) one peer: it is slow, or the network is bad. Send less often, or send state unreliably so that new state replaces old.",
    ),
    (
        "net_sequence_exhausted",
        "A connection's reliable channel used all its sequence numbers (about four billion fragments). Close the connection and join again.",
    ),
    (
        "net_session_unsupported",
        "A snapshot, a fork, a replay recording or a live edit was asked of a networked session. A match is the host, every player and the network in flight, so these are refused; play it again from the start (same scene, seeds and calls give the same match), and save its replay with replay save (no record needed).",
    ),
    (
        "net_socket_failed",
        "The UDP socket of online play failed. Check that the network is up and that a firewall allows the program to use UDP.",
    ),
    (
        "net_state_invalid",
        "A client got a state message from the host that it cannot read (malformed, or changing a value it never had). It was dropped; the host runs another version of the game or is broken.",
    ),
    (
        "net_timed_out",
        "Nothing arrived from the other side of an online connection for timeout_ms (10 seconds by default): the other machine left, crashed or lost its network.",
    ),
    (
        "net_types_mismatch",
        "A client's game does not have every component type the host replicates, or has one with another layout. Host and players must run the same build of the game.",
    ),
    (
        "net_unknown_peer",
        "A message was sent to, or a disconnect asked for, a peer that has no connection (it left, or never joined). Use the peers the endpoint lists or a Connected event.",
    ),
    (
        "net_version_mismatch",
        "The two sides of an online connection speak different versions of the network protocol. Both machines must run the same release of the game or the engine.",
    ),
    (
        "no_camera",
        "The scene has no camera to draw from. Add an entity with Camera and Transform components.",
    ),
    (
        "no_default_scene",
        "No scene was named and the project has no default scene. Pass a scene path, or create a project with wen new.",
    ),
    (
        "no_gpu_adapter",
        "No graphics adapter is available. Use the software adapter (--software, WISH_SOFTWARE), or install a driver; on Linux install mesa-vulkan-drivers.",
    ),
    (
        "no_migration",
        "There is no upgrade path from that document format to the current one.",
    ),
    (
        "no_project_file",
        "The folder has no wish.json, so it is not a Wish Engine project yet. wen new creates one.",
    ),
    (
        "no_session",
        "No play session is open. Start one with sim_start.",
    ),
    (
        "not_a_camera",
        "The entity named as the camera has no Camera component. Name one that does, or leave camera out to use the first one.",
    ),
    (
        "not_a_material_file",
        "Material.file names a file that is not a material file (*.mat.json). To put a picture on a surface, make a material file whose base_color_texture names the picture (asset_create with a .mat.json path) and name that file here.",
    ),
    (
        "not_a_sheet_file",
        "SpriteAnimator.sheet names a file that is not a sprite sheet (*.sheet.json). Make a sheet that names the picture in its \"image\" (asset_create with a .sheet.json path and sheet {image, grid or frames, clips}) and name that file here.",
    ),
    (
        "not_a_tilemap_file",
        "Tilemap.map names a file that is not a tilemap (*.tilemap.json). Make one with asset_create (a .tilemap.json path and tilemap {tileset, layers}) and name it here.",
    ),
    (
        "not_a_tileset_file",
        "A tilemap's tileset names a file that is not a tileset (*.tileset.json). Make a tileset with asset_create (a .tileset.json path and tileset {image, grid, tiles}) and name it in the map.",
    ),
    (
        "not_an_instance",
        "The operation works on prefab instances only, and the entity is not one.",
    ),
    (
        "not_canonical",
        "A file is valid but not in canonical form. The engine writes canonical files itself; wen fmt rewrites hand-edited ones.",
    ),
    (
        "not_revertible",
        "That change cannot be taken back on its own: it was undone already, was itself an undo or redo, or is older than a change made outside the engine or than the history kept. Undo or edit by hand.",
    ),
    (
        "nothing_found",
        "A search found nothing. Try fewer or shorter words.",
    ),
    ("nothing_to_do", "There was nothing to undo or redo."),
    (
        "nothing_to_move",
        "The file or folder to move does not exist.",
    ),
    (
        "occluder_without_collider",
        "An entity has an Occluder and no Collider, so it blocks no sight and no 2D light: an occluder's outline is its collider's in the X-Y plane. Add a Collider (a Box for a door), or remove the Occluder.",
    ),
    (
        "old_format",
        "A scene or prefab file is of an older format than this engine writes. It is read as its own format meant it (a default that changed since keeps its old value for this file), so nothing looks or plays differently. wen migrate rewrites it in the current format with those values written in; so does the next change made through the engine.",
    ),
    (
        "output_exists",
        "A picture would replace a file that already exists. Choose a new name, or pass overwrite: true to replace an earlier capture.",
    ),
    (
        "override_not_found",
        "The prefab instance has no override for that entity and component.",
    ),
    (
        "override_on_root",
        "Overrides are for the prefab's other entities; change the instance's own components with set or replace instead.",
    ),
    (
        "overrides_without_prefab",
        "An entity has overrides but is not a prefab instance. Remove the overrides or make it an instance.",
    ),
    (
        "package_engine_mismatch",
        "A package says it works with other engine versions than this one. Use a version of the package made for this engine.",
    ),
    (
        "package_exists",
        "The package is installed already. Pass replace: true to put this version in its place (the old one goes to the trash).",
    ),
    (
        "package_failed",
        "`wen package` could not build the package (engine development only): the message says which step.",
    ),
    (
        "package_hash_mismatch",
        "A package from the index does not have the files the index names (its content hash differs). Nothing was installed.",
    ),
    (
        "package_index_invalid",
        "The package index's index.json is not valid, or names a folder that holds another package or version. The pointer says where.",
    ),
    (
        "package_index_missing",
        "There is no package index: name a folder of packages with the index parameter or WISH_PACKAGE_INDEX.",
    ),
    (
        "package_install_failed",
        "A package could not be copied into place. Nothing was half-installed: it is copied beside its place and moved in only when complete.",
    ),
    (
        "package_invalid",
        "A package's wish-package.json is not valid, or the package holds a link or uses the engine's own id prefix. The message says which; the hint shows the format.",
    ),
    (
        "package_license_unknown",
        "A package's license is not one of the known permissive licenses. It installs; read its license before you share a game made with it.",
    ),
    (
        "package_lock_mismatch",
        "A project package's files are not the ones wish.lock records, or the lock names a package that is not installed. Write the lock again, or install the package again.",
    ),
    (
        "package_needs_static_crt",
        "A program in the Windows package (wen, the player, the launcher, or the SDK's compile-time macros) needs the Visual C++ runtime DLL, which a clean Windows may not have; build it with the C runtime linked in (RUSTFLAGS=-C target-feature=+crt-static).",
    ),
    (
        "package_not_found",
        "There is no package there: no folder with a wish-package.json, or no installed package of that id.",
    ),
    (
        "package_remove_failed",
        "A package could not be moved to the trash (a program may be using its files).",
    ),
    (
        "package_too_large",
        "A package has more than 4,096 files or 64 MiB. Big assets belong in the project, not in a package.",
    ),
    (
        "package_unreadable",
        "A package's folder or files could not be read.",
    ),
    (
        "package_version_not_found",
        "No version of the package in the index fits this engine (and the version asked for). The hint lists the versions there are.",
    ),
    (
        "package_without_compiler_notices",
        "The compiler being packed has no licence notices where rustup puts them (share/doc/rust), so the package ships the compiler without them. Package with a rustup toolchain.",
    ),
    (
        "package_without_launcher",
        "The launcher (wish-engine) is not built beside wen, so the package has no program to start the editor from a shortcut or a double-click (on Windows, without a console window). Build wish_cli (both programs) first.",
    ),
    (
        "package_without_player",
        "The player program is not built beside wen, so the package cannot export games. Build wish_player first.",
    ),
    (
        "paint_too_large",
        "One tilemap_edit call would change more than 65,536 cells (or one rectangle covers more). Paint in several calls; each is its own step in the history.",
    ),
    (
        "path_case_mismatch",
        "A path differs only in letter case from an existing file. Paths are case-sensitive: write the existing name exactly, or the same file would be opened twice under two names.",
    ),
    (
        "permission_denied",
        "Only a person can do that (change the permission mode, accept or reject a proposal). Ask the person working with you.",
    ),
    (
        "physics_3d_in_2d",
        "A scene whose PhysicsSettings.profile is 2D (TopDown2D or SideView2D) writes something only 3D physics uses: a Cylinder collider (in 2D it acts as the rectangle seen from its side: use Box), gravity in a top-down scene (seen from above nothing falls, so gravity is not used), or a gravity with a z part (2D physics lies in the X-Y plane). Given by check, at the field in the scene file.",
    ),
    (
        "player_missing",
        "The player program (wish_player) was not found next to the engine. Build it or set WISH_PLAYER.",
    ),
    ("png_decode", "An image could not be decoded as PNG."),
    ("png_encode", "A picture could not be encoded as PNG."),
    (
        "prefab_edit_invalid",
        "An edit of a prefab file would leave it no longer a prefab (an entity without a parent besides root, root removed or renamed, a resource, a prefab instance inside). The message says which rule.",
    ),
    (
        "prefab_entity_outside_root",
        "Every entity of a prefab must be below its root entity.",
    ),
    (
        "prefab_not_found",
        "No prefab file has that path. Prefabs are project-relative .prefab.json files.",
    ),
    (
        "prefab_override_orphaned",
        "An edit of a prefab removes or renames an entity that instances in scenes override. Drop those overrides first (remove_override, or Revert to prefab in the editor).",
    ),
    (
        "prefab_root_with_parent",
        "A prefab's root entity cannot have a parent.",
    ),
    (
        "prefab_with_resources",
        "Prefabs cannot set resources; put them in the scene.",
    ),
    (
        "prefab_without_root",
        "A prefab must have an entity called root.",
    ),
    (
        "primitive_inconsistent",
        "A model's primitive has indices or attributes that do not match its vertices (or values that are not numbers), so it was skipped. The file is damaged; re-export it.",
    ),
    (
        "primitive_without_positions",
        "A model's primitive has no positions and was skipped.",
    ),
    (
        "project_exists",
        "The folder already has a wish.json. wen new never overwrites a project.",
    ),
    (
        "project_files_exist",
        "A new project would replace files already in its folder.",
    ),
    ("project_not_found", "The project folder does not exist."),
    (
        "proposal_decided",
        "That proposal was accepted, rejected or failed already; only waiting ones can be decided.",
    ),
    (
        "proposal_not_found",
        "There is no proposal with that id. proposals with action list shows them.",
    ),
    (
        "replay_assets_changed",
        "Project files the simulation reads (sound and animation clip lengths, fonts) changed since the replay was recorded, so the run may differ. Re-record the replay if the change was intended.",
    ),
    (
        "replay_code_changed",
        "The game code is not the code the replay was recorded with. The replay still plays: if it passes, the change did not alter the run.",
    ),
    (
        "replay_diverged",
        "The replay and the game parted ways: from this tick the world is not the one that was recorded. The session is left open at that tick to look at.",
    ),
    (
        "replay_empty",
        "Nothing was recorded yet; run some ticks with sim_step before saving.",
    ),
    (
        "replay_engine_changed",
        "The replay was recorded by another engine version (or does not say which: replays say so from 1.1 on). It still plays; a patch release never changes a simulation's result, another minor version may, so a divergence may be the engine and not the game.",
    ),
    (
        "replay_executor_changed",
        "The replay's game code ran on another executor than this one. It still plays; a divergence may be the executor and not the game.",
    ),
    (
        "replay_exists",
        "A replay of that name exists. Choose another name or pass overwrite.",
    ),
    (
        "replay_not_at_start",
        "A replay is recorded from tick 0; start a new session and call replay with record before the first sim_step.",
    ),
    (
        "replay_not_found",
        "There is no replay of that name; the hint lists the ones there are.",
    ),
    (
        "replay_not_recording",
        "The session is not recording; call replay with action record on a session at tick 0 first.",
    ),
    (
        "replay_scene_changed",
        "The scene has changed since the replay was recorded; a different scene usually plays out differently.",
    ),
    (
        "replay_scene_missing",
        "The replay's scene file does not exist any more.",
    ),
    (
        "replay_too_long",
        "A replay can have at most 54 000 ticks (a quarter of an hour at 60 ticks a second).",
    ),
    (
        "resource_not_found",
        "The resource is not set in the scene or world.",
    ),
    (
        "retype_refused",
        "An engine type's layout cannot change while a world exists; only types declared by game code can. Rename the game's type if it clashes with an engine type.",
    ),
    (
        "revert_conflict",
        "Later changes touched what the change to take back changed; reverting it would undo part of theirs. Take those back first (newest first) or edit by hand.",
    ),
    (
        "rigid_body_without_collider",
        "An entity has a RigidBody but no Collider, so physics ignores it. Add a Collider to the same entity.",
    ),
    (
        "scene_file_invalid",
        "Changes are refused because the scene file on disk is currently invalid (edited by hand). Fix the file first; nothing will overwrite it.",
    ),
    (
        "scene_file_missing",
        "A scene file was deleted outside the engine. The scene is kept in memory and the next change writes it again.",
    ),
    (
        "scene_invalid",
        "A scene could not be loaded. check lists every problem.",
    ),
    (
        "scene_not_found",
        "No scene file has that path. To create a new scene, use edit_apply with create: true.",
    ),
    (
        "scene_not_session",
        "A scene tool (entity_get, entity_query) read the scene file while a play session was open. The answer is the file's values; pass session to read the live world.",
    ),
    (
        "script_abi_mismatch",
        "The game module was built for a different game ABI version than this engine speaks. Rebuild it with this engine's SDK (script_build).",
    ),
    (
        "script_after_engine_system",
        "A game system asks to run after one of the engine's systems (physics, animation, ui, particles, audio, transform_propagation). Those always run after every game system; remove the after and read their results (positions, collisions, clicks) the next tick.",
    ),
    (
        "script_bad_manifest",
        "The game module's manifest (its list of types and systems) is malformed. Rebuild the game code with this engine's SDK (script_build).",
    ),
    (
        "script_build_failed",
        "The game code build could not run (a file could not be read or written, or the SDK could not be built). The message names the file; the previous build stays in use.",
    ),
    (
        "script_build_timeout",
        "The compiler ran longer than two minutes and was stopped; the previous build stays in use. Look for a const or static computed by a long loop, or a deeply recursive macro.",
    ),
    (
        "script_call_failed",
        "Calling into the game module failed for a reason that is not a trap (for example an export with the wrong type). Rebuild with this engine's SDK.",
    ),
    (
        "script_command_failed",
        "A command a game system gave (insert, remove, send, parent) could not be applied and was skipped; the other commands still ran and the system keeps running (a warning in logs_get). Despawning an entity that is already gone is not an error. The system is stopped only when its commands cannot be decoded, there are more than 65536 in one call, or the world already has 4194304 entities.",
    ),
    (
        "script_compile_error",
        "The Rust compiler rejected the game code. The location is the line in scripts/; the message carries the compiler's code (E0308 ...) and the hint its help. The previous build stays in use.",
    ),
    (
        "script_compile_warning",
        "The compiler warned about the game code; the build still succeeded. Worth fixing: warnings often point at real mistakes (an unused variable, an unreachable branch).",
    ),
    (
        "script_duplicate_system",
        "A game module lists the same system twice in wish_sdk::export!. List each system once.",
    ),
    (
        "script_duplicate_type",
        "The game code declares two types with the same name (in different Rust modules, or a component and a resource). Types are known by name alone in the world: rename one.",
    ),
    (
        "script_engine_failed",
        "The WebAssembly engine could not start on this machine. This is not caused by the game code.",
    ),
    (
        "script_forbidden_import",
        "The game module imports something the engine does not provide (files, clock, network, WASI). Game code may only use the engine's API; remove the dependency that needs the operating system.",
    ),
    (
        "script_forbidden_source",
        "The game code uses something that reads the machine while it is built (include_str!, include_bytes!, include!, env!, option_env!, #[path], or a link in scripts/). Game code may not; data belongs in components, resources or assets.",
    ),
    (
        "script_invalid_module",
        "The file is not a valid WebAssembly module. Build game code with script_build instead of editing the .wasm file.",
    ),
    (
        "script_log",
        "A message logged by game code (wish_sdk::info!, warn! or error!), with the system's name.",
    ),
    (
        "script_missing_export",
        "The module lacks an export every game module has (memory, wish_abi_version, wish_alloc, wish_run). Build it with the SDK, which adds them (wish_sdk::export!).",
    ),
    (
        "script_not_finite",
        "Game code wrote, inserted or sent a float that is NaN or infinite; the value was not stored and the system is turned off. The message names the field. Guard divisions and normalizing zero-length vectors.",
    ),
    (
        "script_out_of_fuel",
        "A system did too much work in one call, most likely an endless loop. It is turned off; the world keeps running. Fuel is counted in instructions, so the same input stops at the same place every time.",
    ),
    (
        "script_out_of_memory",
        "A system asked for more memory than the module may have. Look for a collection that grows every tick; game state belongs in components.",
    ),
    (
        "script_panicked",
        "Game code panicked; the message and location name the cause. The system is turned off until the code is rebuilt.",
    ),
    (
        "script_reload_failed",
        "A running session could not switch to the new game code and keeps the old one. Restart it (sim_stop, sim_start) to use the new code; the message says why.",
    ),
    (
        "script_reloaded",
        "A running session switched to the new game code between two ticks; its state (entities, components, resources) was kept.",
    ),
    (
        "script_sdk_missing",
        "The game SDK the engine compiles game code against was not found. Packaged engines ship it in their 'sdk' folder; for engine development run `wen sdk` or set WISH_SDK.",
    ),
    (
        "script_sources_missing",
        "The project has no game code yet: scripts/src/lib.rs does not exist. Projects made with `wen new` have one.",
    ),
    (
        "script_stack_overflow",
        "Game code overflowed its stack, usually through recursion without a base case.",
    ),
    (
        "script_system_name_taken",
        "A game system has the name of one of the engine's own systems (transform_propagation, or a system of the project's example); it would replace it. Rename the function.",
    ),
    (
        "script_toolchain_mismatch",
        "The game SDK and the compiler come from different versions. They must come from the same engine package; reinstall, or rebuild the SDK pack with `wen sdk`.",
    ),
    (
        "script_toolchain_missing",
        "The compiler for game code could not be run. Packaged engines ship it in their 'toolchain' folder; for engine development WISH_RUSTC can point at a rustc.",
    ),
    (
        "script_trap",
        "Game code stopped with a WebAssembly trap (index out of range, division by zero, unreachable). The location points to the line.",
    ),
    (
        "script_type_changed",
        "A type of the game code changed its fields while a game was running; values were moved field by field (same name and still fitting: kept; new fields: defaults). The message lists what was not kept.",
    ),
    (
        "script_type_mismatch",
        "A game module describes a type differently from the world: an engine type from another SDK version, or two game types with the same name. Rebuild with this engine's SDK, or rename one.",
    ),
    (
        "script_unknown_type",
        "A game system uses a component, resource or event the world does not have. The engine installs a module's types when it loads it; this usually means the module and the world do not belong together.",
    ),
    (
        "script_value_size",
        "A command gave a value whose size does not match the component; the command was skipped. The module was built with a different SDK: rebuild it (script_build).",
    ),
    (
        "script_write_failed",
        "A game system wrote a value the engine cannot accept (an invalid enum or bool byte, or a text handle from elsewhere). Values written through a query or ResMut are checked like any other input.",
    ),
    (
        "session_required",
        "Several sessions are open, so the call must say which one.",
    ),
    (
        "session_started",
        "A play session was started (a log entry, not a problem).",
    ),
    (
        "session_stopped",
        "A play session was closed (a log entry, not a problem).",
    ),
    (
        "shader_entry_point_missing",
        "A shader file that replaces an engine shader is valid WGSL but lacks a function the engine builds a pipeline from (the message names it). The file was probably copied from an earlier engine version: start again from the current engine shader and carry the changes over.",
    ),
    (
        "shader_import_cycle",
        "Shader modules import each other in a circle. Move what both need into a third module.",
    ),
    (
        "shader_import_syntax",
        "An #import line is malformed. Write one module name after #import.",
    ),
    (
        "shader_import_unknown",
        "A shader imports a module that does not exist. The hint gives the closest name.",
    ),
    (
        "shader_syntax",
        "A shader has a WGSL syntax error at the line and column given.",
    ),
    (
        "shader_validation",
        "A shader is valid WGSL syntax but breaks a rule (a type mismatch, a wrong binding).",
    ),
    (
        "shadows_out_of_range",
        "RenderSettings.shadow_cascades is 2 or more, so shadows are drawn only up to shadow_distance in front of the camera (depth along the direction it looks), and nothing that is drawn can cast a shadow into that part of the scene: the picture has no shadows and no shadow map is drawn. Raise shadow_distance, or set shadow_cascades to 1 (one map covers the whole scene). To see where shadows end in any picture, use capture_frame with view shadow_cascades: gray has no shadow.",
    ),
    (
        "sheet_file_too_large",
        "A *.sheet.json file is larger than 256 KiB and was not read. A sheet holds its picture's path, how it is cut (a grid, or rectangles) and its clips; use a grid instead of listing many frames, or split the clips over several sheets of the same picture.",
    ),
    (
        "sheet_frames_outside_picture",
        "Check warning: some frames of a sprite sheet reach past the edge of its picture (the message says how many and which is first); the part outside shows nothing. Check the grid's frame size, columns and rows (or the rectangles) against the picture: frames are cut from its top left corner, left to right then down.",
    ),
    (
        "sheet_invalid",
        "A sprite sheet does not say something it must, or says it twice: no \"image\", no way to cut the picture (\"grid\" {frame, columns, rows} or \"frames\" [[x, y, w, h], ...]), both at once, a clip without frames, too many frames or clips, or a clip name that cannot be one. The message names which and where.",
    ),
    (
        "sheet_not_loaded",
        "Render warning: a SpriteAnimator names a sprite sheet that could not be used (it does not exist, is not a *.sheet.json, or has a mistake: the message says which). The sprite shows its own Sprite.image and rect until the sheet is fixed; check lists everything wrong with the project's sheets.",
    ),
    (
        "sight_occluders_cut_short",
        "The world has more than 65,536 wall segments for line of sight; the rest were left out, so some walls do not block sight or 2D light. Use fewer occluders; tilemap walls cost little (solid cells merge into boxes).",
    ),
    (
        "snapshot_exists",
        "A snapshot with that label already exists; snapshots are not overwritten.",
    ),
    (
        "snapshot_mismatch",
        "A snapshot does not fit this app (different systems or types).",
    ),
    (
        "spawn_target_missing",
        "A deferred command refers to an entity that no longer exists.",
    ),
    (
        "sprite_clip_not_found",
        "A SpriteAnimator plays a clip its sheet does not have: check says so where the scene writes it, a picture shows the sheet's first frame, and in a session the animator does not move. The hint lists the sheet's clips; asset_info on the sheet shows them with their frames.",
    ),
    (
        "sprite_frame_not_found",
        "Render warning: a SpriteAnimator without a clip shows the sheet's own frame number SpriteAnimator.frame, and the sheet has fewer frames; its last frame is shown. Frames are numbered from 0, left to right then down in a grid.",
    ),
    (
        "sprites_cut_short",
        "Render warning: a picture had more than a million sprite rectangles and particles (a nine-slice sprite is up to nine); the first million, in entity order, were drawn. Show fewer at once.",
    ),
    (
        "stale_revision",
        "The scene changed since the base_revision the edit was prepared against. Read it again and redo the change on top of the current revision.",
    ),
    (
        "sun_disc_without_light",
        "Sky.sun_size_deg was changed from its default to ask for a sun disc, and the scene has no DirectionalLight, so none is drawn: the disc sits where that light shines from. Add the light, or set sun_size_deg to 0. At the default size a scene without a sun simply has no disc and nothing is reported; an orthographic camera never shows one.",
    ),
    (
        "system_failed",
        "A system returned an error, was switched off, and the world went on without it. The message says why.",
    ),
    (
        "system_order_cycle",
        "System ordering constraints form a circle (a before b before a).",
    ),
    (
        "template_kind_mismatch",
        "wen new was given a --kind and a --template of the other kind (a 2D template for a 3D project, or the other way round). The hint names the template's kind.",
    ),
    (
        "test_not_found",
        "test_run was given a file that is not one of the project's tests (tests/*.test.json); the hint lists them.",
    ),
    (
        "text_unavailable",
        "A text value could not be stored or found.",
    ),
    (
        "texture_not_loaded",
        "Render warning: a texture a material names could not be drawn (the file is missing, is not a PNG, is damaged, is larger than 4096 pixels a side, or the scene's material textures together pass the budget of 2^28 pixels). The message names the file and the texture slots that wanted it; those slots are drawn as if they were empty and the rest of the picture is unharmed.",
    ),
    (
        "texture_not_png",
        "Check warning: a texture field of a material file names a file that is not a PNG. Textures outside models are read from PNG files only (models bring their own PNG or JPEG textures), so it will not be drawn. Convert the picture to an 8-bit PNG.",
    ),
    (
        "texture_too_large_for_gpu",
        "A texture is larger than this GPU accepts per side. Scale it down in the modelling tool.",
    ),
    (
        "tile_not_in_tileset",
        "A tilemap holds a tile number its tileset does not have (or a tileset gives properties to a frame its sheet lacks); such a cell draws nothing. Paint tiles the tileset has (asset_info on it lists them), or make the tileset larger. A warning of check.",
    ),
    (
        "tilemap_file_invalid",
        "A tilemap file was changed on disk (by hand) and no longer reads; tilemap_edit and edit_history refuse to change it, so a half-finished edit is never overwritten. Fix the file (check shows where) or restore it from a checkpoint.",
    ),
    (
        "tilemap_file_too_large",
        "A tilemap file is larger than 4 MiB and is not read. Split the map into several tilemaps, each its own entity with a Tilemap.",
    ),
    (
        "tilemap_invalid",
        "A tilemap file (or asset_create's tilemap) says something a tilemap cannot: no tileset, a layer without a name or with a name twice, a chunk key that is not \"x,y\" in range. The message and the JSON pointer say where; the hint says how to write it.",
    ),
    (
        "tilemap_not_found",
        "tilemap_edit or edit_history names a tilemap file that does not exist. asset_list with kind tilemap lists the project's maps; asset_create with a .tilemap.json path makes one.",
    ),
    (
        "tilemap_not_loaded",
        "Render warning: a Tilemap names a map that could not be drawn (missing, not a *.tilemap.json, a mistake in it, or its tileset, the tileset's sheet or its picture could not be read: the message says which). Nothing is drawn for it; check lists everything wrong with the project's tilemaps and tilesets.",
    ),
    (
        "tilemap_too_large",
        "A tilemap would have more than 4096 chunks of 16 x 16 cells (a million cells) or more than 16 layers. Split the map into several tilemaps (each its own entity with a Tilemap), or erase what is not needed.",
    ),
    (
        "tileset_file_too_large",
        "A tileset file is larger than 256 KiB and is not read. Give properties only to tiles that have some, or split the tiles over two tilesets.",
    ),
    (
        "tileset_invalid",
        "A tileset file (or asset_create's tileset) says something a tileset cannot: no picture or sheet, a picture without a grid, a tile number that is not one, a collision shape on a tile that is not solid or that has no area. The message and the JSON pointer say where; the hint says how to write it.",
    ),
    (
        "too_many_checkpoints",
        "The project keeps at most 200 checkpoints. Delete old ones.",
    ),
    (
        "too_many_entities",
        "The world holds the most entities it can.",
    ),
    (
        "too_many_lights",
        "The scene has more than 1024 point and spot lights; only the first 1024 (in entity order) are drawn. Remove lights that add little or merge them.",
    ),
    (
        "too_many_sessions",
        "The most sessions are already open. Close one with sim_stop.",
    ),
    (
        "too_many_snapshots",
        "A session holds the most snapshots it may (each is a whole world). Restore or fork from one you have, or start a new session.",
    ),
    (
        "too_many_tests",
        "One test_run runs at most 200 tests; run a part with paths.",
    ),
    (
        "too_many_texts",
        "The world holds the most distinct texts it can.",
    ),
    (
        "transform_not_finite",
        "An entity's Transform holds NaN or an infinite value, usually from a division by zero. Find what wrote it.",
    ),
    (
        "type_mismatch",
        "A value has the wrong JSON type for its field (for example a string where a number is expected).",
    ),
    (
        "type_name_conflict",
        "Two different types were registered under the same name.",
    ),
    (
        "ui_atlas_full",
        "One frame's interface needs more glyphs and pictures than fit in its 4096x4096 atlas, so some were not drawn. Use smaller pictures or less text at very large sizes.",
    ),
    (
        "ui_font_missing",
        "A UiText names a font file the project does not have (or cannot read), so the engine's font is used. Fix the path (a .ttf or .otf in the project) or leave font empty.",
    ),
    (
        "ui_image_not_loaded",
        "A UiImage or a Sprite names a picture that could not be loaded (missing, outside the project, or not a readable PNG); the box shows only its background and the sprite is not drawn. Fix the path or the file.",
    ),
    (
        "ui_without_node",
        "An entity has UiText, UiImage or UiButton but no UiNode, so it is not part of the interface and is not shown. Add a UiNode to the same entity: it is the box that places it.",
    ),
    (
        "unknown_action",
        "sim_step's input names an action the project does not have. The candidates are the actions in input.json.",
    ),
    (
        "unknown_component",
        "No component type has that name. The hint gives the closest; schema_get lists them.",
    ),
    (
        "unknown_component_id",
        "A component id does not belong to this world.",
    ),
    (
        "unknown_diagnostic",
        "There is no documentation page for that diagnostic code. Its own hint says how to fix it; docs_search finds related pages.",
    ),
    (
        "unknown_entity",
        "No entity has that id. The hint gives the closest id.",
    ),
    (
        "unknown_event",
        "No event type has that name. schema_get lists the event types.",
    ),
    (
        "unknown_event_id",
        "An event id that this world never issued was used. Look event types up by name in the world you are working with.",
    ),
    ("unknown_example", "No built-in example has that name."),
    (
        "unknown_field",
        "A component or resource has no field with that name. The hint gives the closest; schema_get shows the fields.",
    ),
    (
        "unknown_key",
        "A document has a key the format does not know. The hint gives the closest valid key.",
    ),
    (
        "unknown_language",
        "The editor has no interface language with that code (wen edit --lang). The hint lists the codes.",
    ),
    (
        "unknown_look",
        "edit_apply's apply_look names a look the engine does not have. The hint gives the closest; the candidates are plain, natural and vivid.",
    ),
    (
        "unknown_parameter",
        "A tool has no parameter with that name. The hint gives the closest; the candidates list them all.",
    ),
    (
        "unknown_parent",
        "The parent an entity names does not exist in the scene.",
    ),
    (
        "unknown_player",
        "A player id the session does not have (ids are 0 to the session's player count - 1). Start the session with more players (sim_start players), or a test with \"players\": N.",
    ),
    (
        "unknown_prefab_entity",
        "The prefab has no entity with that id.",
    ),
    ("unknown_resource", "No resource type has that name."),
    (
        "unknown_resource_id",
        "A resource id that this world never issued was used. Look resource types up by name in the world you are working with.",
    ),
    (
        "unknown_resource_uri",
        "No MCP resource has that address. resources/list and resources/templates/list show what exists, starting with wish://docs/guide.",
    ),
    (
        "unknown_session",
        "No open session has that id. The candidates list the open ones.",
    ),
    (
        "unknown_snapshot",
        "The session has no snapshot with that label.",
    ),
    (
        "unknown_system",
        "No system has that name. sim_start lists the session's systems.",
    ),
    (
        "unknown_tile",
        "A tilemap_edit operation paints a tile number the map's tileset does not have; nothing was changed. Use a tile the tileset has (asset_info on the tileset lists them), or -1 to erase.",
    ),
    (
        "unknown_tool",
        "No tool has that name. The candidates list them all.",
    ),
    (
        "unknown_type",
        "No component, resource or event type has that name.",
    ),
    (
        "unknown_variant",
        "An enum field got a value that is not one of its variants. The hint lists them.",
    ),
    (
        "unknown_view",
        "No view has that name: use lit, ids, depth, normals, wireframe, colliders, lights or shadow_cascades.",
    ),
    (
        "unsupported_animation",
        "A model animates something that is not supported (morph target weights); that part does not move. Bake it into a skeletal animation or re-export without morph targets.",
    ),
    (
        "unsupported_extension",
        "A model uses (or requires) a glTF extension the engine does not read. Required ones stop the import; others are drawn without the extension.",
    ),
    (
        "unsupported_format",
        "A document has a format number this engine does not read: newer than the engine (update the engine), or one no engine wrote. An older scene or prefab format the engine knows is read and reported as old_format instead.",
    ),
    (
        "unsupported_primitive_mode",
        "A model's primitive draws points or lines; only triangles are imported.",
    ),
    (
        "unsupported_texcoord_set",
        "A texture uses a second UV set; only the first (TEXCOORD_0) is supported.",
    ),
    (
        "until_not_reached",
        "sim_step's until condition did not become true within max_frames ticks.",
    ),
    (
        "value_out_of_range",
        "A number is outside its field's allowed range (shown with the unit). schema_get shows the ranges.",
    ),
    (
        "warnings_cut_short",
        "Render warning: one picture had more warnings about materials and textures than are listed (32); this one counts the rest. Fix the listed ones first: one missing file often stands behind many.",
    ),
    (
        "window_failed",
        "A window could not be opened (no desktop session). Use capture_frame or wen capture instead.",
    ),
    (
        "wrong_length",
        "A list has the wrong number of items (a Vec3 needs 3, a Quat or Color 4).",
    ),
];

/// The page of a diagnostic code.
pub fn diagnostic_page(code: &str) -> Option<String> {
    DIAGNOSTICS
        .binary_search_by(|(known, _)| (*known).cmp(code))
        .ok()
        .and_then(|index| DIAGNOSTICS.get(index))
        .map(|(code, text)| {
            format!(
                "# Diagnostic `{code}`\n\n{text}\n\nEvery diagnostic also carries a `hint` that says how to fix that particular case, and often `candidates` (the valid values) and a `location` (file, line, JSON path, or the failing operation's index).\n"
            )
        })
}

/// The schema types that say what a kind of file holds: they are in no
/// world, so not in a registry, but `schema_get`, the type pages and the
/// search know them like any other type (kind `file`).
pub fn file_types() -> Vec<wish_schema::TypeSchema> {
    vec![wish_asset::material_schema()]
}

fn type_kind<'a>(registry: &'a Registry, name: &str) -> Option<(&'static str, &'a TypeInfo)> {
    registry
        .components()
        .find(|(_, info)| info.name() == name)
        .map(|(_, info)| ("component", info))
        .or_else(|| {
            registry
                .resources()
                .find(|(_, info)| info.name() == name)
                .map(|(_, info)| ("resource", info))
        })
        .or_else(|| {
            registry
                .events()
                .find(|(_, info)| info.name() == name)
                .map(|(_, info)| ("event", info))
        })
}

/// The page of a component, resource or event type, or of the type a kind
/// of file holds, made from its schema.
pub fn type_page(registry: &Registry, name: &str) -> Option<String> {
    let (kind, type_schema, managed) = match type_kind(registry, name) {
        Some((kind, info)) => (kind, info.schema().clone(), info.is_managed()),
        None => (
            "file",
            file_types()
                .into_iter()
                .find(|schema| schema.name == name)?,
            false,
        ),
    };
    let schema = wish_schema::json_schema::json_schema(&type_schema);
    let mut page = format!("# {name} ({kind})\n\n{}\n", type_schema.doc.trim());
    if managed {
        page.push_str("\nThe engine keeps this one itself: it cannot be written.\n");
    }
    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        page.push_str(
            "\n| Field | Type | Unit | Range | Default | Meaning |\n|---|---|---|---|---|---|\n",
        );
        for (field, spec) in properties {
            let text = |key: &str| {
                spec.get(key).map_or(String::new(), |v| {
                    v.to_string().trim_matches('"').to_owned()
                })
            };
            let kind = if spec.get("x-wish-type").is_some() {
                text("x-wish-type")
            } else {
                text("type")
            };
            let range = match (spec.get("minimum"), spec.get("maximum")) {
                (Some(low), Some(high)) => format!("{low} to {high}"),
                _ => spec
                    .get("enum")
                    .map_or(String::new(), |values| values.to_string()),
            };
            let doc = text("description").replace('\n', " ");
            page.push_str(&format!(
                "| {field} | {kind} | {} | {range} | {} | {doc} |\n",
                text("x-unit"),
                text("default")
            ));
        }
    }
    if kind == "file" {
        page.push_str(
            "\nIn its file: `{ \"format\": 1, \"material\": { ... } }` with any of these fields; the others keep their defaults.\n",
        );
    } else {
        page.push_str(&format!(
            "\nIn a scene file: `\"{name}\": {{ ... }}` with any of these fields; the others keep their defaults.\n"
        ));
    }
    Some(page)
}

/// The engine guide: short enough to read whole at the start of a task.
pub fn guide() -> String {
    let mut tools = String::new();
    for tool in catalog() {
        let first = tool
            .description
            .split(". ")
            .next()
            .unwrap_or(tool.description);
        tools.push_str(&format!(
            "- `{}`: {}.\n",
            tool.name,
            first.trim_end_matches('.')
        ));
    }
    format!(
        "# Wish Engine: guide for agents\n\
\n\
Wish Engine is a deterministic 3D engine you drive with tools. Everything a person can do by hand you can do with\n\
these tools, and the same edits land in one shared history (`actor` says who made each).\n\
\n\
## The loop\n\
\n\
1. `project_info`: where you are and how many problems there are.\n\
2. `schema_get` with `search`: the components you need, their fields, units and ranges.\n\
3. `edit_apply`: change the scene, many operations at once, all or nothing (`dry_run` to check first).\n\
4. `sim_start` + `sim_step`: run it (deterministic: same seed, same result; `until` stops on a condition).\n\
5. `describe_view` / `capture_frame`: check what is on screen; `entity_query` / `entity_get`: check values.\n\
6. `check` before you finish.\n\
\n\
## Tools\n\
\n\
{tools}\n\
## Scenes\n\
\n\
A scene is a file of entities, each an id and components: `{{ \"op\": \"spawn\", \"id\": \"crate\", \"components\": {{ \"Primitive\": {{ \"shape\": \"Cube\" }}, \"Transform\": {{ \"translation\": [0, 0.5, 0] }} }} }}`.\n\
Component values are patches on defaults: give only the fields you change. `parent` puts an entity under another; transforms combine down the tree.\n\
Prefabs are reusable parts: the `instantiate` operation places one (`\"prefab\": \"prefabs/crate.prefab.json\"`, with the instance's own `components`); look inside a prefab with `scene_get` and `\"scene\": \"prefabs/crate.prefab.json\"`.\n\
Something to see needs a `Camera` (with a `Transform`), a `DirectionalLight` for light, and `Primitive` or `Model` entities to draw.\n\
\n\
## Game code\n\
\n\
Behaviour is Rust in `scripts/src/lib.rs` (edit the file directly), compiled by `script_build`:\n\
`use wish_sdk::prelude::*;`, data as `#[component]` / `#[resource]` / `#[event]` structs (every field with a `///` comment),\n\
logic as `#[system]` functions taking `Query<(&mut Transform, &Velocity), With<Player>>`, `Res<Time>`, `ResMut<Score>`,\n\
`Commands` (spawn/insert/despawn), `EventReader<E>` / `EventWriter<E>`; order with `#[system(after = \"other\")]`; end with\n\
`wish_sdk::export! {{ components: [...], resources: [...], systems: [...] }}`.\n\
Compile errors come back with file and line; a failed build keeps the old code. A good build switches running sessions to\n\
the new code between two ticks without losing their state, and its types are usable in `edit_apply` at once.\n\
All state lives in components and resources (the module restarts every tick). No clock, files or network: time is\n\
`Res<Time>`, randomness `random_f32()`; a panic or an endless loop turns that system off and is reported with its line.\n\
Tests: `tests/*.test.json` (`{{ \"format\": 1, \"scene\": ..., \"seed\": 1, \"steps\": [ {{ \"input\": {{...}}, \"frames\": 60 }}, {{ \"expect\": \"Score.points >= 1\" }} ] }}`); `test_run` runs them and explains the first failing step with the tick and the values read.
\nReplays: `replay` records a session from tick 0, saves it, and `run` plays it back checking the world after every tick; run them after changing the code to see whether the game still plays the same.
\nInput is actions (`input.json`: `move`, `jump`, ...); systems take `actions: Actions`. Play with `sim_step` `input`: `{{ \"move\": [0, 1], \"jump\": true }}` is held for the ticks of that call.\n\
\n\
## Game systems\n\
\n\
Physics (`Collider`, `RigidBody`, `CharacterController`, `CollisionStarted`, `physics_query`), sound (`AudioSource`,\n\
`PlaySound`; `logs_get` kinds [\"audio\"]), lights (`PointLight`, `SpotLight`), animation (`AnimationPlayer`), the in-game\n\
interface (`UiNode`, `UiText`, `UiButton`; `describe_view` lists it; `sim_step` input `pointer` and `click`), particles\n\
(`ParticleEmitter`). 2D: orthographic `Camera`, `Sprite`, sprite sheets (`*.sheet.json`) played by `SpriteAnimator`,\n\
`PhysicsSettings` `profile` `TopDown2D` / `SideView2D`. Players: `sim_start` `players`, `sim_step` `players`, `PlayerActions`.\n\
`asset_create` makes placeholder sounds and pictures, sprite sheets, and material files (`*.mat.json`: textures, tiling,\n\
cut-outs) that a `Material` names in its `file`. `capture_frame` and `describe_view` give `stats` (draw calls, triangles,\n\
lights, effects). The look is data (`Ambient`, `Sky`, `Fog`, `Bloom`, `ColorGrading`, `RenderSettings`); a scene starts\n\
with the look `natural`, and `edit_apply` `apply_look` sets a whole look (`plain` is none of it).\n\
**`wish://docs/recipes` has working pieces of games** (a platformer character, a follow camera, pickups with a score,\n\
checkpoints, moving platforms, a win screen and a button, a 2D scene, a texture on a box, tests): start from them.\n\
\n\
## Rules\n\
\n\
- Units: meters, seconds, kilograms; radians in data, but `Transform` also takes `\"euler_deg\": [x, y, z]` in edits.\n\
- Coordinates: right-handed, +Y up; cameras and lights look along their -Z axis.\n\
- Conditions (`where`, `until`): `Transform.translation.y < 0 && has(Camera)`, `distance(Transform.translation, [0,0,0]) < 5`, `matches(id, \"crate_*\")`, resources by name (`Stats.bounces > 3`), `count(Coin) == 0` (entities with a component, in the whole world). A comparison with something missing is unknown and stays unknown under `!`.\n\
- Errors are answers with `hint` (how to fix), often `candidates` and `location`. Read the hint before retrying.\n\
- A person may edit at the same time: mind `changed_by_others`, and use `base_revision` for changes prepared earlier.\n\
- Sessions never change files. To keep something found in a session, make the change with `edit_apply`.\n\
\n\
More: `wish://docs/recipes` for working pieces of games, `wish://docs/sdk` for the game code SDK, `wish://docs/components/<Name>` for any type, `wish://docs/diagnostics/<code>` for any error code, `docs_search` to search.\n"
    )
}

/// One search hit.
struct Hit {
    score: usize,
    uri: String,
    title: String,
    snippet: String,
}

/// Searches the guide, the type pages and the diagnostic pages.
/// A recipe page a package brings (M10a WP5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageRecipe {
    /// Its address: `wish://docs/packages/<id>/<file>`.
    pub uri: String,
    /// Its title (its first `# ` line, else its file name).
    pub title: String,
    /// The package's name.
    pub package: String,
    /// Its text.
    pub text: String,
}

/// The recipes of the project's and the person's packages, in that order
/// (a package in both: the project's).
pub fn package_recipes(project: &std::path::Path) -> Vec<PackageRecipe> {
    let mut found: Vec<PackageRecipe> = Vec::new();
    for scope in [
        crate::packages::Scope::Project,
        crate::packages::Scope::User,
    ] {
        for package in crate::packages::installed(scope, project) {
            let Some(manifest) = package.manifest else {
                continue;
            };
            for file in manifest.contents.get("recipes").into_iter().flatten() {
                let uri = format!("wish://docs/packages/{}/{file}", manifest.id);
                if found.iter().any(|recipe| recipe.uri == uri) {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(package.folder.join(file)) else {
                    continue;
                };
                let title = text
                    .lines()
                    .find_map(|line| line.strip_prefix("# "))
                    .map_or_else(|| file.clone(), str::to_owned);
                found.push(PackageRecipe {
                    uri,
                    title,
                    package: manifest.name.clone(),
                    text,
                });
            }
        }
    }
    found
}

/// Searches the documentation: the guide, the SDK, the recipes (the
/// engine's and `extra`, the packages'), the types and the diagnostics.
pub fn search(registry: &Registry, query: &str, limit: usize) -> Vec<Value> {
    search_with(registry, query, limit, &[])
}

/// [`search`] with the recipes packages bring.
pub fn search_with(
    registry: &Registry,
    query: &str,
    limit: usize,
    extra: &[PackageRecipe],
) -> Vec<Value> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut pages: Vec<(String, String, String)> = Vec::new();
    for section in guide().split("\n## ") {
        let title = section
            .lines()
            .next()
            .unwrap_or_default()
            .trim_start_matches("# ")
            .to_owned();
        pages.push((
            "wish://docs/guide".to_owned(),
            format!("Guide: {title}"),
            section.to_owned(),
        ));
    }
    for section in sdk_page().split(
        "
## ",
    ) {
        let title = section
            .lines()
            .next()
            .unwrap_or_default()
            .trim_start_matches("# ")
            .to_owned();
        pages.push((
            "wish://docs/sdk".to_owned(),
            format!("SDK: {title}"),
            section.to_owned(),
        ));
    }
    for section in recipes_page().split("\n## ").skip(1) {
        let title = section.lines().next().unwrap_or_default().to_owned();
        pages.push((
            "wish://docs/recipes".to_owned(),
            format!("Recipe: {title}"),
            section.to_owned(),
        ));
    }
    for recipe in extra {
        pages.push((
            recipe.uri.clone(),
            format!("Recipe ({}): {}", recipe.package, recipe.title),
            recipe.text.clone(),
        ));
    }
    let names: Vec<String> = registry
        .components()
        .map(|(_, info)| info.name().to_owned())
        .chain(registry.resources().map(|(_, info)| info.name().to_owned()))
        .chain(registry.events().map(|(_, info)| info.name().to_owned()))
        .chain(file_types().into_iter().map(|schema| schema.name))
        .collect();
    for name in names {
        if let Some(page) = type_page(registry, &name) {
            pages.push((format!("wish://docs/components/{name}"), name.clone(), page));
        }
    }
    for (code, text) in DIAGNOSTICS {
        pages.push((
            format!("wish://docs/diagnostics/{code}"),
            format!("Diagnostic {code}"),
            format!("{code} {text}"),
        ));
    }
    let mut hits: Vec<Hit> = pages
        .into_iter()
        .filter_map(|(uri, title, text)| {
            let lower = text.to_lowercase();
            if !words.iter().all(|word| lower.contains(word.as_str())) {
                return None;
            }
            let title_lower = title.to_lowercase();
            let score = words
                .iter()
                .map(|word| {
                    lower.matches(word.as_str()).count()
                        + 10 * usize::from(title_lower.contains(word.as_str()))
                })
                .sum();
            let at = lower.find(words[0].as_str()).unwrap_or(0);
            let start = text[..at]
                .char_indices()
                .rev()
                .nth(60)
                .map_or(0, |(i, _)| i);
            let snippet: String = text[start..]
                .chars()
                .take(200)
                .collect::<String>()
                .replace('\n', " ");
            Some(Hit {
                score,
                uri,
                title,
                snippet,
            })
        })
        .collect();
    hits.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.uri.cmp(&b.uri)));
    hits.into_iter()
        .take(limit)
        .map(|hit| json!({ "uri": hit.uri, "title": hit.title, "snippet": hit.snippet.trim() }))
        .collect()
}

/// Working pieces of games (`wish://docs/recipes`): each a complete game
/// module and the scene operations that go with it, compiled and applied by
/// `tests/recipes.rs` so that they stay true (M7 WP4).
pub fn recipes_page() -> &'static str {
    include_str!("recipes.md")
}

/// The game code SDK reference (`wish://docs/sdk`): everything a system in
/// `scripts/src/lib.rs` can use, with the shapes to copy.
pub fn sdk_page() -> String {
    SDK_PAGE.to_owned()
}

const SDK_PAGE: &str = r#"# Game code SDK (wish_sdk)

Game code is Rust in `scripts/src/lib.rs`, compiled by `script_build` (no Cargo, no other files needed).
Start the file with `use wish_sdk::prelude::*;` and end it with `wish_sdk::export! { ... }`.

## Data

```rust
/// A coin the player can pick up.            // every type and field needs a /// comment
#[component]                                  // data on entities
pub struct Coin {
    /// Points it is worth.
    #[schema(range = 1..=100, default = 1)]   // also: unit = "m/s", alias = "..."
    pub value: u32,
}

/// Points so far.
#[resource]                                   // one value per world
pub struct Score { /// The total.
    pub points: u32 }

/// A coin was picked up.
#[event]                                      // a message between systems
pub struct Collected { /// Its points.
    pub value: u32 }
```

Field types: `bool`, integers, `f32`, `f64`, `Vec2`, `Vec3`, `Vec4`, `Quat`, `Mat4`, `Color`, `Entity`, `Text`,
`AssetRef`, fixed arrays `[T; N]`, and structs / fieldless enums marked `#[schema]`. No `String`, `Vec`, `Option`:
data must have a fixed size. The engine's own types are there too: `Transform`, `GlobalTransform` (read only),
`Camera`, `DirectionalLight`, `Primitive`, `Material`, `Model`, `Sky`, `Ambient`, `Exposure`, `Fog`, `Bloom`,
`ColorGrading`, `RenderSettings`, `Time`.

## Systems

```rust
#[system]                                     // or #[system(after = "move_player", before = "score")]
fn collect(
    players: Query<&Transform, With<Player>>,                 // read
    mut coins: Query<(Entity, &mut Transform, &Coin), Without<Player>>,   // &mut = write
    time: Res<Time>,                                           // a resource, read
    mut score: ResMut<Score>,                                  // a resource, write
    actions: Actions,                                          // the player's input
    mut commands: Commands,                                    // spawn / despawn / insert / remove
    events: EventReader<Collected>,                            // events since this system last ran
    mut sent: EventWriter<Collected>,                          // send events
) { ... }
```

- `Query`: `iter()`, `iter_mut()`, `get(entity)`, `get_mut(entity)`, `single()`, `single_mut()`, `len()`,
  `is_empty()`, `entities()`. Items are tuples in the order asked. Filters: `With<T>`, `Without<T>`, tuples.
- Changes through `&mut` and `ResMut` are written back when the system ends; only real changes count.
- `Commands`: `commands.spawn().insert(Coin { value: 1 }).insert(Transform::from_translation(p))`,
  `commands.despawn(entity)`, `commands.insert(entity, value)`, `commands.remove::<Coin>(entity)`,
  `commands.set_parent(child, parent)`. They happen after the system, in order.
- `Actions` (names from input.json): `held("jump")`, `pressed("jump")` (this tick only), `released("jump")`,
  `axis("throttle")` (-1..1), `axis2("move")` (`Vec2`: x right, y forward). A name not in input.json reads as rest.
- Several players (a session started with `sim_start` `players`, a test with `"players": N`): `players: PlayerActions`
  has `count()`, `ids()` (0 to count - 1) and `of(id)`, which reads like `Actions` (`players.of(1).axis2("move")`).
  `Actions` is player 0. Keep whose an entity is in your own component (`player: PlayerId`, a `u32`). Alone, there is
  one player, 0, so the same code runs single-player.
- `Res<Time>`: `tick`, `delta` (seconds per tick, 1/60), `elapsed`.
- Randomness: `random_f32()` (0..1), `random_range(a, b)`, `random_u64()`: the system's own seeded stream.
- Logging: `wish_sdk::info!("x = {}", x)`, `warn!`, `error!` (shown in logs_get, kind "script").
- A resource the world does not have yet starts at its default.
- A command whose target is gone (an entity despawned earlier) is skipped with a warning in logs_get; despawning
  twice is fine. NaN or infinite floats are never stored: the system stops with `script_not_finite`.

## Export

```rust
wish_sdk::export! {
    components: [Coin, Player],      // types scenes may use (those in queries are found by themselves)
    resources: [Score],
    events: [Collected],
    systems: [move_player, collect, score],
}
```

## Physics

Physics runs after the game's systems every tick. An entity with a `Collider` takes part; with a `RigidBody`
it moves. Physics writes `Transform` and `BodyVelocity` back; writing them yourself teleports or sets the speed.

```rust
// Kick a crate: the event is applied at the next physics step.
sent.send(BodyImpulse { entity: crate_, linear: Vec3::new(0.0, 5000.0, 0.0), angular: Vec3::ZERO });
// Touches: EventReader<CollisionStarted> / <CollisionEnded> give { a, b, sensor }.
```

- Shapes: `ColliderShape::{Box, Sphere, Capsule, Cylinder}`, sized by `size` times the entity's scale.
- `RigidBody { kind: BodyKind::Dynamic | Kinematic | Fixed, gravity_scale, lock_translation, lock_rotation, .. }`.
- Characters: a Kinematic body with `CharacterController`; each tick write `CharacterMotion.velocity` (m/s,
  gravity included) and read `CharacterState { grounded, blocked, moved }`:
  `vy = if state.grounded { if actions.pressed("jump") { 5.0 } else { -1.0 } } else { vy - 9.81 * time.delta }`.
- `BodyForce` pushes every tick while present; `PhysicsSettings` holds gravity and substeps.
- 2D: set `PhysicsSettings.profile` to `TopDown2D` (seen from above: no gravity, no ground, characters slide
  along every wall, `grounded` stays false) or `SideView2D` (a platformer: gravity along -Y, characters stand and
  jump). Everything stays in the X-Y plane and turns only about Z, colliders meet whatever their z (an entity's own
  z is kept), `Sphere` is a circle and `Box` a rectangle (`size.z` unused), and rays and overlaps are in the plane.
- Ray casts: `raycast(from, dir, max) -> Option<RayHit { entity, point, normal, distance }>`, or
  `wish_sdk::physics::cast(Ray { from, dir, max, mask, ignore: Some(me) })`. They see the physics world as it was at
  the start of the tick (the same for every system); each costs a fixed amount of fuel.
- A game may still declare its own `Velocity` for things it moves itself; physics uses `BodyVelocity`.

## Sound

A sound is an entity with an `AudioSource` (clip: a `.wav` or `.ogg` path in the project, `playing`, `looping`,
`spatial`, `volume`, `pitch`, `min_distance`, `max_distance`) and an `AudioListener` somewhere (usually the camera).
A one-off sound is an event: `sent.send(PlaySound { clip: asset("assets/sounds/jump.wav"), bus: AudioBus::Sfx, volume: 1.0, pitch: 1.0,
spatial: true, position })` (`asset(path)` makes the `AssetRef` of a project file; `asset_create` makes placeholder
`.wav` files). The engine writes `AudioPlayback { position, gain, pan, started }` and sends
`SoundStarted` / `SoundEnded` events. Nothing is played in a session: an agent reads what played with
`logs_get kinds: ["audio"]` ("started on 'door': -12.0 dB at the listener, right 100%").

## Animation

A glTF `Model` with clips plays one through an `AnimationPlayer { clip, time, speed, looping, playing }` on the
same entity (clip: the name in the file; empty plays the first). The engine moves `time` on every tick; a clip
that does not loop stops at its end, sets `playing` to false and sends `AnimationFinished { entity }`. To play
another clip, write `clip` and set `time` to 0.

## Interface

The in-game interface is made of entities. An entity with a `UiNode` is a box; its children (in the entity
hierarchy) with a `UiNode` are laid out inside it in a column (or a `direction: Row`), like CSS flexbox:
`width`/`height` (`{ "unit": "Px" | "Percent" | "Auto", "value": n }`), `padding`/`margin` (top, right,
bottom, left), `gap`, `justify`, `align`, `grow`, `position: Absolute` with `offset` [x, y], `background`,
`visible`, `layer`. A box can show `UiText { text, size, color, align, wrap, font }` (the engine's font is
Roboto; `font` names a .ttf/.otf of the project) or `UiImage { image, tint }` (a PNG), and be a `UiButton`.

Positions are interface pixels on a virtual screen (`UiSettings`, 1280 x 720 by default, (0, 0) at the top
left), scaled to fit any window. Every tick the engine writes each box's place into `UiLayout` and, for buttons,
`UiInteraction { hovered, pressed, clicks, last_click_tick }`; a click sends `ButtonClicked { entity }` (read it
the next tick with `EventReader<ButtonClicked>`). The pointer is the `Pointer` resource.

Game code writes text with `text(...)`: `label.text = text(&format!("Score: {}", score.points));`, and reads
one back with `text_of(handle)`. A tool clicks with `sim_step` input `{ "pointer": [x, y], "click": true }`
and then a step without `click`; `describe_view` lists the interface with its text.

## 2D and particles

A 2D game looks through a `Camera` with `projection: Orthographic` (`ortho_height`: meters shown from top to
bottom) and is made of `Sprite { image, rect, color, pixels_per_unit, anchor, flip_x, flip_y, layer, smooth, size,
slice }` entities standing in their XY plane. `color` is the tint (it multiplies the picture: one white character in many
colours); `size` (picture pixels) draws the sprite at another size, and with `slice` borders [left, top, right, bottom]
it is a nine-slice: the corners keep their size and the rest stretches (panels, frames). A walk cycle comes from a
sprite sheet (`*.sheet.json`: a PNG cut into frames by a `grid` or `frames` rectangles, and named `clips` with their
`frames`, `fps`, `loop`; `asset_create` makes one) and a `SpriteAnimator { sheet, clip, playing, speed, time, frame }`
beside the `Sprite`: the engine moves it every tick and writes the frame shown; game code switches clips with
`animator.play(text(\"walk\"))`, and `describe_view` says each sprite's clip and frame. Sprites are not lit (unless 2D lighting is on, below); higher
`layer` is drawn over lower, then a higher `order` (on `Sprite` and `Tilemap`) within a layer. A `SortingLayer { name,
layer, y_sort }` on an entity of its own names a layer (`describe_view` says `sorting_layer`) and with `y_sort` sorts
its sprites by height, lower in front (top-down games). For 2D physics set `PhysicsSettings.profile` to
`SideView2D` or `TopDown2D` (see Physics): no per-body locks are needed.

Pixel art: a `PixelCamera { resolution, pixels_per_unit, letterbox, bars, snap }` beside the `Camera` draws a game
made for `resolution` pixels (320 x 180 by default, 16 a meter) scaled up by a whole number, with bars around it
(`letterbox`) or more of the world; with `snap` the camera, sprites and tilemaps stand on the game's pixels, so
nothing shimmers. The interface fits inside the game's picture. `Pointer.world` is the point of the XY plane under
the pointer through the first camera (`Pointer.in_world` says whether there is one); `describe_view` with `point`
answers `under_point: { world, sprites }` (front first, clear pixels of a sprite's picture do not count).

Rooms and walls are a tilemap: `asset_create` makes a tileset (`*.tileset.json`: a PNG and a `grid`; `tiles`
{\"8\": {solid: true}}) and a tilemap (`*.tilemap.json`: the tileset and `layers`); a `Tilemap { map, cell_size }`
entity draws it and makes solid tiles walls. Paint with `tilemap_edit` (one change per call; `edit_history` with the map
as scene undoes it).

Walls block sight: solid tiles of colliding layers, and entities with an `Occluder { enabled }` beside their `Collider`
(a door: enabled while closed). Game code asks `can_see(from: Vec2, to: Vec2) -> bool` (`can_see_ignoring(from, to,
entity)` when the target is itself an occluder) and `visible_area(from, radius) -> Vec<Vec2>` (the outline of what
is seen: a circle cut by walls); both see the walls as of the end of the previous tick, and `physics_query sight`
answers the same. Online, `NetVisibility::Sight` sends an entity only to players whose `NetViewer` can see it. 2D
lights: the resource `Lighting2D { enabled, ambient }` turns lighting of sprites and tilemaps on (off by default),
each `Light2D { color, intensity, radius, shadows }` adds light that walls stop; a `Vision2D { enabled, radius,
softness, darkness }` on the player's character darkens everything it cannot see. Sabotage the lights by lowering
`Lighting2D.ambient` or the lights' `intensity`.

A `ParticleEmitter { rate, burst, lifetime, speed, spread_deg, gravity, size_start/end, color_start/end, seed,
start_time, duration, max, playing }` gives off soft round particles in a cone around its entity's +Y axis.
Particles keep no state (their places follow from the time): to start again, write `start_time` with
`Time.elapsed`. The engine writes how many are alive into `ParticleState.alive`; `describe_view` lists sprites
and particles with their boxes.

## Rules

- All state lives in components and resources. The module starts fresh every tick: a `static` keeps nothing.
- No clock, files, threads or network; no third-party crates (only `wish_sdk`).
- A panic or an endless loop turns that system off and is reported with its line; the game keeps running.
- Coordinates: +Y up, the camera looks along -Z, so "forward" is -Z: `Vec3::new(go.x, 0.0, -go.y)`.
- Prove behaviour with a scenario test (tests/*.test.json, test_run) and check determinism with replay.
"#;
