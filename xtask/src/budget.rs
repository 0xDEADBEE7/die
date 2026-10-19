//! `cargo xtask budget`: guard 7's line budgets and the per-module ratchets
//! (docs/m0-plan.md §4), with limits in `budget.toml`.

use crate::count::count_formatted;
use crate::util::{Result, read, rust_files};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    #[serde(rename = "crate")]
    pub crates: Vec<CrateBudget>,
    #[serde(default, rename = "module")]
    pub modules: Vec<ModuleBudget>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrateBudget {
    pub name: String,
    pub path: String,
    pub limit: Option<usize>,
    /// Dev-only code: counted, not capped, not in the product total.
    #[serde(default)]
    pub dev: bool,
    pub note: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleBudget {
    #[serde(rename = "crate")]
    pub krate: String,
    pub name: String,
    pub limit: usize,
    /// Paths under the crate's `src/`; one ending in `/` is a directory.
    pub files: Vec<String>,
}

pub struct Counted {
    pub name: String,
    pub lines: usize,
    pub limit: Option<usize>,
    pub dev: bool,
    pub note: Option<String>,
    pub modules: Vec<(String, usize, usize)>,
}

pub fn load(root: &Path) -> Result<Budget> {
    let path = root.join("budget.toml");
    toml::from_str(&read(&path)?).map_err(|e| format!("{}: {e}", path.display()))
}

/// A crate's counted files: everything under `src/`, and `build.rs`.
fn crate_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = rust_files(&dir.join("src"));
    if dir.join("build.rs").exists() {
        files.push(dir.join("build.rs"));
    }
    files
}

pub fn measure(root: &Path, budget: &Budget) -> Result<Vec<Counted>> {
    let mut out = Vec::new();
    for c in &budget.crates {
        let dir = root.join(&c.path);
        let src = dir.join("src");
        // A module may name files that don't exist yet: they're where its code will go.
        let mods: Vec<&ModuleBudget> = budget
            .modules
            .iter()
            .filter(|m| m.krate == c.name)
            .collect();
        let mut total = 0;
        let mut per_mod = vec![0; mods.len()];
        for file in crate_files(&dir) {
            let n =
                count_formatted(&read(&file)?).map_err(|e| format!("{}: {e}", file.display()))?;
            total += n;
            if mods.is_empty() {
                continue;
            }
            let rel = file
                .strip_prefix(&src)
                .unwrap_or(&file)
                .to_string_lossy()
                .into_owned();
            let owners: Vec<usize> = mods
                .iter()
                .enumerate()
                .filter(|(_, m)| {
                    m.files.iter().any(|f| {
                        if f.ends_with('/') {
                            rel.starts_with(f.as_str())
                        } else {
                            rel == *f
                        }
                    })
                })
                .map(|(i, _)| i)
                .collect();
            match owners.as_slice() {
                [i] => per_mod[*i] += n,
                [] => {
                    return Err(format!(
                        "{}/src/{rel} belongs to no module in budget.toml",
                        c.path
                    ));
                }
                _ => {
                    return Err(format!(
                        "{}/src/{rel} belongs to more than one module in budget.toml",
                        c.path
                    ));
                }
            }
        }
        out.push(Counted {
            name: c.name.clone(),
            lines: total,
            limit: c.limit,
            dev: c.dev,
            note: c.note.clone(),
            modules: mods
                .iter()
                .zip(per_mod)
                .map(|(m, n)| (m.name.clone(), n, m.limit))
                .collect(),
        });
    }
    Ok(out)
}

pub fn report(counted: &[Counted]) -> (String, Vec<String>) {
    let mut s = String::new();
    let mut over = Vec::new();
    let w = counted
        .iter()
        .flat_map(|c| c.modules.iter().map(|m| m.0.len() + 2))
        .max()
        .unwrap_or(0)
        .max(24);
    s += &format!("{:<w$} {:>7} {:>7}\n", "crate / module", "lines", "limit");
    let mut product = 0;
    for c in counted {
        let limit = match (c.limit, c.dev) {
            (Some(l), _) => l.to_string(),
            (None, true) => "dev".into(),
            (None, false) => "-".into(),
        };
        let note = c
            .note
            .as_deref()
            .map(|n| format!("  ({n})"))
            .unwrap_or_default();
        s += &format!("{:<w$} {:>7} {:>7}{note}\n", c.name, c.lines, limit);
        if let Some(l) = c.limit.filter(|&l| c.lines > l) {
            over.push(format!("{} is {} lines, over its {l}", c.name, c.lines));
        }
        for (name, n, l) in &c.modules {
            s += &format!("{:<w$} {:>7} {:>7}\n", format!("  {name}"), n, l);
            if n > l {
                over.push(format!("{} / {name} is {n} lines, over its {l}", c.name));
            }
        }
        if !c.dev {
            product += c.lines;
        }
    }
    s += &format!("{:<w$} {:>7}\n", "product code", product);
    (s, over)
}

pub fn run(root: &Path) -> Result {
    let budget = load(root)?;
    let (text, over) = report(&measure(root, &budget)?);
    print!("{text}");
    if over.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "over budget (the cut lists are in docs/m0-plan.md §4):\n  {}",
            over.join("\n  ")
        ))
    }
}
