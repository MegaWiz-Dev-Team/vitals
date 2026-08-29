//! A blood pressure has two numbers, and the gap between them is a vital sign of its own.
//!
//! Systolic and diastolic are separate variables here, and a case only has to declare a `sbp`
//! dynamic for the pair to come apart: the systolic marches down and the diastolic stands exactly
//! where `vitals0` left it. `osce-a` did that for the whole station — `88/60`, `80/60`, `76/60`,
//! `62/60`, and finally `58/58`, which is not a low blood pressure but a number no patient has
//! ever had. The clamp that produced `58/58` was the last guard rail, not the bug.
//!
//! So this walks every case in the repo — the twelve stations, the four season scenarios and the
//! conformance copy of EP1 — from `vitals0` to its end state, untreated and then treated, and
//! checks the pulse pressure the whole way down. Run with `--nocapture` for the traces.

use std::path::{Path, PathBuf};
use vitals_sce::{Sce, SceState};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every scenario the repo ships, by the path a reader can go and open.
fn every_case() -> Vec<(String, Sce, String)> {
    let root = repo_root();
    let mut out = Vec::new();
    let mut push = |p: &Path| {
        let json = std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        let sce = Sce::from_json(&json).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
        let name = p.file_stem().unwrap().to_string_lossy().replace(".sce", "");
        out.push((name, sce, json));
    };
    let mut dir = |d: &str| {
        let mut fs: Vec<_> = std::fs::read_dir(root.join(d))
            .unwrap_or_else(|e| panic!("{d}: {e}"))
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        fs.sort();
        for f in fs {
            push(&f);
        }
    };
    dir("demo/stations");
    dir("demo/scenarios");
    push(&root.join("conformance/sce-anaphylaxis-ep1.json"));
    out
}

/// One run: tick a second at a time to the end state, watching the gap.
///
/// Returns `(samples, ended)` where a sample is `(t, sbp, dbp)` rounded the way a screen prints
/// them — because the screen is where `58/58` was read, and a check on the unrounded pair would
/// have let `58.4/57.6` through.
fn walk(st: &mut SceState, limit_sec: u32) -> (Vec<(u32, f64, f64)>, bool) {
    let mut samples = Vec::new();
    for t in 1..=limit_sec {
        st.tick(1.0);
        samples.push((t, st.vitals.sbp.round(), st.vitals.dbp.round()));
        if st.outcome().is_some() {
            return (samples, true);
        }
    }
    (samples, false)
}

/// The gap has to survive the whole run, not just the calm part.
///
/// Bounds, and why these: a pulse pressure of zero is arithmetically impossible in anything with
/// a heartbeat, so that is the floor the check refuses outright. The upper bound is loose on
/// purpose — a wide pulse pressure is a real finding — and exists only to catch a diastolic that
/// has come unstuck from its systolic in the other direction.
fn check(case: &str, samples: &[(u32, f64, f64)]) -> Vec<String> {
    let mut bad = Vec::new();
    for &(t, sbp, dbp) in samples {
        // A dead patient is zeroed deliberately (`terminate`) and reports no cuff reading at all.
        if sbp <= 0.0 {
            continue;
        }
        let pp = sbp - dbp;
        if pp <= 0.0 {
            bad.push(format!("{case} t={t}s {sbp:.0}/{dbp:.0} pp={pp:.0} — no pulse pressure"));
        } else if pp < 10.0 {
            bad.push(format!("{case} t={t}s {sbp:.0}/{dbp:.0} pp={pp:.0} — pulse pressure below 10"));
        } else if pp > 100.0 {
            bad.push(format!("{case} t={t}s {sbp:.0}/{dbp:.0} pp={pp:.0} — pulse pressure above 100"));
        }
    }
    bad
}

#[test]
fn every_case_keeps_a_pulse_pressure_from_start_to_end_state() {
    let mut failures: Vec<String> = Vec::new();

    for (name, sce, _) in every_case() {
        // ── untreated: the trace that produced 58/58 ──────────────────────────
        let mut st = SceState::new(sce.clone());
        let v0 = sce.vitals0;
        println!(
            "\n── {name} ── vitals0 {:.0}/{:.0} (pp {:.0})",
            v0.sbp,
            v0.dbp,
            v0.sbp - v0.dbp
        );
        let (samples, ended) = walk(&mut st, 1800);
        for &(t, sbp, dbp) in samples.iter().filter(|(t, _, _)| t % 60 == 0) {
            println!("   untreated t={t:>4}s  bp={sbp:.0}/{dbp:.0}  pp={:.0}", sbp - dbp);
        }
        if let Some(&(t, sbp, dbp)) = samples.last() {
            println!(
                "   untreated  end t={t}s  bp={sbp:.0}/{dbp:.0}  pp={:.0}  outcome={:?}",
                sbp - dbp,
                st.outcome_id()
            );
        }
        assert!(ended || samples.len() == 1800, "{name}: walk stopped early");
        failures.extend(check(&format!("{name}/untreated"), &samples));

        // ── treated: every intervention the case declares, so the recovering /
        //    improving branches (where the systolic climbs again) are walked too.
        let mut st = SceState::new(sce.clone());
        st.tick(10.0);
        let ids: Vec<String> = sce.interventions.iter().map(|i| i.id.clone()).collect();
        for id in &ids {
            st.apply_id(id);
        }
        let (samples, _) = walk(&mut st, 1800);
        if let Some(&(t, sbp, dbp)) = samples.last() {
            println!(
                "   treated    end t={t}s  bp={sbp:.0}/{dbp:.0}  pp={:.0}  outcome={:?}",
                sbp - dbp,
                st.outcome_id()
            );
        }
        failures.extend(check(&format!("{name}/treated"), &samples));
    }

    if !failures.is_empty() {
        let n = failures.len();
        // First and last of each case's run, not all of them: a stuck diastolic fails on every
        // tick and the list would be thousands of lines of the same fact.
        let mut shown: Vec<&String> = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        for f in &failures {
            let case = f.split(' ').next().unwrap();
            if !seen.contains(&case) {
                seen.push(case);
                shown.push(f);
            }
        }
        panic!("{n} readings with an impossible pulse pressure; first per case:\n{}",
            shown.iter().map(|s| format!("  {s}")).collect::<Vec<_>>().join("\n"));
    }
}

/// The reported trace, minute by minute, on the case it was reported from.
///
/// `osce-a` is a 71-year-old man in anaphylaxis and it declares dynamics for `sbp`, `spo2` and
/// `hr` — nothing for `dbp`. Untreated, it used to print `88/60`, `84/60`, `80/60`, `76/60`,
/// `62/60` and `58/58`. The systolic here is unchanged; only the number underneath it moves.
#[test]
fn osce_a_no_longer_walks_its_diastolic_down_to_meet_the_systolic() {
    let root = repo_root();
    let sce = Sce::from_json(&std::fs::read_to_string(root.join("demo/stations/osce-a.sce.json")).unwrap()).unwrap();
    let mut st = SceState::new(sce);
    let (samples, _) = walk(&mut st, 360);

    let at = |t: u32| samples.iter().find(|s| s.0 == t).copied().unwrap();
    // The systolic is the case's own, and it must not have moved a millimetre.
    for (t, sbp) in [(60, 88.0), (120, 84.0), (180, 80.0), (240, 76.0), (300, 62.0), (360, 58.0)] {
        assert_eq!(at(t).1, sbp, "t={t}s: the systolic changed — this fix is not allowed to touch it");
    }
    // …and the diastolic is no longer standing at 60 waiting to be caught by the clamp.
    for t in [60, 120, 180, 240, 300, 360] {
        let (_, sbp, dbp) = at(t);
        assert!(dbp < 60.0, "t={t}s: {sbp:.0}/{dbp:.0} — the diastolic is still parked at vitals0");
        assert!(sbp - dbp >= 20.0, "t={t}s: {sbp:.0}/{dbp:.0} — pulse pressure {:.0}", sbp - dbp);
    }
    // The reading that started this. 92/60 is a ratio of 0.652, so a systolic of 58 reads 58/38.
    let (_, sbp, dbp) = at(360);
    assert_eq!((sbp, dbp), (58.0, 38.0), "the 58/58 reading is back");
}

/// A case that drives its own diastolic keeps it, and the engine does not second-guess the number.
///
/// This is the whole reason the derivation is conditional. Distributive shock drops the diastolic
/// disproportionately, cardiogenic shock narrows the pulse pressure early, and which of those a
/// patient is in is a clinical decision that belongs in the scenario file — so a file that makes
/// it must win.
#[test]
fn a_case_that_declares_its_own_diastolic_keeps_control_of_it() {
    let sce = Sce::from_json(&authored("dbp")).unwrap();
    let mut st = SceState::new(sce);
    st.tick(30.0);
    // sbp -60/min and dbp -30/min for half a minute, from 120/80.
    assert_eq!(st.vitals.sbp.round(), 90.0);
    assert_eq!(st.vitals.dbp.round(), 65.0, "the engine overrode an authored dbp dynamic");
    // The ratio the engine would have imposed is 80/120, so a systolic of 90 would read 90/60.
    assert_ne!(st.vitals.dbp.round(), 60.0);
}

/// …and the same for a case that writes `dbp` discretely rather than continuously. An
/// intervention's `set` is an authored number too, and an engine that overwrote it on the next
/// tick would be discarding it silently — the worst way to lose an author's work.
#[test]
fn an_authored_set_on_the_diastolic_counts_as_driving_it() {
    let sce = Sce::from_json(&authored("set")).unwrap();
    let mut st = SceState::new(sce);
    st.tick(30.0);
    assert_eq!(st.vitals.sbp.round(), 90.0);
    assert_eq!(st.vitals.dbp.round(), 80.0, "an authored `set` did not take the diastolic back");
}

/// The control case: the *same* file with the `dbp` line removed is derived, so the two tests
/// above are measuring the declaration and not something else about the scenario.
#[test]
fn the_same_case_without_the_declaration_is_derived() {
    let sce = Sce::from_json(&authored("none")).unwrap();
    let mut st = SceState::new(sce);
    st.tick(30.0);
    assert_eq!(st.vitals.sbp.round(), 90.0);
    assert_eq!(st.vitals.dbp.round(), 60.0, "120/80 is a ratio of 2/3, so 90 should read 90/60");
}

/// One scenario, three ways of saying what the diastolic does: a dynamic, a discrete `set` buried
/// in the `else` arm of a branch on a transition, or nothing at all.
fn authored(mode: &str) -> String {
    let dbp_dynamic = if mode == "dbp" { r#", { "var": "dbp", "rate_per_min": -30 }"# } else { "" };
    let dbp_set = if mode == "set" {
        r#""do": [ { "branch": [ { "if": { "flag": "never" }, "then": [] } ], "else": [ { "set": { "dbp": 80 } } ] } ],"#
    } else {
        ""
    };
    format!(
        r#"{{
          "vitals0": {{ "hr": 80, "sbp": 120, "dbp": 80, "spo2": 98, "rr": 14, "temp": 37.0, "gcs": 15 }},
          "initial_state": "fall",
          "states": [
            {{ "id": "fall", "status": "deteriorating",
              "dynamics": [ {{ "var": "sbp", "rate_per_min": -60 }}{dbp_dynamic} ],
              "transitions": [ {{ "to_state": "fall", {dbp_set} "when": {{ "var": "t_elapsed", "op": "ge", "value": 30 }} }} ] }}
          ],
          "outcomes": [ {{ "id": "win_icu", "kind": "win" }} ]
        }}"#
    )
}
