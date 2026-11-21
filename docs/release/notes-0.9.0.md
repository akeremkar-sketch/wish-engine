# Wish Engine 0.9.0: the 1.0 beta (draft notes)

> **DRAFT** for the release page. The user edits and publishes it.

The first public beta of Wish Engine 1.0: a free, open-source 3D game
engine in Rust, usable fully by hand and fully through an AI agent (MCP),
with both kinds of change in one history.

**Testers:** please read the [tester's guide](https://github.com/akeremkar-sketch/wish-engine/blob/main/docs/release/beta.md) (Türkçe:
[Türkçe kılavuz](https://github.com/akeremkar-sketch/wish-engine/blob/main/docs/user/tr/beta.md)) and report through the
repository's Issues page (*Beta feedback* or *Bug report*).

## What is in it

- **Editor:** a menu bar and toolbar, a Create menu, duplicate, new scene,
  an orbit camera (Alt-drag, middle-drag, wheel, F to frame), a hierarchy
  with a filter, an inspector over every engine type, assets, history and
  checkpoints; a *Manual | With AI* switch for working with an agent.
- **Look:** sky light, fog, colour grading, FXAA, HDR bloom, cascaded
  shadows, material files with textures, see-through surfaces; the default
  look is *Natural*.
- **Game systems:** physics with a character controller, sound, lights,
  skeletal animation, in-game interface, particles, 2D sprites; game code in
  Rust compiled to WebAssembly by the engine itself (no tools to install).
- **Proof:** deterministic simulation, replays that give the same hash on
  every tick, scenario tests with pictures, export of a game that runs
  without the engine.
- **Platforms:** Windows 10/11 and Linux (glibc 2.35 or newer), x86-64.

## How it was tested

There were no outside testers before this beta. The Windows archive was
used from its unpacked copy alone and the tester's guide was run by hand
in the real window by the project's agent; the project's owner listened to
the sound. Eleven findings were fixed before this tag (among them keys over
the game view, an idle editor that kept a core busy, and a placeholder
"wind" that sounded like broken audio):
[the report](https://github.com/akeremkar-sketch/wish-engine/blob/main/docs/design/M12-beta-self-test.md). The Linux archive is checked
end to end by the release workflow. **This beta is where outside people
come in:** please report anything that confuses you.

## Known gaps

See section 5 of the tester's guide: sound checked by ear on Windows only,
the Visual C++ runtime for game code on Windows, no macOS, no code
signature yet.

## Checksums

`SHA256SUMS` lists every archive of this release.
