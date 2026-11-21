//! mono-dark styles every base and UI face, the markdown view's and fig's chrome, and only faces
//! core or an extension of yours declares, and draws as your Emacs does.

use die_test_cap as _;

use die::core::{BASE_FACES, Ctx, Rgb, UI_FACES};
use die_dev::theme::mono_dark;

#[test]
fn mono_dark_styles_every_core_face() {
    let t = mono_dark();
    for (name, _) in BASE_FACES.iter().chain(UI_FACES) {
        assert!(
            t.faces.contains_key(*name),
            "mono-dark doesn't style {name}"
        );
    }
    let core = BASE_FACES.iter().chain(UI_FACES).map(|f| f.0);
    let orchid = die_orchid_client::FACES.iter().map(|f| f.0);
    let markdown = die_markdown::faces();
    let whiteboard = [
        die_whiteboard::face::DIAL,
        die_whiteboard::face::DIAL_ACCENT,
    ];
    let fig = die_fig::faces().iter().copied();
    let known: Vec<&str> = core
        .chain(orchid)
        .chain(markdown.map(|f| f.0))
        .chain(whiteboard)
        .chain(fig)
        .collect();
    for name in t.faces.keys() {
        assert!(
            known.contains(&name.as_str()),
            "{name} is no core, orchid, markdown, whiteboard or fig face"
        );
    }
    // The markdown view draws in your stylesheet's colours: each of its faces is styled here.
    for (name, ..) in markdown {
        assert!(t.faces.contains_key(name), "mono-dark doesn't style {name}");
    }
}

#[test]
fn mono_dark_resolves_like_emacs() {
    let mut ctx = Ctx::new(vec![]).unwrap();
    ctx.set_theme(mono_dark());
    let d = ctx.face_style("default").unwrap();
    assert_eq!(
        (d.fg, d.bg),
        (Some(Rgb::hex("#cccccc")), Some(Rgb::hex("#1a1a1a")))
    );
    assert_eq!(
        ctx.face_style("cursor").unwrap().bg,
        Some(Rgb::hex("#cccccc"))
    );
    assert_eq!(ctx.theme().ansi[1], Rgb::hex("#cc4444"));
}
