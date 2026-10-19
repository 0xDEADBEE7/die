//! mono-dark styles every base and UI face, and draws as your Emacs does.

use die::core::{BASE_FACES, Ctx, Rgb, UI_FACES};
use die_dev::theme::mono_dark;

#[test]
fn mono_dark_styles_every_core_face() {
    let t = mono_dark();
    for (name, _) in BASE_FACES.iter().chain(&UI_FACES) {
        assert!(
            t.faces.contains_key(*name),
            "mono-dark doesn't style {name}"
        );
    }
    assert_eq!(
        t.faces.len(),
        BASE_FACES.len() + UI_FACES.len(),
        "every face it styles is a core face"
    );
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
