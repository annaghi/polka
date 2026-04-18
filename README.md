# Polka

_working draft_

- Plugins are built for the [markdown-it Rust port](https://github.com/markdown-it-rust/markdown-it).
- The attributes plugin uses a state machine adapted from [Jotdown](https://github.com/hellux/jotdown).
- The HTML validator plugin implements a `TokenSink` from [html5ever](https://github.com/servo/html5ever).

## Rules

| Category | Rule        | Syntax                   | Description                               |
| -------- | ----------- | ------------------------ | ----------------------------------------- |
| Inline   | `icon`      | `:set-name:`             | Shortcodes for SVG icons                  |
| Inline   | `span`      | `\|text\|`               | Inline spans                              |
| Inline   | `sup_sub`   | `^sup^` / `~sub~`        | Superscript and subscript                 |
| Core     | `attrs`     | `{ #id .class key=val }` | Jotdown-style inline and block attributes |
| Core     | `validator` |                          | Validates inline and block HTML           |
