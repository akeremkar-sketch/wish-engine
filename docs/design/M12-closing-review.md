# M12 package 12: the independent closing review of 1.0

**Status:** done (20 November 2026).

## 1. Problem

Every M12 package had its own review. None of them looked at the packages
together, at what a person meets on the first day of 1.0, or at the seams
between them: the look lock over materials, blending and cascades; format 2
and the migration over everything that writes a scene; the release archive
over everything the engine now reads from disk. The plan (section 0, item
12) asks for one independent review of the whole short path, with a fix
budget, before the beta.

## 2. Scope

Everything that changed from `eeb760f` (the plan) to the head of
`m9-product`: about 35,000 lines in 166 files under `crates/`, the release
workflow, the public documents. What existed before M12 is in scope only
where M12 code calls it or a 1.0 claim rests on it.

## 3. Method

Three reviewers, each independent of the others and of the authors, each
with one lens and a crate split, read-only (no builds into the shared build
folder; the release `wen` may be run in a scratch folder under
`C:/wish-build/review-tmp` to reproduce a finding):

1. **Render and look**: `wish_render` (passes, pool, look, bloom, cascades,
   materials, blended pass, shaders), the GPU rules of CLAUDE.md, the look
   lock's "Plain is exact", budgets, hostile numbers reaching shaders.
2. **Data, formats and the control plane**: `wish_scene` (format 2,
   migration, recorded defaults), `wish_asset` (material files),
   `wish_control` (tools touched by M12, `migrate`, `engines`, checkpoints,
   documents), `wish_types`, determinism, hostile files, the two-process
   parity rule, invariants 1 to 9.
3. **Release, platforms and the public face**: the `dist` profile, `wen
   package`, `wish_control::archive`/`release`, `.github/workflows/`,
   Linux, `wish_evals` (the scorecard), `README.md`, `docs/STABILITY.md`,
   the guides, `CONTRIBUTING.md`, every public claim against its command
   (ADR-027 rule 4).

Each finding states severity (high: a crash, data loss, a wrong result a
person would trust, a false public claim, a broken invariant; medium: a
real defect with a workaround or a narrow trigger; low: everything else),
the file and line, a concrete failure scenario and a suggested fix.

## 4. Fix budget

- Every high and medium finding is verified by reading the code, then fixed
  with a test that fails before the fix, or rejected with a reason written
  here.
- Low findings are fixed when the fix is small and safe; the others are
  recorded here with the release on the train that takes them.
- One implementer pass and the full gate. A finding whose fix would grow
  the 1.0 scope is recorded, not fixed, and named to the user.

## 5. Test plan

The regression tests of the fixes; then the full gate (fmt, clippy with
`-D warnings`, `cargo test --workspace`, `cargo test --release -p
wish_render`) and CI on Windows and Linux.

## 6. Findings

Twenty-one findings: two high, eight medium, eleven low. The render and look
reviewer found two low findings and nothing else (it recomputed the uniform
layouts, the pass order, the stats and the pool budget by hand and drew
hostile scenes). The data reviewer reproduced seven of its ten with the
release `wen`. Twenty are fixed with a regression test seen failing first,
except where a row says otherwise; one is recorded for the release it
belongs to.

### Data, formats and the control plane

| # | Sev. | Finding | Outcome |
|---|---|---|---|
| D1 | high | `checkpoint restore` took document paths from the checkpoint's manifest unchecked: a crafted `checkpoint.json` in a shared project (`"../escape.cmd"`, an absolute path) wrote any file the user can write. M12 widened it (material and meta files come back as raw text). | Fixed: every manifest path must be in its exact normalised project form and a checkpoint document kind, or the whole restore and its dry run are refused (`checkpoint_damaged`); reads stay inside the checkpoint's `files/`; every write goes through `inside_project`. Test `a_checkpoint_cannot_write_outside_the_project` (ten hostile paths). |
| D2 | high | The "before" picture of a checkpoint (`capture_frame checkpoint`, the editor's and the cockpit's compare) drew the checkpoint's scene with the current material files (and prefabs): before and after looked the same after a material change, exactly the case the guides send people to checkpoints for. | Fixed: material files and prefabs come from the checkpoint when it kept them; the live cache is told apart and drawing "now" afterwards reads the current files. Textures are not kept by checkpoints: such a picture warns `checkpoint_textures_current`. Test `a_checkpoint_picture_uses_the_material_files_it_kept`. |
| D3 | medium | A restore was not all or nothing: a prefab deleted since the checkpoint was written back but not loaded, the scene placing it failed `prefab_not_found`, after material files were already overwritten. | Fixed: every kept document is read and loaded against the checkpoint's own prefabs before anything is written; a failure half-way names what was written (`checkpoint_restore_incomplete`). Test `a_restore_brings_back_a_deleted_prefab_and_its_instance`. |
| D4 | medium | Material, meta and project files were overwritten by a restore with no copy of the replaced text. | Fixed: the replaced texts go to `.wish/trash/checkpoints/<name>-<n>/`, named in the answer (`replaced_kept_in`). Test `a_restore_keeps_the_texts_it_replaces`. |
| D5 | medium | The editor wiped a material file that did not parse when a person changed one field (the answer had no `written` part, and the editor sent only the edited field with `overwrite`). | Fixed in the editor: no `written`, no change, and the status says why. `asset_create` with `overwrite` still replaces a broken file on purpose (it is the way to repair one). Test `a_broken_material_file_is_not_overwritten_by_hand`. |
| D6 | low | The first call of a process drawing a checkpoint whose scene places prefabs failed `prefab_not_found` (prefabs were loaded lazily). | Fixed with D2's prefab library. Test `a_fresh_workspace_draws_a_checkpoint_with_its_own_prefabs`. |
| D7 | low | After an asset move, restoring an older checkpoint brought back material files naming textures that are gone, and said nothing. | Fixed: a restore checks the texture references of the material files it wrote and warns `asset_not_found`. Orphan meta files are not reported (release 1.1). Test `a_restore_names_the_textures_it_leaves_missing`. |
| D8 | low | The 256 KiB cap of a material file held in two places only; `asset_info`, moves, `wen migrate` and checkpoints read any size. | Fixed: one capped reader (`wish_asset::material::read_material_file`) everywhere; a checkpoint leaves a too-large file out with a warning. Test `material_files_are_read_with_their_size_cap`. |
| D9 | low | A file migrated by editing (format N to N+1 on its next save) does not raise `wish.json`'s `engine`, so an older launcher can pick an engine that refuses it. | Recorded for release 1.2, the first release with a newer format: the engine that writes the newer format is the one that must raise the key, and at 1.0 there is no newer format to write. Added to the format window's list in [road-to-2-0.md](road-to-2-0.md). |
| D10 | low | A dry-run restore answered `written_back: true`. | Fixed: `would_write_back`. |

### Render and look

| # | Sev. | Finding | Outcome |
|---|---|---|---|
| G1 | low | Textures and interface pictures were refused by a byte cap sized for 8-bit pixels, though 16-bit PNGs are accepted and common for normal maps. | Fixed: one cap sized for 16 bits a channel, with a message naming the rule. Test `a_sixteen_bit_picture_of_the_largest_size_is_read`. |
| G2 | low | `reload_shaders` dropped the old pool's GPU objects outside the GPU lock. | Fixed: the pool is kept through a reload (it holds no pipeline objects) and the rest is dropped under the lock. Test `the_pool_stays_through_a_shader_reload`. |

### Release, platforms and the public face

| # | Sev. | Finding | Outcome |
|---|---|---|---|
| R1 | medium | The scorecard's time limit did not stop the agent on Windows: killing `claude.cmd` left the real client running, still spending and editing, and the harness waited for it. | Fixed: the whole process tree is ended (`taskkill /T /F`; a process group on Unix), the reader is waited for at most ten seconds, and the time is taken after it. Test `a_try_past_its_limit_stops_the_whole_client` (before: 61 s for a 3 s limit). |
| R2 | medium | An agent with file tools could read earlier tries, the other model's solutions, logs quoting the hidden check, the held-out folder and the repository (which holds the checks). | Fixed: each try runs in a live folder of its own and is moved to its kept place after; the client gets `Read(...)`/`Edit(...)` deny rules for the kept tries, the held-out folders, the repository, the home folder and the client's own folder. Tests on the command line; and checked once against the real client (Haiku, under a cent, 20 November 2026): a permitted file was read, a denied file and a `Grep` in a denied folder were refused, and the rule over the repository's non-ASCII path held. |
| R3 | medium | The card's gap compared different task sets when the cap stopped the second model part way. | Fixed: the gap is scored over the tasks both models ran and says how many; none in common, no gap. The run order is now task by task, every model in turn, so a stop leaves pairs. Test `the_gap_compares_only_the_tasks_both_models_ran`. |
| R4 | medium | The glibc a Linux archive needs was neither stated nor controlled (built on `ubuntu-latest`, which moves), and the guides said the runner proved it "needs nothing else". | Fixed: the Linux archive is built on `ubuntu-22.04`, a step fails if any shipped ELF file needs a `GLIBC_` above 2.35, the README and guides state "glibc 2.35 or newer", and the claim says what the full runner image proves. A run in a bare container is a follow-up (release 1.1). Verified by CI only. |
| R5 | medium | Twenty-four MIT-only crates had no licence text in `THIRD_PARTY_LICENSES.md`; two of them (`taffy`, `drm-fourcc`) ship in the Windows or Linux programs. | Fixed: upstream texts in `licenses/`, used by `tools/third_party.py` when a crate ships none; a test fails if a library whose licence asks for its text has none. `dispatch` (macOS only) has no licence file upstream: its author's MIT text from `rust-objc` stands in, marked so. Test `every_library_whose_license_asks_for_its_text_has_one`. |
| R6 | low | A package's `sdk/sdk.json` carried the absolute path of the build machine's compiler (a user name in a locally made archive), and the release smoke could not tell the packed compiler was used. | Fixed: `wen package` writes it without `rustc_path`; the smoke checks the packed compiler and builds with rustup and cargo out of reach. Test `a_packaged_sdk_names_no_path_of_the_machine_that_made_it`. |
| R7 | low | The README said the AI-mode picture shows a plan and a conversation; it shows the empty panels of a new project. | Fixed: the caption says what it shows. |
| R8 | low | The release dry run missed inputs that change the archive; actions were pinned by tag; `ci.yml` had no permissions default. | Fixed: the paths are added, every action is pinned to a commit with its tag in a comment, `ci.yml` reads only. |
| R9 | low | With one try per task the card's consistency rows read as evidence. | Fixed: they print "– (one try per task)"; the small baseline's card was regenerated (only those rows changed). Test `consistency_needs_two_tries_of_a_task`. |

## 7. Result

All high and medium findings are fixed; of the low ones, one (D9) is placed
in release 1.2 where its fix belongs, and two small remainders are recorded
for release 1.1 (orphan meta files in `check`; a smoke run of the Linux
archive in a bare container). Nothing grew the 1.0 scope.
