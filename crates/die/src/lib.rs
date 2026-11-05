//! DIE's facade (scope §12, Crates): `die::run(config)`, with die-vim always registered,
//! the real clock, the files named at startup, and one window showing the first of them.
#![warn(missing_docs)]

#[cfg(test)]
use die_test_cap as _;

pub use die_core as core;
pub use die_render as render;
pub use die_vim as vim;

use die_core::{Clock, Ctx, Extension, StatusTextFn};
use die_render::driver::Config as Font;
use die_render::{check, window};
use die_vim::Vim;
use std::path::PathBuf;

/// Everything your config says.
pub struct Config {
    /// Bindings, commands, faces, leader, theme, settings and the startup hook. Its clock
    /// is ignored: a running DIE has a real one.
    pub core: die_core::Config,
    /// Extensions, after die-vim.
    pub extensions: Vec<Box<dyn Extension>>,
    /// What status lines show. None shows the buffer's name.
    pub status_text: Option<StatusTextFn>,
    /// The font: files, size and who draws which characters. Its scale becomes the window's.
    pub font: Font,
    /// The files to open: the first is on screen. A relative path is from where DIE started.
    pub files: Vec<PathBuf>,
}

impl Default for Config {
    /// die-vim alone, an empty buffer on screen, and Ioskeley Mono Term at 16 pt.
    fn default() -> Config {
        Config {
            core: die_core::Config::default(),
            extensions: vec![],
            status_text: None,
            font: Font::term(16.0, 1.0),
            files: vec![],
        }
    }
}

/// Everything before the window opens: the editor, with one window showing the first file
/// that opened, or an empty buffer. A file that won't open is logged, and the next one goes
/// on screen instead. Every problem with the config comes back at once (C8).
pub fn start(config: Config) -> Result<Ctx, Vec<String>> {
    let Config {
        mut core,
        extensions,
        status_text,
        files,
        ..
    } = config;
    core.clock = Clock::real();
    let mut exts: Vec<Box<dyn Extension>> = vec![Box::new(Vim::default())];
    exts.extend(extensions);
    let mut ctx = Ctx::with_config(exts, core)?;
    if let Some(f) = status_text {
        ctx.set_status_text(f);
    }
    let here = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let mut first = None;
    for file in files {
        match ctx.open_file(&here.join(file)) {
            Ok(buf) => first = first.or(Some(buf)),
            Err(e) => ctx.messages.push(format!("{e:?}")),
        }
    }
    let buf = first.unwrap_or_else(|| ctx.create_text("*scratch*", ""));
    ctx.open_window(buf, 80, 24)
        .map_err(|e| vec![format!("{e:?}")])?;
    Ok(ctx)
}

/// Run DIE until its window closes. The window opens without taking focus. The exit code:
/// 2 if it couldn't start, and it says why.
pub fn run(config: Config) -> i32 {
    launch(config, None)
}

/// Run `die --check`'s checks on a window of DIE's own (`die_render::check`). The exit code:
/// 0 if they passed, 1 if one failed, and 2 if there's no window to check.
pub fn check(config: Config, names: Vec<String>) -> i32 {
    launch(config, Some(check::script(names)))
}

fn launch(config: Config, script: Option<window::Script>) -> i32 {
    let font = config.font.clone();
    let ctx = match start(config) {
        Ok(ctx) => ctx,
        Err(errors) => {
            for e in errors {
                eprintln!("die: {e}");
            }
            return 2;
        }
    };
    // `*messages*` isn't there to show these yet (K11a), and a file that didn't open is one.
    for message in &ctx.messages {
        eprintln!("die: {message}");
    }
    let options = window::Options {
        font,
        title: "DIE".into(),
        script,
    };
    window::run(ctx, options).unwrap_or_else(|e| {
        eprintln!("die: {e}");
        2
    })
}
