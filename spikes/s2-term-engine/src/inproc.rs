//! In-process shape: one parser thread per terminal owns the PTY reader and every !Send
//! engine object; frames, images (heap Arc) and effects reach the main thread by channel,
//! with a self-pipe to wake the main loop.
use crate::client::TermClient;
use crate::engine::{Cfg, Engine, heap_alloc};
use crate::host::Host;
use crate::proto::{ToDie, ToEngine};
use crate::util::{drain_pipe, pfd, poll, wake, wake_pipe};
use std::os::fd::RawFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

pub struct InProc {
    tx: Sender<ToEngine>,
    eng_wake: RawFd,
    rx: Receiver<ToDie>,
    main_wake_rd: RawFd,
    main_flag: Arc<AtomicBool>,
    child: i32,
    pub thread: Option<JoinHandle<()>>,
}

impl InProc {
    pub fn spawn(argv: &[String], cfg: Cfg, env: &[(&str, &str)]) -> std::io::Result<InProc> {
        let pty = crate::pty::spawn(argv, cfg.cols, cfg.rows, cfg.cell_w, cfg.cell_h, env)?;
        let (tx, rx_eng) = channel::<ToEngine>();
        let (tx_die, rx) = channel::<ToDie>();
        let (eng_wake_rd, eng_wake) = wake_pipe();
        let (main_wake_rd, main_wake_wr) = wake_pipe();
        let main_flag = Arc::new(AtomicBool::new(false));
        let flag = main_flag.clone();
        let (master, child) = (pty.master, pty.child);
        let thread = std::thread::Builder::new()
            .name("die-term-parser".into())
            .spawn(move || {
                let mut host = Host::new(Engine::new(cfg), master, child);
                let mut send = |m: ToDie| {
                    let _ = tx_die.send(m);
                    if !flag.swap(true, SeqCst) {
                        wake(main_wake_wr);
                    }
                };
                loop {
                    let pty_ev = libc::POLLIN | if host.pty_out.is_empty() { 0 } else { libc::POLLOUT };
                    let mut fds = [pfd(master, pty_ev), pfd(eng_wake_rd, libc::POLLIN)];
                    poll(&mut fds, host.timeout_ms());
                    host.stats.loop_wakeups += 1;
                    if fds[1].revents != 0 {
                        drain_pipe(eng_wake_rd);
                        let mut quit = false;
                        loop {
                            match rx_eng.try_recv() {
                                Ok(m) => host.handle(m, &mut send),
                                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                                Err(_) => {
                                    quit = true;
                                    break;
                                }
                            }
                        }
                        if quit {
                            // DIE dropped the terminal.
                            unsafe { libc::kill(child, libc::SIGHUP) };
                            break;
                        }
                    }
                    if fds[0].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 {
                        host.read_pty(1 << 20);
                    }
                    host.flush_pty();
                    host.produce(&mut heap_alloc, &mut send);
                    if host.eof {
                        let st = host.exit_status();
                        send(ToDie::Exited { status: st });
                        break;
                    }
                }
                crate::util::close(master);
                crate::util::close(eng_wake_rd);
            })?;
        Ok(InProc { tx, eng_wake, rx, main_wake_rd, main_flag, child, thread: Some(thread) })
    }
}

impl TermClient for InProc {
    fn fd(&self) -> RawFd {
        self.main_wake_rd
    }
    fn send(&mut self, m: ToEngine) {
        if self.tx.send(m).is_ok() {
            wake(self.eng_wake);
        }
    }
    fn drain(&mut self, out: &mut Vec<ToDie>) -> Result<(), String> {
        drain_pipe(self.main_wake_rd);
        self.main_flag.store(false, SeqCst);
        loop {
            match self.rx.try_recv() {
                Ok(m) => out.push(m),
                Err(std::sync::mpsc::TryRecvError::Empty) => return Ok(()),
                Err(_) => return Err("parser thread ended".into()),
            }
        }
    }
    fn engine_pid(&self) -> i32 {
        crate::util::pid()
    }
    fn child_pid(&self) -> i32 {
        self.child
    }
    fn shape(&self) -> &'static str {
        "inproc"
    }
}

impl Drop for InProc {
    fn drop(&mut self) {
        // Dropping the sender ends the parser thread at its next wake.
        let (tx, _) = channel();
        drop(std::mem::replace(&mut self.tx, tx));
        wake(self.eng_wake);
        // Detach, never join: the main thread does not wait on a parser thread.
        drop(self.thread.take());
    }
}
