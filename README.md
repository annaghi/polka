# Polka

_working draft_

- Plugins built for [markdown-it Rust port](https://github.com/markdown-it-rust/markdown-it)
- Attributes plugin uses state machine from [Jotdown](https://github.com/hellux/jotdown)

## Rules

| Category | Rule        | Syntax                   | Description                               |
| -------- | ----------- | ------------------------ | ----------------------------------------- |
| Inline   | `icon`      | `:family-name:`          | SVG icon shortcodes                       |
| Inline   | `span`      | `\|this\|`               | bracketed span                            |
| Inline   | `sup_sub`   | `^this^` / `~this~`      | superscript and subscript                 |
| Core     | `attrs`     | `{ #id .class key=val }` | Jotdown style inline and block attributes |
| Core     | `validator` |                          | HTML validator for inline and block HTML  |
