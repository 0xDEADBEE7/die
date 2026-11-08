//! Contract §12: a program on a PTY holds no fd of DIE's but its terminal's, in a DIE whose
//! window and Metal are up, as yours are. Metal opens its shader cache without close-on-exec,
//! which die-core's own PTY tests never start. The trampoline is this binary, so it has no test
//! harness: `main` calls `die::other_mode()` first, as DIE's does. Its window can't be seen.

use die_test_cap as _;

use die::core::{Ctx, PtyCommand, PtySize};
use die::render::driver::{self, Config};
use die::render::window::{self, Remote};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The test failed. A quit ends the loop with 0, so this keeps that from hiding it.
static FAILED: AtomicBool = AtomicBool::new(false);

type Said = Arc<Mutex<(Vec<u8>, bool)>>;

/// `ls -l /dev/fd/` on a PTY, from inside the loop, and what it said once it ended.
fn listing(r: &Remote) -> Result<String, String> {
    let said = Said::default();
    let (out, end) = (said.clone(), said.clone());
    r.call(move |shell, _| {
        let cmd = PtyCommand::new("/bin/ls")
            .arg("-l")
            .arg("/dev/fd/")
            .on_output(move |_, bytes| {
                out.lock().unwrap().0.extend_from_slice(bytes);
                Ok(())
            })
            .on_exit(move |_, _| {
                end.lock().unwrap().1 = true;
                Ok(())
            });
        let size = PtySize {
            cols: 200,
            rows: 50,
            width: 0,
            height: 0,
        };
        // Kept to its end, which drops it.
        shell
            .ctx()
            .pty(cmd, size)
            .map(std::mem::forget)
            .map_err(|e| e.to_string())
    })??;
    let start = Instant::now();
    while !said.lock().unwrap().1 {
        if start.elapsed() > Duration::from_secs(20) {
            return Err("ls never ended".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(String::from_utf8_lossy(&said.lock().unwrap().0).into_owned())
}

/// Past the terminal's 0, 1 and 2, only directories: what `ls` has open itself. A Metal cache
/// file would be a regular file (`-`), and another terminal a device.
fn no_fd_metal_opened_reaches_a_program(r: &Remote) -> Result<(), String> {
    let said = listing(r)?;
    let fds: Vec<(char, u32)> = said
        .lines()
        .filter(|l| !l.starts_with("total"))
        .filter_map(|l| {
            Some((
                l.chars().next()?,
                l.split_whitespace().last()?.parse().ok()?,
            ))
        })
        .collect();
    let ok = fds.iter().take(3).map(|f| f.1).eq([0, 1, 2])
        && fds.iter().take(3).all(|f| f.0 == 'c')
        && fds.iter().skip(3).all(|f| f.0 == 'd');
    ok.then_some(()).ok_or(said)
}

fn script(r: &Remote) -> i32 {
    let name = "no_fd_metal_opened_reaches_a_program";
    match no_fd_metal_opened_reaches_a_program(r) {
        Ok(()) => println!("test {name} ... ok"),
        Err(why) => {
            println!("test {name} ... FAILED\n{why}");
            FAILED.store(true, Ordering::SeqCst);
        }
    }
    let _ = r.call(|shell, _| shell.ctx().quit());
    std::thread::sleep(Duration::from_secs(10));
    println!("the loop didn't stop");
    1
}

fn main() {
    if let Some(code) = die::other_mode() {
        std::process::exit(code);
    }
    if !window::unseen() {
        let me = std::env::current_exe().unwrap();
        let run = std::process::Command::new(me)
            .env(window::UNSEEN, "1")
            .status();
        std::process::exit(run.ok().and_then(|s| s.code()).unwrap_or(1));
    }
    if driver::ready().is_err() {
        println!("no window here: skipped");
        return;
    }
    let options = window::Options {
        font: Config::term(16.0, 1.0),
        title: "DIE fds".into(),
        script: Some(Box::new(script)),
        hidden: true,
        first_frame: None,
        on_open: None,
    };
    let code = window::run(Ctx::new(vec![]).unwrap(), options).expect("a window");
    std::process::exit(if FAILED.load(Ordering::SeqCst) {
        1
    } else {
        code
    });
}
