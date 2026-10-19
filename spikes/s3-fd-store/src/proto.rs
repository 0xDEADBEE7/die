//! Wire protocol between the fd store and any DIE-owned process.
//!
//! A Unix stream socket, strict request/response. Each frame is a u32 LE length and a
//! UTF-8 body, with at most one fd attached (SCM_RIGHTS). The body is
//! `VERB k=v k=v` on the first line and one `item k=v ...` line per list entry.
//! Values are percent-escaped. Unknown keys are ignored and missing keys take
//! defaults, so builds that differ in protocol minor talk to each other.
//! The `HELLO` exchange, `proto`, `code`, `GET`, `LIST` and `EXIT` are frozen forever,
//! so any future build can still empty an old store.

use std::io::{self, IoSlice, IoSliceMut};
use std::mem::MaybeUninit;
use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd};

use rustix::net::{RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, SendAncillaryBuffer, SendAncillaryMessage, SendFlags};

pub const MAJOR: u32 = if cfg!(feature = "proto-major2") { 2 } else { 1 };
pub const MINOR: u32 = if cfg!(feature = "proto-n1") { 1 } else { 0 };
pub const BUILD: &str = if cfg!(feature = "proto-major2") {
    "M2"
} else if cfg!(feature = "proto-n1") {
    "N+1"
} else {
    "N"
};
pub const SOCK: &str = "store.sock";
pub const LOCK: &str = "store.lock";
const MAX_FRAME: usize = 1 << 20;

pub fn proto() -> String {
    format!("{MAJOR}.{MINOR}")
}

pub type Kv = Vec<(String, String)>;

#[derive(Clone, Debug, Default)]
pub struct Msg {
    pub verb: String,
    pub kv: Kv,
    pub items: Vec<Kv>,
}

impl Msg {
    pub fn new(verb: &str) -> Msg {
        Msg { verb: verb.into(), ..Default::default() }
    }
    pub fn with(mut self, k: &str, v: impl ToString) -> Msg {
        self.set(k, v);
        self
    }
    pub fn set(&mut self, k: &str, v: impl ToString) {
        set(&mut self.kv, k, v.to_string());
    }
    pub fn get(&self, k: &str) -> Option<&str> {
        get(&self.kv, k)
    }
    pub fn err(code: &str, msg: &str) -> Msg {
        Msg::new("ERR").with("code", code).with("msg", msg)
    }
    pub fn encode(&self) -> Vec<u8> {
        let mut s = self.verb.clone();
        line(&mut s, &self.kv);
        for it in &self.items {
            s.push_str("\nitem");
            line(&mut s, it);
        }
        s.into_bytes()
    }
    pub fn decode(b: &[u8]) -> io::Result<Msg> {
        let s = std::str::from_utf8(b).map_err(|_| bad("frame is not UTF-8"))?;
        let mut lines = s.split('\n');
        let mut first = lines.next().unwrap_or("").split(' ');
        let mut m = Msg::new(first.next().unwrap_or(""));
        m.kv = parse(first)?;
        for l in lines {
            let mut w = l.split(' ');
            if w.next() == Some("item") {
                m.items.push(parse(w)?);
            }
        }
        Ok(m)
    }
}

pub fn get<'a>(kv: &'a Kv, k: &str) -> Option<&'a str> {
    kv.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str())
}

pub fn set(kv: &mut Kv, k: &str, v: String) {
    match kv.iter_mut().find(|(a, _)| a == k) {
        Some(e) => e.1 = v,
        None => kv.push((k.into(), v)),
    }
}

fn line(s: &mut String, kv: &Kv) {
    for (k, v) in kv {
        s.push(' ');
        s.push_str(&esc(k));
        s.push('=');
        s.push_str(&esc(v));
    }
}

fn parse<'a>(words: impl Iterator<Item = &'a str>) -> io::Result<Kv> {
    words
        .filter(|w| !w.is_empty())
        .map(|w| {
            let (k, v) = w.split_once('=').ok_or_else(|| bad("field without ="))?;
            Ok((unesc(k)?, unesc(v)?))
        })
        .collect()
}

fn esc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"._/:@+,-".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

fn unesc(s: &str) -> io::Result<String> {
    let b = s.as_bytes();
    let mut o = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let h = s.get(i + 1..i + 3).ok_or_else(|| bad("bad escape"))?;
            o.push(u8::from_str_radix(h, 16).map_err(|_| bad("bad escape"))?);
            i += 3;
        } else {
            o.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(o).map_err(|_| bad("bad escape"))
}

fn bad(s: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, s)
}

pub fn send(sock: BorrowedFd<'_>, m: &Msg, fd: Option<BorrowedFd<'_>>) -> io::Result<()> {
    let body = m.encode();
    let mut frame = (body.len() as u32).to_le_bytes().to_vec();
    frame.extend_from_slice(&body);
    let mut space = [MaybeUninit::<u8>::uninit(); rustix::cmsg_space!(ScmRights(1))];
    let mut cmsg = SendAncillaryBuffer::new(&mut space);
    let fds;
    if let Some(fd) = fd {
        fds = [fd];
        cmsg.push(SendAncillaryMessage::ScmRights(&fds));
    }
    let mut off = rustix::net::sendmsg(sock, &[IoSlice::new(&frame)], &mut cmsg, SendFlags::empty())?;
    while off < frame.len() {
        off += rustix::io::write(sock, &frame[off..])?;
    }
    Ok(())
}

/// Receive one frame. `Ok(None)` is a clean EOF before any byte.
pub fn recv(sock: BorrowedFd<'_>) -> io::Result<Option<(Msg, Option<OwnedFd>)>> {
    let mut hdr = [0u8; 4];
    let mut space = [MaybeUninit::<u8>::uninit(); rustix::cmsg_space!(ScmRights(4))];
    let mut cmsg = RecvAncillaryBuffer::new(&mut space);
    let r = rustix::net::recvmsg(sock, &mut [IoSliceMut::new(&mut hdr)], &mut cmsg, RecvFlags::empty())?;
    let mut fd = None;
    for c in cmsg.drain() {
        if let RecvAncillaryMessage::ScmRights(fds) = c {
            for f in fds {
                // macOS has no MSG_CMSG_CLOEXEC: set it now (see the record's fd-race note).
                rustix::io::fcntl_setfd(&f, rustix::io::FdFlags::CLOEXEC)?;
                if fd.is_none() {
                    fd = Some(f);
                } // extra fds are closed on drop
            }
        }
    }
    if r.bytes == 0 {
        return Ok(None);
    }
    read_exact(sock, &mut hdr[r.bytes..])?;
    let n = u32::from_le_bytes(hdr) as usize;
    if n > MAX_FRAME {
        return Err(bad("frame too large"));
    }
    let mut body = vec![0u8; n];
    read_exact(sock, &mut body)?;
    Ok(Some((Msg::decode(&body)?, fd)))
}

fn read_exact(sock: BorrowedFd<'_>, mut b: &mut [u8]) -> io::Result<()> {
    while !b.is_empty() {
        let n = rustix::io::read(sock, &mut *b)?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "short frame"));
        }
        b = &mut b[n..];
    }
    Ok(())
}

/// The peer's uid and pid on a connected Unix socket.
pub fn peer(sock: BorrowedFd<'_>) -> (u32, i32) {
    let (mut uid, mut gid) = (u32::MAX, 0);
    unsafe { libc::getpeereid(sock.as_raw_fd(), &mut uid, &mut gid) };
    let mut pid: libc::pid_t = -1;
    let mut len = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    unsafe { libc::getsockopt(sock.as_raw_fd(), libc::SOL_LOCAL, libc::LOCAL_PEERPID, (&mut pid as *mut i32).cast(), &mut len) };
    (uid, pid)
}
