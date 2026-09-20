//! The review flag: a pack is `provisional` until the case's own `world.review` block in
//! `case.meta.yaml` says the advisor has read it — `provisional = !(status == "reviewed")` —
//! and then the sheet's note names the reviewer and the date instead of "Not clinically
//! reviewed". A rejected case is not compiled at all. The block is read exactly as the
//! library's `tools/world-meta.py` writes it (status · by role · date), never a name.

mod common;

use vitals_casefactory::source::{parse_world_review, Library, Review};
use vitals_casefactory::{compile, compile_with, Source};

const SYNTHETIC: &str = include_str!("fixtures/synthetic-septic-shock.json");

fn src() -> Source {
    Source::of("embla-cases", "test", SYNTHETIC)
}

fn reviewed() -> Review {
    Review { status: "reviewed".into(), by: Some("clinical-advisor".into()), date: Some("2026-09-20".into()) }
}

#[test]
fn a_case_without_a_review_block_compiles_provisional_and_says_so() {
    let pack = compile(SYNTHETIC, src()).unwrap();
    assert!(pack.provisional);
    assert!(pack.review.is_none());
    let status = pack.rubric["status"].as_str().unwrap();
    assert!(status.contains("Not clinically reviewed"), "{status}");
    assert!(status.starts_with("provisional"), "{status}");
    assert!(pack.sce["_note"].as_str().unwrap().to_lowercase().contains("not clinically reviewed"));
}

#[test]
fn a_reviewed_case_clears_the_flag_and_the_note_names_the_reviewer_and_the_date() {
    let pack = compile_with(SYNTHETIC, src(), Some(reviewed())).unwrap();
    assert!(!pack.provisional);
    assert_eq!(pack.review, Some(reviewed()));
    let status = pack.rubric["status"].as_str().unwrap();
    assert!(status.contains("reviewed by the clinical advisor on 2026-09-20"), "{status}");
    assert!(!status.contains("Not clinically reviewed"), "{status}");
    assert!(!status.starts_with("provisional"), "{status}");
    let note = pack.sce["_note"].as_str().unwrap();
    assert!(!note.to_lowercase().contains("not clinically reviewed"), "{note}");
    assert!(note.contains("reviewed by the clinical advisor on 2026-09-20"), "{note}");
    // the pack still serialises, and the flag is what the ward's door reads
    let v = serde_json::to_value(&pack).unwrap();
    assert_eq!(v["provisional"], false);
    assert_eq!(v["review"]["status"], "reviewed");
    assert_eq!(v["review"]["by"], "clinical-advisor");
    assert_eq!(v["review"]["date"], "2026-09-20");
}

#[test]
fn a_provisional_review_keeps_the_flag_and_a_rejected_one_is_not_compiled() {
    let pack = compile_with(SYNTHETIC, src(), Some(Review { status: "provisional".into(), by: None, date: None })).unwrap();
    assert!(pack.provisional);
    assert!(pack.rubric["status"].as_str().unwrap().contains("Not clinically reviewed"));
    let err = compile_with(SYNTHETIC, src(), Some(Review { status: "rejected".into(), by: Some("clinical-advisor".into()), date: Some("2026-09-19".into()) })).unwrap_err();
    assert!(err.reason.contains("rejected by the clinical advisor on 2026-09-19"), "{}", err.reason);
}

#[test]
fn the_review_block_is_read_as_the_library_writes_it() {
    let yaml = "id: x\nversion: 0.1.0\ncontent_hash: \"sha256:0\"\nlifecycle: provisional\nprovenance:\n  source: \"embla-gen\"\n  review: \"pending clinical review by the advisor — not released\"\n  references:\n    - \"WHO. Something; 2023\"\nworld:\n  world_ready: true\n  country: KEN\n  endemic: true\n  review:\n    status: reviewed\n    by: clinical-advisor\n    date: 2026-09-20\n  compiled:\n    compiler_version: 0.9.4\n    pack_sha256: \"abc\"\n    date: 2026-09-17\ndeployments: []\nowners: []\n";
    assert_eq!(parse_world_review(yaml), Some(reviewed()));
    // the provenance's own `review:` line is prose, not the ruling — only the world block counts
    let provisional = yaml.replace("    status: reviewed\n    by: clinical-advisor\n    date: 2026-09-20\n", "    status: provisional\n    by: null\n    date: null\n");
    assert_eq!(parse_world_review(&provisional), Some(Review { status: "provisional".into(), by: None, date: None }));
    let no_block = "id: x\nversion: 0.1.0\nprovenance:\n  review: \"pending\"\ndeployments: []\n";
    assert_eq!(parse_world_review(no_block), None);
    let null_review = "id: x\nworld:\n  world_ready: false\n  country: null\n  endemic: false\n  review: null\n  compiled: null\n";
    assert_eq!(parse_world_review(null_review), None);
}

#[test]
fn the_library_reads_the_review_beside_the_case() {
    let root = std::env::temp_dir().join(format!("vitals-casefactory-review-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let a = root.join("cases").join("synthetic-septic-shock-test");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::write(a.join("case.json"), SYNTHETIC).unwrap();
    std::fs::write(a.join("case.meta.yaml"), "id: synthetic-septic-shock-test\nversion: 0.0.1\nworld:\n  world_ready: true\n  country: ZZZ\n  endemic: false\n  review:\n    status: reviewed\n    by: clinical-advisor\n    date: 2026-09-20\n  compiled: null\ndeployments: []\n").unwrap();
    let lib = Library::open(root.to_str().unwrap()).unwrap();
    assert_eq!(lib.review("synthetic-septic-shock-test"), Some(reviewed()));
    assert_eq!(lib.review("no-such-case"), None);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_eighteen_endemic_cases_are_still_provisional_today() {
    // the founder is confirming the review's provenance; tools/mark-reviewed.py is prepared and not applied
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../embla-cases-world");
    if !root.join("cases").is_dir() { return common::skip("embla-cases-world not present"); }
    let lib = Library::open(root.to_str().unwrap()).unwrap();
    for id in common::ENDEMIC_SIX {
        let review = lib.review(id).unwrap_or_else(|| panic!("{id}: no world.review block"));
        assert_eq!(review.status, "provisional", "{id}");
        let json = lib.read(id).unwrap();
        let pack = compile_with(&json, Source::of("embla-cases", "worktree", &json), Some(review)).unwrap_or_else(|e| panic!("{}", e.reason));
        assert!(pack.provisional, "{id}");
    }
}
