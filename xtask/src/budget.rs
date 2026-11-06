//! `cargo xtask budget`: guard 7's line budgets and the per-module ratchets
//! (docs/m0-plan.md §4), with limits in `budget.toml`.
//!
//! A crate with per-platform code lists its `builds`, and each of its modules
//! is neutral (in every build) or tagged with the `platforms` whose builds it's
//! in. A build's lines are the crate's total less the modules tagged only for
//! other platforms: its neutral modules plus its platform's, or the whole
//! crate if it has no modules. `build_limit` caps each build, as `limit` caps
//! the crate's total (docs/records/platforms.md §5.7).

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
    /// The crate's total, across every platform.
    pub limit: Option<usize>,
    /// Dev-only code: counted, not capped, not in the product total.
    #[serde(default)]
    pub dev: bool,
    pub note: Option<String>,
    /// The platforms the crate builds for, when its modules differ by platform.
    #[serde(default)]
    pub builds: Vec<String>,
    /// The cap on each build: the neutral modules plus one platform's.
    pub build_limit: Option<usize>,
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
    /// The builds the module is in; none means every build (neutral).
    #[serde(default)]
    pub platforms: Vec<String>,
}

pub struct Counted {
    pub name: String,
    pub lines: usize,
    pub limit: Option<usize>,
    pub dev: bool,
    pub note: Option<String>,
    pub modules: Vec<CountedModule>,
    /// Each build: (platform, lines, limit).
    pub builds: Vec<(String, usize, Option<usize>)>,
}

pub struct CountedModule {
    pub name: String,
    pub lines: usize,
    pub limit: usize,
    pub platforms: Vec<String>,
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
        check_platforms(c, &mods)?;
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
        let modules: Vec<CountedModule> = mods
            .iter()
            .zip(per_mod)
            .map(|(m, n)| CountedModule {
                name: m.name.clone(),
                lines: n,
                limit: m.limit,
                platforms: m.platforms.clone(),
            })
            .collect();
        out.push(Counted {
            name: c.name.clone(),
            lines: total,
            limit: c.limit,
            dev: c.dev,
            note: c.note.clone(),
            builds: builds(c, total, &modules),
            modules,
        });
    }
    Ok(out)
}

/// Platform tags need a crate that lists its builds, and must name one of them.
fn check_platforms(c: &CrateBudget, mods: &[&ModuleBudget]) -> Result {
    if c.build_limit.is_some() && c.builds.is_empty() {
        return Err(format!(
            "budget.toml: {} has a build_limit but no builds",
            c.name
        ));
    }
    for m in mods {
        for p in &m.platforms {
            if !c.builds.contains(p) {
                return Err(format!(
                    "budget.toml: module {:?} is tagged {p:?}, which isn't one of {}'s builds ({:?})",
                    m.name, c.name, c.builds
                ));
            }
        }
    }
    Ok(())
}

/// Each of the crate's builds: its `total` less the modules tagged only for
/// other platforms. Lines in no module, as in a crate with none, are in every
/// build.
fn builds(
    c: &CrateBudget,
    total: usize,
    modules: &[CountedModule],
) -> Vec<(String, usize, Option<usize>)> {
    c.builds
        .iter()
        .map(|b| {
            let elsewhere: usize = modules
                .iter()
                .filter(|m| !m.platforms.is_empty() && !m.platforms.contains(b))
                .map(|m| m.lines)
                .sum();
            (b.clone(), total - elsewhere, c.build_limit)
        })
        .collect()
}

pub fn report(counted: &[Counted]) -> (String, Vec<String>) {
    let mut s = String::new();
    let mut over = Vec::new();
    let module_label = |m: &CountedModule| {
        if m.platforms.is_empty() {
            format!("  {}", m.name)
        } else {
            format!("  {} [{}]", m.name, m.platforms.join(", "))
        }
    };
    let build_label = |b: &str| format!("  {b} build: neutral + {b}");
    let w = counted
        .iter()
        .flat_map(|c| {
            c.modules
                .iter()
                .map(|m| module_label(m).len())
                .chain(c.builds.iter().map(|b| build_label(&b.0).len()))
        })
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
        for m in &c.modules {
            s += &format!("{:<w$} {:>7} {:>7}\n", module_label(m), m.lines, m.limit);
            if m.lines > m.limit {
                over.push(format!(
                    "{} / {} is {} lines, over its {}",
                    c.name, m.name, m.lines, m.limit
                ));
            }
        }
        for (b, n, l) in &c.builds {
            let limit = l.map_or("-".into(), |l| l.to_string());
            s += &format!("{:<w$} {:>7} {:>7}\n", build_label(b), n, limit);
            if let Some(l) = l.filter(|&l| *n > l) {
                over.push(format!(
                    "{}'s {b} build (its neutral and {b} modules) is {n} lines, over its {l}",
                    c.name
                ));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn render() -> (CrateBudget, Vec<CountedModule>) {
        let krate = CrateBudget {
            name: "die-render".into(),
            path: "crates/die-render".into(),
            limit: Some(7000),
            dev: false,
            note: None,
            builds: vec!["macos".into(), "linux".into()],
            build_limit: Some(5000),
        };
        let module = |name: &str, lines, platforms: &[&str]| CountedModule {
            name: name.into(),
            lines,
            limit: 3000,
            platforms: platforms.iter().map(|p| p.to_string()).collect(),
        };
        let modules = vec![
            module("neutral", 2500, &[]),
            module("platform::macos", 2000, &["macos"]),
            module("portable", 1600, &["linux"]),
            module("platform::linux", 900, &["linux"]),
        ];
        (krate, modules)
    }

    #[test]
    fn platform_macos_is_held_to_what_the_unsafe_review_was_planned_for() {
        // docs/m0-plan.md §3 planned for 900; budget.toml says what raised it. Raising it means
        // changing this too, with a count in hand: 1,040 at 1,060.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let budget = load(root).unwrap();
        let macos = budget.modules.iter().find(|m| m.name == "platform::macos");
        assert!(macos.unwrap().limit <= 1060);
    }

    #[test]
    fn a_build_is_its_neutral_modules_plus_its_platforms() {
        let (krate, modules) = render();
        assert_eq!(
            builds(&krate, 7000, &modules),
            [
                ("macos".to_string(), 4500, Some(5000)),
                ("linux".to_string(), 5000, Some(5000)),
            ]
        );
    }

    #[test]
    fn a_crate_without_modules_is_whole_in_every_build() {
        let (krate, _) = render();
        let counted = Counted {
            name: krate.name.clone(),
            lines: 5200,
            limit: krate.limit,
            dev: false,
            note: None,
            builds: builds(&krate, 5200, &[]),
            modules: Vec::new(),
        };
        assert_eq!(
            counted.builds,
            [
                ("macos".to_string(), 5200, Some(5000)),
                ("linux".to_string(), 5200, Some(5000)),
            ]
        );
        let (_, over) = report(&[counted]);
        assert_eq!(over.len(), 2, "{over:?}");
    }

    #[test]
    fn each_build_and_the_total_have_their_limits() {
        let (krate, mut modules) = render();
        modules[3].lines = 1000;
        let total = modules.iter().map(|m| m.lines).sum();
        let counted = Counted {
            name: krate.name.clone(),
            lines: total,
            limit: krate.limit,
            dev: false,
            note: None,
            builds: builds(&krate, total, &modules),
            modules,
        };
        let (text, over) = report(&[counted]);
        assert!(text.contains("  platform::macos [macos]"), "{text}");
        assert!(text.contains("  linux build: neutral + linux"), "{text}");
        assert_eq!(
            over,
            [
                "die-render is 7100 lines, over its 7000",
                "die-render's linux build (its neutral and linux modules) is 5100 lines, over its 5000",
            ]
        );
    }

    #[test]
    fn a_tag_must_name_one_of_the_crates_builds() {
        let (krate, _) = render();
        let module = ModuleBudget {
            krate: "die-render".into(),
            name: "platform::freebsd".into(),
            limit: 100,
            files: vec!["platform/freebsd/".into()],
            platforms: vec!["freebsd".into()],
        };
        let err = check_platforms(&krate, &[&module]).unwrap_err();
        assert!(err.contains("tagged \"freebsd\""), "{err}");
    }
}
