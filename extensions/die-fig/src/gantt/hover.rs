//! What the pointer is on in a gantt: a task, by its bar or its label.

use super::{GanttLayout, GanttModel};
use crate::interact::Cursor;

/// What the pointer can be on: a task, by its index in the model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    /// A task.
    Task(u32),
}

impl Target {
    /// The task.
    pub fn task(self) -> usize {
        match self {
            Target::Task(i) => i as usize,
        }
    }

    /// Where a click on it puts the cursor: on the task.
    pub fn cursor(self, model: &GanttModel) -> Cursor {
        model.cursor(self.task())
    }
}

/// What the pointer at (x, y) is on: the task whose bar holds it (and a little round it: see
/// [`super::TaskGeom::reach`]), the nearest across where two bars' reaches meet; else the task
/// whose label's cell in the gutter holds it. None on a heading, on the plot off any bar (the
/// overlay then says the date there), and off the rows.
pub fn hover(_model: &GanttModel, layout: &GanttLayout, x: f32, y: f32) -> Option<Target> {
    let p = layout.plot;
    if y < p.y || y >= p.bottom() {
        return None;
    }
    let in_gutter = x >= layout.gutter.x && x < p.x - 4.0;
    let in_plot = x >= p.x && x < p.right();
    let mut best: Option<(f32, usize)> = None;
    for (i, g) in layout.tasks.iter().enumerate() {
        let Some(g) = g else { continue };
        if y < g.strip.y || y >= g.strip.bottom() {
            continue;
        }
        if in_gutter {
            return Some(Target::Task(i as u32));
        }
        if in_plot && g.reach().contains(x, y) {
            let mid = g.bar.x + g.bar.w / 2.0;
            let d = ((x - mid).abs() - g.bar.w / 2.0).max(0.0);
            if best.is_none_or(|(b, _)| d < b) {
                best = Some((d, i));
            }
        }
    }
    best.map(|(_, i)| Target::Task(i as u32))
}
