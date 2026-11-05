//! A status line shows the buffer's name, unless the config says otherwise (F3).

use die_test_cap as _;

use die_core::{Ctx, WindowPlacement};

fn status(ctx: &mut Ctx, cols: usize) -> String {
    let w = ctx.focused().unwrap();
    let (_, rows) = ctx.window_size(w).unwrap();
    let placement = WindowPlacement {
        window: w,
        col: 0,
        row: 0,
    };
    let s = ctx.compose(cols, rows + 2, &[placement]);
    let row = s.row(rows).iter().map(|c| c.glyph.to_string());
    row.collect::<String>().trim_end().to_owned()
}

fn one_window(name: &str) -> Ctx {
    let mut ctx = Ctx::new(vec![]).unwrap();
    let b = ctx.create_text(name, "text\n");
    ctx.open_window(b, 40, 4).unwrap();
    ctx
}

#[test]
fn the_default_is_the_buffers_name() {
    assert_eq!(status(&mut one_window("a.txt"), 40), " a.txt");
}

#[test]
fn the_configured_function_gets_the_name_and_says_what_shows() {
    let mut ctx = one_window("a.txt");
    ctx.set_status_text(|name, file| format!("[{name}] {}", file.is_some()));
    assert_eq!(status(&mut ctx, 40), " [a.txt] false");
}

#[test]
fn it_gets_the_file_a_buffer_visits() {
    let dir = std::env::temp_dir().join(format!("die-status-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("b.txt");
    std::fs::write(&path, "hello\n").unwrap();
    let mut ctx = Ctx::new(vec![]).unwrap();
    let b = ctx.open_file(&path).unwrap();
    ctx.open_window(b, 200, 4).unwrap();
    ctx.set_status_text(|_, file| file.unwrap().path.display().to_string());
    assert_eq!(status(&mut ctx, 200), format!(" {}", path.display()));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_long_text_is_cut_at_the_windows_edge() {
    let mut ctx = one_window("a.txt");
    ctx.set_status_text(|_, _| "x".repeat(100));
    assert_eq!(status(&mut ctx, 40), format!(" {}", "x".repeat(39)));
}

#[test]
fn a_panic_falls_back_to_the_name_and_logs_once() {
    let mut ctx = one_window("a.txt");
    ctx.set_status_text(|_, _| panic!("no status today"));
    for _ in 0..5 {
        assert_eq!(status(&mut ctx, 40), " a.txt");
    }
    let logged = ctx
        .messages
        .iter()
        .filter(|m| m.contains("no status today"));
    assert_eq!(logged.count(), 1, "{:?}", ctx.messages);
}

#[test]
fn a_second_call_replaces_the_first() {
    let mut ctx = one_window("a.txt");
    ctx.set_status_text(|_, _| "one".into());
    ctx.set_status_text(|_, _| "two".into());
    assert_eq!(status(&mut ctx, 40), " two");
}
