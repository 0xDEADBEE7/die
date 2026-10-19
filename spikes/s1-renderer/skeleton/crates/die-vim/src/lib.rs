//! Stand-in for die-vim: ~3k lines of generated but ordinary Rust, a realistic leaf to edit.
#![allow(dead_code)]
use die_core::Buffer;

#[derive(Clone, Copy, Debug, PartialEq)] pub enum Motion { Char(i64), Line(i64), Word(i64), Para(i64) }
pub struct State { pub count: usize, pub reg: char, pub pos: usize, pub hist: Vec<String> }

pub fn op_0(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 10 { st.hist.remove(0); }
    st.hist.push(format!("op_0 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_1(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 11 { st.hist.remove(0); }
    st.hist.push(format!("op_1 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_2(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 12 { st.hist.remove(0); }
    st.hist.push(format!("op_2 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_3(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 13 { st.hist.remove(0); }
    st.hist.push(format!("op_3 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_4(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 14 { st.hist.remove(0); }
    st.hist.push(format!("op_4 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_5(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 15 { st.hist.remove(0); }
    st.hist.push(format!("op_5 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_6(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 16 { st.hist.remove(0); }
    st.hist.push(format!("op_6 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_7(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 17 { st.hist.remove(0); }
    st.hist.push(format!("op_7 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_8(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 18 { st.hist.remove(0); }
    st.hist.push(format!("op_8 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_9(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 19 { st.hist.remove(0); }
    st.hist.push(format!("op_9 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_10(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 20 { st.hist.remove(0); }
    st.hist.push(format!("op_10 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_11(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 21 { st.hist.remove(0); }
    st.hist.push(format!("op_11 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_12(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 22 { st.hist.remove(0); }
    st.hist.push(format!("op_12 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_13(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 23 { st.hist.remove(0); }
    st.hist.push(format!("op_13 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_14(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 24 { st.hist.remove(0); }
    st.hist.push(format!("op_14 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_15(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 25 { st.hist.remove(0); }
    st.hist.push(format!("op_15 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_16(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 26 { st.hist.remove(0); }
    st.hist.push(format!("op_16 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_17(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 27 { st.hist.remove(0); }
    st.hist.push(format!("op_17 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_18(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 28 { st.hist.remove(0); }
    st.hist.push(format!("op_18 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_19(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 29 { st.hist.remove(0); }
    st.hist.push(format!("op_19 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_20(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 30 { st.hist.remove(0); }
    st.hist.push(format!("op_20 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_21(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 31 { st.hist.remove(0); }
    st.hist.push(format!("op_21 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_22(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 32 { st.hist.remove(0); }
    st.hist.push(format!("op_22 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_23(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 33 { st.hist.remove(0); }
    st.hist.push(format!("op_23 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_24(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 34 { st.hist.remove(0); }
    st.hist.push(format!("op_24 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_25(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 35 { st.hist.remove(0); }
    st.hist.push(format!("op_25 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_26(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 36 { st.hist.remove(0); }
    st.hist.push(format!("op_26 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_27(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 37 { st.hist.remove(0); }
    st.hist.push(format!("op_27 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_28(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 38 { st.hist.remove(0); }
    st.hist.push(format!("op_28 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_29(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 39 { st.hist.remove(0); }
    st.hist.push(format!("op_29 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_30(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 40 { st.hist.remove(0); }
    st.hist.push(format!("op_30 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_31(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 41 { st.hist.remove(0); }
    st.hist.push(format!("op_31 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_32(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 42 { st.hist.remove(0); }
    st.hist.push(format!("op_32 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_33(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 43 { st.hist.remove(0); }
    st.hist.push(format!("op_33 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_34(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 44 { st.hist.remove(0); }
    st.hist.push(format!("op_34 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_35(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 45 { st.hist.remove(0); }
    st.hist.push(format!("op_35 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_36(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 46 { st.hist.remove(0); }
    st.hist.push(format!("op_36 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_37(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 47 { st.hist.remove(0); }
    st.hist.push(format!("op_37 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_38(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 48 { st.hist.remove(0); }
    st.hist.push(format!("op_38 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_39(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 49 { st.hist.remove(0); }
    st.hist.push(format!("op_39 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_40(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 50 { st.hist.remove(0); }
    st.hist.push(format!("op_40 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_41(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 51 { st.hist.remove(0); }
    st.hist.push(format!("op_41 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_42(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 52 { st.hist.remove(0); }
    st.hist.push(format!("op_42 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_43(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 53 { st.hist.remove(0); }
    st.hist.push(format!("op_43 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_44(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 54 { st.hist.remove(0); }
    st.hist.push(format!("op_44 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_45(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 55 { st.hist.remove(0); }
    st.hist.push(format!("op_45 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_46(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 56 { st.hist.remove(0); }
    st.hist.push(format!("op_46 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_47(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 57 { st.hist.remove(0); }
    st.hist.push(format!("op_47 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_48(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 58 { st.hist.remove(0); }
    st.hist.push(format!("op_48 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_49(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 59 { st.hist.remove(0); }
    st.hist.push(format!("op_49 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_50(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 60 { st.hist.remove(0); }
    st.hist.push(format!("op_50 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_51(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 61 { st.hist.remove(0); }
    st.hist.push(format!("op_51 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_52(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 62 { st.hist.remove(0); }
    st.hist.push(format!("op_52 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_53(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 63 { st.hist.remove(0); }
    st.hist.push(format!("op_53 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_54(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 64 { st.hist.remove(0); }
    st.hist.push(format!("op_54 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_55(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 65 { st.hist.remove(0); }
    st.hist.push(format!("op_55 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_56(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 66 { st.hist.remove(0); }
    st.hist.push(format!("op_56 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_57(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 67 { st.hist.remove(0); }
    st.hist.push(format!("op_57 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_58(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 68 { st.hist.remove(0); }
    st.hist.push(format!("op_58 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_59(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 69 { st.hist.remove(0); }
    st.hist.push(format!("op_59 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_60(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 70 { st.hist.remove(0); }
    st.hist.push(format!("op_60 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_61(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 71 { st.hist.remove(0); }
    st.hist.push(format!("op_61 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_62(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 72 { st.hist.remove(0); }
    st.hist.push(format!("op_62 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_63(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 73 { st.hist.remove(0); }
    st.hist.push(format!("op_63 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_64(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 74 { st.hist.remove(0); }
    st.hist.push(format!("op_64 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_65(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 75 { st.hist.remove(0); }
    st.hist.push(format!("op_65 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_66(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 76 { st.hist.remove(0); }
    st.hist.push(format!("op_66 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_67(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 77 { st.hist.remove(0); }
    st.hist.push(format!("op_67 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_68(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 78 { st.hist.remove(0); }
    st.hist.push(format!("op_68 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_69(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 79 { st.hist.remove(0); }
    st.hist.push(format!("op_69 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_70(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 80 { st.hist.remove(0); }
    st.hist.push(format!("op_70 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_71(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 81 { st.hist.remove(0); }
    st.hist.push(format!("op_71 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_72(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 82 { st.hist.remove(0); }
    st.hist.push(format!("op_72 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_73(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 83 { st.hist.remove(0); }
    st.hist.push(format!("op_73 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_74(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 84 { st.hist.remove(0); }
    st.hist.push(format!("op_74 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_75(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 85 { st.hist.remove(0); }
    st.hist.push(format!("op_75 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_76(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 86 { st.hist.remove(0); }
    st.hist.push(format!("op_76 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_77(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 87 { st.hist.remove(0); }
    st.hist.push(format!("op_77 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_78(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 88 { st.hist.remove(0); }
    st.hist.push(format!("op_78 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_79(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 89 { st.hist.remove(0); }
    st.hist.push(format!("op_79 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_80(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 90 { st.hist.remove(0); }
    st.hist.push(format!("op_80 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_81(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 91 { st.hist.remove(0); }
    st.hist.push(format!("op_81 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_82(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 92 { st.hist.remove(0); }
    st.hist.push(format!("op_82 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_83(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 93 { st.hist.remove(0); }
    st.hist.push(format!("op_83 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_84(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 94 { st.hist.remove(0); }
    st.hist.push(format!("op_84 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_85(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 95 { st.hist.remove(0); }
    st.hist.push(format!("op_85 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_86(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 96 { st.hist.remove(0); }
    st.hist.push(format!("op_86 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_87(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 97 { st.hist.remove(0); }
    st.hist.push(format!("op_87 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_88(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 98 { st.hist.remove(0); }
    st.hist.push(format!("op_88 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_89(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 99 { st.hist.remove(0); }
    st.hist.push(format!("op_89 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_90(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 100 { st.hist.remove(0); }
    st.hist.push(format!("op_90 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_91(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 101 { st.hist.remove(0); }
    st.hist.push(format!("op_91 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_92(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 102 { st.hist.remove(0); }
    st.hist.push(format!("op_92 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_93(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 103 { st.hist.remove(0); }
    st.hist.push(format!("op_93 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_94(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 104 { st.hist.remove(0); }
    st.hist.push(format!("op_94 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_95(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 105 { st.hist.remove(0); }
    st.hist.push(format!("op_95 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_96(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 106 { st.hist.remove(0); }
    st.hist.push(format!("op_96 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_97(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 107 { st.hist.remove(0); }
    st.hist.push(format!("op_97 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_98(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 108 { st.hist.remove(0); }
    st.hist.push(format!("op_98 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_99(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 109 { st.hist.remove(0); }
    st.hist.push(format!("op_99 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_100(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 110 { st.hist.remove(0); }
    st.hist.push(format!("op_100 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_101(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 111 { st.hist.remove(0); }
    st.hist.push(format!("op_101 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_102(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 112 { st.hist.remove(0); }
    st.hist.push(format!("op_102 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_103(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 113 { st.hist.remove(0); }
    st.hist.push(format!("op_103 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_104(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 114 { st.hist.remove(0); }
    st.hist.push(format!("op_104 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_105(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 115 { st.hist.remove(0); }
    st.hist.push(format!("op_105 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_106(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 116 { st.hist.remove(0); }
    st.hist.push(format!("op_106 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_107(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 117 { st.hist.remove(0); }
    st.hist.push(format!("op_107 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_108(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 118 { st.hist.remove(0); }
    st.hist.push(format!("op_108 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_109(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 119 { st.hist.remove(0); }
    st.hist.push(format!("op_109 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_110(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 120 { st.hist.remove(0); }
    st.hist.push(format!("op_110 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_111(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 121 { st.hist.remove(0); }
    st.hist.push(format!("op_111 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_112(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 122 { st.hist.remove(0); }
    st.hist.push(format!("op_112 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_113(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 123 { st.hist.remove(0); }
    st.hist.push(format!("op_113 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_114(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 124 { st.hist.remove(0); }
    st.hist.push(format!("op_114 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_115(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 125 { st.hist.remove(0); }
    st.hist.push(format!("op_115 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_116(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 126 { st.hist.remove(0); }
    st.hist.push(format!("op_116 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_117(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 127 { st.hist.remove(0); }
    st.hist.push(format!("op_117 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_118(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 128 { st.hist.remove(0); }
    st.hist.push(format!("op_118 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_119(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 129 { st.hist.remove(0); }
    st.hist.push(format!("op_119 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_120(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 130 { st.hist.remove(0); }
    st.hist.push(format!("op_120 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_121(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 131 { st.hist.remove(0); }
    st.hist.push(format!("op_121 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_122(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 132 { st.hist.remove(0); }
    st.hist.push(format!("op_122 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_123(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 133 { st.hist.remove(0); }
    st.hist.push(format!("op_123 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_124(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 134 { st.hist.remove(0); }
    st.hist.push(format!("op_124 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_125(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 135 { st.hist.remove(0); }
    st.hist.push(format!("op_125 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_126(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 136 { st.hist.remove(0); }
    st.hist.push(format!("op_126 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_127(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 137 { st.hist.remove(0); }
    st.hist.push(format!("op_127 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_128(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 138 { st.hist.remove(0); }
    st.hist.push(format!("op_128 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_129(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 139 { st.hist.remove(0); }
    st.hist.push(format!("op_129 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_130(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 140 { st.hist.remove(0); }
    st.hist.push(format!("op_130 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_131(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 141 { st.hist.remove(0); }
    st.hist.push(format!("op_131 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_132(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 142 { st.hist.remove(0); }
    st.hist.push(format!("op_132 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_133(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 143 { st.hist.remove(0); }
    st.hist.push(format!("op_133 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_134(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 144 { st.hist.remove(0); }
    st.hist.push(format!("op_134 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_135(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 145 { st.hist.remove(0); }
    st.hist.push(format!("op_135 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_136(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 146 { st.hist.remove(0); }
    st.hist.push(format!("op_136 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_137(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 147 { st.hist.remove(0); }
    st.hist.push(format!("op_137 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_138(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 148 { st.hist.remove(0); }
    st.hist.push(format!("op_138 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_139(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 149 { st.hist.remove(0); }
    st.hist.push(format!("op_139 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_140(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 150 { st.hist.remove(0); }
    st.hist.push(format!("op_140 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_141(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 151 { st.hist.remove(0); }
    st.hist.push(format!("op_141 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_142(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 152 { st.hist.remove(0); }
    st.hist.push(format!("op_142 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_143(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 153 { st.hist.remove(0); }
    st.hist.push(format!("op_143 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_144(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 154 { st.hist.remove(0); }
    st.hist.push(format!("op_144 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_145(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 155 { st.hist.remove(0); }
    st.hist.push(format!("op_145 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_146(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 156 { st.hist.remove(0); }
    st.hist.push(format!("op_146 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_147(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 157 { st.hist.remove(0); }
    st.hist.push(format!("op_147 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_148(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 158 { st.hist.remove(0); }
    st.hist.push(format!("op_148 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_149(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 159 { st.hist.remove(0); }
    st.hist.push(format!("op_149 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_150(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 160 { st.hist.remove(0); }
    st.hist.push(format!("op_150 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_151(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 161 { st.hist.remove(0); }
    st.hist.push(format!("op_151 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_152(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 162 { st.hist.remove(0); }
    st.hist.push(format!("op_152 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_153(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 163 { st.hist.remove(0); }
    st.hist.push(format!("op_153 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_154(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 164 { st.hist.remove(0); }
    st.hist.push(format!("op_154 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_155(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 165 { st.hist.remove(0); }
    st.hist.push(format!("op_155 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_156(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 166 { st.hist.remove(0); }
    st.hist.push(format!("op_156 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_157(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 167 { st.hist.remove(0); }
    st.hist.push(format!("op_157 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_158(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 168 { st.hist.remove(0); }
    st.hist.push(format!("op_158 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_159(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 169 { st.hist.remove(0); }
    st.hist.push(format!("op_159 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_160(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 170 { st.hist.remove(0); }
    st.hist.push(format!("op_160 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_161(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 171 { st.hist.remove(0); }
    st.hist.push(format!("op_161 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_162(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 172 { st.hist.remove(0); }
    st.hist.push(format!("op_162 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_163(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 173 { st.hist.remove(0); }
    st.hist.push(format!("op_163 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_164(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 174 { st.hist.remove(0); }
    st.hist.push(format!("op_164 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_165(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 175 { st.hist.remove(0); }
    st.hist.push(format!("op_165 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_166(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 176 { st.hist.remove(0); }
    st.hist.push(format!("op_166 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_167(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 177 { st.hist.remove(0); }
    st.hist.push(format!("op_167 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_168(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 178 { st.hist.remove(0); }
    st.hist.push(format!("op_168 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_169(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 179 { st.hist.remove(0); }
    st.hist.push(format!("op_169 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_170(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 180 { st.hist.remove(0); }
    st.hist.push(format!("op_170 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_171(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 181 { st.hist.remove(0); }
    st.hist.push(format!("op_171 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_172(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 182 { st.hist.remove(0); }
    st.hist.push(format!("op_172 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_173(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 183 { st.hist.remove(0); }
    st.hist.push(format!("op_173 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_174(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 184 { st.hist.remove(0); }
    st.hist.push(format!("op_174 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_175(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 185 { st.hist.remove(0); }
    st.hist.push(format!("op_175 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_176(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 186 { st.hist.remove(0); }
    st.hist.push(format!("op_176 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_177(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 187 { st.hist.remove(0); }
    st.hist.push(format!("op_177 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_178(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 188 { st.hist.remove(0); }
    st.hist.push(format!("op_178 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_179(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 189 { st.hist.remove(0); }
    st.hist.push(format!("op_179 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_180(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 190 { st.hist.remove(0); }
    st.hist.push(format!("op_180 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_181(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 191 { st.hist.remove(0); }
    st.hist.push(format!("op_181 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_182(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 192 { st.hist.remove(0); }
    st.hist.push(format!("op_182 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_183(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 193 { st.hist.remove(0); }
    st.hist.push(format!("op_183 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_184(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 194 { st.hist.remove(0); }
    st.hist.push(format!("op_184 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_185(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 195 { st.hist.remove(0); }
    st.hist.push(format!("op_185 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_186(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 196 { st.hist.remove(0); }
    st.hist.push(format!("op_186 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_187(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 197 { st.hist.remove(0); }
    st.hist.push(format!("op_187 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_188(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 198 { st.hist.remove(0); }
    st.hist.push(format!("op_188 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_189(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 199 { st.hist.remove(0); }
    st.hist.push(format!("op_189 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_190(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 200 { st.hist.remove(0); }
    st.hist.push(format!("op_190 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_191(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 201 { st.hist.remove(0); }
    st.hist.push(format!("op_191 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_192(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 202 { st.hist.remove(0); }
    st.hist.push(format!("op_192 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_193(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 203 { st.hist.remove(0); }
    st.hist.push(format!("op_193 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_194(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 204 { st.hist.remove(0); }
    st.hist.push(format!("op_194 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_195(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 205 { st.hist.remove(0); }
    st.hist.push(format!("op_195 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_196(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 206 { st.hist.remove(0); }
    st.hist.push(format!("op_196 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_197(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 207 { st.hist.remove(0); }
    st.hist.push(format!("op_197 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_198(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 208 { st.hist.remove(0); }
    st.hist.push(format!("op_198 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_199(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 209 { st.hist.remove(0); }
    st.hist.push(format!("op_199 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_200(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 210 { st.hist.remove(0); }
    st.hist.push(format!("op_200 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_201(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 211 { st.hist.remove(0); }
    st.hist.push(format!("op_201 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_202(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 212 { st.hist.remove(0); }
    st.hist.push(format!("op_202 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_203(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 213 { st.hist.remove(0); }
    st.hist.push(format!("op_203 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_204(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 214 { st.hist.remove(0); }
    st.hist.push(format!("op_204 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_205(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 215 { st.hist.remove(0); }
    st.hist.push(format!("op_205 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_206(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 216 { st.hist.remove(0); }
    st.hist.push(format!("op_206 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_207(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 217 { st.hist.remove(0); }
    st.hist.push(format!("op_207 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_208(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 218 { st.hist.remove(0); }
    st.hist.push(format!("op_208 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_209(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 219 { st.hist.remove(0); }
    st.hist.push(format!("op_209 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_210(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 220 { st.hist.remove(0); }
    st.hist.push(format!("op_210 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_211(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 221 { st.hist.remove(0); }
    st.hist.push(format!("op_211 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_212(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 222 { st.hist.remove(0); }
    st.hist.push(format!("op_212 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_213(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 223 { st.hist.remove(0); }
    st.hist.push(format!("op_213 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_214(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 224 { st.hist.remove(0); }
    st.hist.push(format!("op_214 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_215(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 225 { st.hist.remove(0); }
    st.hist.push(format!("op_215 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_216(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 226 { st.hist.remove(0); }
    st.hist.push(format!("op_216 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_217(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 227 { st.hist.remove(0); }
    st.hist.push(format!("op_217 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_218(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 228 { st.hist.remove(0); }
    st.hist.push(format!("op_218 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_219(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 229 { st.hist.remove(0); }
    st.hist.push(format!("op_219 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_220(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 230 { st.hist.remove(0); }
    st.hist.push(format!("op_220 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_221(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 231 { st.hist.remove(0); }
    st.hist.push(format!("op_221 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_222(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 232 { st.hist.remove(0); }
    st.hist.push(format!("op_222 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_223(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 233 { st.hist.remove(0); }
    st.hist.push(format!("op_223 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_224(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 234 { st.hist.remove(0); }
    st.hist.push(format!("op_224 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_225(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 235 { st.hist.remove(0); }
    st.hist.push(format!("op_225 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_226(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 236 { st.hist.remove(0); }
    st.hist.push(format!("op_226 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_227(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 237 { st.hist.remove(0); }
    st.hist.push(format!("op_227 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_228(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 238 { st.hist.remove(0); }
    st.hist.push(format!("op_228 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_229(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 239 { st.hist.remove(0); }
    st.hist.push(format!("op_229 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_230(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 240 { st.hist.remove(0); }
    st.hist.push(format!("op_230 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_231(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 241 { st.hist.remove(0); }
    st.hist.push(format!("op_231 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_232(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 242 { st.hist.remove(0); }
    st.hist.push(format!("op_232 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_233(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 3;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 243 { st.hist.remove(0); }
    st.hist.push(format!("op_233 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_234(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 4;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 244 { st.hist.remove(0); }
    st.hist.push(format!("op_234 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_235(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 5;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 1) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 245 { st.hist.remove(0); }
    st.hist.push(format!("op_235 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_236(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 6;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 2) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 246 { st.hist.remove(0); }
    st.hist.push(format!("op_236 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_237(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 7;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 3) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 247 { st.hist.remove(0); }
    st.hist.push(format!("op_237 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_238(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 1;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 4) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 248 { st.hist.remove(0); }
    st.hist.push(format!("op_238 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn op_239(st: &mut State, b: &Buffer, m: Motion) -> usize {
    let n = st.count.max(1) * 2;
    let len = b.text.len();
    let mut p = st.pos;
    for _ in 0..n {
        p = match m { Motion::Char(d) => (p as i64 + d).clamp(0, len as i64) as usize, Motion::Line(d) => b.text[..p.min(len)].matches('\n').count() + d as usize, Motion::Word(d) => p.saturating_add((d * 5) as usize).min(len), Motion::Para(_) => len };
    }
    if st.hist.len() > 249 { st.hist.remove(0); }
    st.hist.push(format!("op_239 {:?} {}", m, p));
    st.pos = p;
    p
}

pub fn dispatch(st: &mut State, b: &Buffer, k: usize, m: Motion) -> usize {
    match k {
        0 => op_0(st, b, m),
        1 => op_1(st, b, m),
        2 => op_2(st, b, m),
        3 => op_3(st, b, m),
        4 => op_4(st, b, m),
        5 => op_5(st, b, m),
        6 => op_6(st, b, m),
        7 => op_7(st, b, m),
        8 => op_8(st, b, m),
        9 => op_9(st, b, m),
        10 => op_10(st, b, m),
        11 => op_11(st, b, m),
        12 => op_12(st, b, m),
        13 => op_13(st, b, m),
        14 => op_14(st, b, m),
        15 => op_15(st, b, m),
        16 => op_16(st, b, m),
        17 => op_17(st, b, m),
        18 => op_18(st, b, m),
        19 => op_19(st, b, m),
        20 => op_20(st, b, m),
        21 => op_21(st, b, m),
        22 => op_22(st, b, m),
        23 => op_23(st, b, m),
        24 => op_24(st, b, m),
        25 => op_25(st, b, m),
        26 => op_26(st, b, m),
        27 => op_27(st, b, m),
        28 => op_28(st, b, m),
        29 => op_29(st, b, m),
        30 => op_30(st, b, m),
        31 => op_31(st, b, m),
        32 => op_32(st, b, m),
        33 => op_33(st, b, m),
        34 => op_34(st, b, m),
        35 => op_35(st, b, m),
        36 => op_36(st, b, m),
        37 => op_37(st, b, m),
        38 => op_38(st, b, m),
        39 => op_39(st, b, m),
        40 => op_40(st, b, m),
        41 => op_41(st, b, m),
        42 => op_42(st, b, m),
        43 => op_43(st, b, m),
        44 => op_44(st, b, m),
        45 => op_45(st, b, m),
        46 => op_46(st, b, m),
        47 => op_47(st, b, m),
        48 => op_48(st, b, m),
        49 => op_49(st, b, m),
        50 => op_50(st, b, m),
        51 => op_51(st, b, m),
        52 => op_52(st, b, m),
        53 => op_53(st, b, m),
        54 => op_54(st, b, m),
        55 => op_55(st, b, m),
        56 => op_56(st, b, m),
        57 => op_57(st, b, m),
        58 => op_58(st, b, m),
        59 => op_59(st, b, m),
        60 => op_60(st, b, m),
        61 => op_61(st, b, m),
        62 => op_62(st, b, m),
        63 => op_63(st, b, m),
        64 => op_64(st, b, m),
        65 => op_65(st, b, m),
        66 => op_66(st, b, m),
        67 => op_67(st, b, m),
        68 => op_68(st, b, m),
        69 => op_69(st, b, m),
        70 => op_70(st, b, m),
        71 => op_71(st, b, m),
        72 => op_72(st, b, m),
        73 => op_73(st, b, m),
        74 => op_74(st, b, m),
        75 => op_75(st, b, m),
        76 => op_76(st, b, m),
        77 => op_77(st, b, m),
        78 => op_78(st, b, m),
        79 => op_79(st, b, m),
        80 => op_80(st, b, m),
        81 => op_81(st, b, m),
        82 => op_82(st, b, m),
        83 => op_83(st, b, m),
        84 => op_84(st, b, m),
        85 => op_85(st, b, m),
        86 => op_86(st, b, m),
        87 => op_87(st, b, m),
        88 => op_88(st, b, m),
        89 => op_89(st, b, m),
        90 => op_90(st, b, m),
        91 => op_91(st, b, m),
        92 => op_92(st, b, m),
        93 => op_93(st, b, m),
        94 => op_94(st, b, m),
        95 => op_95(st, b, m),
        96 => op_96(st, b, m),
        97 => op_97(st, b, m),
        98 => op_98(st, b, m),
        99 => op_99(st, b, m),
        100 => op_100(st, b, m),
        101 => op_101(st, b, m),
        102 => op_102(st, b, m),
        103 => op_103(st, b, m),
        104 => op_104(st, b, m),
        105 => op_105(st, b, m),
        106 => op_106(st, b, m),
        107 => op_107(st, b, m),
        108 => op_108(st, b, m),
        109 => op_109(st, b, m),
        110 => op_110(st, b, m),
        111 => op_111(st, b, m),
        112 => op_112(st, b, m),
        113 => op_113(st, b, m),
        114 => op_114(st, b, m),
        115 => op_115(st, b, m),
        116 => op_116(st, b, m),
        117 => op_117(st, b, m),
        118 => op_118(st, b, m),
        119 => op_119(st, b, m),
        120 => op_120(st, b, m),
        121 => op_121(st, b, m),
        122 => op_122(st, b, m),
        123 => op_123(st, b, m),
        124 => op_124(st, b, m),
        125 => op_125(st, b, m),
        126 => op_126(st, b, m),
        127 => op_127(st, b, m),
        128 => op_128(st, b, m),
        129 => op_129(st, b, m),
        130 => op_130(st, b, m),
        131 => op_131(st, b, m),
        132 => op_132(st, b, m),
        133 => op_133(st, b, m),
        134 => op_134(st, b, m),
        135 => op_135(st, b, m),
        136 => op_136(st, b, m),
        137 => op_137(st, b, m),
        138 => op_138(st, b, m),
        139 => op_139(st, b, m),
        140 => op_140(st, b, m),
        141 => op_141(st, b, m),
        142 => op_142(st, b, m),
        143 => op_143(st, b, m),
        144 => op_144(st, b, m),
        145 => op_145(st, b, m),
        146 => op_146(st, b, m),
        147 => op_147(st, b, m),
        148 => op_148(st, b, m),
        149 => op_149(st, b, m),
        150 => op_150(st, b, m),
        151 => op_151(st, b, m),
        152 => op_152(st, b, m),
        153 => op_153(st, b, m),
        154 => op_154(st, b, m),
        155 => op_155(st, b, m),
        156 => op_156(st, b, m),
        157 => op_157(st, b, m),
        158 => op_158(st, b, m),
        159 => op_159(st, b, m),
        160 => op_160(st, b, m),
        161 => op_161(st, b, m),
        162 => op_162(st, b, m),
        163 => op_163(st, b, m),
        164 => op_164(st, b, m),
        165 => op_165(st, b, m),
        166 => op_166(st, b, m),
        167 => op_167(st, b, m),
        168 => op_168(st, b, m),
        169 => op_169(st, b, m),
        170 => op_170(st, b, m),
        171 => op_171(st, b, m),
        172 => op_172(st, b, m),
        173 => op_173(st, b, m),
        174 => op_174(st, b, m),
        175 => op_175(st, b, m),
        176 => op_176(st, b, m),
        177 => op_177(st, b, m),
        178 => op_178(st, b, m),
        179 => op_179(st, b, m),
        180 => op_180(st, b, m),
        181 => op_181(st, b, m),
        182 => op_182(st, b, m),
        183 => op_183(st, b, m),
        184 => op_184(st, b, m),
        185 => op_185(st, b, m),
        186 => op_186(st, b, m),
        187 => op_187(st, b, m),
        188 => op_188(st, b, m),
        189 => op_189(st, b, m),
        190 => op_190(st, b, m),
        191 => op_191(st, b, m),
        192 => op_192(st, b, m),
        193 => op_193(st, b, m),
        194 => op_194(st, b, m),
        195 => op_195(st, b, m),
        196 => op_196(st, b, m),
        197 => op_197(st, b, m),
        198 => op_198(st, b, m),
        199 => op_199(st, b, m),
        200 => op_200(st, b, m),
        201 => op_201(st, b, m),
        202 => op_202(st, b, m),
        203 => op_203(st, b, m),
        204 => op_204(st, b, m),
        205 => op_205(st, b, m),
        206 => op_206(st, b, m),
        207 => op_207(st, b, m),
        208 => op_208(st, b, m),
        209 => op_209(st, b, m),
        210 => op_210(st, b, m),
        211 => op_211(st, b, m),
        212 => op_212(st, b, m),
        213 => op_213(st, b, m),
        214 => op_214(st, b, m),
        215 => op_215(st, b, m),
        216 => op_216(st, b, m),
        217 => op_217(st, b, m),
        218 => op_218(st, b, m),
        219 => op_219(st, b, m),
        220 => op_220(st, b, m),
        221 => op_221(st, b, m),
        222 => op_222(st, b, m),
        223 => op_223(st, b, m),
        224 => op_224(st, b, m),
        225 => op_225(st, b, m),
        226 => op_226(st, b, m),
        227 => op_227(st, b, m),
        228 => op_228(st, b, m),
        229 => op_229(st, b, m),
        230 => op_230(st, b, m),
        231 => op_231(st, b, m),
        232 => op_232(st, b, m),
        233 => op_233(st, b, m),
        234 => op_234(st, b, m),
        235 => op_235(st, b, m),
        236 => op_236(st, b, m),
        237 => op_237(st, b, m),
        238 => op_238(st, b, m),
        239 => op_239(st, b, m),
        _ => st.pos,
    }
}
pub fn s1_edit_4(x: usize) -> usize { x * 7 + 1 }
