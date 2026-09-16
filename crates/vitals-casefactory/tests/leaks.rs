//! The scans that refuse a pack: a season marker or the patient's name anywhere in it.

use serde_json::json;
use vitals_casefactory::validate::scan;

#[test]
fn season_markers_are_caught_wherever_they_sit() {
    for bad in ["osce-a2", "the EP3 still", "Somsri said", "somchai", "station 4", "/img/x.webp", "/clip/y.mp4"] {
        let pack = json!({ "sce": { "interventions": [ { "effects": [ { "beat": bad } ] } ] } });
        assert!(!scan(&pack, "Nobody").is_empty(), "{bad:?} should be caught");
    }
}

#[test]
fn an_episode_marker_needs_its_own_word() {
    // STEP1 and DEEP5 are not episodes; EP1 is.
    assert!(scan(&json!({ "a": "STEP1 of the guide" }), "Nobody").is_empty());
    assert!(scan(&json!({ "a": "DEEP5 breathing" }), "Nobody").is_empty());
    assert!(!scan(&json!({ "a": "see EP1 for the arc" }), "Nobody").is_empty());
}

#[test]
fn the_patients_name_is_caught_as_whole_words_and_particles_are_ignored() {
    let pack = json!({ "voice": { "ask_x": { "words": "My name is Amara, I am fine." } } });
    let leaks = scan(&pack, "Amara da Costa");
    assert_eq!(leaks.len(), 1, "{leaks:?}");
    assert!(leaks[0].contains("amara"));
    // 'da' is two letters and never a name token; 'costa' appears nowhere
    let clean = json!({ "voice": { "ask_x": { "words": "Amaranth is a grain; the coast is far." } } });
    assert!(scan(&clean, "Amara da Costa").is_empty());
}

#[test]
fn a_thai_name_is_caught_as_a_substring() {
    let pack = json!({ "title": "ผู้ป่วยชื่อ สมหญิง มาด้วยไข้" });
    assert!(!scan(&pack, "สมหญิง รักดี").is_empty());
}

#[test]
fn the_opening_is_scrubbed_of_the_name_but_the_voice_is_not() {
    use vitals_casefactory::text::scrub;
    assert_eq!(scrub("Amara came in with her brother. AMARA is 34.", "Amara da Costa"), "the patient came in with her brother. the patient is 34.");
    assert_eq!(scrub("Amaranth is a grain.", "Amara da Costa"), "Amaranth is a grain.");
    assert_eq!(scrub("ผู้ป่วยชื่อสมหญิงมาด้วยไข้", "สมหญิง รักดี"), "ผู้ป่วยชื่อผู้ป่วยมาด้วยไข้");
}
