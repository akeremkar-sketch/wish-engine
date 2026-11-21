# Wish Engine beta: a guide for testers

Thank you for trying Wish Engine before its 1.0 release. The beta lasts two
weeks. What you find in it decides what is fixed before 1.0, so a short note
about something that confused you is as useful as a crash report.

A Turkish copy of this guide: [../user/tr/beta.md](../user/tr/beta.md).

## 1. What you need

- **Windows 10 or 11** (64-bit, x86-64), or **Linux** (x86-64) with glibc 2.35
  or newer (Ubuntu 22.04, Debian 12 or later).
- About 1 GB of free disk space: the unpacked engine is about 560 MB (it
  carries its own compiler for game code), and your projects need room too.
- No graphics card is required; a recent one makes the editor smoother.
- An AI helper is **optional**. Everything can be done by hand. If you have
  [Claude Code](https://claude.com/claude-code) installed and signed in, the
  editor can use it with your own account; nothing is sent anywhere
  otherwise.

## 2. Download and install

1. Open the repository's
   [Releases page](https://github.com/akeremkar-sketch/wish-engine/releases)
   and take the beta's archive for your system:
   `wish-engine-<version>-windows-x86_64.zip` or
   `wish-engine-<version>-linux-x86_64.tar.gz`, and `SHA256SUMS`.
2. Check the download (optional, recommended). On Windows, in PowerShell:
   `Get-FileHash .\wish-engine-<version>-windows-x86_64.zip` and compare
   the hash with the line in `SHA256SUMS`. On Linux:
   `sha256sum -c SHA256SUMS --ignore-missing`.
3. Unpack the archive into a folder of your own (for example
   `Documents\Wish Engine`). It holds one folder,
   `wish-engine-<version>-<system>`: the programs are inside it. Nothing is
   installed into the system.
4. Start it:
   - Windows: double-click `wish-engine.exe`. The program is not signed
     yet, so Windows may show "Windows protected your PC": choose *More info*
     and then *Run anyway*.
   - Linux: run `./wen edit` in the unpacked folder.

## 3. What to try (about an hour)

Do as much as you like, in any order. Write down where you hesitated.

1. **Start.** Watch the opening, then on the start screen make a project
   from the **Islands** template and press *Play* (Ctrl+P). Win it if you
   can.
2. **By hand.** Make a new scene (*File → New scene*), add a cube from the
   *Create* menu, move it with the gizmo, duplicate it (Ctrl+D), look around
   (Alt and the left button orbit, the wheel zooms, F frames the selection),
   change a field in the inspector, undo and redo.
3. **Game code.** *Game → Edit code* opens `scripts/src/lib.rs`; change a
   number, save, then *Game → Build code* (Ctrl+B) and play again.
4. **With AI (optional).** Switch the toolbar to *With AI*, fill in a few
   lines of the game card and ask the agent for a small change. Every task
   can be taken back.
5. **Export.** *File → Export game* makes a folder under `exports/` that runs
   without the engine; start the game from there.

The user guide explains every part:
[English](../user/en/guide.md), [Türkçe](../user/tr/guide.md).

## 4. How to report

Open an issue on the repository's
[Issues page](https://github.com/akeremkar-sketch/wish-engine/issues/new/choose)
and choose **Beta feedback** (anything: confusing, slow, ugly, missing) or
**Bug report** (something went wrong). Please give:

- what you did, what you expected and what happened;
- your system (Windows or Linux version) and graphics card, if you know it;
- a screenshot when it is about something you saw;
- for a crash: the newest file in the crash folder. Crash reports stay on
  your computer until you attach one; nothing is sent automatically.
  - Windows: `%LOCALAPPDATA%\Wish\crashes` (and, if the editor did not
    start, `%LOCALAPPDATA%\Wish\launcher-error.txt`)
  - Linux: `~/.config/wish/crashes` (or `$XDG_CONFIG_HOME/wish/crashes`)

## 5. Known gaps in this beta

- **Sound** works but has not yet been checked by ear on every system; tell
  us if a game is silent.
- **Windows:** building game code needs the Microsoft Visual C++
  Redistributable (`VCRUNTIME140.dll`), which most Windows machines already
  have. If *Build code* says it is missing, install the
  [Visual C++ Redistributable](https://learn.microsoft.com/cpp/windows/latest-supported-vc-redist)
  and try again.
- **macOS** is not part of 1.0.
- The program is not code-signed yet (see the Windows warning above).
- Not in 1.0 yet, planned for later releases: save games and switching
  scenes while a game runs, asset thumbnails as tiles, web export.

Your projects are ordinary folders of text files: if a later build cannot
open one, keep it, and tell us.
