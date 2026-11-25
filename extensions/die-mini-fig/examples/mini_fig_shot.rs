//! mini_fig_shot: a `.fig` drawn by mini-fig and written as a PNG, to judge the look without
//! DIE, beside die-fig's `fig_shot`:
//!
//! ```sh
//! cargo run --release -p die-mini-fig --example mini_fig_shot -- <spec> <out.png> \
//!     [--size 1600x1000] [--scale 2] [--hover X,Y] [--cursor N] [--scroll PX]
//! ```
//!
//! The size is the area's in logical px, the scale the display's. `--hover` puts the pointer at
//! a place and shows the card of what's there; `--cursor` shows the card of the nth hit in key
//! order, as the keys would; `--scroll` scrolls the rows up under the header. It prints where
//! each panel landed, how many hits there are and what the card says. A spec that can't be
//! read is drawn as the line saying why.

use die_test_cap as _;

use die_draw::Book;
use die_mini_fig::canvas::{Canvas, Rect, Theme};
use die_mini_fig::{dash, hit, spec};
use std::path::Path;

fn main() {
    if let Err(wrong) = run() {
        eprintln!("mini_fig_shot: {wrong}");
        std::process::exit(1);
    }
}

/// Two numbers with `sep` between.
fn pair(v: &str, sep: char) -> Result<(f32, f32), String> {
    let (a, b) = v.split_once(sep).ok_or(format!("{v}: not two numbers"))?;
    let num = |s: &str| {
        s.trim()
            .parse::<f32>()
            .map_err(|_| format!("{v}: not numbers"))
    };
    Ok((num(a)?, num(b)?))
}

fn run() -> Result<(), String> {
    let (mut size, mut scale, mut scroll) = ((1600.0, 1000.0), 2.0, 0.0);
    let (mut hover, mut cursor, mut files) = (None, None, Vec::new());
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = || args.next().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--size" => size = pair(&value()?, 'x')?,
            "--scale" => scale = value()?.parse().map_err(|_| "--scale: not a number")?,
            "--scroll" => scroll = value()?.parse().map_err(|_| "--scroll: not a number")?,
            "--hover" => hover = Some(pair(&value()?, ',')?),
            "--cursor" => cursor = Some(value()?.parse::<usize>().map_err(|_| "--cursor: N")?),
            _ => files.push(a),
        }
    }
    let [spec_path, out] = files.as_slice() else {
        return Err("usage: mini_fig_shot <spec> <out.png> [--size WxH] [--scale S] [--hover X,Y] [--cursor N] [--scroll PX]".into());
    };
    let mut book = Book::load(None)?;
    let th = Theme::default();
    let mut c = Canvas::new(&mut book, size, scale, th.surface).ok_or("no pixels at that size")?;
    match spec::read(Path::new(spec_path)).map(dash::Ready::new) {
        Err(why) => {
            dash::message(&mut c, &th, &why);
            println!("can't draw: {why}");
        }
        Ok(ready) => {
            let drawn = dash::draw(&mut c, &th, &ready, scroll);
            for (i, f) in drawn.frames.iter().enumerate() {
                let n = drawn.hits.iter().filter(|h| h.key.0 == i).count();
                println!(
                    "panel {i} at {:.0},{:.0} {:.0}x{:.0}: {n} hits",
                    f.x, f.y, f.w, f.h
                );
            }
            println!("rows end at {:.0}; {} hits", drawn.height, drawn.hits.len());
            let at = hover.and_then(|(x, y)| hit::at(&drawn.hits, x, y));
            if let Some(h) = cursor.or(at).and_then(|i| drawn.hits.get(i)) {
                hit::card(&mut c, &th, h, Rect::new(0.0, 0.0, size.0, size.1));
                println!("card {:?} \"{}\": {:?}", h.key, h.title, h.rows);
            }
        }
    }
    let (w, h) = (c.pixmap.width(), c.pixmap.height());
    let png = die_draw::image::encode(w, h, c.pixmap.data())?;
    std::fs::write(out, png).map_err(|e| format!("{out}: {e}"))
}
