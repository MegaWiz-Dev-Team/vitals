//! Where cases are read from: a checkout on disk, or a git ref inside one — read-only either way.
//!
//! `--cases /path/to/embla-cases` reads `cases/<id>/case.json` from the working tree;
//! `--cases /path/to/embla-cases@some/branch` reads the same path out of that ref with
//! `git show`, so a case on a branch nobody has checked out compiles without anyone touching
//! the library's working tree. Nothing here writes, checks out, or stashes.

use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The advisor's ruling on a case — the `world.review` block of its `case.meta.yaml`, as the
/// library's own `tools/world-meta.py review` writes it: a status, a reviewing *role* (never a
/// name) and a date. `reviewed` is the only status that clears a pack's `provisional` flag.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct Review {
    /// `provisional` | `reviewed` | `rejected`
    pub status: String,
    /// A role — `clinical-advisor`, `paediatrician`, … — as the library's validator allows.
    pub by: Option<String>,
    /// `YYYY-MM-DD`
    pub date: Option<String>,
}

impl Review {
    pub fn reviewed(&self) -> bool {
        self.status == "reviewed"
    }
    pub fn rejected(&self) -> bool {
        self.status == "rejected"
    }
    /// "the clinical advisor", from the role the block names; "the reviewer" when it names none.
    pub fn who(&self) -> String {
        match self.by.as_deref().map(str::trim).filter(|b| !b.is_empty()) {
            Some(role) => format!("the {}", role.replace(['-', '_'], " ")),
            None => "the reviewer".to_string(),
        }
    }
    /// "reviewed by the clinical advisor on 2026-09-20" — the sentence the pack's notes carry.
    pub fn sentence(&self) -> String {
        format!("{} by {} on {}", self.status, self.who(), self.date.as_deref().unwrap_or("an unrecorded date"))
    }
}

/// Read `world.review` out of a `case.meta.yaml`. The block is written in one canonical shape by
/// the library's tools (`world:` at the margin, its keys two spaces in, `review:` with `status`,
/// `by`, `date` four spaces in — or `review: null`), so this reads that shape and nothing more
/// general: the `review:` line under `provenance:` is prose and is never the ruling. `None` when
/// the case is not on the World lane or its review is null.
pub fn parse_world_review(yaml: &str) -> Option<Review> {
    let mut in_world = false;
    let mut in_review = false;
    let mut review: Option<Review> = None;
    for raw in yaml.lines() {
        let line = raw.trim_end();
        if line.is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent == 0 {
            if in_world {
                break;
            }
            in_world = line == "world:";
            continue;
        }
        if !in_world {
            continue;
        }
        if indent == 2 {
            let body = line.trim_start();
            if body == "review:" {
                in_review = true;
                review = Some(Review::default());
            } else {
                in_review = false;
                if body.starts_with("review:") {
                    // `review: null` — or anything else on one line — is no ruling
                    review = None;
                }
            }
            continue;
        }
        if in_review && indent == 4 {
            if let (Some(r), Some((k, v))) = (review.as_mut(), line.trim_start().split_once(':')) {
                let v = v.trim().trim_matches('"').trim_matches('\'').to_string();
                let v = if v.is_empty() || v == "null" || v == "~" { None } else { Some(v) };
                match k.trim() {
                    "status" => r.status = v.unwrap_or_default(),
                    "by" => r.by = v,
                    "date" => r.date = v,
                    _ => {}
                }
            }
        }
    }
    review.filter(|r| !r.status.is_empty())
}

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

    /// The advisor's ruling on one case, from `cases/<id>/case.meta.yaml` beside the case —
    /// `None` when there is no meta file, no world block, or no review.
    pub fn review(&self, id: &str) -> Option<Review> {
        let text = match &self.git_ref {
            None => std::fs::read_to_string(self.dir.join("cases").join(id).join("case.meta.yaml")).ok()?,
            Some(r) => {
                let out = Command::new("git").arg("-C").arg(&self.dir).arg("show").arg(format!("{r}:cases/{id}/case.meta.yaml")).output().ok()?;
                if !out.status.success() {
                    return None;
                }
                String::from_utf8(out.stdout).ok()?
            }
        };
        parse_world_review(&text)
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
