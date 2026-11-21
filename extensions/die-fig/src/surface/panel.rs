//! The detail panel in a window (contract-p3 §4, briefs/mouse-ux.md §4.10): `⏎` opens it for
//! what the cursor is on, beside the figure (which lays out in what's left), and `⏎` again or
//! `Esc` closes it; `Tab` gives it the keys and `S-Tab` gives them back, and while it has them
//! `j k h l gg G y Y ⏎` are its own. It follows the cursor while it's open, and the data as it
//! changes. The pointer: a row that names a mark brings it forward in the figure, a click moves
//! the panel's cursor (and gives it the keys), the triangle or a double-click folds a branch, a
//! double-click on a row that names a mark takes the figure's cursor there, and the wheel
//! scrolls it. It's drawn on the base, beside the figure, and alone when only it changed.

use super::{FigState, Kit, Pane, measured};
use crate::data::SeriesKey;
use crate::detail::{self, DetailTree, Item, PanelAction, PanelKey, PanelState};
use crate::graph::GraphModel;
use crate::interact::consts::{CLICK_SLOP, SCROLL_BURST_MS};
use crate::interact::{Cursor, Gesture, View};
use crate::plot::ChartModel;
use crate::raster::{self, PixelBox};
use crate::scene::{Hit, Rect};
use crate::theme::Theme;
use die_core::{Button, Ctx, PointerKind, RegKind, Register, SurfacePointer};
use std::collections::BTreeSet;
use std::time::Duration;

/// How the yanked text reaches the registers: die-vim's kill (the clipboard and the ring), as
/// your config gives it ([`crate::Fig::copying_with`]).
pub type Kill = fn(&mut Ctx, Register);

/// The detail panel open in a window.
pub struct Panel {
    /// Its rows.
    pub tree: DetailTree,
    /// Its cursor, scroll and focus.
    pub state: PanelState,
    /// The mark it's the detail of.
    pub item: Option<Item>,
    /// The figure's cursor it follows: a new one brings a new tree.
    pub(super) cursor: Option<Cursor>,
    /// What it was built from: the figure's model and the view's hidden set and solo, which a
    /// chart's trees leave hidden series out by.
    pub(super) built: (u64, BTreeSet<SeriesKey>, Option<SeriesKey>),
    /// Goes up with each change to what it draws.
    pub rev: u64,
    /// What the wheel hasn't scrolled yet, in rows: less than one.
    pub(super) carry: f32,
}

/// What a panel's trees come from: the figure's chart or graph.
#[derive(Clone, Copy)]
pub(super) struct Source<'a> {
    pub chart: Option<&'a ChartModel>,
    pub graph: Option<&'a GraphModel>,
}

impl Source<'_> {
    /// The tree of what an item names.
    fn tree(&self, view: &View, item: &Item) -> Option<DetailTree> {
        match (self.graph, self.chart) {
            (Some(g), _) => g.detail_of(item),
            (None, Some(c)) => c.detail_of(view, item),
            _ => None,
        }
    }

    /// The item the cursor is on.
    fn item(&self, cursor: &Cursor) -> Option<Item> {
        match self.graph {
            Some(g) => g.item_of(cursor),
            None => Item::of_cursor(cursor),
        }
    }
}

/// Where the panel goes in an area of `size`, and what's left for the figure.
pub fn rects(size: (f32, f32)) -> (Rect, Rect) {
    let (fig, panel, _) = detail::split(size);
    (fig, panel)
}

impl Pane {
    /// The figure's size: the area's, or what the panel leaves.
    pub fn figure_size(&self) -> (f32, f32) {
        let size = self.size();
        match self.panel {
            Some(_) => {
                let (f, _) = rects(size);
                (f.w, f.h)
            }
            None => size,
        }
    }

    /// The panel's rectangle in the area, while it's open.
    pub fn panel_rect(&self) -> Option<Rect> {
        self.panel.as_ref().map(|_| rects(self.size()).1)
    }

    /// Whether the panel has the keys.
    pub fn panel_focused(&self) -> bool {
        self.panel.as_ref().is_some_and(|p| p.state.focused)
    }

    /// The panel's tree and state in line with the figure: built again for the cursor's mark
    /// when the cursor moved, for its own mark when the data or the hidden set changed (its
    /// place kept where the mark still has the row), and closed when its mark is gone.
    pub(super) fn sync_panel(&mut self, src: Source<'_>, model_gen: u64) {
        let Some(panel) = self.panel.as_mut() else {
            return;
        };
        let moved = self.cursor.is_some() && self.cursor != panel.cursor;
        let built = (model_gen, self.view.hidden.clone(), self.view.only.clone());
        if !moved && built == panel.built {
            return;
        }
        let item = match moved {
            true => self.cursor.as_ref().and_then(|c| src.item(c)),
            false => panel.item.clone(),
        };
        let tree = item.as_ref().and_then(|i| src.tree(&self.view, i));
        let Some(tree) = tree else {
            if moved {
                // The cursor is on nothing with a detail: the panel keeps what it showed.
                panel.cursor = self.cursor.clone();
                return;
            }
            self.panel = None;
            self.drawn = None;
            return;
        };
        let same = panel.item == item;
        let mut state = PanelState::open(&tree, item.as_ref());
        state.focused = panel.state.focused;
        if same && !moved {
            // The same mark's tree again: the place as it was, where it still is.
            if panel.state.row < tree.rows.len() {
                state.row = panel.state.row;
            }
            state.top = panel.state.top;
        }
        (panel.tree, panel.state, panel.item) = (tree, state, item);
        panel.cursor = self.cursor.clone();
        panel.built = built;
        panel.rev += 1;
        // The row under the pointer names something else now, or nothing: its mark goes back
        // until the pointer moves again.
        if self.row_hover.take().is_some() {
            self.rehover = true;
        }
        self.reveal_panel();
    }

    /// The panel's cursor's row brought into view.
    pub(super) fn reveal_panel(&mut self) {
        let Some(rect) = self.panel_rect() else {
            return;
        };
        if let Some(p) = self.panel.as_mut() {
            detail::reveal(&p.tree, &mut p.state, rect);
        }
    }
}

impl FigState {
    /// The panel's source: the figure's chart or graph.
    pub(super) fn panel_source(&self) -> Source<'_> {
        Source {
            chart: self.model.as_deref(),
            graph: self.graph.as_deref(),
        }
    }

    /// Open the detail panel in window `w` for `item`, else what the cursor is on. Why not, if
    /// it can't.
    pub fn open_panel(&mut self, w: die_core::WindowId, item: Option<Item>) -> Result<(), String> {
        let src = self.panel_source();
        let p = self
            .panes
            .get(&w)
            .ok_or("fig: the figure isn't shown yet")?;
        let item = match item {
            Some(i) => i,
            None => {
                let c = (p.cursor.as_ref())
                    .ok_or("fig: no cursor yet: h l j k or a click puts it on the data")?;
                src.item(c)
                    .ok_or("fig: the cursor is on nothing with a detail")?
            }
        };
        let tree = src
            .tree(&p.view, &item)
            .ok_or("fig: the cursor is on nothing with a detail")?;
        let state = PanelState::open(&tree, Some(&item));
        let built = (self.model_gen, p.view.hidden.clone(), p.view.only.clone());
        let cursor = p.cursor.clone();
        let Some(p) = self.panes.get_mut(&w) else {
            return Ok(());
        };
        let rev = p.panel.as_ref().map_or(0, |x| x.rev + 1);
        p.panel = Some(Panel {
            tree,
            state,
            item: Some(item),
            cursor,
            built,
            rev,
            carry: 0.0,
        });
        p.reveal_panel();
        p.rehover = true;
        self.rev += 1;
        Ok(())
    }

    /// Close window `w`'s panel; false if none was open.
    pub fn close_panel(&mut self, w: die_core::WindowId) -> bool {
        let Some(p) = self.panes.get_mut(&w) else {
            return false;
        };
        if p.panel.take().is_none() {
            return false;
        }
        p.row_hover = None;
        p.rehover = true;
        self.rev += 1;
        true
    }

    /// `Tab` and `S-Tab`: the keys to the panel and back (ruling 7's focus stops: the figure,
    /// then the panel). False with no panel open.
    pub fn focus_panel(&mut self, w: die_core::WindowId) -> bool {
        let Some(panel) = self.panes.get_mut(&w).and_then(|p| p.panel.as_mut()) else {
            return false;
        };
        panel.state.focused = !panel.state.focused;
        panel.rev += 1;
        true
    }

    /// One of the panel's keys, `n` times: a move or a fold brings its row into view, a yank
    /// goes to the registers, and `⏎` on a row that names a mark takes the figure's cursor
    /// there (and the panel follows it).
    pub fn panel_key(
        &mut self,
        ctx: &mut Ctx,
        w: die_core::WindowId,
        key: PanelKey,
        n: u32,
    ) -> die_core::Result {
        let kill = self.kill;
        let mut jump = None;
        {
            let Some(p) = self.panes.get_mut(&w) else {
                return Ok(());
            };
            let rect = p.panel_rect();
            let Some(panel) = p.panel.as_mut() else {
                return Ok(());
            };
            for _ in 0..n.clamp(1, 10_000) {
                match detail::keys(&mut panel.tree, &mut panel.state, key) {
                    PanelAction::Moved | PanelAction::Folded => {
                        if let Some(r) = rect {
                            detail::reveal(&panel.tree, &mut panel.state, r);
                        }
                        panel.rev += 1;
                    }
                    PanelAction::Yank(text) => {
                        yank(ctx, kill, text);
                        break;
                    }
                    PanelAction::Jump(item) => {
                        jump = Some(item);
                        break;
                    }
                    PanelAction::None => break,
                }
            }
        }
        if let Some(item) = jump {
            self.jump(ctx, w, &item);
        }
        self.rev += 1;
        Ok(())
    }

    /// The figure's cursor to what a panel row names: a sample, a band, a cell, a node (an
    /// edge has nowhere of its own); the view keeps it in sight. On a chart, a row naming a
    /// series, or the value the cursor is on already, opens the series' own tree in the panel
    /// (its count, least, most, mean and last), the cursor where it was.
    pub fn jump(&mut self, ctx: &mut Ctx, w: die_core::WindowId, item: &Item) {
        let now = ctx.now();
        let here = self.panes.get(&w).and_then(|p| p.cursor.clone());
        let series = match item {
            Item::Series(k) => Some(k.clone()),
            Item::Point { series, .. } if item.cursor() == here => Some(series.clone()),
            _ => None,
        };
        if let (None, Some(k)) = (&self.graph, series) {
            let focused = self.panes.get(&w).is_some_and(|p| p.panel_focused());
            if let Err(why) = self.open_panel(w, Some(Item::Series(k))) {
                ctx.messages.push(why);
            }
            if let Some(panel) = self.panes.get_mut(&w).and_then(|p| p.panel.as_mut()) {
                panel.state.focused = focused;
            }
            return;
        }
        if self.graph.is_some() {
            if let Some(c) = item.cursor() {
                self.graph_act(w, now, |g| g.put_cursor(c));
            }
            return;
        }
        let Some(c) = self.model.as_ref().and_then(|m| m.cursor_of(item)) else {
            return;
        };
        self.act(w, now, |a| a.put_cursor(c));
    }

    /// A pointer event the panel takes: over it with no gesture under way, a press on it and
    /// what follows from that press, and a scroll burst begun over it. False for the figure's.
    pub(super) fn panel_pointer(
        &mut self,
        ctx: &mut Ctx,
        p: &SurfacePointer,
        at: (f32, f32),
        swallow: bool,
    ) -> bool {
        let w = p.window;
        let me = self.me.clone();
        let Some(pane) = self.panes.get_mut(&w) else {
            return false;
        };
        let Some(rect) = pane.panel_rect() else {
            return false;
        };
        let inside = rect.contains(at.0, at.1);
        let mut jump = None;
        let took = match p.kind {
            PointerKind::Move if !pane.gesturing() => {
                let row = match (inside, pane.hit_at(at), pane.panel.as_ref()) {
                    (true, Hit::Detail(i), Some(panel)) => {
                        panel.tree.rows.get(i as usize).and_then(|r| r.item.clone())
                    }
                    _ => None,
                };
                if row != pane.row_hover {
                    pane.row_hover = row;
                    pane.rehover = true;
                }
                if inside {
                    // Over the panel the figure's hover goes, as when the pointer leaves it.
                    pane.hovering = false;
                    pane.rehover = true;
                }
                inside
            }
            PointerKind::Press(b) if inside && !pane.gesturing() => {
                pane.help = false;
                pane.held = Some(b);
                pane.panel_press = true;
                if swallow || b != Button::Left {
                    pane.gesture = Gesture::Swallowed;
                    return true;
                }
                let hit = pane.hit_at(at);
                if p.clicks >= 2 {
                    pane.gesture = Gesture::Swallowed;
                    jump = pane.panel_double(&hit);
                } else {
                    pane.gesture = Gesture::Pressed {
                        at,
                        hit,
                        button: b,
                        mods: p.mods,
                        clicks: p.clicks,
                    };
                }
                true
            }
            PointerKind::Drag(_) if pane.panel_press => true,
            PointerKind::Release(b) if pane.panel_press => {
                if pane.held.is_some_and(|h| h != b) {
                    return true;
                }
                (pane.held, pane.panel_press) = (None, false);
                let g = std::mem::replace(&mut pane.gesture, Gesture::None);
                if let Gesture::Pressed { hit, at: from, .. } = g
                    && (from.0 - at.0).hypot(from.1 - at.1) < CLICK_SLOP
                {
                    pane.panel_click(&hit);
                }
                true
            }
            PointerKind::Scroll { dy, .. } => {
                let ours = match &pane.burst {
                    Some(b) => b.panel,
                    None => inside && !pane.gesturing(),
                };
                if !ours || !dy.is_finite() {
                    return ours;
                }
                let timer = ctx.after(Duration::from_millis(SCROLL_BURST_MS), move |ctx| {
                    if let Some(s) = me.upgrade() {
                        let mut st = s.borrow_mut();
                        st.end_burst(w);
                        st.refresh(ctx);
                    }
                    Ok(())
                });
                match &mut pane.burst {
                    Some(b) => b._timer = timer,
                    None => {
                        pane.burst = Some(super::Burst {
                            zoom: false,
                            axis: None,
                            across: None,
                            panel: true,
                            before: pane.view.clone(),
                            _timer: timer,
                        });
                    }
                }
                let cell = pane.cell.height as f32 / pane.scale();
                if let Some(panel) = pane.panel.as_mut() {
                    let by = dy * cell / detail::ROW_H + panel.carry;
                    let whole = by.trunc();
                    panel.carry = by - whole;
                    if whole != 0.0 {
                        detail::scroll(&panel.tree, &mut panel.state, rect, whole as i32);
                        panel.rev += 1;
                    }
                }
                true
            }
            _ => false,
        };
        if let Some(item) = jump {
            self.jump(ctx, w, &item);
        }
        took
    }
}

impl Pane {
    /// A click on the panel, on its release: a fold triangle folds or unfolds; a row takes the
    /// panel's cursor; either gives the panel the keys, as a click anywhere on it does.
    fn panel_click(&mut self, hit: &Hit) {
        let rect = self.panel_rect();
        let Some(panel) = self.panel.as_mut() else {
            return;
        };
        match hit {
            Hit::DetailFold(i) => {
                detail::toggle(&mut panel.tree, &mut panel.state, *i as usize);
            }
            Hit::Detail(i) => {
                let i = *i as usize;
                let rests =
                    (panel.tree.rows.get(i)).is_some_and(|r| r.kind != detail::RowKind::Section);
                if rests {
                    panel.state.row = i;
                }
            }
            _ => {}
        }
        panel.state.focused = true;
        if let Some(r) = rect {
            detail::reveal(&panel.tree, &mut panel.state, r);
        }
        panel.rev += 1;
    }

    /// A double click on the panel, at its second press: a branch folds or unfolds, and a row
    /// that names a mark is the item the figure's cursor goes to.
    fn panel_double(&mut self, hit: &Hit) -> Option<Item> {
        let panel = self.panel.as_mut()?;
        // On the triangle, the first click folded or unfolded the branch already.
        let Hit::Detail(i) = hit else {
            return None;
        };
        let i = *i as usize;
        let row = panel.tree.rows.get(i)?;
        panel.state.focused = true;
        panel.rev += 1;
        if row.kind == detail::RowKind::Branch {
            detail::toggle(&mut panel.tree, &mut panel.state, i);
            return None;
        }
        panel.state.row = i;
        row.item.clone()
    }
}

/// The yanked text to the registers: through die-vim's kill where your config gives it (the
/// clipboard and the kill ring, as Evil's `y`), else the unnamed register and the clipboard.
fn yank(ctx: &mut Ctx, kill: Option<Kill>, text: String) {
    let shown: String = text.chars().take(40).collect();
    let more = if text.chars().count() > 40 { "…" } else { "" };
    let r = Register {
        kind: RegKind::Char,
        text,
    };
    match kill {
        Some(kill) => kill(ctx, r),
        None => {
            ctx.write_clipboard(&r);
            ctx.set_register('"', r);
        }
    }
    ctx.messages.push(format!("Copied {shown}{more}"));
}

/// The key the panel's last drawing is under: its tree's revision and state, where it is, and
/// the colours.
pub(super) type PanelKey2 = (u64, PanelState, [u32; 4], u64);

/// The key for a pane's panel as it stands.
pub(super) fn panel_key_of(p: &Pane, theme_gen: u64) -> Option<PanelKey2> {
    let panel = p.panel.as_ref()?;
    let r = p.panel_rect()?;
    Some((
        panel.rev,
        panel.state,
        [r.x.to_bits(), r.y.to_bits(), r.w.to_bits(), r.h.to_bits()],
        theme_gen,
    ))
}

/// The panel's scene at its rectangle in the area.
pub(super) fn panel_scene(p: &Pane, theme: &Theme, kit: &mut Kit) -> Option<crate::scene::Scene> {
    let panel = p.panel.as_ref()?;
    let rect = p.panel_rect()?;
    let scale = p.scale();
    Some(measured(kit.book.as_mut(), scale, |m| {
        detail::panel(&panel.tree, panel.state, rect, scale, theme, m)
    }))
}

/// Draw the panel alone, where only it changed: its rectangle of the base, presented alone,
/// and its regions in place of the last.
pub(super) fn draw_panel_only(p: &mut Pane, theme: &Theme, kit: &mut Kit) {
    let Some(scene) = panel_scene(p, theme, kit) else {
        return;
    };
    let Some(rect) = p.panel_rect() else {
        return;
    };
    let scale = p.scale();
    let (aw, ah) = p.area_px();
    let Some(producer) = p.base.as_mut() else {
        return;
    };
    let (w, h, stride, px) = super::target(producer);
    let b: PixelBox = raster::to_physical(rect, scale, aw.min(w), ah.min(h));
    if b.is_empty() {
        return;
    }
    raster::draw(
        &scene,
        px,
        w,
        h,
        stride,
        Some(b),
        Some(theme.surface),
        kit.book.as_mut(),
    );
    producer.present(b.into());
    if let Some(s) = p.scene.as_mut() {
        s.regions.truncate(p.fig_regions);
        s.regions.extend(scene.regions);
    }
}
