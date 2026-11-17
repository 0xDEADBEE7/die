//! One buffer's rendered view: the mode a markdown file has, which holds whether the view is on,
//! the decorations that make it, and the faces it gives each frame.
//!
//! The view is the buffer's own text under stored decorations (contract §9, C16): a Hide for
//! each piece of markup, with what shows in its place, virtual text for a code block's padding,
//! and a keymap over each link. Faces aren't stored: `decorate` gives the frame's from the plan.
//! Turning it off removes every one of them, and the text was never touched, so raw is exactly
//! what it was.
//!
//! The view is for reading: while it's on, the buffer is read-only to you. A hidden mark has no
//! cursor position, so a key that works on a character works on the marks beside it too, and
//! `x` on a link's last letter would take its address with it, unseen. Raw is where you edit.
//! The text can still change under the view, by a reload from disk, and the view follows.
use crate::plan::{self, Plan, Shown};
use die_core::{
    BufferId, Ctx, Deco, DecorationId, Mode, Result, Reveal, Styled, TextBuf, WindowId,
};
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

/// The buffers whose rendered view is on, each with whether it was read-only before the view
/// made it so, which the extension keeps over a restart. A view puts its buffer in when it's
/// turned on, and takes it out when it's turned off or dropped.
pub(crate) type Rendered = Rc<RefCell<Vec<(BufferId, bool)>>>;

/// The width a plan is made for when no window shows the buffer and it has no maximum width.
const WIDTH: usize = 80;

/// What a decoration of the view is, as the plan had it.
#[derive(Clone, PartialEq)]
enum What {
    Hide(Option<Shown>),
    Virtual(Shown),
    Link,
}

impl What {
    fn deco(&self) -> Deco {
        match self.clone() {
            // A search that lands in hidden markup leaves it hidden: the view isn't the place
            // to read a link's address, and raw is one key away.
            What::Hide(shown) => Deco::Hide {
                replacement: shown.map(styled),
                reveal: Reveal::Never,
            },
            What::Virtual(shown) => Deco::Virtual(styled(shown)),
            What::Link => Deco::Keymap(crate::LINK_KEYS.into()),
        }
    }
}

/// The mode: a markdown buffer's view, raw until it's toggled.
pub struct View {
    /// Your `front_matter` option.
    front_matter: bool,
    rendered: Rendered,
    /// The buffer, once the view has been on.
    buf: Option<BufferId>,
    on: bool,
    /// Whether the buffer was read-only for a reason of its own when the view came on, which
    /// raw gives it back. A file that can't be edited is core's to say, then as now.
    read_only: bool,
    /// The decorations the view on screen is made of, in the plan's order, each with what the
    /// plan said it was.
    pieces: Vec<(DecorationId, What)>,
    /// The plan's faces, in stacking order.
    faces: Vec<(Range<usize>, &'static str)>,
    /// The revision and the width the plan on screen was made for.
    rev: u64,
    width: usize,
}

impl View {
    pub(crate) fn new(rendered: Rendered, front_matter: bool) -> View {
        View {
            front_matter,
            rendered,
            buf: None,
            on: false,
            read_only: false,
            pieces: vec![],
            faces: vec![],
            rev: 0,
            width: 0,
        }
    }

    /// Whether the rendered view is on.
    pub fn is_rendered(&self) -> bool {
        self.on
    }

    /// The other view: rendered from raw, raw from rendered.
    pub(crate) fn toggle(&mut self, buf: BufferId, ctx: &mut Ctx) -> Result {
        if self.on {
            self.clear(buf, ctx)
        } else {
            self.render(buf, ctx)
        }
    }

    /// The view as a restart found it: on, over a buffer whose own read-only setting was
    /// `read_only` before the view. The session kept the setting as the view had it.
    pub(crate) fn restore(&mut self, buf: BufferId, read_only: bool, ctx: &mut Ctx) -> Result {
        ctx.change_settings(buf, |s| s.read_only = read_only)?;
        self.render(buf, ctx)
    }

    /// Plan the text as it is now and make the buffer's decorations the plan's, as one change.
    /// It turns the view on. A plan core refuses changes nothing, and is the error.
    ///
    /// Core has moved the view's decorations through every edit since the last plan, so most
    /// of them are already where the new plan wants them: a letter typed in a paragraph moves
    /// nothing but what's after it, all by one. Only the stretch between the first piece that
    /// differs and the last is swapped, so a line changed on disk in a long file costs the plan
    /// and a comparison, not a new set of thousands of decorations.
    pub(crate) fn render(&mut self, buf: BufferId, ctx: &mut Ctx) -> Result {
        let width = width(ctx, buf);
        let t = ctx.text(buf)?;
        let (rev, tab_width) = (t.rev(), t.settings().tab_width);
        let options = plan::Options {
            width,
            tab_width,
            front_matter: self.front_matter,
        };
        let source = t.rope().to_string();
        let Plan {
            hides,
            virtuals,
            faces,
            links,
            ..
        } = plan::plan(&source, &options);
        let hides = hides.into_iter().map(|h| (h.range, What::Hide(h.shown)));
        let virtuals = virtuals
            .into_iter()
            .map(|v| (v.at..v.at, What::Virtual(v.shown)));
        let links = links.into_iter().map(|l| (l.text, What::Link));
        let mut new: Vec<(Range<usize>, What)> = hides.chain(virtuals).chain(links).collect();

        let t = ctx.text_mut(buf)?;
        let same = |(id, what): &(DecorationId, What), (range, wanted): &(Range<usize>, What)| {
            what == wanted && t.decoration(*id).is_some_and(|d| d.range == *range)
        };
        let head = self.pieces.iter().zip(&new);
        let mut head = head.take_while(|(p, n)| same(p, n)).count();
        let tail = self.pieces[head..]
            .iter()
            .rev()
            .zip(new[head..].iter().rev());
        let mut tail = tail.take_while(|(p, n)| same(p, n)).count();
        // Virtual text at one position shows in the order it was added, and a piece swapped in
        // is added last. So the pieces of one position go together: a kept one that shares its
        // position with a swapped one beside it is swapped too.
        let together = |a: usize, b: usize| {
            let at = |i: usize| matches!(new[i].1, What::Virtual(_)).then_some(new[i].0.start);
            at(a).is_some() && at(a) == at(b)
        };
        while head > 0 && head < new.len() - tail && together(head - 1, head) {
            head -= 1;
        }
        while tail > 0
            && head < new.len() - tail
            && together(new.len() - tail - 1, new.len() - tail)
        {
            tail -= 1;
        }
        let stale = head..self.pieces.len() - tail;
        let fresh: Vec<(Range<usize>, What)> = new.drain(head..new.len() - tail).collect();
        if !stale.is_empty() || !fresh.is_empty() {
            let old: Vec<DecorationId> = self.pieces[stale.clone()].iter().map(|p| p.0).collect();
            let decos = fresh.iter().map(|(r, what)| (r.clone(), what.deco()));
            let ids = t.redecorate(&old, decos.collect())?;
            let pieces = ids.into_iter().zip(fresh.into_iter().map(|f| f.1));
            self.pieces.splice(stale, pieces);
        }
        (self.faces, self.rev, self.width) = (faces, rev, width);
        self.buf = Some(buf);
        if !self.on {
            self.on = true;
            self.read_only = ctx.settings(buf)?.read_only && !uneditable(ctx, buf);
            self.rendered.borrow_mut().push((buf, self.read_only));
        }
        // Each time, since a file that becomes editable again takes the setting back.
        ctx.change_settings(buf, |s| s.read_only = true)
    }

    /// Back to raw: every decoration of the view goes, and the text is as it always was.
    pub(crate) fn clear(&mut self, buf: BufferId, ctx: &mut Ctx) -> Result {
        self.on = false;
        self.faces.clear();
        self.rendered.borrow_mut().retain(|b| b.0 != buf);
        let ids: Vec<DecorationId> = self.pieces.drain(..).map(|p| p.0).collect();
        ctx.text_mut(buf)?.redecorate(&ids, vec![])?;
        let read_only = self.read_only || uneditable(ctx, buf);
        ctx.change_settings(buf, |s| s.read_only = read_only)
    }

    /// Plan again if the text or the width has changed since the plan on screen was made. Off,
    /// or with neither changed, this costs a comparison.
    pub(crate) fn refresh(&mut self, buf: BufferId, ctx: &mut Ctx) -> Result {
        if !self.on {
            return Ok(());
        }
        if ctx.text(buf)?.rev() != self.rev || width(ctx, buf) != self.width {
            return self.render(buf, ctx);
        }
        Ok(())
    }
}

impl Mode for View {
    /// The mode has left the buffer, as when it's saved under a name that isn't markdown's: the
    /// view goes with it.
    fn on_disable(&mut self, buf: BufferId, ctx: &mut Ctx) {
        if let Err(e) = self.clear(buf, ctx) {
            ctx.messages.push(format!("markdown: {e}"));
        }
    }

    /// A rule is as long as the text area is wide, and a code block's box stops at its edge.
    fn on_resize(&mut self, _win: WindowId, _cols: usize, _rows: usize, ctx: &mut Ctx) {
        if let Some(buf) = self.buf
            && let Err(e) = self.refresh(buf, ctx)
        {
            ctx.messages.push(format!("markdown: {e}"));
        }
    }

    /// The plan's faces over what's visible. A text the plan wasn't made for gets none: the
    /// plan follows before the next frame (`on_edited`), and its ranges would be off until then.
    fn decorate(
        &mut self,
        text: &TextBuf,
        visible: Range<usize>,
        out: &mut Vec<(Range<usize>, &'static str)>,
    ) {
        if self.on && text.rev() == self.rev {
            let seen = |r: &Range<usize>| r.start < visible.end && r.end > visible.start;
            out.extend(self.faces.iter().filter(|(r, _)| seen(r)).cloned());
        }
    }
}

impl Drop for View {
    /// A killed buffer's mode is dropped, not disabled, so this is where its buffer leaves the
    /// list the session reads.
    fn drop(&mut self) {
        if let Some(buf) = self.buf {
            self.rendered.borrow_mut().retain(|b| b.0 != buf);
        }
    }
}

/// What's shown, as core takes it: the text in its face. [`plan::PLAIN`] is no face at all, so
/// an entity's character draws as the text around it does.
fn styled(shown: Shown) -> Styled {
    let faces = match shown.face {
        plan::PLAIN => vec![],
        face => vec![(0..shown.text.len(), face)],
    };
    Styled {
        text: shown.text,
        faces,
    }
}

/// Whether the buffer's file can't be edited safely, which makes it read-only whatever the view
/// does: core's reason, not the buffer's own setting.
fn uneditable(ctx: &Ctx, buf: BufferId) -> bool {
    ctx.file(buf).is_some_and(|f| f.read_only.is_some())
}

/// The width to plan for: the narrowest text area among the windows showing the buffer, so no
/// rule or box wraps in any of them; with no window on it, its maximum width, or 80.
fn width(ctx: &Ctx, buf: BufferId) -> usize {
    let Ok(t) = ctx.text(buf) else {
        return WIDTH;
    };
    let windows = ctx.window_list().into_iter();
    let showing = windows.filter(|w| ctx.window_buffer(*w) == Some(buf));
    let narrowest = showing
        .filter_map(|w| t.geometry(w))
        .map(|g| g.text_w)
        .min();
    narrowest.or(t.settings().max_width).unwrap_or(WIDTH)
}
