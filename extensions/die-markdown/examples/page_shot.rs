//! page_shot: a markdown file drawn as the page view draws it, written as a PNG. For looking at
//! the typesetter's work without DIE, and for timing it:
//!
//! ```sh
//! cargo run --release --offline -p die-markdown --example page_shot -- in.md out.png \
//!     [--width PX] [--scale S] [--top Y] [--height H] [--palette NAME] [--focus N] \
//!     [--front-matter] [--zoom N] [--find TEXT]
//! ```
//!
//! The width is the area's, in physical pixels (1800 unless given), the scale the display's (2).
//! `--top` and `--height` choose the rows drawn: the whole page unless given, and never more
//! than 16,000 rows. The palettes are `mono-dark`, `github-dark` and `github-light`. `--focus`
//! rings a link, by its number in reading order. `--zoom` writes each pixel N times as large,
//! for looking at edges, and `--find` starts the picture a little above the line that shows the
//! first TEXT in the source. It prints the page's height, the links,
//! headings and pictures it found, and how long the layout and the paint took.

use die_test_cap as _;

use die_markdown::typeset::{Env, Fonts, Marks, Page, Palette, Picture, Pictures};
use std::path::Path;
use std::time::Instant;

/// The most rows one picture holds.
const TALLEST: u32 = 16_000;

fn main() {
    if let Err(wrong) = run() {
        eprintln!("page_shot: {wrong}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let usage = "page_shot <in.md> <out.png> [--width PX] [--scale S] [--top Y] [--height H] \
                 [--palette mono-dark|github-dark|github-light] [--focus N] [--front-matter] [--zoom N] [--find TEXT]";
    let (Some(input), Some(output)) = (args.next(), args.next()) else {
        return Err(usage.to_owned());
    };
    let (mut width, mut scale, mut top, mut rows) = (1800u32, 2.0f32, 0i32, None::<u32>);
    let (mut palette, mut focus, mut front_matter) = (Palette::mono_dark(), None, false);
    let (mut zoom, mut find) = (1usize, None::<String>);
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or(format!("{flag} needs a value"));
        let number = |v: String| v.parse::<f64>().map_err(|_| format!("{v}: not a number"));
        match flag.as_str() {
            "--width" => width = number(value()?)? as u32,
            "--scale" => scale = number(value()?)? as f32,
            "--top" => top = number(value()?)? as i32,
            "--height" => rows = Some(number(value()?)? as u32),
            "--focus" => focus = Some(number(value()?)? as usize),
            "--front-matter" => front_matter = true,
            "--find" => find = Some(value()?),
            "--zoom" => zoom = (number(value()?)? as usize).clamp(1, 16),
            "--palette" => {
                palette = match value()?.as_str() {
                    "mono-dark" => Palette::mono_dark(),
                    "github-dark" => Palette::github_dark(),
                    "github-light" => Palette::github_light(),
                    other => return Err(format!("{other}: no such palette")),
                }
            }
            other => return Err(format!("{other}: no such flag\n{usage}")),
        }
    }
    let text = std::fs::read_to_string(&input).map_err(|e| format!("{input}: {e}"))?;
    let started = Instant::now();
    let mut fonts = Fonts::load(None)?;
    let loaded = started.elapsed();
    let env = Env {
        width,
        scale,
        palette: &palette,
        dir: Path::new(&input).parent(),
        front_matter,
        tab_width: 8,
    };
    // A first layout says which pictures the page wants; with those answered, the layout
    // that's drawn.
    let mut pictures = Pictures::new();
    let started = Instant::now();
    let mut page = Page::new(&text, &env, &mut fonts, &pictures);
    let first = started.elapsed();
    let wanted = page.wants().to_vec();
    for path in &wanted {
        match Picture::load(path) {
            Ok(picture) => pictures.insert(path.clone(), picture),
            Err(why) => {
                eprintln!("page_shot: {why}");
                pictures.fail(path.clone());
            }
        }
    }
    let started = Instant::now();
    page = Page::new(&text, &env, &mut fonts, &pictures);
    let again = started.elapsed();
    if let Some(needle) = &find {
        let at = text
            .find(needle.as_str())
            .ok_or(format!("{needle}: not in {input}"))?;
        top = (page.y_of(at) - 12.0 * scale) as i32;
    }
    let whole = page.height().ceil() as u32;
    let height = rows
        .unwrap_or(whole.saturating_sub(top.max(0) as u32))
        .clamp(1, TALLEST);
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    let marks = Marks { focus };
    let started = Instant::now();
    page.paint(
        &mut fonts,
        &pictures,
        &mut pixels,
        width,
        height,
        top,
        0..height,
        &marks,
    );
    let cold = started.elapsed();
    let started = Instant::now();
    page.paint(
        &mut fonts,
        &pictures,
        &mut pixels,
        width,
        height,
        top,
        0..height,
        &marks,
    );
    let warm = started.elapsed();
    // Zoomed, each pixel is a square of its colour.
    let (across, down) = (width as usize * zoom, height as usize * zoom);
    let mut large = Vec::with_capacity(if zoom > 1 { across * down * 4 } else { 0 });
    for row in pixels
        .chunks_exact((width as usize * 4).max(1))
        .filter(|_| zoom > 1)
    {
        let wide: Vec<u8> = row.chunks_exact(4).flat_map(|px| px.repeat(zoom)).collect();
        (0..zoom).for_each(|_| large.extend_from_slice(&wide));
    }
    let pixels = if zoom > 1 { large } else { pixels };
    let file = std::fs::File::create(&output).map_err(|e| format!("{output}: {e}"))?;
    let (across, down) = (across.max(1) as u32, down as u32);
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), across, down);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| format!("{output}: {e}"))?;
    if width > 0 {
        writer
            .write_image_data(&pixels)
            .map_err(|e| format!("{output}: {e}"))?;
    }
    let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
    println!("fonts: {} ({:.2} ms)", fonts.found(), ms(loaded));
    println!(
        "page: {width} wide at scale {scale}, {whole} rows; {} links, {} headings, {} pictures wanted",
        page.links().len(),
        page.headings().len(),
        wanted.len()
    );
    println!(
        "layout: {:.2} ms with fonts cold, {:.2} ms again",
        ms(first),
        ms(again)
    );
    println!(
        "paint: rows {top}..{} ({width}x{height}): {:.2} ms cold, {:.2} ms warm",
        i64::from(top) + i64::from(height),
        ms(cold),
        ms(warm)
    );
    println!("wrote {output}");
    Ok(())
}
