//! mono-dark, your Emacs theme, as DIE theme data. The colors and their use
//! are copied from its source, `mono-dark-theme.el`; each
//! face notes the Emacs face it comes from.

use die::core::{Rgb, Style, Theme, Underline};

// mono-dark's palette, public so a config's extensions style their faces in it (`Extra::theme`).

/// The ground: `default`'s background.
pub const BG: Rgb = Rgb::hex("#1a1a1a");
/// A ground a step up: status lines, the header line, the current line number.
pub const BG_SUBTLE: Rgb = Rgb::hex("#252525");
/// The highlight's ground: the region, search matches, the current candidate.
pub const BG_HL: Rgb = Rgb::hex("#2e2e2e");
/// Borders and dividers.
pub const BORDER: Rgb = Rgb::hex("#3a3a3a");
/// The dimmest text: shadow, line numbers, an inactive status line.
pub const FG_DIM: Rgb = Rgb::hex("#555555");
/// Text a step back: the current line number, the header line.
pub const FG_MID: Rgb = Rgb::hex("#888888");
/// Text.
pub const FG: Rgb = Rgb::hex("#cccccc");
/// Bright text: bold, headings, prompts.
pub const FG_BRIGHT: Rgb = Rgb::hex("#eeeeee");
/// The one accent: errors and warnings.
pub const RED: Rgb = Rgb::hex("#cc4444");
/// Success.
pub const GREEN: Rgb = Rgb::hex("#55aa55");

/// The 16 ANSI colors: the theme's restrained, desaturated palette.
const ANSI: [&str; 16] = [
    "#1a1a1a", "#cc4444", "#55aa55", "#b8963e", "#5588aa", "#886688", "#4a8a8a", "#aaaaaa",
    "#444444", "#dd6666", "#77cc77", "#ccaa55", "#7799bb", "#aa88aa", "#66aaaa", "#eeeeee",
];

/// mono-dark: every base and UI face, styled as your Emacs draws it, and nothing else, as DIE
/// reports a face a theme styles that no one declared. A config's extensions style their own
/// faces on top, in this palette (`Extra::theme`).
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
}
