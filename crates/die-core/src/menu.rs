//! `buffer-menu`: Emacs's Buffer List (buff-menu.el), with the keys your evil-collection gives it
//! in normal state. A row per buffer, the one used last first, with Emacs's columns: `C` is `.`
//! for the buffer you came from, `>` for one marked to show and `D` for one to kill, `R` is `%`
//! for read-only, and `M` is `*` for unsaved changes or `S` for one to save. Then the name, the
//! size in characters, the mode and the file. DIE has no header lines, so the titles are the first
//! line of text, as with Emacs's `Buffer-menu-use-header-line` off.
//!
//! As in tabulated-list, a mark changes its one character in the text and a row killed loses its
//! line, so cursors stay where Emacs's point does. Only listing again prints the whole buffer.
use crate::commands::kill_asking;
use crate::dispatch::Args;
use crate::error::{Error, Result};
use crate::ext::{Content, Ctx, Registry};
use crate::ids::{BufferId, DecorationId, WindowId};
use crate::keymap::State;
use crate::text::{Deco, Origin};
use crate::workspace::{ShowIn, Side};
use std::ops::Range;
use std::path::Path;

pub(crate) const MENU: &str = "*Buffer List*";
const HELP: &str = "Commands: d, s, x, u; go, gO, 1, 2, m, gv; ~, %; q to quit";
const TITLES: [&str; 7] = ["C", "R", "M", "Buffer", "Size", "Mode", "File"];
const NAME: usize = 3;
const SIZE: usize = 4;

/// A buffer's row: what each column says.
struct Row {
    buf: BufferId,
    cols: [String; 7],
}

/// What `*Buffer List*` lists, as tabulated-list keeps it, in the order shown.
#[derive(Default)]
pub(crate) struct Menu {
    rows: Vec<Row>,
    /// The column it's sorted by, and whether that's the other way round.
    sort: Option<(usize, bool)>,
    files_only: bool,
    name_width: usize,
    /// The window it opened in and what that showed, for `q`.
    came_from: Option<(WindowId, BufferId)>,
    faces: Vec<DecorationId>,
}

type Body = fn(&mut Ctx, &Args, WindowId) -> Result;

/// The keys, each command's name, what it does, and how. Each acts in a window on the menu.
#[rustfmt::skip]
const KEYS: &[(&str, &str, &str, Body)] = &[
    ("d <C-k>", "buffer-menu-delete", "Mark this line's buffer for x to kill, and move down; with a count, that many", |ctx, a, w| ctx.menu_flag(w, "D", a.count.unwrap_or(1), 1)),
    ("<C-d>", "buffer-menu-delete-backwards", "Mark this line's buffer for x to kill, and move up; with a count, that many", |ctx, a, w| ctx.menu_flag(w, "D", a.count.unwrap_or(1), -1)),
    ("s", "buffer-menu-save", "Mark this line's buffer for x to save, and move down", |ctx, _, w| ctx.menu_flag(w, "S", 1, 1)),
    ("m", "buffer-menu-mark", "Mark this line's buffer for gv to show, and move down", |ctx, _, w| ctx.menu_flag(w, ">", 1, 1)),
    ("u", "buffer-menu-unmark", "Take this line's marks off and move down, or up with a count", |ctx, a, w| ctx.menu_flag(w, " ", 1, if a.count.is_some() { -1 } else { 1 })),
    ("<Del>", "buffer-menu-backup-unmark", "Move up a line and take its marks off", |ctx, _, w| { let line = ctx.menu_here(w).0; ctx.menu_goto(w, line.saturating_sub(1), 0)?; ctx.menu_flag(w, " ", usize::from(line > 1), 0) }),
    ("U", "buffer-menu-unmark-all", "Take every mark off", |ctx, _, _| (0..ctx.menu.rows.len()).try_for_each(|i| ctx.menu_unmark(i))),
    ("x", "buffer-menu-execute", "Save the buffers marked S and kill those marked D, asking for unsaved changes to a file", |ctx, _, _| ctx.menu_execute(0)),
    ("~", "buffer-menu-not-modified", "Mark this line's buffer as having no unsaved changes, or with a count as having some", |ctx, a, w| ctx.menu_toggle(w, 2, a.count.is_some())),
    ("%", "buffer-menu-toggle-read-only", "Make this line's buffer read-only, or not", |ctx, _, w| ctx.menu_toggle(w, 1, false)),
    ("T", "buffer-menu-toggle-files-only", "List only buffers with files, or every one; with a count, only those with files", |ctx, a, w| ctx.menu_files_only(w, a.count)),
    ("X", "buffer-menu-bury", "Put this line's buffer last in the buffer list", |ctx, _, w| ctx.menu_bury(w)),
    ("gr", "buffer-menu-refresh", "List the buffers again", |ctx, _, w| ctx.menu_refresh(w)),
    ("o S", "buffer-menu-sort", "Sort by the column at the cursor, or the count's from 0, and the other way when it's sorted by that", |ctx, a, w| ctx.menu_sort_by(w, a.count)),
    ("go <CR>", "buffer-menu-this-window", "Show this line's buffer in this window", |ctx, _, w| ctx.menu_select(w, "go")),
    ("gO", "buffer-menu-other-window", "Show this line's buffer in another window, and go there", |ctx, _, w| ctx.menu_select(w, "gO")),
    ("1", "buffer-menu-1-window", "Show this line's buffer in this window, alone", |ctx, _, w| ctx.menu_select(w, "1")),
    ("2", "buffer-menu-2-window", "Show this line's buffer, and the one before it in another window", |ctx, _, w| ctx.menu_select(w, "2")),
    ("gv", "buffer-menu-select", "Show this line's buffer alone, with each marked > in a window below", |ctx, _, w| ctx.menu_select(w, "gv")),
    ("q ZZ", "buffer-menu-quit", "Show what this window showed before the menu, and put the menu last in the buffer list", |ctx, _, w| ctx.menu_quit(w)),
];

pub(crate) fn register(reg: &mut Registry) {
    reg.command(
        "buffer-menu",
        "List the buffers in *Buffer List*, in this window, to show, save or kill them. With a \
         count, only those with files",
        |ctx, a| ctx.buffer_menu(a.count.is_some()),
    );
    for &(_, name, doc, body) in KEYS {
        reg.command(name, doc, move |ctx, a| {
            let menu = ctx.find_buffer(MENU);
            let w = a.window.or(ctx.focused);
            match w.filter(|w| ctx.window_buffer(*w) == menu) {
                Some(w) => body(ctx, a, w),
                None => Err(Error::Msg(format!("Not in {MENU}"))),
            }
        });
    }
}

impl Ctx {
    /// Open the menu in this window, listed afresh, the cursor on its first row.
    fn buffer_menu(&mut self, files_only: bool) -> Result {
        let Some(w) = self.current_window() else {
            return Err(Error::Msg("No window".into()));
        };
        let old = self.window_buffer(w);
        let menu = match self.find_buffer(MENU) {
            Some(menu) => menu,
            None => {
                let menu = self.create_text(MENU, "");
                self.text_mut(menu)?.settings_mut().read_only = true;
                for k in KEYS
                    .iter()
                    .flat_map(|k| k.0.split(' ').map(|key| (key, k.1)))
                {
                    self.buffers[menu].local.bind(State::Normal, k.0, k.1);
                }
                self.menu = Menu::default();
                menu
            }
        };
        if old != Some(menu) {
            self.menu.came_from = old.map(|b| (w, b));
        }
        // As Emacs's mode starts again: only the `>` marks stay, read from the old listing.
        (self.menu.sort, self.menu.files_only) = (None, files_only);
        self.show(menu, ShowIn::Current)?;
        self.menu_list(w, old);
        self.messages.push(HELP.into());
        self.menu_print(w, None, 0)
    }

    fn menu_buf(&self) -> Result<BufferId> {
        let menu = self.find_buffer(MENU);
        menu.ok_or(Error::Msg(format!("No {MENU}")))
    }

    /// List the buffers, as `list-buffers--refresh` does: `old` is the `.`, the `>` marks stay,
    /// and the name column is as wide as the longest name, between 19 and the window's text width
    /// over 4.2. Names starting with a space are left out unless they have a file.
    fn menu_list(&mut self, w: WindowId, old: Option<BufferId>) {
        let menu = self.find_buffer(MENU);
        let rows = std::mem::take(&mut self.menu.rows).into_iter();
        let marked: Vec<_> = rows.filter(|r| r.cols[0] == ">").map(|r| r.buf).collect();
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        for b in self.buffer_list().into_iter().filter(|b| Some(*b) != menu) {
            let name = self.buffer_name(b).unwrap_or_default().to_string();
            let file = self.file(b).map(|f| {
                let rest = home.as_ref().and_then(|h| f.path.strip_prefix(h).ok());
                rest.map_or_else(|| f.path.clone(), |rest| Path::new("~").join(rest))
            });
            if file.is_none() && (name.starts_with(' ') || self.menu.files_only) {
                continue;
            }
            let t = self.text(b).ok();
            let mark = |on: bool, c: &str| if on { c } else { " " }.to_string();
            let here = if Some(b) == old { "." } else { ">" };
            let cols = [
                mark(Some(b) == old || marked.contains(&b), here),
                mark(t.is_some_and(|t| t.settings().read_only), "%"),
                mark(t.is_some_and(|t| t.modified()), "*"),
                name,
                t.map_or(0, |t| t.rope().len_chars()).to_string(),
                self.mode_name(b),
                file.map(|f| f.display().to_string()).unwrap_or_default(),
            ];
            self.menu.rows.push(Row { buf: b, cols });
        }
        let max = menu.and_then(|b| self.text(b).ok()?.settings().max_width);
        let cols = self
            .window_size(w)
            .map_or(80, |s| s.0.min(max.unwrap_or(s.0)));
        let longest = self.menu.rows.iter().map(|r| r.cols[NAME].chars().count());
        let fits = ((cols as f64 / 4.2) as usize).min(longest.max().unwrap_or(0));
        self.menu.name_width = fits.max(19);
        self.menu_sort();
    }

    /// The Mode column: a buffer's first mode, which is its file type's (contract §8), or else a
    /// surface's kind. A text buffer with neither is Fundamental, as Emacs calls having none.
    fn mode_name(&self, b: BufferId) -> String {
        let buf = &self.buffers[b];
        match (buf.modes.first(), &buf.content) {
            (Some(m), _) => self.modes[m.spec].name.into(),
            (None, Content::Custom { kind, .. }) => kind.to_string(),
            (None, Content::Text(_)) => "Fundamental".into(),
        }
    }

    /// Sort as tabulated-list does, keeping the order of rows that compare equal: by text, but by
    /// size the biggest first.
    fn menu_sort(&mut self) {
        let Some((n, flip)) = self.menu.sort else {
            return;
        };
        let size = |r: &Row| r.cols[SIZE].parse::<usize>().unwrap_or(0);
        self.menu.rows.sort_by(|a, b| {
            let (a, b) = if flip { (b, a) } else { (a, b) };
            match n {
                SIZE => size(b).cmp(&size(a)),
                _ => a.cols[n].cmp(&b.cols[n]),
            }
        });
    }

    /// The titles, with the sort's arrow after the column it's by, as Emacs puts it on all but
    /// the one-letter columns.
    fn menu_titles(&self) -> [String; 7] {
        let mut titles = TITLES.map(String::from);
        if let Some((n, flip)) = self.menu.sort.filter(|(n, _)| *n >= NAME) {
            titles[n] += if flip { " ▲" } else { " ▼" };
        }
        titles
    }

    /// Print the listing over what was there, with `w`'s cursor on `buf`'s row at `col`, or else
    /// on the first row, as `tabulated-list-print` keeps the place.
    fn menu_print(&mut self, w: WindowId, buf: Option<BufferId>, col: usize) -> Result {
        let (width, rows) = (self.menu.name_width, &self.menu.rows);
        let mut text = print_row(&self.menu_titles(), width, 4).0;
        let mut names = vec![];
        let size = |i: usize| {
            rows.get(i)
                .map_or(0, |r: &Row| r.cols[SIZE].chars().count())
        };
        for (i, r) in rows.iter().enumerate() {
            let near = size(i.wrapping_sub(1)).max(size(i)).max(size(i + 1));
            let (row, name) = print_row(&r.cols, width, near);
            text.push('\n');
            names.push(text.len() + 4..text.len() + 4 + name);
            text += &row;
        }
        let head = 0..text.find('\n').unwrap_or(text.len());
        text.push('\n');
        let old = std::mem::take(&mut self.menu.faces);
        let t = self.text_mut(self.menu_buf()?)?;
        for id in old {
            t.undecorate(id);
        }
        let all = 0..t.len();
        self.menu_edit(all, &text)?;
        self.menu_face(head, "header-line")?;
        for name in names {
            self.menu_face(name, "buffer-menu-buffer")?;
        }
        let row = self.menu.rows.iter().position(|r| Some(r.buf) == buf);
        self.menu_goto(w, row.map_or(1, |i| i + 1), row.map_or(0, |_| col))
    }

    /// Replace bytes of the menu's text, which isn't a change to save.
    fn menu_edit(&mut self, r: Range<usize>, s: &str) -> Result {
        let t = self.text_mut(self.menu_buf()?)?;
        let tx = t.tx(Origin::Extension).silent();
        tx.replace(r, s).commit()?;
        t.set_modified(false);
        Ok(())
    }

    fn menu_face(&mut self, r: Range<usize>, face: &'static str) -> Result {
        let t = self.text_mut(self.menu_buf()?)?;
        let id = t.decorate(r, Deco::Face(face))?;
        self.menu.faces.push(id);
        Ok(())
    }

    /// Set a mark column, `C`, `R` or `M`, of row `i`: one character in the text.
    fn menu_set_col(&mut self, i: usize, col: usize, s: &str) -> Result {
        self.menu.rows[i].cols[col] = s.into();
        let at = self.text(self.menu_buf()?)?.line_start(i + 1) + col;
        self.menu_edit(at..at + 1, s)
    }

    /// Take row `i` out of the list and its line out of the text.
    fn menu_drop(&mut self, i: usize) -> Result<Row> {
        let t = self.text(self.menu_buf()?)?;
        let lines = t.line_start(i + 1)..t.line_start(i + 2);
        self.menu_edit(lines, "")?;
        Ok(self.menu.rows.remove(i))
    }

    /// The line and column of `w`'s cursor in the menu. The titles are line 0.
    fn menu_here(&self, w: WindowId) -> (usize, usize) {
        let t = self.menu_buf().and_then(|b| self.text(b));
        let Some((t, at)) = t.ok().and_then(|t| Some((t, t.cursor(w)?))) else {
            return (1, 0);
        };
        let (line, rope) = (t.line_of(at), t.rope());
        (
            line,
            rope.byte_to_char(at) - rope.byte_to_char(t.line_start(line)),
        )
    }

    /// Put `w`'s cursor on a line, at a column or the line's last character.
    fn menu_goto(&mut self, w: WindowId, line: usize, col: usize) -> Result {
        let t = self.text_mut(self.menu_buf()?)?;
        let line = line.min(t.line_count() - 1);
        let chars: Vec<char> = t.rope().line(line).chars().filter(|c| *c != '\n').collect();
        let col = col.min(chars.len().saturating_sub(1));
        let bytes: usize = chars[..col].iter().map(|c| c.len_utf8()).sum();
        t.set_cursor(w, t.line_start(line) + bytes);
        Ok(())
    }

    fn menu_row(&self, line: usize) -> Option<usize> {
        line.checked_sub(1).filter(|i| *i < self.menu.rows.len())
    }

    /// The row at `w`'s cursor and its buffer, or Emacs's error for there being none.
    fn menu_line_buffer(&self, w: WindowId) -> Result<(usize, BufferId)> {
        let none = || Error::Msg("No buffer on this line".into());
        let i = self.menu_row(self.menu_here(w).0).ok_or_else(none)?;
        let buf = self.menu.rows[i].buf;
        match self.is_live(buf) {
            true => Ok((i, buf)),
            false => Err(Error::Msg("This buffer has been killed".into())),
        }
    }

    /// Put `mark` on the row at the cursor, then move `by` lines, `times` times, as `d`, `s`, `m`
    /// and `u` do: `D` and `S` only on a buffer still alive, and ` ` takes the marks off.
    fn menu_flag(&mut self, w: WindowId, mark: &str, times: usize, by: isize) -> Result {
        let mut line = self.menu_here(w).0;
        for _ in 0..times {
            if let Some(i) = self.menu_row(line) {
                let live = self.is_live(self.menu.rows[i].buf);
                match mark {
                    " " => self.menu_unmark(i)?,
                    ">" => self.menu_set_col(i, 0, mark)?,
                    "S" if live => self.menu_set_col(i, 2, mark)?,
                    _ if live => self.menu_set_col(i, 0, mark)?,
                    _ => {}
                }
            }
            line = line.saturating_add_signed(by).min(self.menu.rows.len() + 1);
        }
        self.menu_goto(w, line, 0)
    }

    fn menu_unmark(&mut self, i: usize) -> Result {
        self.menu_set_col(i, 0, " ")?;
        let buf = self.menu.rows[i].buf;
        let unsaved = self.text(buf).is_ok_and(|t| t.modified());
        match self.is_live(buf) {
            true => self.menu_set_col(i, 2, if unsaved { "*" } else { " " }),
            false => Ok(()),
        }
    }

    /// `x`, from row `i` on: save each buffer marked `S`, kill each marked `D` and drop its row,
    /// and drop the rows of buffers already killed. A modified buffer with a file asks first, and
    /// a cancelled question stops there.
    fn menu_execute(&mut self, mut i: usize) -> Result {
        while let Some(r) = self.menu.rows.get(i) {
            let (buf, kill, save) = (r.buf, r.cols[0] == "D", r.cols[2] == "S");
            if !self.is_live(buf) {
                self.menu_drop(i)?;
                continue;
            }
            if save {
                self.menu_save(i)?;
            }
            if kill {
                return kill_asking(self, buf, move |ctx, killed| match killed {
                    Some(true) => ctx.menu_drop(i).and_then(|_| ctx.menu_execute(i)),
                    Some(false) => ctx.menu_execute(i + 1),
                    None => Ok(()),
                });
            }
            i += 1;
        }
        Ok(())
    }

    /// Save row `i`'s buffer, as Emacs's `save-buffer` does: one with no file and nothing to save
    /// is done with. A failure is said, and the `S` stays.
    fn menu_save(&mut self, i: usize) -> Result {
        let buf = self.menu.rows[i].buf;
        if self.file(buf).is_none() && !self.text(buf).is_ok_and(|t| t.modified()) {
            self.messages.push("(No changes need to be saved)".into());
            return self.menu_set_col(i, 2, " ");
        }
        match self.save(buf) {
            Ok(()) => self.menu_set_col(i, 2, " "),
            Err(e) => {
                let name = &self.menu.rows[i].cols[NAME];
                self.messages.push(format!("Error saving {name}: {e}"));
                Ok(())
            }
        }
    }

    /// `~` (column 2), which marks the buffer as saved, or with `on` as not, and `%` (column 1),
    /// which turns read-only on or off.
    fn menu_toggle(&mut self, w: WindowId, col: usize, on: bool) -> Result {
        let (i, buf) = self.menu_line_buffer(w)?;
        let t = self.text_mut(buf)?;
        let on = if col == 1 {
            !t.settings().read_only
        } else {
            on
        };
        match col {
            1 => t.settings_mut().read_only = on,
            _ => t.set_modified(on),
        }
        self.menu_set_col(i, col, if on { ["", "%", "*"][col] } else { " " })
    }

    fn menu_files_only(&mut self, w: WindowId, count: Option<usize>) -> Result {
        self.menu.files_only = count.map_or(!self.menu.files_only, |n| n > 0);
        self.messages.push(match self.menu.files_only {
            true => "Showing only file-visiting buffers.".into(),
            false => "Showing all buffers except internal ones.".into(),
        });
        self.menu_refresh(w)
    }

    /// `X`: the buffer goes last in the buffer list and its row to the end, and the cursor stays
    /// on its line.
    fn menu_bury(&mut self, w: WindowId) -> Result {
        let line = self.menu_here(w).0;
        let Some(i) = self.menu_row(line) else {
            return Ok(());
        };
        let row = self.menu_drop(i)?;
        if !self.is_live(row.buf) {
            self.messages
                .push("Buffer is dead; removing from list.".into());
            return Ok(());
        }
        self.buffers[row.buf].used = 0;
        let above = self
            .menu
            .rows
            .last()
            .map_or(0, |r| r.cols[SIZE].chars().count());
        let near = above.max(row.cols[SIZE].chars().count());
        let (text, name) = print_row(&row.cols, self.menu.name_width, near);
        self.menu.rows.push(row);
        let end = self.text(self.menu_buf()?)?.len();
        self.menu_edit(end..end, &format!("{text}\n"))?;
        self.menu_face(end + 4..end + 4 + name, "buffer-menu-buffer")?;
        self.messages.push("Buffer buried.".into());
        self.menu_goto(w, line, 0)
    }

    /// `gr`: list again, with no `.`, the cursor staying with its row.
    fn menu_refresh(&mut self, w: WindowId) -> Result {
        let (line, col) = self.menu_here(w);
        let buf = self.menu_row(line).map(|i| self.menu.rows[i].buf);
        self.menu_list(w, None);
        self.menu_print(w, buf, col)
    }

    /// `o`: sort by the column at the cursor, or the count's, counting from 0, and the other way
    /// if it's sorted by that already. The columns after the name start where they would with a
    /// name that fits.
    fn menu_sort_by(&mut self, w: WindowId, count: Option<usize>) -> Result {
        let (line, col) = self.menu_here(w);
        let width = self.menu.name_width;
        let name = match self.menu_row(line) {
            Some(i) => self.menu.rows[i].cols[NAME].chars().count(),
            None => 0,
        };
        let starts = [0, 1, 2, 4, 5 + width.max(name), width + 13, width + 30];
        let n = match count {
            Some(n) if n >= TITLES.len() => return Err(Error::Msg("Invalid column number".into())),
            Some(n) => n,
            None if line <= self.menu.rows.len() => {
                starts.iter().rposition(|s| *s <= col).unwrap_or(0)
            }
            None => return Err(Error::Msg("Cannot sort by nil".into())),
        };
        self.menu.sort = Some((n, self.menu.sort == Some((n, false))));
        let buf = self.menu_row(line).map(|i| self.menu.rows[i].buf);
        self.menu_sort();
        self.menu_print(w, buf, col)
    }

    fn menu_put(&mut self, w: WindowId, buf: BufferId) {
        self.set_window_buffer(w, buf);
        self.focus(w);
    }

    /// Emacs's `other-buffer`: the buffer used last that isn't the menu and that no window shows,
    /// or else the one used last, or else a new `*scratch*`.
    fn menu_other_buffer(&mut self) -> BufferId {
        let menu = self.find_buffer(MENU);
        let named = |b: &BufferId| self.buffer_name(*b).is_some_and(|n| !n.starts_with(' '));
        let all = self
            .fallback_buffers()
            .into_iter()
            .filter(|b| Some(*b) != menu);
        let all: Vec<_> = all.filter(named).collect();
        let unseen = all.iter().find(|b| self.windows_showing(**b).is_empty());
        match unseen.or(all.first()) {
            Some(b) => *b,
            None => self.create_text("*scratch*", ""),
        }
    }

    /// Show `buf` in another window and go there, in the one `switch-to-buffer-other-window`
    /// picks: one showing it already, or a split of this one if it's alone or 160 columns wide,
    /// Emacs's `split-width-threshold`, to the right if it's that wide, or else the one used
    /// longest ago.
    fn menu_pop(&mut self, w: WindowId, buf: BufferId) -> Result {
        let others: Vec<_> = self
            .windows_by_use()
            .into_iter()
            .filter(|o| *o != w)
            .collect();
        let wide = self.window_size(w).is_some_and(|(cols, _)| cols >= 160);
        let to = match others.iter().find(|o| self.window_buffer(**o) == Some(buf)) {
            Some(o) => *o,
            None if !wide && !others.is_empty() => others[others.len() - 1],
            None => self.split_window(w, if wide { Side::Right } else { Side::Below })?,
        };
        self.menu_put(to, buf);
        Ok(())
    }

    /// Show this line's buffer: in this window (`go`), in another (`gO`), alone (`1`), beside
    /// what `other-buffer` gives (`2`), or alone with each marked `>` in a window below, the marks
    /// taken off (`gv`). All but the first two put the menu last in the buffer list.
    fn menu_select(&mut self, w: WindowId, how: &str) -> Result {
        let (buf, menu) = (self.menu_line_buffer(w)?.1, self.menu_buf()?);
        match how {
            "go" => self.menu_put(w, buf),
            "gO" => return self.menu_pop(w, buf),
            "1" => {
                self.menu_put(w, buf);
                self.only_window(w)?;
            }
            "2" => {
                self.only_window(w)?;
                let other = self.menu_other_buffer();
                self.menu_put(w, other);
                self.menu_pop(w, buf)?;
            }
            _ => {
                let mut shown = vec![];
                for i in 0..self.menu.rows.len() {
                    if self.menu.rows[i].cols[0] == ">" {
                        self.menu_set_col(i, 0, " ")?;
                        shown.push(self.menu.rows[i].buf);
                    }
                }
                shown.retain(|b| *b != buf && self.is_live(*b));
                self.only_window(w)?;
                self.menu_put(w, buf);
                let mut last = w;
                for b in shown {
                    last = self.split_window(last, Side::Below)?;
                    self.set_window_buffer(last, b);
                }
                self.balance_frame();
                self.focus(w);
            }
        }
        if how != "go" {
            self.buffers[menu].used = 0;
        }
        Ok(())
    }

    /// `q`: back to what the window showed before the menu, or else `other-buffer`'s, and the menu
    /// goes last in the buffer list, as `quit-window` does.
    fn menu_quit(&mut self, w: WindowId) -> Result {
        let menu = self.menu_buf()?;
        let back = self
            .menu
            .came_from
            .filter(|(from, b)| *from == w && self.is_live(*b));
        let back = match back {
            Some((_, b)) => b,
            None => self.menu_other_buffer(),
        };
        self.menu_put(w, back);
        self.buffers[menu].used = 0;
        Ok(())
    }
}

/// A row as tabulated-list prints it and Emacs shows it: the name runs into the size's room as
/// far as the widest size `near` it leaves, in its row and those either side, and is cut with `…`
/// past that, as a mode is at 16 and a size at 7. The columns after it stay where they'd be.
/// Returns the row and the name's length in bytes, from byte 4.
fn print_row(cols: &[String; 7], width: usize, near: usize) -> (String, usize) {
    let cut = |s: &str, room: usize| match s.chars().count() > room {
        true => s.chars().take(room - 1).chain(['…']).collect(),
        false => s.to_string(),
    };
    let [c, r, m, name, size, mode, file] = cols;
    let size = cut(size, 7);
    let name = cut(name, width + 7 - near.min(7));
    let room = 7 - name.chars().count().saturating_sub(width);
    let mode = cut(mode, 16);
    let row = format!("{c}{r}{m} {name:<width$} {size:>room$} {mode:<16} {file}");
    (row.trim_end().into(), name.len())
}
