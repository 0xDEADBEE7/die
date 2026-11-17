---
title: Kitchen sink
tags: [markdown, test]
---

# Kitchen sink: every element once

A first paragraph with **bold**, *italic*, ***bold italic***, ~~struck~~, `inline code`, a [link to GitHub](https://github.com), a bare address https://example.com/a/b?c=d, and a footnote reference.[^one] It runs long enough to wrap at any sensible page width, so the line height and the rag of a proportional paragraph can be judged against the real thing.

A second paragraph with a hard  
line break, "straight quotes", “curly quotes”, an em dash — an ellipsis … arrows → ← ⇒, a check ✓, and accents: café, naïve, über, señor.

## Headings

### Third level

#### Fourth level

##### Fifth level

###### Sixth level, muted

Text after the headings, so the space under a small heading shows.

## Lists

- A bullet
- Another, with `code` and a [link](#tables)
  - Nested once
    - Nested twice
      - Nested three times
- Back at the top

1. First
2. Second
   1. Second, first
   2. Second, second
3. Third
10. Tenth, to test the width of the number

A loose list:

- First loose item, a paragraph of its own.

  Its second paragraph, indented under the bullet.

- Second loose item.

### Tasks

- [x] Done
- [ ] Not done, with a longer line that wraps around to show where the second line of a task item starts relative to its box, which should be under the text
- [x] Done, **bold**

## Quotes and alerts

> A block quote. It has a bar on the left and muted text, and it wraps as any paragraph does when the line is long enough to need it.
>
> A second paragraph in the quote.
>
> > A quote inside the quote.

> [!NOTE]
> Useful information that users should know, even when skimming content.

> [!TIP]
> Helpful advice for doing things better or more easily.

> [!IMPORTANT]
> Key information users need to know to achieve their goal.

> [!WARNING]
> Urgent info that needs immediate user attention to avoid problems.

> [!CAUTION]
> Advises about risks or negative outcomes of certain actions.

## Code

Inline: run `cargo test --workspace` and then `git push`.

```rust
/// A fenced block with a language.
fn main() {
    let greeting = "hello, world";
    println!("{greeting}"); // a comment
}
```

```
A fenced block with no language.
    Indentation is kept.
```

    An indented code block,
    four spaces in.

```sh
# a very long line that runs past the right edge of the block to show what the page does with it: wrap, clip or scroll, and how the block's padding looks then
echo done
```

## Tables

| Element | Syntax | Notes |
| :------ | :----: | ----: |
| Bold | `**text**` | left, centre, right |
| Italic | `*text*` | 2 |
| Link | `[text](url)` | a [link](https://example.com) in a cell |
| Long cell | a cell with enough words in it that it has to wrap onto a second line in a narrow window | 4 |

| A | B |
|---|---|
| 1 | 2 |

## Rules and pictures

Above the rule.

---

Below the rule. A picture: ![a missing picture](no-such-file.png)

![The DIE icon, if there is one](pic.png)

## HTML

<p align="center">A centred paragraph in HTML with <kbd>Ctrl</kbd>+<kbd>C</kbd>, H<sub>2</sub>O and x<sup>2</sup>.</p>

<details>
<summary>A summary</summary>

Hidden details, with **markdown** inside.

</details>

<!-- a comment that must not show -->

Line one<br>line two.

## Footnotes and the end

The last paragraph refers to another note.[^two]

[^one]: The first footnote, with a [link](https://example.com).
[^two]: The second footnote.
