# The editor's languages

Each file here is one language of the editor's interface: `{ "English text": "translation" }`.
The English text is the key, exactly as the code writes it in `t("...")` or `tf("...", ...)`.

- **Change a translation:** edit the value; keep every `{name}` of the English text (their order may change).
- **Add a language:** add `<code>.json` with every key, and a line to `LANGUAGES` in `src/i18n.rs`.
  The font (Roboto) covers Latin, Cyrillic and Greek letters; other scripts need a font first.
- **Check:** `cargo test -p wish_editor --test languages` lists what a file lacks (it would show in
  English), what it has that the code no longer shows, wrong `{names}`, and letters the font lacks.

Choose the language from the project menu (Language), with `wen edit --lang <code>`, or `WISH_LANG`.
The choice is kept in the editor's preferences (`%APPDATA%\Wish\editor.json`, `~/.config/wish/editor.json`).
