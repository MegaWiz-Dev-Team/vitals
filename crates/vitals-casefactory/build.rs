// The commit this compiler is built from, stamped on every pack as `compiler.commit`.
//
// `compiler.version` is the workspace's version and moves with releases, not with the compiler:
// on 23 Sep 2026 the same source compiled to a different pack under the same 0.9.4, and the
// ward's door had no way to tell the two apart except by their bytes. The commit is the fact
// that did change. Read from git at build time (a worktree's `.git` file resolves the same way);
// a tree without git, or without a commit, stamps "unknown" rather than failing the build.
use std::process::Command;

fn main() {
    let git = |args: &[&str]| -> Option<String> {
        let out = Command::new("git").args(args).output().ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
        (!s.is_empty()).then_some(s)
    };
    // The commit names the code only when the tree is what the commit holds. Built with
    // uncommitted changes, the stamp says so ("-dirty"), so two different packs cannot carry
    // one commit and pass for the same compiler — a stamp that reported the intention rather
    // than the outcome would be the day's failure in one more shape.
    let dirty = git(&["status", "--porcelain", "--untracked-files=no", "--", "."]).is_some();
    let commit = std::env::var("VITALS_CASEFACTORY_COMMIT")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| git(&["rev-parse", "--short=12", "HEAD"]).map(|c| if dirty { format!("{c}-dirty") } else { c }))
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=CASEFACTORY_COMMIT={commit}");
    println!("cargo:rerun-if-env-changed=VITALS_CASEFACTORY_COMMIT");
    // Rerun when the commit moves (the reflog moves on every commit and checkout) and when the
    // crate's own files change (what makes the tree dirty or clean again). Naming any path here
    // turns off cargo's default of watching the whole package, so the crate's files are named.
    for p in ["logs/HEAD", "HEAD"] {
        if let Some(path) = git(&["rev-parse", "--git-path", p]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    for p in ["src", "data", "Cargo.toml", "build.rs"] {
        println!("cargo:rerun-if-changed={p}");
    }
}
