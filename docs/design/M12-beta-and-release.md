# M12 package 13: the beta, the release candidates and 1.0.0

**Status:** prepared (21 November 2026): the tester's guide, its Turkish
copy, the issue forms and the first beta's release notes are written.
**User decision, 21 November 2026: there are no outside testers; the agent
tests the beta itself.** It does so as a tester would (section 5), and the
findings are fixed before any tag. Nothing is tagged yet.

## 1. Problem

Everything of the short path is built, reviewed and merged into `main`
(21 November 2026). Nobody but its author has used it. The plan (M12 note,
section 0, item 13) ends 1.0 with a two-week beta, release candidates and
the 1.0.0 tag.

## 2. Decisions

- **Versions** (ADR-027 rule 3: three numbers, no suffix, pre-releases are
  `0.x.0` tags). The beta is **0.9.0**; a release candidate after it is
  **0.10.0** (then 0.11.0 if another is needed); the release is **1.0.0**.
  The tag is the bare version (`0.9.0`), and the release workflow refuses a
  tag that is not the workspace's version.
- **Freeze.** From the beta's tag on, no feature is merged: only fixes of
  what the beta finds, each with a test, through the full gate.
- **Who does what.** The agent bumps the version, writes the notes and
  pushes the tag **after the user says yes for that tag**; the workflow
  builds both archives, checks them from the extracted copy and makes a
  **draft** release; the user reads the draft and publishes it. Every tag
  is its own yes.
- **Where testers report.** GitHub issues with two forms
  (`.github/ISSUE_TEMPLATE/`): *Beta feedback* and *Bug report*; any
  language is welcome. The guide is [../release/beta.md](../release/beta.md)
  (Turkish: [../user/tr/beta.md](../user/tr/beta.md)).

## 3. Steps

1. **Beta 0.9.0.** Bump the workspace version to 0.9.0 (`Cargo.toml`,
   `Cargo.lock`); tests that write `0.1.0` where they mean "this engine"
   read `env!("CARGO_PKG_VERSION")` instead, tests that mean a fixed old
   version keep it; `engines::OPENS_FROM` stays 0.1.0 (no format changed
   since). Full gate, then the tag (user's yes), the draft, the user
   publishes it and sends the guide to the testers.
2. **Two weeks.** Triage every issue the day it comes: a fix with a test,
   or a reason written on the issue; the README's "Status" says "beta".
3. **Release candidate 0.10.0** with the fixes; a few days without a new
   high finding.
4. **Before the 1.0 label** (the plan asks the user once more): the full
   baseline of the AI scorecard (about 16 to 20 $, the user's approval),
   and whether "scene switching with save games" waits for release 1.1.
5. **1.0.0.** The README and the guides say "1.0"; the tag; the user
   publishes.

## 4. Test plan

The release workflow's own checks on every tag (both archives built twice
to the same bytes, then used end to end from the extracted copy); the full
gate before each tag; the link test for the guides.

## 5. The agent's own beta (no outside testers)

The user has no testers for now (21 November 2026) and asked the agent to
test. The agent cannot be an outside person: it knows the code, so it
misses what a stranger trips on, and it is not a second machine. What it
can do, and does:

1. Package the Windows archive exactly as the release workflow does (the
   `dist` profile, the static C runtime, `wen package`), unpack it into an
   empty folder and use only that copy, with the developer's Rust taken
   off `PATH`.
2. Run the tester's guide by hand in the real window (the desktop, seen
   and clicked through), step by step, writing down every hesitation,
   wrong word, ugly corner and failure, with pictures.
3. Write the findings into a report beside this note and fix each with a
   test, or give the reason it waits.

The formal tests with outside people stay where the train puts them
(release 1.4); the README must not say that the beta had outside testers.

