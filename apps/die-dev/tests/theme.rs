//! mono-dark styles every base and UI face, and only those, as no extension is here to declare
//! another, and draws as your Emacs does.

use die_test_cap as _;

use die::core::{BASE_FACES, Ctx, Rgb, UI_FACES};
use die_dev::theme::mono_dark;

#[test]
fn mono_dark_styles_every_core_face_and_no_other() {
    let t = mono_dark();
    let core: Vec<&str> = BASE_FACES.iter().chain(UI_FACES).map(|f| f.0).collect();
    for name in &core {
        assert!(
            t.faces.contains_key(*name),
            "mono-dark doesn't style {name}"
        );
    }
    for name in t.faces.keys() {
        assert!(
            core.contains(&name.as_str()),
            "{name} is no core face: an extension's face is its config's to style"
        );
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
