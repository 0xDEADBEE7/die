//! `cargo xtask api-diff`: every public item a branch adds, removes or changes
//! in die-core, and every change to die-vim's grammar module (docs/m0-plan.md §3).
//!
//! Every `pub` item in die-core's source counts, reachable or not, so the list
//! errs toward showing too much. Doc-only changes are listed apart.

use crate::util::{Result, git, merge_base, output, read, rust_files};
use quote::ToTokens;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use syn::{Attribute, ImplItem, Item, TraitItem, Visibility};

const API_SRC: &str = "crates/die-core/src";
const GRAMMAR: &[&str] = &[
    "crates/die-vim/src/grammar.rs",
    "crates/die-vim/src/grammar/",
];

/// One public item: its rendering without bodies, and its docs.
#[derive(PartialEq)]
struct Entry {
    sig: String,
    docs: String,
}

pub fn run(root: &Path, base: Option<&str>) -> Result {
    let base = match base {
        Some(b) => b.to_string(),
        None => match merge_base(root, "HEAD") {
            Ok(base) => base,
            Err(_) => output(git(root).args(["rev-parse", "HEAD"]))?
                .trim()
                .to_string(),
        },
    };
    let short = output(git(root).args(["rev-parse", "--short", &base]))?
        .trim()
        .to_string();

    let mut old_src = Vec::new();
    for path in
        output(git(root).args(["ls-tree", "-r", "--name-only", &base, "--", API_SRC]))?.lines()
    {
        if path.ends_with(".rs") {
            let src = output(git(root).args(["show", &format!("{base}:{path}")]))?;
            old_src.push((format!("{path} at {short}"), module_of(path), src));
        }
    }
    let mut new_src = Vec::new();
    for file in rust_files(&root.join(API_SRC)) {
        let rel = file
            .strip_prefix(root)
            .unwrap_or(&file)
            .to_string_lossy()
            .into_owned();
        new_src.push((rel.clone(), module_of(&rel), read(&file)?));
    }
    let (old, new) = (collect(&old_src)?, collect(&new_src)?);

    println!("die-core public API, {short} -> working tree");
    let mut n = 0;
    for (k, e) in &new {
        match old.get(k) {
            None => {
                n += 1;
                println!("\n+ {k}\n{}", indent(&e.sig, "    "));
            }
            Some(o) if o.sig != e.sig => {
                n += 1;
                println!(
                    "\n~ {k}\n  was:\n{}\n  now:\n{}",
                    indent(&o.sig, "    "),
                    indent(&e.sig, "    ")
                );
            }
            Some(o) if o.docs != e.docs => {
                n += 1;
                println!("\n~ {k} (docs only)");
            }
            _ => {}
        }
    }
    for (k, e) in &old {
        if !new.contains_key(k) {
            n += 1;
            println!("\n- {k}\n{}", indent(&e.sig, "    "));
        }
    }
    if n == 0 {
        println!("  (no changes)");
    }
    println!();

    let mut args = vec![
        "diff".to_string(),
        "--name-status".into(),
        base.clone(),
        "--".into(),
    ];
    args.extend(GRAMMAR.iter().map(|s| s.to_string()));
    let changed = output(git(root).args(&args))?;
    let mut others = vec![
        "ls-files".to_string(),
        "--others".into(),
        "--exclude-standard".into(),
        "--".into(),
    ];
    others.extend(GRAMMAR.iter().map(|s| s.to_string()));
    let untracked = output(git(root).args(&others))?;
    println!(
        "die-vim grammar ({}), {short} -> working tree",
        GRAMMAR.join(", ")
    );
    let lines: Vec<String> = changed
        .lines()
        .map(String::from)
        .chain(untracked.lines().map(|l| format!("A\t{l}")))
        .collect();
    if lines.is_empty() {
        println!("  (no changes)");
    }
    for l in lines {
        println!("  {l}");
    }
    Ok(())
}

/// `crates/die-core/src/text/undo.rs` -> `die_core::text::undo`.
fn module_of(path: &str) -> String {
    let rel = path
        .trim_start_matches(API_SRC)
        .trim_start_matches('/')
        .trim_end_matches(".rs");
    let mut parts = vec!["die_core".to_string()];
    for p in rel.split('/') {
        if p != "lib" && p != "mod" {
            parts.push(p.to_string());
        }
    }
    parts.join("::")
}

fn indent(text: &str, by: &str) -> String {
    text.trim_end()
        .lines()
        .map(|l| format!("{by}{l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every public item in `files`: (name for errors, module path, source).
fn collect(files: &[(String, String, String)]) -> Result<BTreeMap<String, Entry>> {
    let mut parsed = Vec::new();
    for (name, module, src) in files {
        let file = syn::parse_file(src).map_err(|e| format!("{name}: {e}"))?;
        parsed.push((module, file));
    }
    // Methods and trait impls count only on types that are themselves `pub`.
    let mut types = BTreeSet::new();
    for (_, file) in &parsed {
        pub_types(&file.items, &mut types);
    }
    let mut out = BTreeMap::new();
    for (module, file) in &parsed {
        items(module, &file.items, &types, &mut out);
    }
    Ok(out)
}

fn pub_types(list: &[Item], out: &mut BTreeSet<String>) {
    for item in list {
        match item {
            Item::Mod(m) => {
                if let Some((_, inner)) = &m.content {
                    pub_types(inner, out);
                }
            }
            Item::Struct(_) | Item::Enum(_) | Item::Union(_) | Item::Trait(_) | Item::Type(_) => {
                if let Some((name, vis, _)) = head(item)
                    && is_pub(vis)
                {
                    out.insert(name);
                }
            }
            _ => {}
        }
    }
}

/// The name of an impl's type: `Tx` for `Tx<'_>`.
fn self_type(imp: &syn::ItemImpl) -> Option<String> {
    match &*imp.self_ty {
        syn::Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
    }
}

fn items(module: &str, list: &[Item], types: &BTreeSet<String>, out: &mut BTreeMap<String, Entry>) {
    for item in list {
        match item {
            Item::Mod(m) => {
                if let Some((_, inner)) = &m.content {
                    items(&format!("{module}::{}", m.ident), inner, types, out);
                }
            }
            Item::Impl(imp) if !self_type(imp).is_some_and(|t| types.contains(&t)) => {}
            Item::Impl(imp) => {
                let mut bare = imp.clone();
                bare.attrs.clear();
                bare.items.clear();
                let header = pretty(Item::Impl(bare.clone()))
                    .trim_end()
                    .trim_end_matches("{}")
                    .trim_end()
                    .to_string();
                if imp.trait_.is_some() {
                    // A trait impl is public whenever its type is; list the impl itself.
                    let sig = format!("{header} {{ .. }}");
                    out.insert(
                        format!("{module}::{header}"),
                        Entry {
                            sig,
                            docs: String::new(),
                        },
                    );
                    continue;
                }
                for it in &imp.items {
                    let (name, attrs) = match it {
                        ImplItem::Fn(f) if is_pub(&f.vis) => (f.sig.ident.to_string(), &f.attrs),
                        ImplItem::Const(c) if is_pub(&c.vis) => (c.ident.to_string(), &c.attrs),
                        _ => continue,
                    };
                    let mut one = bare.clone();
                    let mut it = it.clone();
                    match &mut it {
                        ImplItem::Fn(f) => {
                            f.attrs.retain(keep_attr);
                            f.block = syn::parse_quote!({});
                        }
                        ImplItem::Const(c) => c.attrs.retain(keep_attr),
                        _ => {}
                    }
                    one.items.push(it);
                    let sig = pretty(Item::Impl(one));
                    out.insert(
                        format!("{module}::{header}::{name}"),
                        Entry {
                            sig,
                            docs: docs(attrs),
                        },
                    );
                }
            }
            _ => {
                let Some((name, vis, attrs)) = head(item) else {
                    continue;
                };
                if is_pub(vis) {
                    out.insert(
                        format!("{module}::{name}"),
                        Entry {
                            sig: render(item),
                            docs: docs(attrs),
                        },
                    );
                }
            }
        }
    }
}

fn head(item: &Item) -> Option<(String, &Visibility, &[Attribute])> {
    Some(match item {
        Item::Fn(i) => (i.sig.ident.to_string(), &i.vis, &i.attrs),
        Item::Struct(i) => (i.ident.to_string(), &i.vis, &i.attrs),
        Item::Enum(i) => (i.ident.to_string(), &i.vis, &i.attrs),
        Item::Trait(i) => (i.ident.to_string(), &i.vis, &i.attrs),
        Item::Type(i) => (i.ident.to_string(), &i.vis, &i.attrs),
        Item::Const(i) => (i.ident.to_string(), &i.vis, &i.attrs),
        Item::Static(i) => (i.ident.to_string(), &i.vis, &i.attrs),
        Item::Union(i) => (i.ident.to_string(), &i.vis, &i.attrs),
        Item::Use(i) => (
            format!("use {}", i.tree.to_token_stream()),
            &i.vis,
            &i.attrs,
        ),
        _ => return None,
    })
}

fn is_pub(v: &Visibility) -> bool {
    matches!(v, Visibility::Public(_))
}

/// Attributes that are part of an item's API.
fn keep_attr(a: &Attribute) -> bool {
    ["derive", "repr", "non_exhaustive", "must_use"]
        .iter()
        .any(|n| a.path().is_ident(n))
}

fn docs(attrs: &[Attribute]) -> String {
    let doc = attrs.iter().filter(|a| a.path().is_ident("doc"));
    doc.map(|a| a.meta.to_token_stream().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn pretty(item: Item) -> String {
    prettyplease::unparse(&syn::File {
        shebang: None,
        attrs: vec![],
        items: vec![item],
    })
}

/// An item as Rust source, without bodies, private fields or docs. A default
/// method body in a trait shows as `{}`.
fn render(item: &Item) -> String {
    let mut item = item.clone();
    let mut private = false;
    match &mut item {
        Item::Fn(f) => {
            f.attrs.retain(keep_attr);
            *f.block = syn::parse_quote!({});
        }
        Item::Struct(s) => {
            s.attrs.retain(keep_attr);
            for f in s.fields.iter_mut() {
                f.attrs.clear();
            }
            if let syn::Fields::Named(named) = &mut s.fields {
                let all = std::mem::take(&mut named.named);
                private = all.iter().any(|f| !is_pub(&f.vis));
                named.named = all.into_iter().filter(|f| is_pub(&f.vis)).collect();
            } else if let syn::Fields::Unnamed(un) = &mut s.fields {
                for f in un.unnamed.iter_mut().filter(|f| !is_pub(&f.vis)) {
                    private = true;
                    f.ty = syn::parse_quote!(_);
                }
            }
        }
        Item::Enum(e) => {
            e.attrs.retain(keep_attr);
            for v in e.variants.iter_mut() {
                v.attrs.retain(keep_attr);
                for f in v.fields.iter_mut() {
                    f.attrs.clear();
                }
            }
        }
        Item::Trait(t) => {
            t.attrs.retain(keep_attr);
            for it in t.items.iter_mut() {
                match it {
                    TraitItem::Fn(f) => {
                        f.attrs.clear();
                        if f.default.is_some() {
                            f.default = Some(syn::parse_quote!({}));
                        }
                    }
                    TraitItem::Const(c) => c.attrs.clear(),
                    TraitItem::Type(ty) => ty.attrs.clear(),
                    _ => {}
                }
            }
        }
        Item::Type(i) => i.attrs.retain(keep_attr),
        Item::Const(i) => i.attrs.retain(keep_attr),
        Item::Static(i) => i.attrs.retain(keep_attr),
        Item::Union(i) => i.attrs.retain(keep_attr),
        Item::Use(i) => i.attrs.clear(),
        _ => {}
    }
    let mut s = pretty(item);
    if private {
        s = format!("{}\n// and private fields", s.trim_end());
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api(src: &str) -> BTreeMap<String, String> {
        let out = collect(&[("test".into(), "die_core".into(), src.into())]).unwrap();
        out.into_iter().map(|(k, e)| (k, e.sig)).collect()
    }

    #[test]
    fn impls_on_private_types_are_not_api() {
        let a = api(
            "pub(crate) struct H;\nimpl H { pub fn x(&self) {} }\nimpl Clone for H { fn clone(&self) -> H { H } }\npub struct P;\nimpl P { pub fn y(&self) {} }",
        );
        let keys: Vec<&str> = a.keys().map(String::as_str).collect();
        assert_eq!(keys, ["die_core::P", "die_core::impl P::y"]);
    }

    #[test]
    fn lists_pub_items_only() {
        let a = api(
            "pub fn f(x: u8) -> u8 { x }\nfn g() {}\npub(crate) fn h() {}\npub struct S { pub a: u8, b: u8 }\nimpl S { pub fn m(&self) {} fn p(&self) {} }\nimpl Default for S { fn default() -> S { todo!() } }",
        );
        let keys: Vec<&str> = a.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "die_core::S",
                "die_core::f",
                "die_core::impl Default for S",
                "die_core::impl S::m"
            ]
        );
        assert!(a["die_core::S"].contains("and private fields"), "{a:?}");
        assert!(!a["die_core::S"].contains(" b "), "{a:?}");
    }

    #[test]
    fn bodies_and_docs_dont_change_signatures() {
        let a = api("/// one\npub fn f() -> u8 { 1 }");
        let b = api("/// two\npub fn f() -> u8 { 2 }");
        assert_eq!(a, b);
        let c = api("pub fn f() -> u16 { 1 }");
        assert_ne!(a, c);
    }

    #[test]
    fn derives_are_api() {
        assert_ne!(
            api("#[derive(Clone)] pub struct S;"),
            api("#[derive(Clone, Copy)] pub struct S;")
        );
    }

    #[test]
    fn module_paths() {
        assert_eq!(module_of("crates/die-core/src/lib.rs"), "die_core");
        assert_eq!(module_of("crates/die-core/src/text.rs"), "die_core::text");
        assert_eq!(
            module_of("crates/die-core/src/text/mod.rs"),
            "die_core::text"
        );
        assert_eq!(
            module_of("crates/die-core/src/text/undo.rs"),
            "die_core::text::undo"
        );
    }
}
