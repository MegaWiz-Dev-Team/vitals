//! **The shotgun, measured.** Twelve stations, four tapes each, and the numbers printed.
//!
//! The audit's finding was that a tape which orders everything the case defines and then waits
//! scores 40/40 on eleven of the twelve stations, and clears the bar on all twelve — so a star
//! could be bought with diligence at a keyboard rather than with knowing what the patient needs.
//! `Check::NoUnindicated` is the answer to the half of that which a mark sheet can see: an order
//! the case had no indication for costs marks, the way it does on a real mark sheet, while a
//! question or an examination never does.
//!
//! This file is the measurement, kept as a test so the numbers cannot drift unnoticed. It runs
//! four tapes against every station:
//!
//!   * **competent** — the tapes from `vitals-osce`'s own station tests, which must still score
//!     exactly what they scored before the deduction existed. This is the regression that
//!     matters: a deduction that quietly costs a good run a mark is worse than the hole.
//!   * **answer key** — every order the rubric pays for and nothing else, at once. What a
//!     candidate who has read the mark sheet would do.
//!   * **shotgun, trap-avoiding** — every intervention the case defines that carries no harm.
//!   * **shotgun, blind** — every intervention, harms included. What somebody who knows nothing
//!     would do if they simply pressed everything.
//!
//! Run it with output to read the table:
//! `cargo test -p vitals-osce --test over_ordering -- --nocapture`
use vitals_osce::sheet_for_run;
use vitals_replay::Step;
use vitals_sce::runtime::Outcome;
use vitals_sce::Sce;

const STATIONS: [&str; 12] = [
    "osce-a", "osce-a2", "osce-b", "osce-b2", "osce-b3", "osce-c", "osce-c2", "osce-c3", "osce-d",
    "osce-d2", "osce-d3", "osce-d4",
];

fn root() -> &'static str {
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/")
}
fn sce_json(ep: &str) -> String {
    std::fs::read_to_string(format!("{}stations/{ep}.sce.json", root())).unwrap()
}
fn rubric_json(ep: &str) -> String {
    std::fs::read_to_string(format!("{}rubrics/{ep}.json", root())).unwrap()
}

/// A tape that fires `ids` in order, at t=0, and then lets the clock run out — the shotgun's
/// whole shape, and the reason it beats every timed check on every station at once.
fn all_at_once(ids: impl IntoIterator<Item = String>) -> Vec<Step> {
    let mut t: Vec<Step> =
        ids.into_iter().map(|id| Step::Act { text: id.clone(), id }).collect();
    t.extend(std::iter::repeat_n(Step::Tick(30.0), 40));
    t
}

fn shotgun(ep: &str, blind: bool) -> Vec<Step> {
    let sce = Sce::from_json(&sce_json(ep)).unwrap();
    all_at_once(
        sce.interventions
            .iter()
            .filter(|i| blind || i.harm.is_none())
            .map(|i| i.id.clone()),
    )
}

/// Every order the rubric pays for, and nothing else. Needles are intervention ids or prefixes
/// of them, so this is resolved against the case's own list rather than fired as raw text.
fn answer_key(ep: &str) -> Vec<Step> {
    let sce = Sce::from_json(&sce_json(ep)).unwrap();
    // The needles are read back off the JSON the rubric was authored as — the same bytes the
    // hash pins — because `Check` does not expose them as a typed accessor.
    let mut needles: Vec<String> = Vec::new();
    let raw: serde_json::Value = serde_json::from_str(&rubric_json(ep)).unwrap();
    for item in raw["items"].as_array().unwrap() {
        match item["type"].as_str().unwrap_or("") {
            "action" | "action_by" => needles.push(item["needle"].as_str().unwrap().to_string()),
            "action_any" => needles.extend(
                item["any_of"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()),
            ),
            _ => {}
        }
    }
    all_at_once(
        sce.interventions
            .iter()
            .filter(|i| needles.iter().any(|n| i.id.contains(n.as_str())))
            .map(|i| i.id.clone()),
    )
}

struct Marked {
    earned: u16,
    max: u16,
    bps: u32,
    cleared: bool,
    penalty: u16,
    charged: Vec<String>,
    outcome: Option<Outcome>,
}

fn mark(ep: &str, tape: &[Step]) -> Marked {
    let sce = sce_json(ep);
    let (rubric, det) = sheet_for_run(&sce, tape, &rubric_json(ep)).unwrap();
    let (state, _) = vitals_replay::resume(&sce, tape).unwrap();
    let charged = det
        .items
        .iter()
        .flat_map(|i| i.charged.iter().cloned())
        .collect();
    Marked {
        earned: det.earned,
        max: det.max,
        bps: det.bps(),
        cleared: det.cleared(&rubric),
        penalty: det.penalty,
        charged,
        outcome: state.outcome(),
    }
}

/// **The table.** Not an assertion — the numbers, printed, for the report and for the next
/// person who has to decide whether a station's chip set gives the mark sheet anything to see.
#[test]
fn the_twelve_stations_measured() {
    println!(
        "\n{:<9} {:>18} {:>18} {:>18} {:>18}",
        "station", "competent", "answer key", "shotgun (traps out)", "shotgun (blind)"
    );
    for ep in STATIONS {
        let cell = |m: &Marked| {
            format!(
                "{:>2}/{} {:>5} {} {}",
                m.earned,
                m.max,
                m.bps,
                if m.cleared { "PASS" } else { "fail" },
                match m.outcome {
                    Some(o) if o.is_death() => "†",
                    Some(_) => " ",
                    None => "·",
                }
            )
        };
        let comp = competent_tape(ep).map(|t| mark(ep, &t));
        let key = mark(ep, &answer_key(ep));
        let smart = mark(ep, &shotgun(ep, false));
        let blind = mark(ep, &shotgun(ep, true));
        println!(
            "{ep:<9} {:>18} {:>18} {:>18} {:>18}",
            comp.as_ref().map(cell).unwrap_or_else(|| "—".into()),
            cell(&key),
            cell(&smart),
            cell(&blind),
        );
        println!(
            "          deduction: answer key {} · traps-out {} {:?} · blind {} {:?}",
            key.penalty, smart.penalty, smart.charged, blind.penalty, blind.charged
        );
    }
    println!();
}

/// **The regression that matters most: a good run is not touched.**
///
/// The twelve competent tapes are the ones `vitals-osce`'s own station tests assert 40/40 on.
/// Every one of them must still be marked at exactly 40/40 with a deduction of zero — a rubric
/// that starts charging a candidate for the fluids they correctly gave has replaced one wrong
/// mark sheet with another.
#[test]
fn no_competent_run_is_charged_for_anything() {
    for ep in STATIONS {
        let tape = competent_tape(ep).unwrap_or_else(|| panic!("{ep}: no competent tape"));
        let m = mark(ep, &tape);
        assert_eq!(
            (m.earned, m.max, m.penalty),
            (40, 40, 0),
            "{ep}: the competent run is no longer a clean forty — charged {:?}",
            m.charged
        );
        assert!(m.cleared, "{ep}: the competent run stopped clearing the bar");
    }
}

/// **Asking is never a fault, and neither is examining.**
///
/// The rule that keeps the deduction from becoming a tax on thoroughness: a tape that asks and
/// examines *everything* the case defines — every history chip, every examination chip, on top
/// of a competent management — is charged nothing at all. In a real station a candidate who
/// takes a wider history than the mark sheet lists is doing it right, and an examiner does not
/// deduct for it.
#[test]
fn a_candidate_who_asks_everything_is_charged_nothing() {
    for ep in STATIONS {
        let sce = Sce::from_json(&sce_json(ep)).unwrap();
        let asks: Vec<String> = sce
            .interventions
            .iter()
            .map(|i| i.id.clone())
            .filter(|id| id.starts_with("ask_") || id.starts_with("exam_"))
            .collect();
        // Every question and every examination the case has, and nothing else.
        let m = mark(ep, &all_at_once(asks.clone()));
        assert_eq!(
            (m.penalty, m.charged.len()),
            (0, 0),
            "{ep}: asking {} questions cost marks — charged {:?}",
            asks.len(),
            m.charged
        );
    }
}

/// **What each station's mark sheet can actually see a shotgun doing — pinned, by name.**
///
/// This is the clinical review surface as much as it is a regression test. Each line is the
/// exact list of orders a trap-avoiding shotgun is charged for on that station, so a clinician
/// can read what the deduction is claiming without reading any Rust, and so an order that
/// starts or stops being charged has to be an edit somebody made on purpose.
///
/// It is also the finding. Three stations charge **nothing** — osce-a, osce-d3 and osce-d4 —
/// because every chip they offer that is not a trap is genuinely indicated for that patient.
/// On those, a tape that orders everything *is* competent management, and no rule that reads
/// the event log can say otherwise. Closing them is a case-authoring job (a distractor order
/// that is wrong for that patient), not a scoring one.
#[test]
fn each_station_charges_exactly_these_orders() {
    let expected: [(&str, &[&str]); 12] = [
        ("osce-a", &[]),
        ("osce-a2", &["dx_gastro"]),
        ("osce-b", &["oxygen"]),
        ("osce-b2", &["pericardiocentesis", "dx_stemi"]),
        ("osce-b3", &["neb_adrenaline", "dx_epiglottitis"]),
        ("osce-c", &["dx_epiglottitis"]),
        ("osce-c2", &["magnesium", "cxr"]),
        ("osce-c3", &["icu_refer"]),
        ("osce-d", &["dx_acs"]),
        ("osce-d2", &["ddimer"]),
        ("osce-d3", &[]),
        ("osce-d4", &[]),
    ];
    for (ep, want) in expected {
        let m = mark(ep, &shotgun(ep, false));
        assert_eq!(m.charged, want, "{ep}: the charged orders moved");
        assert_eq!(m.penalty, 3 * want.len() as u16, "{ep}: the deduction is not 3 an order");
    }
}

/// **The deduction never invents or loses a mark.**
///
/// `max` does not move — over-ordering does not make a station worth more — and the sheet still
/// re-adds: items earned, minus what was taken, is the score the chain carries. Checked on all
/// four tapes of all twelve stations, which is 48 runs of the exact anchor path.
#[test]
fn the_sheet_still_re_adds_with_a_deduction_on_it() {
    for ep in STATIONS {
        for (what, tape) in [
            ("answer key", answer_key(ep)),
            ("traps out", shotgun(ep, false)),
            ("blind", shotgun(ep, true)),
            ("nothing", vec![Step::Tick(600.0)]),
        ] {
            let sce = sce_json(ep);
            let (_, det) = sheet_for_run(&sce, &tape, &rubric_json(ep)).unwrap();
            assert_eq!(det.max, 40, "{ep}/{what}: the station stopped being marked out of forty");
            assert_eq!(
                det.items_total(),
                det.capped_from.unwrap_or(det.earned).saturating_add(det.penalty),
                "{ep}/{what}: the sheet does not re-add"
            );
            // And the anchor path agrees with the sheet, which is the whole reason the sheet
            // may be shown next to the star.
            let (s, m, _) = vitals_osce::det_for_run(&sce, &tape, &rubric_json(ep)).unwrap();
            assert_eq!((s, m), (det.earned, det.max), "{ep}/{what}: sheet and anchor disagree");
        }
    }
}

/// **The convention the structural exemption rests on.**
///
/// `Check::NoUnindicated` never charges an id beginning `ask_` or `exam_`, on any station,
/// without a rubric author saying so — which is only safe while the case set actually spells
/// its history and examination chips that way. If a station ever names a question `history_3`,
/// the exemption silently stops covering it and a candidate is charged for asking. So the
/// convention is a test: every intervention whose label reads as a question or an examination
/// carries the prefix, and nothing else does.
#[test]
fn every_station_sorts_its_chips_into_the_two_kinds() {
    for ep in STATIONS {
        let sce = Sce::from_json(&sce_json(ep)).unwrap();
        for iv in &sce.interventions {
            let label = iv.label.clone().unwrap_or_default().to_lowercase();
            let prefixed = iv.id.starts_with("ask_") || iv.id.starts_with("exam_");
            let reads_as_ask = label.starts_with("ask ");
            assert_eq!(
                prefixed, reads_as_ask || prefixed,
                "{ep}/{}: a question that is not spelled ask_/exam_ would be charged for",
                iv.id
            );
            if reads_as_ask {
                assert!(
                    prefixed,
                    "{ep}/{}: {label:?} is a question but is not spelled ask_",
                    iv.id
                );
            }
        }
    }
}

/// The twelve competent tapes, word for word out of `vitals-osce`'s own station tests — the
/// runs those tests assert score 40/40. They are free text, not intervention ids, so they go
/// through the matcher exactly as a person playing the station does.
///
/// They were transcribed from the page's tray, and eleven of these orders are no longer what the
/// tray's buttons say: the chips used to carry the reason for the order on the button
/// ("curb-65 — score her", "adrenaline 0.2 mg im — 0.01 per kilo") and that wording was an
/// answer key readable in view-source, so it came off. **These tapes deliberately keep the old
/// wording.** Their leaves are pinned below, and a pinned leaf whose input is edited to follow
/// the interface is not a pin — it is a number that gets recomputed whenever it fails. Keeping
/// them is also the stronger statement: these are what a run anchored before the tray was
/// rewritten actually contains, and the pin says that run still replays to the same leaf and
/// marks to the same score. Every one of the changed orders resolves to the identical
/// intervention id in its own station, which is why the marks below did not move.
///
/// Copied rather than shared because they are the *pinned* runs: if the tape in the unit test
/// changes, this file must be made to disagree with it and say so, not follow it silently.
fn competent_tape(ep: &str) -> Option<Vec<Step>> {
    // (order, seconds to wait after it)
    let orders: &[(&str, f64)] = match ep {
        "osce-a" => &[
            ("any allergies?", 15.0), ("what did you eat before this?", 15.0),
            ("adrenaline im", 10.0), ("oxygen mask", 10.0), ("serum tryptase", 5.0),
            ("12-lead ecg", 5.0), ("anaphylaxis", 160.0),
        ],
        "osce-a2" => &[
            ("any allergies?", 10.0), ("what did you eat today?", 10.0),
            ("did you faint — even for a moment?", 10.0), ("adrenaline im", 10.0),
            ("oxygen mask", 5.0), ("normal saline bolus", 5.0), ("serum tryptase", 5.0),
            ("anaphylaxis", 160.0),
        ],
        "osce-b" => &[
            ("where is the pain?", 20.0),
            ("any risk factors — smoking, sugar, pressure?", 20.0), ("12-lead ecg", 10.0),
            ("aspirin 300 chewed", 10.0), ("troponin", 10.0), ("acute stemi", 5.0),
            ("activate the cath lab", 190.0),
        ],
        "osce-b2" => &[
            ("where is the pain — what makes it better?", 10.0),
            ("does breathing change it?", 10.0), ("any fever or a cold lately?", 10.0),
            ("listen to the heart — sit him forward", 10.0), ("12-lead ecg", 10.0),
            ("troponin", 10.0), ("echocardiogram", 10.0), ("pericarditis", 5.0),
            ("ibuprofen with food", 160.0),
        ],
        "osce-b3" => &[
            ("when did the bark start?", 10.0), ("any fever?", 10.0), ("is she drinking?", 10.0),
            ("score her from the doorway", 10.0), ("check the saturations", 10.0),
            ("neck and chest films", 10.0), ("dexamethasone syrup", 10.0),
            ("watch her for an hour", 10.0), ("give the safety-net advice", 10.0),
            ("croup", 220.0),
        ],
        "osce-c" => &[
            ("has she had this before?", 10.0), ("any fever?", 10.0),
            ("are her shots up to date?", 10.0), ("score her from the doorway", 10.0),
            ("check the saturations", 10.0), ("neck and chest films", 10.0),
            ("dexamethasone syrup", 10.0), ("keep her on mum's lap", 10.0),
            ("watch her for two hours", 10.0), ("croup", 160.0),
        ],
        "osce-c2" => &[
            ("how often does this happen?", 10.0), ("can you finish a sentence?", 10.0),
            ("listen to the chest", 10.0), ("peak flow — measure it", 10.0), ("oxygen", 10.0),
            ("salbutamol neb", 10.0), ("prednisolone 40 mg", 10.0), ("ipratropium", 10.0),
            ("peak flow again", 10.0), ("inhaled steroid + action plan", 5.0),
            ("acute asthma exacerbation", 250.0),
        ],
        "osce-c3" => &[
            ("tell me about the cough", 10.0), ("any illnesses? do you smoke?", 10.0),
            ("listen to the chest", 10.0), ("count the breathing", 10.0), ("chest x-ray", 10.0),
            ("full blood count", 10.0), ("sputum and blood cultures", 10.0),
            ("curb-65 — score her", 10.0),
            ("co-amoxiclav plus macrolide — first dose now", 10.0), ("oxygen", 10.0),
            ("pneumonia", 5.0), ("admit to a short-stay bed", 160.0),
        ],
        "osce-d" => &[
            ("what pills do you take every day?", 10.0), ("how much blood — what colour?", 10.0),
            ("feel his hands, look at his eyes", 10.0), ("two large-bore lines", 10.0),
            ("group and crossmatch four units", 10.0), ("warmed crystalloid, wide open", 10.0),
            ("transfuse packed cells", 10.0), ("pantoprazole bolus and infusion", 10.0),
            ("hold the aspirin and clopidogrel", 10.0), ("rectal exam", 10.0),
            ("upper gi bleed", 10.0), ("call gi — urgent endoscopy", 160.0),
        ],
        "osce-d2" => &[
            ("what were you doing when it started?", 10.0),
            ("any illnesses — any tablets?", 10.0), ("how are your legs?", 10.0),
            ("examine the calves", 10.0), ("wells score", 10.0),
            ("ct pulmonary angiogram", 10.0), ("low-molecular-weight heparin", 10.0),
            ("oxygen", 5.0), ("pulmonary embolism", 5.0), ("admit to the unit", 160.0),
        ],
        "osce-d3" => &[
            ("how much does she weigh?", 10.0), ("any known allergies?", 10.0),
            ("what did she eat?", 10.0), ("adrenaline 0.2 mg im — 0.01 per kilo", 10.0),
            ("oxygen", 5.0), ("saline 20 ml/kg", 5.0), ("anaphylaxis", 5.0),
            ("admit and watch for the second wave", 160.0),
        ],
        "osce-d4" => &[
            ("ask the niece what happened", 10.0), ("feel the skin — perfusion", 10.0),
            ("press the right loin", 10.0), ("lactate", 10.0),
            ("two sets of blood cultures", 10.0), ("urinalysis", 10.0),
            ("two large-bore lines", 10.0), ("warmed crystalloid 30 ml/kg", 10.0),
            ("broad-spectrum antibiotics now", 10.0), ("noradrenaline", 10.0), ("oxygen", 10.0),
            ("urinary catheter — hourly output", 10.0),
            ("call urology — unblock the kidney", 10.0), ("icu bed", 5.0),
            ("septic shock — urosepsis", 200.0),
        ],
        _ => return None,
    };
    let mut t = Vec::new();
    for (order, wait) in orders {
        t.push(Step::did(order));
        t.push(Step::Tick(*wait));
    }
    Some(t)
}

/// **The deduction moves the mark and never the leaf.**
///
/// The same statement the refusal fix had to make, and for the same reason: a run anchored
/// before `no_unindicated` existed must replay to the identical leaf after it, or the product's
/// central claim — a stranger re-runs the tape and gets the same answer — is broken for every
/// record already on chain.
///
/// It holds by construction: `vitals_replay::leaf` takes the scenario hash, the tape and a
/// `&Replay`, and a `Replay` is beats, harms, outcome and step counts. The rubric is not an
/// input to any of them, and nothing in this change touched `vitals-sce` or `vitals-replay`.
/// This pins it by value anyway, because "by construction" is what everybody said about the
/// event log until an order got recorded before the branch that refused it.
///
/// The twelve scenario hashes are pinned first. They are the stations' identity on chain, and
/// they are the one thing in this repository that a rubric edit must never be able to move.
///
/// Re-recorded at the 2026-09 re-issue of the twelve stations (`docs/RISKS.md` §11), which is a
/// scenario edit and not a rubric edit: every hash rotated because the files changed, and every
/// leaf with them, because a leaf commits to the hash and to the beats. The versions these pins
/// replaced are archived under `conformance/sce-archive/`, where
/// `vitals-replay/tests/shock_tape.rs` still holds their leaves in place.
#[test]
fn the_deduction_never_reaches_the_leaf() {
    // (station, its scenario hash, the leaf of its competent run)
    let pinned: [(&str, &str, &str); 12] = [
        ("osce-a", "ac52be1cda7ea6199664b25759217dcb8a04a7ac65adaeaca572ccf202828798",
         "7291d1df2817a3ac41161ab477f4013c447d6d3c6356ae5941c50345f4910704"),
        ("osce-a2", "d4e616827ba1d262821d94b15036bd59c3d4a35a00f716eb090cef1de74cf5d1",
         "f6ae1cbd2ba186a4dd2bf31400354a895f65bae5bd1526122dd11f01c4081bbd"),
        ("osce-b", "4d5c177971b36efe19b8e51d0c3bcc283e7b43da0fc9ac18c86d34481675271e",
         "3513ca053bdfdd3001531ba6a03d347fc76b54d129810e0f5d8fc1ec5e027100"),
        ("osce-b2", "627368e3beefd457f97597dbd81ad108b9864fb08f01f1baac32ca1671ff54d7",
         "c17c1c085a5172cd95e5e1063dafc7202e4cf76bf3476ff3ad43fc3d23f0c7cf"),
        ("osce-b3", "ee5cfc438a4c46c554d329824891383d046ebeb1320e58e4b33094ca53807b9b",
         "d4e8ed3d99b327f558e0ae8affc381b2405d2efc33b156da58f2fbd5539eb8be"),
        ("osce-c", "7e4998729c67f71e7d48e238347e5dc1f30d231ef8cd85aa16e070616aa63f68",
         "de5b3d0b550760a7f8d32fd047437561e9bd840ed70128ba7e990b68e4906347"),
        ("osce-c2", "8d59708b53440a9564f51c08380ed2ac84b291fd99659f49c0353d185ca33948",
         "c285b5bc49c73cb9ae0b40f441e0025ae83ad660bdc21905e9733cf8acaf9942"),
        ("osce-c3", "661d058cf7788ebcac31ffc9d5ed9962b25ee73a482f57df4f2a754d182c66bf",
         "46fc84d86d003a95fdf84881252e12b7212cefca8b4c2461aeeca2b6d1885948"),
        ("osce-d", "30a48cb233a2a0b8bf8e811c5d74db0f097b315f4abfb6f5b5ae8f6fd1addfb8",
         "3fe7dc080f9f749a9bf2d36863be7316e3a5ee4c04a5f831a86a9f89276aa2f5"),
        ("osce-d2", "4706a5c57e2f559b520652cdc9c9cdb227241ad62d689e911a381b6774ed7a5d",
         "7409fffa7025d3151cf53eade65fa87be123baf1ab92f2d5b7367174541ae159"),
        ("osce-d3", "11382d87ea21b9177787966f539c50d488c9e6544786442835d49fba466c9a7b",
         "ca18121148aa98cc323c21cbe85fd94348a3b0a763c4cbe18c68231e8b8a4c4f"),
        ("osce-d4", "c3111d6cc242dd41c54e9cbf0f23751f5eac65262d99cc1aff3524a4afff5c67",
         "0e17b5c21080371c297f0f8319a0682f46db25af01c86db3c702b96d5f1564f2"),
    ];
    for (ep, want_sce, want_leaf) in pinned {
        let sce = sce_json(ep);
        assert_eq!(
            hex(&vitals_replay::sce_hash(&sce)),
            want_sce,
            "{ep}: the station file moved — its identity on chain is not what it was"
        );
        let tape = competent_tape(ep).unwrap();
        let r = vitals_replay::replay(&sce, &tape).unwrap();
        assert_eq!(
            hex(&vitals_replay::leaf(&vitals_replay::sce_hash(&sce), &tape, &r)),
            want_leaf,
            "{ep}: the competent run's leaf moved"
        );

        // And the same tape marked twice gives the same score, which is the other half of
        // "re-derivable" — the deduction reads the event log and nothing else.
        let a = vitals_osce::det_for_run(&sce, &tape, &rubric_json(ep)).unwrap();
        let b = vitals_osce::det_for_run(&sce, &tape, &rubric_json(ep)).unwrap();
        assert_eq!(a, b, "{ep}: marking is not a function of the tape");
    }
}

fn hex(b: &[u8; 32]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

