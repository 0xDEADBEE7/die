# 04. Should die-vim match your Evil setup, or textbook Vim?

**Question:** D4's rationale is "covers a vanilla Evil setup like yours". But §10 and §8 describe Vim in places where your Evil behaves differently. When they disagree, which should win?

## Where they differ

| | Your Evil today | Docs (Vim-style) |
|---|---|---|
| `j`/`k` | logical lines; `gj`/`gk` move by screen line (Evil overrides `line-move-visual`) | visual lines (§8) |
| Clipboard | every yank and delete goes to the macOS clipboard, and `p` pastes whatever you last copied anywhere | a separate `+` register |
| Scrolling | `C-d C-f C-b C-e C-y`, `zz zt zb`; `C-u` doesn't scroll | missing; all `z…` keys handed to folding |
| Windows | `C-w` map, `:sp :vs :only :close`, `:q` closes a split | missing |
| Editing | autoindent, one undo step per insert session | not specified |
| Search | incremental, highlights every match, smartcase, Emacs regex syntax | not specified |

## Options

- **A. Your Evil wins.** die-vim reproduces what you use today. The one deliberate change is Rust regex syntax instead of Emacs's.
- **B. Vim wins,** as currently written. Expect friction on the first day, especially the clipboard.

## Recommendation

**A.** I'll use your Evil config as the spec, and flag any other deliberate change.

## Your answer

**Accepted 2026-10-19:** "go with your recommendations on the rest". A, your Evil setup is the spec. The one deliberate change is Rust regex syntax.
