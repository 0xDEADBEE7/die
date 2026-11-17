//! mono-dark, your Emacs theme, as DIE theme data. The colors and their use
//! are copied from `~/.emacs.d/lisp/colorschemes/mono-dark-theme.el`; each
//! face notes the Emacs face it comes from.

use die::core::{Rgb, Style, Theme, Underline};

const BG: Rgb = Rgb::hex("#1a1a1a");
const BG_SUBTLE: Rgb = Rgb::hex("#252525");
const BG_HL: Rgb = Rgb::hex("#2e2e2e");
const BORDER: Rgb = Rgb::hex("#3a3a3a");
const FG_DIM: Rgb = Rgb::hex("#555555");
const FG_MID: Rgb = Rgb::hex("#888888");
const FG: Rgb = Rgb::hex("#cccccc");
const FG_BRIGHT: Rgb = Rgb::hex("#eeeeee");
const RED: Rgb = Rgb::hex("#cc4444");
const GREEN: Rgb = Rgb::hex("#55aa55");

/// highlight.js's grayscale-dark, which your preview colours code blocks with
/// (`markdown-xwidget`'s `highlight_css/grayscale-dark.css`): its base16 greys by the role it
/// gives each. Comments are base03's #525252 lifted to #545454, the least that reads at 2:1 on
/// a page's code panel, as `Palette::mono_dark` has it.
const CODE_COMMENT: Rgb = Rgb::hex("#545454"); // base03
const CODE_FUNCTION: Rgb = Rgb::hex("#686868"); // base0D: functions, headings
const CODE_META: Rgb = Rgb::hex("#5e5e5e"); // base0F
const CODE_KEYWORD: Rgb = Rgb::hex("#747474"); // base0E: keywords, types
const CODE_VARIABLE: Rgb = Rgb::hex("#7c7c7c"); // base08: variables, tags, deletions
const CODE_STRING: Rgb = Rgb::hex("#8e8e8e"); // base0B: strings, additions
const CODE_CONSTANT: Rgb = Rgb::hex("#999999"); // base09: numbers, literals, attributes

/// The 16 ANSI colors: the theme's restrained, desaturated palette.
const ANSI: [&str; 16] = [
    "#1a1a1a", "#cc4444", "#55aa55", "#b8963e", "#5588aa", "#886688", "#4a8a8a", "#aaaaaa",
    "#444444", "#dd6666", "#77cc77", "#ccaa55", "#7799bb", "#aa88aa", "#66aaaa", "#eeeeee",
];

/// mono-dark: every base and UI face, styled as your Emacs draws it, the orchid faces whose
/// Emacs faces no base face matches, and the markdown view's, from the stylesheet your preview
/// uses (`~/.emacs.d/lisp/markdown-xwidget/resources/mono-dark.css`). A markdown page takes its
/// colours from the same faces, and `tests/markdown.rs` holds them to the stylesheet's.
pub fn mono_dark() -> Theme {
    let s = Style::new;
    Theme::new("mono-dark", ANSI.map(Rgb::hex))
        .face("default", s().fg(FG).bg(BG))
        .face("shadow", s().fg(FG_DIM))
        .face("bold", s().fg(FG_BRIGHT).bold())
        .face("highlight", s().bg(BG_HL))
        // region
        .face("selection", s().bg(BG_HL))
        // lazy-highlight
        .face("search", s().fg(FG_BRIGHT).bg(BG_HL))
        // isearch
        .face("search-current", s().fg(BG).bg(FG))
        .face("link", s().fg(FG).underline(Underline::Single))
        // org-level-1
        .face("heading", s().fg(FG_BRIGHT).bold())
        .face("error", s().fg(RED).bold())
        .face("warning", s().fg(RED))
        .face("success", s().fg(GREEN).bold())
        .face("line-number", s().fg(FG_DIM).bg(BG))
        // line-number-current-line
        .face("line-number-current", s().fg(FG_MID).bg(BG_SUBTLE))
        // mode-line and mode-line-inactive, without their box
        .face("status-line", s().fg(FG).bg(BG_SUBTLE))
        .face("status-line-inactive", s().fg(FG_DIM).bg(BG))
        // vertical-border
        .face("divider", s().fg(BORDER))
        // show-paren-match
        .face("bracket-match", s().fg(FG_BRIGHT).bg(BG_HL).bold())
        // minibuffer-prompt
        .face("prompt", s().fg(FG_BRIGHT).bold())
        // vertico-current
        .face("prompt-current", s().fg(FG_BRIGHT).bg(BG_HL))
        // completions-common-part
        .face("prompt-match", s().fg(FG_BRIGHT).bold())
        // header-line, without its box: the Buffer List's titles
        .face("header-line", s().fg(FG_MID).bg(BG_SUBTLE))
        // buffer-menu-buffer, which mono-dark leaves at Emacs's bold
        .face("buffer-menu-buffer", s().fg(FG).bold())
        .face("cursor", s().bg(FG))
        // font-lock-string-face, which emacs-orchid-client's chat header shows its workspace in.
        // Its other faces come out as mono-dark has them from the base faces they inherit.
        .face("orchid-chat-header-workspace", s().fg(FG_MID))
        // seek's search line, `(:background "#3a3a3a")`, which the session browser draws while
        // you type a query.
        .face("orchid-seek-search", s().bg(BORDER))
        // h1 and h2: bright and bold over a border-coloured line. In the view in place a cell
        // has one size of letter, so the two lines stand in for 2em, and one for 1.5em.
        .face(
            "markdown-h1",
            heading(FG_BRIGHT).underline(Underline::Double),
        )
        .face(
            "markdown-h2",
            heading(FG_BRIGHT).underline(Underline::Single),
        )
        // h3 to h5 are bright and bold there too, each smaller, as a page draws them; h6 is
        // the dimmer one, as GitHub's is.
        .face("markdown-h3", heading(FG_BRIGHT))
        .face("markdown-h4", heading(FG_BRIGHT))
        .face("markdown-h5", heading(FG_BRIGHT))
        .face("markdown-h6", s().fg(FG_MID))
        // strong, em and del
        .face("markdown-bold", s().fg(FG_BRIGHT).bold())
        .face("markdown-italic", s().fg(FG_MID).italic())
        .face("markdown-strike", s().fg(FG_DIM).strike())
        // code, and pre with its own text colour. Inline code is on the highlight grey, a
        // shade over a block's, so it reads as a pill on a table's striped row too.
        .face("markdown-code", s().fg(FG_MID).bg(BG_HL))
        .face("markdown-code-block", s().bg(BG_SUBTLE))
        // a
        .face(
            "markdown-link",
            s().fg(RED).bold().underline(Underline::Single),
        )
        // blockquote's text; its bar, hr and a table's borders are the border colour
        .face("markdown-quote", s().fg(FG_MID).italic())
        .face("markdown-rule", s().fg(BORDER))
        // th, without its background: a row's padding is drawn by its borders
        .face("markdown-table-header", s().fg(FG_BRIGHT).bold())
        // What a browser draws in the text's colour, dimmed here so the text leads: bullets,
        // numbers and checkboxes; and what it doesn't draw at all: HTML, a fence's language.
        .face("markdown-marker", s().fg(FG_MID))
        .face("markdown-meta", s().fg(FG_DIM))
        .face("markdown-alert", s().fg(FG_BRIGHT).bold())
        // A page's code blocks, in grayscale-dark's greys; what's none of these is the block's
        // own text, the default's.
        .face("markdown-code-keyword", s().fg(CODE_KEYWORD))
        .face("markdown-code-string", s().fg(CODE_STRING))
        .face("markdown-code-comment", s().fg(CODE_COMMENT))
        .face("markdown-code-number", s().fg(CODE_CONSTANT))
        .face("markdown-code-function", s().fg(CODE_FUNCTION))
        .face("markdown-code-type", s().fg(CODE_KEYWORD))
        .face("markdown-code-constant", s().fg(CODE_CONSTANT))
        .face("markdown-code-meta", s().fg(CODE_META))
        .face("markdown-code-variable", s().fg(CODE_VARIABLE))
        .face("markdown-code-tag", s().fg(CODE_VARIABLE))
        .face("markdown-code-attr", s().fg(CODE_CONSTANT))
        .face("markdown-code-inserted", s().fg(CODE_STRING))
        .face("markdown-code-deleted", s().fg(CODE_VARIABLE))
        .face("markdown-code-heading", s().fg(CODE_FUNCTION))
}

/// A heading: bold in `fg`, and any underline in the border's colour, as the stylesheet's
/// `border-bottom` is.
fn heading(fg: Rgb) -> Style {
    Style::new().fg(fg).bold().underline_color(BORDER)
}
