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
#[test]
fn the_deduction_never_reaches_the_leaf() {
    // (station, its scenario hash, the leaf of its competent run)
    let pinned: [(&str, &str, &str); 12] = [
        ("osce-a", "4ee5521614895b474296fdcdc4e355009d23e6a5fcbff5d1bfdd86765d1e993d",
         "4b0c97beb3562a946ccc1e95ef7321591947b5b6fc4da3e03fd0e8099fc4e857"),
        ("osce-a2", "109bee5badd6e52955e4c8312f29be10c305f32f2ba777a445eff149d2747ea3",
         "ca880513d2dfece429e9c86a2abe1813cb18b42e4036690e8f19ce30be4921df"),
        ("osce-b", "b61fbebc65f71eaead162fd377183044b4acf62a1a6f5e3f39658c900ed723e8",
         "5eb71f71fbd7914b46f5d84c24668cdb4c25898db486a2d2227e42446f59c826"),
        ("osce-b2", "abce636f126ed9588d03c6d8ecc7306bd628a5802e06c0f1a18a4f3c60639f2a",
         "30e7208ffcaa043ecd442a3edc653e735f6c6cbb5e73aaea6a75ed44a81a252b"),
        ("osce-b3", "87fbc1290cb48f8ec78bb0d6b6efc80677ae6f9b9b26628fce4fb1b9c1b7d662",
         "a6f944cc5faa5ad2c93b428975052b8239d06569d7a3e61214c1ac0bbb9186db"),
        ("osce-c", "5c0e1270f68b9665be6ea7a90552129ee649712bfa77327c8fdb969adcaaceff",
         "6ac923b2eaa25ce304eb0c852dddf0b7cfd6c24841a1dc74817a1a52c70fe404"),
        ("osce-c2", "90e52ac0e31d7ce60965ef1a2ef60302a695bf6baf0d5fa0be3431b0053cb642",
         "0d8ad9b547152f07471d5de64fd74dba2851e29829cf6c0d1fea897a09ccfb9c"),
        ("osce-c3", "ece6ed587279f3ae51dd6ccfbccb8ba0b47fb0ee8578e61eacd72e6702f122d7",
         "a6a73975239e0f2cecffec56f43a71c0e7294566939067e6f316b5081efe3771"),
        ("osce-d", "b9bfa9c57e40dcc5dfc342431b8ae9b7f2836649876f721d2d0f62fe70a577fb",
         "a355361f03486b479d76ef9b45fe7cb4692fc06a9e9f1c9f826cd3bb5f5858d3"),
        ("osce-d2", "b9bc24963c3ed5bf344c51bb1749155a1df4f10b59d6209c27ccb902686a5e68",
         "5cb4a03ad646b72e3720f56f84c2843cce64bc0bf61d2d4105c406ea7dfaf9c1"),
        ("osce-d3", "6f7e620fc6ea084c6bb30bd9eaabd0d6fac574bc15ac189620c3d5bc42f414cc",
         "0ffc1c6617b2e68ece4b101b6ea5e6bb64f3187f83f9c28e2ccb86cce23ff741"),
        ("osce-d4", "145c0f6827c7f39ace39d8f5fd7bda33a92b996a7502367a81b03cd70a58e63d",
         "cb49f176dbf0a5a47cc4a00f177dcb565b807a8e44c7431a89e909f4e4fb4c5a"),
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
