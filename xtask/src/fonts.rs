//! `cargo xtask fonts`: Ioskeley Mono Term v2.1.0 into `fonts/` at the workspace
//! root, checked by SHA-256. Nothing is installed. The zip goes into a fresh
//! temporary directory, and only the four `.ttf` files come out of it.

use crate::util::{Result, TempDir, run, sha256};
use std::path::Path;
use std::process::Command;

const URL: &str =
    "https://github.com/ahatem/IoskeleyMono/releases/download/v2.1.0/IoskeleyMono-Term.zip";
const ZIP_SHA256: &str = "a45f766d60fb7f1ed244ecd456a890d68cad8bd480f7803325d04133995bc000";
pub const FILES: [(&str, &str); 4] = [
    (
        "IoskeleyMonoTerm-Regular.ttf",
        "02c9ec06134b288b0fab08ee9ed10e80eb555110273b0d5dceedcb779776412e",
    ),
    (
        "IoskeleyMonoTerm-Bold.ttf",
        "720f78b0000b5cd9c50e2ff8a8d6c0e99f87931044e3f0b2d8732d11caf937ab",
    ),
    (
        "IoskeleyMonoTerm-Italic.ttf",
        "6f63d9827ff7303b96aeca1f8a78d426bef6fc601eceaf6d99449c74adb0f4d5",
    ),
    (
        "IoskeleyMonoTerm-BoldItalic.ttf",
        "198e40634004a8b98fde11ff37975b270073bb16abb50b34af8cc974cb27d46e",
    ),
];

/// Whether `dir` holds all four files with the right checksums.
pub fn present(dir: &Path) -> Result<bool> {
    for (name, sum) in FILES {
        let p = dir.join(name);
        if !p.exists() {
            return Ok(false);
        }
        if sha256(&p)? != sum {
            return Err(format!(
                "{} has the wrong checksum; delete it and run cargo xtask fonts",
                p.display()
            ));
        }
    }
    Ok(true)
}

pub fn run_fonts(root: &Path) -> Result {
    let out = root.join("fonts");
    if present(&out)? {
        println!("fonts: ok ({})", out.display());
        return Ok(());
    }
    let tmp = TempDir::new("fonts")?;
    let zip = tmp.path().join("term.zip");
    println!("fonts: downloading {URL}");
    run(Command::new("/usr/bin/curl")
        .args(["-fsSL", "-A", "Mozilla/5.0 (generic)", "-o"])
        .arg(&zip)
        .arg(URL))?;
    let got = sha256(&zip)?;
    if got != ZIP_SHA256 {
        return Err(format!(
            "fonts: the zip's SHA-256 is {got}, not {ZIP_SHA256}"
        ));
    }
    let x = tmp.path().join("x");
    for (name, sum) in FILES {
        run(Command::new("/usr/bin/unzip")
            .args(["-o", "-j", "-q"])
            .arg(&zip)
            .arg(format!("Normal/Hinted/{name}"))
            .arg("-d")
            .arg(&x))?;
        if sha256(&x.join(name))? != sum {
            return Err(format!("fonts: {name} from the zip has the wrong checksum"));
        }
    }
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    for (name, _) in FILES {
        std::fs::copy(x.join(name), out.join(name)).map_err(|e| format!("{name}: {e}"))?;
    }
    println!("fonts: ok ({})", out.display());
    Ok(())
}
