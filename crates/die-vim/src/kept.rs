//! What a restart keeps of what die-vim holds itself, in the session as JSON (contract §14). So
//! `.`, `@@`, `;`, `,`, `n`, `N` and `gv` go on as if DIE hadn't stopped, and `&`, whose pattern
//! is `n`'s and which would delete with that alone. A `.` is keys in notation, so all of it can
//! be kept. What's read back is read part by part: a part this build can't make sense of is
//! forgotten, and the rest stays.
//!
//! Not kept, as nothing is waiting for them after a restart: the keys of an insert session still
//! open, which a restore ends and which never reached the Escape `.` needs, and a `q` still
//! recording. Not kept yet: `g;`.

use crate::Vim;
use crate::motions::Find;
use crate::repeat::{Change, Extent};
use crate::search;
use crate::visual;
use die_core::session::{Kept, Value, json};
use die_core::{Ctx, Dir, Result};

fn one_char(v: &Value) -> Option<char> {
    let s = v.as_str()?;
    let mut chars = s.chars();
    chars.next().filter(|_| chars.next().is_none())
}

fn size(v: &Value) -> Option<usize> {
    usize::try_from(v.as_u64()?).ok()
}

fn saved_extent(e: Extent) -> Value {
    match e {
        Extent::Char { lines, chars } => json!({"shape": "char", "lines": lines, "across": chars}),
        Extent::Line { lines } => json!({"shape": "line", "lines": lines}),
        Extent::Block { lines, cols } => json!({"shape": "block", "lines": lines, "across": cols}),
    }
}

fn restored_extent(v: &Value) -> Option<Extent> {
    let lines = size(&v["lines"])?;
    let across = size(&v["across"]);
    match v["shape"].as_str()? {
        "char" => across.map(|chars| Extent::Char { lines, chars }),
        "line" => Some(Extent::Line { lines }),
        "block" => across.map(|cols| Extent::Block { lines, cols }),
        _ => None,
    }
}

fn saved_change(c: &Change) -> Value {
    json!({
        "count": c.count,
        "register": c.register.map(String::from),
        "keys": c.keys,
        "visual": c.visual.map(saved_extent),
    })
}

fn restored_change(v: &Value) -> Option<Change> {
    Some(Change {
        count: size(&v["count"]),
        register: one_char(&v["register"]),
        keys: v["keys"].as_str().filter(|k| !k.is_empty())?.to_owned(),
        visual: restored_extent(&v["visual"]),
    })
}

fn saved_find(f: Find) -> Value {
    json!({
        "char": String::from(f.ch),
        "forward": f.dir == Dir::Forward,
        "till": f.till,
        "repeat": f.repeat,
    })
}

fn restored_find(v: &Value) -> Option<Find> {
    let forward = v["forward"].as_bool()?;
    Some(Find {
        ch: one_char(&v["char"])?,
        dir: if forward { Dir::Forward } else { Dir::Backward },
        till: v["till"].as_bool()?,
        repeat: v["repeat"].as_bool()?,
    })
}

pub fn save(vim: &Vim, ctx: &Ctx) -> Kept {
    let rec = vim.rec.borrow();
    let (change, last_macro, then_find) = rec.kept();
    let find = then_find.unwrap_or(vim.find);
    let buffers = vim
        .last_visual
        .iter()
        .filter_map(|(b, last)| {
            let v = last.saved(ctx.text(*b).ok()?)?;
            Some((*b, json!({"gv": v})))
        })
        .collect();
    let (with, global) = &vim.substitute;
    let state = json!({
        "dot": change.map(saved_change),
        "macro": last_macro.map(String::from),
        "find": find.map(saved_find),
        "search": vim.search.as_ref().map(search::Last::saved),
        "substitute": {"with": with, "global": global},
    });
    Kept { state, buffers }
}

pub fn restore(vim: &mut Vim, ctx: &mut Ctx, kept: &Kept) -> Result {
    let s = &kept.state;
    vim.rec
        .borrow_mut()
        .restore(restored_change(&s["dot"]), one_char(&s["macro"]));
    vim.find = restored_find(&s["find"]);
    vim.search = search::Last::restored(&s["search"]);
    if let (Some(with), Some(global)) = (
        s["substitute"]["with"].as_str(),
        s["substitute"]["global"].as_bool(),
    ) {
        vim.substitute = (with.to_owned(), global);
    }
    for (b, v) in &kept.buffers {
        let t = ctx.text_mut(*b)?;
        if let Some(last) = visual::Last::restored(t, &v["gv"])? {
            vim.last_visual.insert(*b, last);
        }
    }
    Ok(())
}
