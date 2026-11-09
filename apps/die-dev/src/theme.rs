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

/// The 16 ANSI colors: the theme's restrained, desaturated palette.
const ANSI: [&str; 16] = [
    "#1a1a1a", "#cc4444", "#55aa55", "#b8963e", "#5588aa", "#886688", "#4a8a8a", "#aaaaaa",
    "#444444", "#dd6666", "#77cc77", "#ccaa55", "#7799bb", "#aa88aa", "#66aaaa", "#eeeeee",
];

/// mono-dark: every base and UI face, styled as your Emacs draws it, and the orchid faces whose
/// Emacs faces no base face matches.
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
}
