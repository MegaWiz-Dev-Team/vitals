//! Everything the server reads at runtime has to be in the image.
//!
//! `demo/personas` was not, from the day the stations got voices until this test existed. Nothing
//! failed: the server reads a persona per case off the disk and a case with no file plays mute
//! rather than erroring, which is right for a case that has no persona and wrong for a case whose
//! persona simply never arrived. Production printed `personas 1/17 voiced` at every boot and
//! served twelve silent patients.
//!
//! So this is not a test that `demo/personas` is copied. It is a test that **every directory the
//! server loads from is copied**, because the next one added will be forgotten the same way.

use std::path::PathBuf;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The runtime roots, named here because the code that reads them names them separately:
/// `scenario_path`, `persona_path`, `rubric_path` and the archive. A path that appears in any of
/// those and not in the Dockerfile is a file the server will look for and not find.
const RUNTIME_PATHS: &[&str] = &[
    "demo/scenarios",
    "demo/stations",
    "demo/rubrics",
    "demo/personas",
    "demo/ep1-en.json",
    "conformance",
];

#[test]
fn the_image_carries_every_path_the_server_reads() {
    let root = repo();
    let dockerfile = std::fs::read_to_string(root.join("Dockerfile")).expect("Dockerfile");
    let copied: Vec<&str> = dockerfile
        .lines()
        .filter_map(|l| l.trim().strip_prefix("COPY "))
        .flat_map(|l| l.split_whitespace().next())
        .collect();

    for path in RUNTIME_PATHS {
        assert!(
            root.join(path).exists(),
            "{path} is listed as a runtime path and is not in the repository"
        );
        assert!(
            copied.contains(path),
            "the server reads {path} at runtime and the Dockerfile does not copy it — in the \
             image it will simply not be there, and the failure will be silence rather than an \
             error. Copied: {copied:?}"
        );
    }
}

/// Every case that has a persona file must be one the server can actually play.
///
/// A persona for a case id nothing serves is a file nobody reads; the inverse — a case with no
/// persona — is allowed and visible, and `/api/chain`'s `voiced` reports it.
#[test]
fn every_persona_belongs_to_a_case() {
    let root = repo();
    let personas = std::fs::read_dir(root.join("demo/personas")).expect("demo/personas");
    for e in personas.filter_map(Result::ok) {
        let name = e.file_name().to_string_lossy().replace(".json", "");
        let station = root.join(format!("demo/stations/{name}.sce.json"));
        let scenario = root.join(format!("demo/scenarios/{name}.json"));
        assert!(
            station.exists() || scenario.exists(),
            "demo/personas/{name}.json has no case behind it — nothing will ever read it"
        );
    }
}

/// How many cases can speak, so the number in verify-deploy.sh comes from the tree.
///
/// Today it is thirteen: twelve stations with persona files, plus ep1, which reads its own
/// scenario. **ep2 to ep5 have no persona and are mute**, which is a real gap and not one the
/// image can fix — the files do not exist. Naming it here stops thirteen being read as "all".
#[test]
fn the_voiced_count_is_the_personas_plus_ep1() {
    let root = repo();
    let personas: Vec<String> = std::fs::read_dir(root.join("demo/personas"))
        .expect("demo/personas")
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().replace(".json", ""))
        .collect();
    assert_eq!(personas.len(), 12, "the persona count moved: {personas:?}");
    for mute in ["ep2", "ep3", "ep4", "ep5"] {
        assert!(
            !personas.contains(&mute.to_string()),
            "{mute} has a persona now — the voiced count in scripts/verify-deploy.sh is derived \
             from this directory, so it follows automatically, but the four numbered episodes \
             being mute was a known gap and this test is where it was written down"
        );
    }
}

/// Everything the crate bakes in has to be in the **build** stage's context.
///
/// The neighbour above is about files the server reads at runtime. This is the other half, and it
/// failed the first time it could: `ward::case_patient` include_str!s the twelve persona files, the
/// crate compiled here, and Cloud Build answered with twelve `couldn't read` errors — because the
/// build stage copies `Cargo.toml`, `Cargo.lock`, `crates/` and `pitch/`, and `demo/` arrives only
/// in the runtime stage, long after rustc has finished.
///
/// It is the same class of mistake as the runtime one and it deserves the same kind of test: not
/// "is demo/personas copied" but "is every path this crate bakes in copied", because the next one
/// added will be forgotten the same way.
#[test]
fn the_build_stage_copies_everything_the_crate_bakes_in() {
    let dockerfile = std::fs::read_to_string(repo().join("Dockerfile")).expect("a Dockerfile");

    // The build stage is what rustc sees: from its FROM to the next one.
    let build_stage: String = dockerfile
        .split("\nFROM ")
        .find(|s| s.starts_with("rust:"))
        .expect("a build stage on a rust base")
        .to_string();
    let copied: Vec<String> = build_stage
        .lines()
        .filter_map(|l| l.trim().strip_prefix("COPY "))
        .flat_map(|rest| {
            let mut parts: Vec<&str> = rest.split_whitespace().collect();
            parts.pop(); // the destination
            parts.into_iter().map(|p| p.trim_start_matches("./").to_string()).collect::<Vec<_>>()
        })
        .collect();
    assert!(copied.iter().any(|c| c == "crates"), "the build stage must copy the crates: {copied:?}");

    // Every include_str! in the crate, resolved against the file that wrote it.
    let mut baked: Vec<(PathBuf, String)> = Vec::new();
    let mut stack = vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("a source directory").flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let src = std::fs::read_to_string(&path).expect("a source file");
            for macro_name in ["include_str!(\"", "include_bytes!(\""] {
                for piece in src.split(macro_name).skip(1) {
                    let Some(rel) = piece.split('"').next() else { continue };
                    baked.push((path.clone(), rel.to_string()));
                }
            }
        }
    }
    assert!(baked.len() >= 12, "the crate bakes in more than this test found: {}", baked.len());

    let repo_root = repo().canonicalize().expect("the repository");
    for (from, rel) in baked {
        let resolved = from.parent().expect("a directory").join(&rel);
        let resolved = resolved.canonicalize()
            .unwrap_or_else(|e| panic!("{} bakes in {rel}, which is not there: {e}", from.display()));
        let inside = resolved.strip_prefix(&repo_root)
            .unwrap_or_else(|_| panic!("{rel} resolves outside the repository"));
        let inside = inside.to_string_lossy().replace('\\', "/");
        assert!(copied.iter().any(|c| inside == *c || inside.starts_with(&format!("{c}/"))),
                "{} bakes in {inside}, and the build stage copies {copied:?} — rustc will not \
                 find it, and the failure arrives twenty minutes into a build rather than here",
                from.display());

        // A COPY line is only half the answer. The ignore files decide what that COPY can see, and
        // a path excluded there is absent from the context with no error until rustc looks for it.
        for ignore in ["\u{2e}dockerignore", "\u{2e}gcloudignore"] {
            let patterns = ignore_patterns(&repo(), ignore);
            assert!(!excluded_by(&inside, &patterns),
                    "{} bakes in {inside}, and {ignore} keeps it out of the build context. The \
                     COPY line is there and the file is in the repository; it simply never \
                     arrives, and Cloud Build says `couldn't read {inside}` twenty minutes in. \
                     Either whitelist it with a ! line or put it where the crate already owns \
                     its own assets.",
                    from.display());
        }
    }
}

/// The ignore file's patterns, in order, with gcloud's `#!include:` directive expanded.
///
/// Comments and blank lines dropped; everything else kept as written, because order is meaning:
/// the last pattern that matches a path decides, and a `!` line only works if it comes after the
/// line it is undoing.
fn ignore_patterns(root: &std::path::Path, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(text) = std::fs::read_to_string(root.join(name)) else { return out };
    for line in text.lines() {
        let line = line.trim();
        if let Some(included) = line.strip_prefix("#!include:") {
            out.extend(ignore_patterns(root, included.trim()));
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        out.push(line.trim_end_matches('/').to_string());
    }
    out
}

/// Would this path be missing from the build context?
///
/// Docker's rule, which is the one that bit us: the last matching pattern wins, `!` undoes, and a
/// pattern that matches a **parent directory** excludes everything under it — `pitch/*` matches
/// `pitch/logo`, so `pitch/logo/favicon-world.svg` never reaches the context even though no
/// pattern names it. That last part is the whole reason this function exists rather than a
/// `contains` check.
fn excluded_by(path: &str, patterns: &[String]) -> bool {
    let mut excluded = false;
    for pattern in patterns {
        let (negated, pattern) = match pattern.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, pattern.as_str()),
        };
        if matches_or_parent(path, pattern) {
            excluded = !negated;
        }
    }
    excluded
}

/// The pattern against the path and against every directory above it.
fn matches_or_parent(path: &str, pattern: &str) -> bool {
    let segments: Vec<&str> = path.split('/').collect();
    (1..=segments.len()).any(|n| glob_match(&segments[..n], pattern))
}

/// Segment-wise glob: `**` spans any number of segments, `*` and `?` stay inside one.
fn glob_match(path: &[&str], pattern: &str) -> bool {
    let pat: Vec<&str> = pattern.split('/').collect();
    fn walk(path: &[&str], pat: &[&str]) -> bool {
        match (path.first(), pat.first()) {
            (None, None) => true,
            (_, Some(&"**")) => walk(path, &pat[1..]) || (!path.is_empty() && walk(&path[1..], pat)),
            (Some(p), Some(q)) => segment_match(p, q) && walk(&path[1..], &pat[1..]),
            _ => false,
        }
    }
    walk(path, &pat)
}

/// One segment against one pattern segment.
fn segment_match(seg: &str, pat: &str) -> bool {
    let (s, p): (Vec<char>, Vec<char>) = (seg.chars().collect(), pat.chars().collect());
    fn walk(s: &[char], p: &[char]) -> bool {
        match (s.first(), p.first()) {
            (None, None) => true,
            (_, Some('*')) => walk(s, &p[1..]) || (!s.is_empty() && walk(&s[1..], p)),
            (Some(_), Some('?')) => walk(&s[1..], &p[1..]),
            (Some(a), Some(b)) if a == b => walk(&s[1..], &p[1..]),
            _ => false,
        }
    }
    walk(&s, &p)
}

/// The matcher itself, against the case that got through and the lines that were already there.
///
/// A test whose own helper is wrong passes for the wrong reason, and this helper is a
/// reimplementation of somebody else's semantics — so it is pinned against real lines from the
/// repository's own ignore files rather than invented ones.
#[test]
fn the_ignore_matcher_reads_the_rules_the_way_docker_does() {
    let pitch = vec!["pitch/*".to_string(), "!pitch/deck.html".to_string()];
    assert!(excluded_by("pitch/logo/favicon-world.svg", &pitch),
            "a pattern that matches a parent directory takes everything under it — this is the \
             deploy that failed on 16 ก.ย.");
    assert!(excluded_by("pitch/video/week3.mp4", &pitch));
    assert!(!excluded_by("pitch/deck.html", &pitch), "and a later ! line undoes it");
    assert!(!excluded_by("crates/vitals-web/static/world/index.html", &pitch),
            "nothing outside pitch/ is touched by a pitch/ rule");

    let starred = vec!["**/*.mp4".to_string(), "target".to_string()];
    assert!(excluded_by("pitch/video/a.mp4", &starred), "** spans directories");
    assert!(excluded_by("target/debug/vitals-web", &starred), "a bare directory takes its contents");
    assert!(!excluded_by("crates/vitals-web/src/main.rs", &starred));

    // Order is meaning: the same two lines the other way round exclude the deck as well.
    let reversed = vec!["!pitch/deck.html".to_string(), "pitch/*".to_string()];
    assert!(excluded_by("pitch/deck.html", &reversed), "the last matching pattern wins");
}
