# M7: Wish Engine through MCP Inspector

- Date: 2026-11-16
- Client: MCP Inspector 2.13.0 (CLI mode, `--strict` for tools/list); every step a fresh `wen mcp` over stdio
- Result: **11/11**

| Step | Result | Detail |
|---|---|---|
| tools/list, strict schema portability | ok | 25 tools, every one with input and output schema; no portability problem reported |
| resources/list and resources/read (guide) | ok | 53 resources; guide is 8979 characters |
| prompts/list and prompts/get | ok | prompts: new_scene, reproduce_and_fix |
| project_info | ok | scene scenes/main.scene.json, 32 component types |
| schema_get (search) | ok | finds Collider |
| edit_apply: a falling crate | ok | spawned and read back |
| an error carries code and hint | ok | unknown_component: Did you mean 'Collider'? |
| describe_view and capture_frame | ok | crate visible; PNG 4355 bytes |
| game code: script_build | ok | built |
| test_run: a scenario test of the falling crate and the counter | ok | 2 tests passed |
| asset_create and check | ok | sound made; project passes check |
