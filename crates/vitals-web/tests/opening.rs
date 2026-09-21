//! The opening date is written once. Everything that shows it reads it.

use std::path::PathBuf;

fn read_all(dir: &str, ext: &[&str]) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(dir);
    fn walk(d: &PathBuf, ext: &[&str], out: &mut Vec<(PathBuf, String)>) {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() { walk(&p, ext, out); continue; }
            if p.extension().and_then(|x| x.to_str()).is_some_and(|x| ext.contains(&x)) {
                if let Ok(s) = std::fs::read_to_string(&p) { out.push((p, s)); }
            }
        }
    }
    walk(&root, ext, &mut out);
    out
}

/// **`2026-09-21` appears in the source exactly once, and no page hard-codes the day.**
///
/// A date that lives in three places is three dates: the one that gets updated and the two that
/// quietly go on saying the old thing. The pages render what the API gives them.
#[test]
fn the_opening_date_is_written_once_and_pages_do_not_hard_code_it() {
    let src = read_all("src", &["rs"]);
    let hits: Vec<_> = src.iter()
        .flat_map(|(p, s)| s.matches("2026-09-21").map(move |_| p.display().to_string()))
        .collect();
    assert_eq!(hits.len(), 1, "the ISO date lives in one constant, not {}: {hits:?}", hits.len());

    let pages = read_all("static", &["html", "js"]);
    for (p, s) in &pages {
        assert!(!s.contains("21 Sep 2026") && !s.contains("2026-09-21"),
                "{} hard-codes the opening date — it should show the sentence the API gives it",
                p.display());
    }
}
