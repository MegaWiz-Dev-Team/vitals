//! Where cases are read from: a checkout on disk, or a git ref inside one — read-only either way.
//!
//! `--cases /path/to/embla-cases` reads `cases/<id>/case.json` from the working tree;
//! `--cases /path/to/embla-cases@some/branch` reads the same path out of that ref with
//! `git show`, so a case on a branch nobody has checked out compiles without anyone touching
//! the library's working tree. Nothing here writes, checks out, or stashes.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The deployment target whose cases are the season's. World never carries them.
pub const SEASON_TARGET: &str = "vitals";

pub struct Library {
    pub dir: PathBuf,
    /// `None` reads the working tree; `Some(ref)` reads that ref.
    pub git_ref: Option<String>,
}

impl Library {
    /// Parse `<dir>` or `<dir>@<ref>`.
    pub fn open(spec: &str) -> Result<Library, String> {
        let (dir, git_ref) = match spec.rsplit_once('@') {
            // a path can contain '@' only if the piece after it is not a plausible ref, and a
            // directory that exists on disk under the whole spec is a directory
            Some((d, r)) if !Path::new(spec).is_dir() && !r.is_empty() => (d.to_string(), Some(r.to_string())),
            _ => (spec.to_string(), None),
        };
        let dir = PathBuf::from(dir);
        if !dir.is_dir() {
            return Err(format!("{} is not a directory", dir.display()));
        }
        if git_ref.is_none() && !dir.join("cases").is_dir() {
            return Err(format!("{} has no cases/ directory", dir.display()));
        }
        Ok(Library { dir, git_ref })
    }

    /// The label a pack records as `source.ref`.
    pub fn ref_label(&self) -> String {
        self.git_ref.clone().unwrap_or_else(|| "worktree".to_string())
    }

    /// The commit the ref (or HEAD) resolves to, when the directory is a git checkout.
    pub fn commit(&self) -> Option<String> {
        let out = Command::new("git")
            .arg("-C").arg(&self.dir)
            .arg("rev-parse").arg(self.git_ref.as_deref().unwrap_or("HEAD"))
            .output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// The raw bytes of one case's `case.json`.
    pub fn read(&self, id: &str) -> Result<String, String> {
        match &self.git_ref {
            None => {
                let p = self.dir.join("cases").join(id).join("case.json");
                std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))
            }
            Some(r) => {
                let spec = format!("{r}:cases/{id}/case.json");
                let out = Command::new("git").arg("-C").arg(&self.dir).arg("show").arg(&spec).output().map_err(|e| format!("git: {e}"))?;
                if !out.status.success() {
                    return Err(format!("git show {spec}: {}", String::from_utf8_lossy(&out.stderr).trim()));
                }
                String::from_utf8(out.stdout).map_err(|e| format!("{spec}: not utf-8: {e}"))
            }
        }
    }

    /// The raw text of a file at the library root, from the working tree or the ref.
    fn root_file(&self, name: &str) -> Option<String> {
        match &self.git_ref {
            None => std::fs::read_to_string(self.dir.join(name)).ok(),
            Some(r) => {
                let out = Command::new("git").arg("-C").arg(&self.dir).arg("show").arg(format!("{r}:{name}")).output().ok()?;
                out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
            }
        }
    }

    /// Case ids the library records as deployed to the season (`deployments.jsonl`, target
    /// `vitals`). The founder's rule: World never carries the season's content, so these are
    /// refused by name before the compiler ever reads them. An absent or unreadable file means
    /// an empty set — and the report says which it was.
    pub fn season_sources(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let Some(text) = self.root_file("deployments.jsonl") else { return out };
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
            if v["target"].as_str() == Some(SEASON_TARGET) {
                if let Some(id) = v["case_id"].as_str() {
                    out.insert(id.to_string());
                }
            }
        }
        out
    }

    /// Every case id in the library, sorted.
    pub fn list(&self) -> Result<Vec<String>, String> {
        let mut ids: Vec<String> = match &self.git_ref {
            None => std::fs::read_dir(self.dir.join("cases"))
                .map_err(|e| format!("cases/: {e}"))?
                .filter_map(|e| e.ok())
                .filter(|e| e.path().join("case.json").is_file())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect(),
            Some(r) => {
                let out = Command::new("git").arg("-C").arg(&self.dir).arg("ls-tree").arg("--name-only").arg(format!("{r}:cases")).output().map_err(|e| format!("git: {e}"))?;
                if !out.status.success() {
                    return Err(format!("git ls-tree {r}:cases: {}", String::from_utf8_lossy(&out.stderr).trim()));
                }
                String::from_utf8_lossy(&out.stdout).lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
            }
        };
        ids.sort();
        Ok(ids)
    }
}
