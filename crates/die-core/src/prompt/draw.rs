//! Drawing a prompt's candidates into the scene.
use crate::ext::*;
use crate::faces::Style;
use crate::scene::*;
use unicode_segmentation::UnicodeSegmentation;

impl Ctx {
    /// Draw the prompt's candidates over the frame's last `room` rows, and clear the row above them
    /// for the prompt's own line. The windows keep their size, so nothing under it reflows.
    pub(crate) fn draw_prompt_list(
        &mut self,
        scene: &mut Scene,
        row: usize,
        room: usize,
        base: Style,
    ) {
        if room == 0 {
            return;
        }
        let cols = scene.cols;
        let blank = Cell::styled(Glyph::Char(' '), base);
        for r in row..=row + room {
            for c in 0..cols {
                scene.put(r, c, blank.clone());
            }
        }
        let current = self.faces.get("prompt-current").over(base);
        let matched = self.faces.get("prompt-match");
        let shown = self.prompt_rows(room);
        for (i, (item, hits, chosen)) in shown.iter().enumerate() {
            let r = row + 1 + i;
            let style = if *chosen { current } else { base };
            if *chosen {
                for c in 0..cols {
                    scene.put(r, c, Cell::styled(Glyph::Char(' '), style));
                }
            }
            let hit = matched.over(style);
            let mut at = 0;
            let parts = item.graphemes(true).flat_map(|g| {
                let n = g.chars().count();
                let on = hits.iter().any(|h| (at..at + n).contains(h));
                at += n;
                glyphs(g, if on { hit } else { style }).collect::<Vec<_>>()
            });
            put_text(scene, r, 0, cols, parts);
        }
    }
}
