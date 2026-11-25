//! mini-fig through your config, beside fig: a `.fig` file opens as fig's figure, `\V` there
//! shows the same file's figure in mini-fig and `\V` again goes back to fig's; from the file's
//! text (fig's `\v`), `\V` shows it in mini-fig too and comes back to the text.

use die_test_cap as _;

use die::core::{CellPixels, Harness, ShowIn};

/// Your cells on your panel: 19 by 40 pixels at 2x.
const CELL: CellPixels = CellPixels {
    width: 19,
    height: 40,
    scale: 2.0,
};

fn keys(h: &mut Harness, k: &str) {
    for k in h.type_keys(k).unwrap() {
        assert!(k.errors.is_empty(), "{}: {:?}", k.key, k.errors);
    }
    h.run_until_idle();
}

fn name(h: &Harness) -> String {
    let buf = h.focused_buffer().unwrap();
    h.buffer_name(buf).unwrap_or_default().to_string()
}

#[test]
fn leader_shift_v_shows_a_fig_files_figure_in_mini_fig_from_figs_and_from_its_text() {
    let d = std::env::temp_dir().join(format!("die-dev-mini-fig-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let d = d.canonicalize().unwrap();
    std::fs::write(d.join("rates.csv"), "t,p\n1,10\n2,12\n3,11\n").unwrap();
    let spec = d.join("rates.fig");
    let text = r#"{"type": "line", "title": "Rates", "data": "rates.csv", "x": "t", "y": "p"}"#;
    std::fs::write(&spec, text).unwrap();
    let config = die_dev::config(vec![]);
    let mut exts = die::built_in();
    exts.extend(config.extensions);
    let mut h = Harness::with_config(exts, config.core).unwrap();
    h.open_text("notes", "my notes\n").unwrap();
    h.set_window_size(120, 40);
    h.set_cell_pixels(CELL);
    h.run_until_idle();
    let src = h.open_file(&spec).unwrap();
    h.show(src, ShowIn::Current).unwrap();
    h.run_until_idle();
    assert_eq!(name(&h), "rates.fig *fig*", "fig's figure");
    let theirs = h.focused_buffer().unwrap();
    keys(&mut h, "\\V");
    assert_eq!(name(&h), "rates.fig *mini-fig*");
    let placed: Vec<i8> = h.scene().placements.iter().map(|p| p.z).collect();
    assert_eq!(placed, [-1], "mini-fig's one buffer");
    assert!(h.screen().contains("mini-fig: Rates"), "{}", h.screen());
    keys(&mut h, "\\V");
    assert_eq!(h.focused_buffer(), Some(theirs), "back to fig's");
    keys(&mut h, "\\v");
    assert_eq!(h.focused_buffer(), Some(src), "fig's \\v: the text");
    keys(&mut h, "\\V");
    assert_eq!(name(&h), "rates.fig *mini-fig*");
    keys(&mut h, "\\V");
    assert_eq!(h.focused_buffer(), Some(src), "back to the text");
    let _ = std::fs::remove_dir_all(&d);
}
