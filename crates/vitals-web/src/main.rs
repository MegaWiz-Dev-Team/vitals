//! Play EP1 in a browser.
//!
//! One process: the same `vitals-sce` automaton the verifier runs, a session per player, and a
//! tape that is recorded as you play. Reach a terminal state and the tape reduces to a leaf —
//! the same bytes `vitals-replay` would produce from the same tape, because it is the same code.
//!
//! Deliberately small. No framework, no database, no build step: tiny_http, a single HTML page,
//! and sessions in a map. The point is to make the automaton playable, not to ship a platform.

mod chain;
use vitals_web::{
    archive, authors, fuel, lang, meter, news2, patient, payout, reading, rebuild, review, serve,
    store, usage, ward, ward_case, ward_chain,
};

use serde::Serialize;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use std::sync::{Arc, Mutex};
use tiny_http::{Header, Method, Response, Server};
use vitals_progress::record::AttemptRecord;
use vitals_progress::Difficulty;
use vitals_replay::{hex, leaf, record_for, replay, resume, sce_hash, Step};
use vitals_sce::{render_beat, Sce, SceState};

const PAGE: &str = include_str!("../static/index.html");
/// The play surface — the bar, the patient, the monitor, the kit, the log — composed into both
/// pages at their `<!--BAY-->` marker.
///
/// One copy, because two products drawing the same monitor from two copies of the markup is two
/// monitors that drift. The Eternal entry wraps it in the season; the ward's shift page wraps it
/// in a patient and nothing else.
const SURFACE: &str = include_str!("../static/bay-surface.html");
/// The bay's stylesheet and the bay's script, shared by both pages and served as files.
const BAY_CSS: &str = include_str!("../static/bay.css");
const BAY_JS: &str = include_str!("../static/bay.js");
/// The ward's own shift page: the same surface, none of the season.
const SHIFT: &str = include_str!("../static/world/shift.html");
/// The clinical reviewer's list of cases the ward holds — `/ward/review`.
///
/// Named for the ward rather than for review, because `REVIEW` is already the case-review form a
/// physician fills in for the season and the two have nothing to do with each other.
const WARD_CASES: &str = include_str!("../static/world/review.html");
/// The front door. The product page lives at `/` and the game one click behind it at `/play`,
/// because the first visitor a public URL meets is as likely to be a reviewer deciding what this
/// company is as a learner deciding whether to press play — and the bay answers only the second.
const LANDING: &str = include_str!("../static/landing.html");
/// The ward's holding page. `vitals-world` is a second Cloud Run service on world.vitals.academy
/// (CWF_PLAN.md); until the ward itself exists, its root serves this so the link in the hackathon
/// form is never dead. The same binary, one env var: the Eternal entry at vitals.academy is not
/// built differently and is not touched.
/// The ward host's front page: the globe. Self-contained on purpose — d3-geo, topojson-client
/// and the 110m atlas are inlined, so a judge on a bad connection gets the whole thing in one
/// response and no CDN sits between them and the page. `static/world.html` was the holding page
/// that preceded it; the globe carries the same two lines beneath it until the ward has patients.
const WORLD: &str = include_str!("../static/world/index.html");

/// Is this process the ward host rather than the Eternal entry? Set by the deploy script when
/// SERVICE=vitals-world; absent everywhere else, so vitals.academy is unchanged by this code.
fn ward_mode() -> bool {
    std::env::var("VITALS_WORLD").map(|v| v == "1").unwrap_or(false)
}
/// Where the money goes: the treasury address, a Solana Pay QR, and the explorer link to audit
/// it. Baked into the binary like the landing — a donation page that can 404 is a donation lost.
const DONATE: &str = include_str!("../static/donate.html");
/// What this server does with what it collects, and the one action on it nobody can undo.
///
/// Baked in for the same reason [`DONATE`] is, and for one more: a privacy policy that 404s is
/// worse than no privacy policy at all, because every place that links to it — an OAuth consent
/// screen, a footer, a mail to a reviewer — keeps pointing at nothing. It is also the page an
/// external party checks *first* and never comes back to, so the copy that must never go missing
/// is this one.
///
/// Stamped with [`BUILD`] on the way out like [`REVIEW`] is: a policy is a claim about a
/// particular build's behaviour, and a reader who cannot tell which build they are reading about
/// cannot check it against the code.
const PRIVACY: &str = include_str!("../static/privacy.html");
/// The terms, stamped the same way and for the same reason.
const TERMS: &str = include_str!("../static/terms.html");
/// The reviewer's form, served by the same process that stores what it collects.
///
/// Baked in for the same reason the landing is: the two people this page exists for are a
/// student and a physician who were handed one link, and a form that 404s is a review that never
/// arrives. It also travels as a standalone file — mailed, or opened straight off disk — and
/// tells the two copies apart by whether the server stamped [`BUILD_STAMP`] into it: a stamped
/// copy posts to `/api/review`, an unstamped one hands the reviewer their answers to send by
/// hand. Neither can lose what was typed.
const REVIEW: &str = include_str!("../static/review.html");
/// The placeholder the served copy of [`REVIEW`] has replaced, and the standalone copy still
/// carries. The page reads it to decide whether there is a server behind it.
const BUILD_STAMP: &str = "__VITALS_BUILD__";
/// Which build a reviewer saw, stamped into [`REVIEW`] and carried back on the submission.
///
/// `review::Submission::revision` exists so an answer can be read against the thing that produced
/// it: "the timing felt wrong" means one thing against 0.5.1 and another against whatever ships
/// after the physician's rulings land. Without a stamp the field arrives empty and every answer
/// looks like it was written about the current build, whenever anyone happens to read it.
const BUILD: &str = concat!("vitals ", env!("CARGO_PKG_VERSION"));
/// How much of a reviewer's submission this server will read.
///
/// **1 MiB**, and the number is measured rather than guessed. The physician's list is the
/// twenty-eight rulings his review document asks for, each carrying the four lines that document
/// puts in front of a ruling — what the system does now, why we think it is wrong, what we would
/// change it to, the question — so that he can answer from a phone with nothing open beside him.
/// Filling every one of those to the store's four-thousand-character clamp, in Thai at three
/// bytes a character, with a chosen option on every item, produces **381 KiB**; the student's
/// eleven items produce 178 KiB. The cap sits at two and a half times the larger of them, so a
/// reviewer cannot reach it and anything that does is not a review.
///
/// It was 256 KiB when the form asked sixteen one-line questions. Carrying the documents' own
/// items multiplied both the number of items and the context stored beside each answer, and a cap
/// left at the old number would have started refusing exactly the submissions worth having: the
/// long ones, from the reviewer who answered everything.
///
/// Enforced by refusing, never by truncating. A review cut short still *looks* like a review — it
/// parses, it stores, it reads as though the reviewer simply stopped writing — and nobody, least
/// of all the physician whose ruling lost its second half, ever finds out. A 413 is visible: the
/// page keeps the draft and hands the answers back to be sent by hand.
const REVIEW_MAX: usize = 1024 * 1024;
/// The pitch, served by the same process that serves the bay.
///
/// Baked in rather than read from disk. Twice in one day a path that existed on the build machine
/// did not exist in the container — the patient could not speak, and the film would not play — and
/// both failures looked like something else entirely. A deck cannot go missing halfway through a
/// pitch if there is no file for it to go missing from.
const DECK: &str = include_str!("../../../pitch/deck.html");
/// The ward host's mark. Under `static/world/` with the pages that link it, not under `pitch/`
/// with the brand it came from: `.dockerignore` keeps all of `pitch/` out of the build context
/// bar the deck, so a runtime asset kept there is one Cloud Build cannot read. Baked in for the
/// same reason the deck is — a favicon read off the disk is a favicon that goes missing in the
/// container, and a tab icon fails silently when it does.
const FAVICON_WORLD: &[u8] = include_bytes!("../static/world/favicon.svg");
/// The same mark at 180 px for a home-screen bookmark, because iOS takes a PNG and nothing else.
/// Rendered from the SVG above rather than drawn again, so there is one mark and one file to edit.
const TOUCH_ICON_WORLD: &[u8] = include_bytes!("../static/world/apple-touch-icon.png");
/// The speaking script is deliberately **not** compiled in beside the deck.
///
/// `pitch/script.html` is the presenter's own notes — what to say, what not to say, and what the
/// room is assumed to be thinking — and it was baked into this binary and served at
/// `/slides/script` with nothing in front of it. The deck is the public artefact. The notes
/// behind the deck are not, and there is no `include_str!` for them here on purpose.
/// Arrow keys, a counter and a progress rule, appended to the deck when it is served.
///
/// Kept out of `pitch/deck.html` deliberately: that file is regenerated by scripts, so anything
/// written into it is one regeneration away from being lost — and `build-pdf.sh` renders it
/// straight off disk through a `file://` URL, where a printed deck has no use for arrow keys.
const PRESENT: &str = include_str!("../static/present.html");
/// The team's dashboard — one page, two endpoints, every thirty seconds. Served here because the
/// ward sends no CORS headers and a page that reads it has to come from it. Public: nothing on it
/// a stranger cannot already read from /api/ward and /api/usage. `static/world/stats.html` is the
/// only copy; the founder's request of 20 ก.ย.
const WARD_STATS: &str = include_str!("../static/world/stats.html");

/// The two-minute guide, and the eight screenshots it walks a stranger through.
///
/// Compiled in like every other page this server has. The alternative — reading them off disk at
/// request time — is how a page becomes something no test can see and no revision can pin: the
/// image a reader gets would depend on what happened to be in a directory, not on the commit that
/// was deployed.
///
/// The list is the route. `/start/img/<name>` answers only for a name in it, so a path a stranger
/// types cannot reach anything but these eight, and a file added here without a caption in the
/// page is a file nobody is served.
const WARD_START: &str = include_str!("../static/world/start/index.html");
const WARD_START_IMG: &[(&str, &[u8])] = &[
    ("01-globe.jpg", include_bytes!("../static/world/start/img/01-globe.jpg")),
    ("02-bedside-before.jpg", include_bytes!("../static/world/start/img/02-bedside-before.jpg")),
    ("03-after-take.jpg", include_bytes!("../static/world/start/img/03-after-take.jpg")),
    ("04-first-order.jpg", include_bytes!("../static/world/start/img/04-first-order.jpg")),
    ("05-treating.jpg", include_bytes!("../static/world/start/img/05-treating.jpg")),
    ("06-handover-armed.jpg", include_bytes!("../static/world/start/img/06-handover-armed.jpg")),
    ("07-handed-over.jpg", include_bytes!("../static/world/start/img/07-handed-over.jpg")),
    ("08-receipt.jpg", include_bytes!("../static/world/start/img/08-receipt.jpg")),
];
/// The real bedside monitor, vendored from Embla's device page.
///
/// Not reimplemented: it already draws ECG morphology in milliseconds (P 80ms, PR 160ms, a QRS
/// that stays 90ms at any rate), sweeps a cursor the way a monitor does instead of scrolling, and
/// knows VF from asystole from PEA. A hand-rolled trace reads as fake to a clinician instantly —
/// which is exactly what the first version of this app did.
const MONITOR: &str = include_str!("../static/device/monitor.html");
const VENT: &str = include_str!("../static/device/vent.html");
const PUMP: &str = include_str!("../static/device/pump.html");

/// Where the rendered EP1 clips live.
///
/// Served from disk rather than baked into the binary: 20 clips is 43MB, and the point of
/// reusing already-rendered film is that it does not need to be moved around again.
fn clips_dir() -> std::path::PathBuf {
    std::env::var("VITALS_CLIPS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::PathBuf::from("/Users/mimir/Developer/Embla/app-swift/Resources/cutscenes/ep1")
        })
}

/// The patient, keyed by the clinical status the automaton is reporting.
///
/// This is the Director's job in the real Story Mode, reduced to its smallest useful form: the
/// engine says how she is, and the screen shows it. The stills are EP1's, already rendered.
const STILLS: &[(&str, &[u8])] = &[
    ("stable", include_bytes!("../static/img/stable.jpg")),
    ("deteriorating", include_bytes!("../static/img/deteriorating.jpg")),
    ("critical", include_bytes!("../static/img/critical.jpg")),
    ("arrest", include_bytes!("../static/img/arrest.jpg")),
    ("improving", include_bytes!("../static/img/improving.jpg")),
    ("recovered", include_bytes!("../static/img/recovered.jpg")),
    ("dead", include_bytes!("../static/img/dead.jpg")),
];

/// The episode key art — one face per shift, keyed the way [`STILLS`] is so `/img/<key>.jpg`
/// serves both from one arm.
///
/// Kept apart from `STILLS` because they answer different questions: a still says how the
/// patient *is* right now and is swapped by the automaton, while key art is the episode's
/// portrait and never changes during a run. Two entries per episode — a 16:9 billboard crop and
/// a 3:2 one for a narrow screen, which the page picks between with `<picture>` so a phone never
/// downloads the wide one. Canon for these faces is locked in `docs/internal/SEASON_ARC.md`
/// ("Canon ภาพตัวละคร"); EP1's portrait is `stable.jpg`, a real frame of its own patient, and
/// `ep1_ing_3x2` is that same frame cropped — not a second photograph of her, so the shelf and
/// the landing carousel cannot end up disagreeing about what she looks like.
const KEY_ART: &[(&str, &[u8])] = &[
    ("ep1_ing_3x2", include_bytes!("../static/img/ep1_ing_3x2.jpg")),
    ("ep2_prasit", include_bytes!("../static/img/ep2_prasit.jpg")),
    ("ep2_prasit_3x2", include_bytes!("../static/img/ep2_prasit_3x2.jpg")),
    ("ep3_khaopun", include_bytes!("../static/img/ep3_khaopun.jpg")),
    ("ep3_khaopun_3x2", include_bytes!("../static/img/ep3_khaopun_3x2.jpg")),
    ("ep4_mali", include_bytes!("../static/img/ep4_mali.jpg")),
    ("ep4_mali_3x2", include_bytes!("../static/img/ep4_mali_3x2.jpg")),
    ("ep5_boonsong", include_bytes!("../static/img/ep5_boonsong.jpg")),
    ("ep5_boonsong_3x2", include_bytes!("../static/img/ep5_boonsong_3x2.jpg")),
];

struct Session {
    /// Which scenario, so a resumed run reloads the same automaton it was played against.
    ep: String,
    /// Whose case this is. `None` for a kiosk or a browser that cannot make a key — then the
    /// session id is the only secret, which is why it is not a counter any more.
    owner: Option<String>,
    state: SceState,
    tape: Vec<Step>,
    beats: Vec<String>,
    /// The films this run has ordered, oldest first, accumulated the way `beats` is so a
    /// reloaded or resumed run still shows what it has already seen — `View` is a full snapshot,
    /// not a delta. Presentation only: see [`Film`].
    films: Vec<&'static Film>,
    sce_json: String,
    scenario: String,
    difficulty: Difficulty,
    anchored: bool,
    /// The declaration this run answers: (commitment hash, the slot it landed at, the nonce).
    ///
    /// Written when the player's commit transaction confirms, read when the record is built —
    /// the leaf must carry the same (hash, slot) the program stamped, or the server's local
    /// leaf list forks from the tree on chain and every later proof fails. The nonce stays
    /// here so the run can be revealed later; it never reaches the chain.
    commit: Option<([u8; 32], u64, [u8; 32])>,
    /// Whether this run was declared an exam — bound into the commitment hash before play, so
    /// it is carried from commit to anchor and stamped into the record from here, never
    /// re-chosen after the outcome is known.
    exam_mode: bool,
    /// The conversation, kept only so she remembers what she already told you.
    ///
    /// Never hashed and never anchored, and that half is structural: the leaf commits to the
    /// tape, the tape carries the question (`Step::Ask`) and never her reply, so a model's words
    /// stay out of the proof path entirely.
    ///
    /// It does leave this process, twice — this comment used to say it never did, which stopped
    /// being true the moment runs were written down. It is a field of [`Saved`], so every
    /// `persist` puts it in the store, which is Firestore on Cloud Run and files elsewhere; and
    /// the last eight messages of it are sent to the model as history on every `/api/say`, which
    /// is the local gateway when it is reachable and Vertex AI when it is not (`patient.rs`).
    /// The stored copy lives as long as the run does. /privacy §4 and §5 say the same to a player.
    said: Vec<(String, String)>,
    /// Last time this run was written to disk. Ticks arrive about once a second and are cheap to
    /// lose — the tape is the truth and a few seconds of it is a few seconds of sim — so they are
    /// throttled. Anything the player actually *did* is written immediately.
    saved_at: Option<std::time::Instant>,
    /// Which shift on the ward this run is, when it is one. `None` is the Eternal bay, unchanged.
    ward: Option<WardShift>,
    /// Which case this run is *reading*, when it is a review run rather than a shift.
    ///
    /// A review run plays a held case with an invented person, so that eighteen cases waiting for a
    /// clinical advisor do not have to be caught one at a time as the ticker admits them. It is not
    /// on the ward and not on the chain: no bed, no lease, nothing anchored, nothing counted. The
    /// chain routes read this to refuse it in words that say what kind of run it is.
    review: Option<String>,
    /// Handed over: reduced, named and finished. Nothing more goes on this tape.
    ///
    /// Set when `/api/handover` has computed the leaf, because that is the moment a tape stops
    /// being a thing in progress and becomes a thing with a name. A step after it lands on a tape
    /// that has already been counted — which on 16 ก.ย. put one hash on chain and another in the
    /// store, and left a patient nobody could open.
    handed_over: bool,
}

/// One shift on the ward: whose, which link in her chain, and the head it must extend.
///
/// The bay is one bay and this is the parameter (producer, 16 ก.ย.). Absent, every line of the bay
/// behaves as it did — `bay_unchanged.rs` is what says so.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct WardShift {
    patient_id: u64,
    /// Her chain's length when this shift began, which is this tape's index in it.
    index: u32,
    /// The slot her lease was taken at. The gap before this shift ends here, and a restored
    /// session must resume to this slot rather than to the moment the browser came back.
    taken_slot: u64,
    /// The head this shift extends, hex. Named before the work rather than read after it: the
    /// program refuses a reveal that does not extend the head it was told, and that refusal is
    /// the mechanic.
    head: String,
    /// Her pictures, as the pack carries them — every state the factory has made a face for.
    ///
    /// Copied into the session at the moment she is opened rather than read per view: a view that
    /// reached for the store to draw a face would be a view nobody can test, and this set does not
    /// change during a shift (the factory adds faces to patients, and add-only is the rule).
    #[serde(default)]
    faces: std::collections::BTreeMap<String, String>,
    /// How old **she** is — the patient the ward placed here, not the patient the case was
    /// authored about.
    ///
    /// Carried in the session for the same reason her faces are: a view that reached for the store
    /// to answer it would be a view nobody can test. And carried at all because the alternative
    /// was asking the case table, which has never heard of her — on 22 ก.ย. that returned nothing,
    /// `news2` read nothing as an adult, and a girl of eight was scored on the adult chart in
    /// production.
    ///
    /// `u16` and not an option: a patient in a bed always has an age. A restored session written
    /// before this field existed defaults to 0, which reads as under sixteen and is therefore not
    /// scored — the safe direction, and the one the old default got backwards.
    #[serde(default)]
    age: u16,
}

/// A run as it sits on disk.
///
/// The state is not here, because the state is not a fact — it is what the tape computes. Storing
/// it would create a second copy that can disagree with the tape, which is the failure this repo
/// has already had once. On load the tape is replayed and the machine is rebuilt from it.
#[derive(serde::Serialize, serde::Deserialize)]
struct Saved {
    ep: String,
    #[serde(default)]
    owner: Option<String>,
    /// The scenario this run was played against. A rewritten scenario must not silently resume a
    /// run into a different automaton — the outcome would be re-derived under rules the player
    /// never played.
    sce_hash: String,
    tape: Vec<Step>,
    said: Vec<(String, String)>,
    anchored: bool,
    #[serde(default)]
    commit: Option<([u8; 32], u64, [u8; 32])>,
    #[serde(default)]
    exam_mode: bool,
    /// The ward shift this run is, if it is one. Defaulted, so every run stored before the ward
    /// existed reads back as what it was: a run in the bay.
    #[serde(default)]
    ward: Option<WardShift>,
    /// The case this run is reading, if it is a review run.
    #[serde(default)]
    review: Option<String>,
    /// Handed over, so a restart cannot un-finish a shift.
    #[serde(default)]
    handed_over: bool,
}

const SESSIONS: &str = "sessions";

impl Session {
    /// May this caller touch this case?
    ///
    /// An owned case answers only to its owner. An anonymous one answers to whoever holds the id,
    /// which is safe only because the id is now 128 bits of randomness rather than `s7`.
    fn answers_to(&self, who: Option<&str>) -> bool {
        match (&self.owner, who) {
            (None, _) => true,
            (Some(mine), Some(you)) => mine == you,
            (Some(_), None) => false,
        }
    }
}

/// A session id nobody can walk to from the one before it.
///
/// It used to be `s1`, `s2`, `s3`. On a server two people can reach, a counter is an index of
/// everybody else's cases — and every route looked a session up by id and did as it was told.
fn fresh_id() -> String {
    use solana_sdk::signature::{Keypair, Signer};
    hex_bytes(&Keypair::new().pubkey().to_bytes()[..16])
}

/// The same answer for "there is no such case" and "that case is not yours".
///
/// Two different answers would let a guesser tell live ids from dead ones, which is most of the
/// work of finding somebody to interfere with.
fn no_such_session() -> Response<std::io::Cursor<Vec<u8>>> {
    json(serde_json::json!({ "error": "no such session" }))
}

impl Session {
    fn saved(&self) -> Saved {
        Saved {
            ep: self.ep.clone(),
            owner: self.owner.clone(),
            sce_hash: hex(&sce_hash(&self.sce_json)),
            tape: self.tape.clone(),
            said: self.said.clone(),
            anchored: self.anchored,
            commit: self.commit,
            exam_mode: self.exam_mode,
            ward: self.ward.clone(),
            review: self.review.clone(),
            handed_over: self.handed_over,
        }
    }

    /// Rebuild a run from disk by replaying its tape.
    ///
    /// `prior` is `None` for the bay and carries the chain's account of her for a ward run: a
    /// shift's tape means nothing without the patient it was played on, so the state is rebuilt
    /// the way it was built the first time — every anchored shift in the chain's order, then this
    /// one. Passed in rather than read here, because a rebuild that reaches for a chain is a
    /// rebuild nobody can test.
    fn restore(
        saved: Saved,
        prior: Option<&WardRebuild>,
        tape_of: &dyn Fn(&str) -> Option<Vec<Step>>,
        dated: &dyn Fn(u64) -> Option<i64>,
    ) -> Result<Session, String> {
        let sce_json = std::fs::read_to_string(scenario_path(&saved.ep)).map_err(|e| e.to_string())?;
        let want = hex(&sce_hash(&sce_json));
        if want != saved.sce_hash {
            return Err(format!("scenario {} changed under this run", saved.ep));
        }
        let (state, r) = match (&saved.ward, prior) {
            (None, _) => resume(&sce_json, &saved.tape)?,
            // Resumed to the slot this shift opened at, not to now: the shift's own clock started
            // then, and the idle time since belongs to whoever comes next.
            (Some(w), Some(p)) => {
                let (mut st, _) = ward_chain::resumed(
                    &sce_json,
                    &p.shifts,
                    tape_of,
                    p.admitted_slot,
                    w.taken_slot,
                    // Whatever this ward has already asked the chain about. A restored session is
                    // rebuilt with no RPC in hand; a slot it cannot date advances her by nothing,
                    // and the next read that can date it does.
                    dated,
                )?;
                let r = vitals_replay::shift(&mut st, &saved.tape, 0.0);
                (st, r)
            }
            (Some(w), None) => {
                return Err(format!(
                    "this is a shift on patient {}, and the chain could not be read to rebuild \
                     her — the run is dropped rather than replayed onto a patient nobody checked",
                    w.patient_id
                ))
            }
        };
        Ok(Session {
            ep: saved.ep.clone(),
            owner: saved.owner.clone(),
            review: saved.review.clone(),
            state,
            beats: r.beats,
            films: films_from_tape(&saved.ep, &saved.tape),
            tape: saved.tape,
            sce_json,
            scenario: title(&saved.ep),
            difficulty: difficulty(&saved.ep),
            anchored: saved.anchored,
            commit: saved.commit,
            exam_mode: saved.exam_mode,
            said: saved.said,
            saved_at: Some(std::time::Instant::now()),
            ward: saved.ward,
            handed_over: saved.handed_over,
        })
    }
}

/// Count a run that has just ended — once, on the edge, and never again.
///
/// `was_over` is read *before* the request touches the automaton, so the increment happens on the
/// transition into a terminal state and nowhere else. A run resumed from disk after the bell
/// replays into an outcome that is already set, so it arrives here with `was_over` true and is
/// not counted a second time; a run that ends and is never touched again was counted by the very
/// request that ended it.
///
/// The bucket is the case's own outcome id, not a label invented here — the scenario author's
/// vocabulary is the only one that stays true when a case is rewritten.
fn count_finish(u: &mut usage::Usage, s: &Session, was_over: bool, store: &store::Store) {
    if was_over || !s.over() {
        return;
    }
    match s.state.outcome() {
        Some(o) => u.finished(
            s.state.outcome_id().unwrap_or("unknown"),
            o.is_death(),
            s.owner.is_some(),
            store,
        ),
        // A run the clock ended. It has no outcome id to borrow, because the case never reached
        // one of its own endings — so it is counted under a name of its own rather than folded
        // into somebody else's ending or, as before this existed, not counted at all.
        None => u.finished(TIME_CALLED, false, s.owner.is_some(), store),
    }
}

/// The bucket a run the clock ended is counted under. Not a scenario's word, because no scenario
/// says it: it is what happened when none of them did.
const TIME_CALLED: &str = "time_called";

/// Write a run to disk. `urgent` is false only for a bare tick.
fn persist(store: &store::Store, id: &str, s: &mut Session, urgent: bool) {
    const THROTTLE: std::time::Duration = std::time::Duration::from_secs(3);
    if !urgent {
        if let Some(t) = s.saved_at {
            if t.elapsed() < THROTTLE {
                return;
            }
        }
    }
    if let Err(e) = store.put(SESSIONS, id, &s.saved()) {
        eprintln!("could not save session {id}: {e}");
    }
    s.saved_at = Some(std::time::Instant::now());
}

/// The anchoring tree this server is filling.
///
/// Shared by every player on the box, which is what the on-chain seeds say: the tree is keyed by
/// its id alone, and a Merkle proof needs every leaf in it. What is *not* shared is who a run
/// belongs to — the claim and progress accounts are seeded on the player, so two people on one
/// server have two separate records.
///
/// It has to outlive the process. The leaves are on chain either way, but the proof is built from
/// this list, so a server that forgets them can no longer prove anything it anchored.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Tree {
    tree_id: u64,
    leaves: Vec<[u8; 32]>,
}

const TREE: &str = "tree";

/// How long `/api/authors` holds its chain read.
///
/// A bound on how often that endpoint can fan out `get_program_accounts`, not a freshness
/// promise: at most one chain read a minute however hard the endpoint is asked for. The
/// re-derivation path (`verify_player --authors`) is uncached, because reading the chain is the
/// whole point of it.
const AUTHOR_COUNT_TTL: Duration = Duration::from_secs(60);

/// Everything `/api/authors` needs from the chain, taken in one pass and held for the TTL.
///
/// One struct rather than three caches, because the three answers have to be from the same
/// moment: a paid count taken a minute after the proven count it is displayed beside would show
/// a case paid more times than it was played.
#[derive(Clone, Default)]
struct ChainView {
    /// Case hash → proven attempts.
    per_case: std::collections::BTreeMap<String, u64>,
    /// Case hash → what has been paid for it.
    paid_by_case: std::collections::BTreeMap<String, authors::Paid>,
    /// Leaf → (lamports, signature), so `/api/payout` can answer for a leaf this instance did
    /// not pay itself — a restart, or another instance.
    paid_leaves: std::collections::BTreeMap<String, (u64, String)>,
}

/// The held chain read: when it was taken, and everything it produced.
type AuthorCounts = Arc<Mutex<Option<(Instant, ChainView)>>>;

/// How long `/api/ward` holds its read of the ward.
///
/// A bound on the fan-out, not a freshness promise — and it does not need to be one, because the
/// payload carries `as_of_slot`: a reader can see exactly how old the numbers are rather than
/// having to trust that they are new. Thirty seconds, because the board is meant to feel live and
/// a full pass costs one `get_program_accounts` plus one signature page per patient.
const WARD_TTL: Duration = Duration::from_secs(30);

/// The held ward census.
/// The board this process last read, and when. Its provenance is stamped into the board itself —
/// see [`ward::board_note`] — so every answer carrying one board carries identical bytes.
type WardView = Arc<Mutex<Option<(Instant, serde_json::Value)>>>;

/// How often the ward looks at itself: a bed that freed, a queue that has somebody in it.
///
/// A minute, because that is the resolution a watcher of the board would notice and because the
/// work is one `get_program_accounts` — at this rate the ward's own upkeep is 1440 chain reads a
/// day, which is nothing, and the alternative is a person noticing.
const WARD_TICK: Duration = Duration::from_secs(60);

/// How often a page holding a head says it is still there, and how long the ward waits before
/// taking the head back.
///
/// Thirty seconds and two missed beats. The page sends one plain POST per beat — no signature, no
/// chain read — so the cost of beating often is nothing and the cost of being wrong is a bed taken
/// off somebody standing in the room. Fifteen seconds' slack for a page on a slow train.
///
/// The sweep is faster than the grace on purpose: the answer to "how long is a bed held after
/// somebody walks away" should be the grace, not the grace plus however long the sweeper sleeps.
const BEAT_EVERY: Duration = Duration::from_secs(30);
const BEAT_GRACE_MS: u64 = 75_000;
const BEAT_SWEEP: Duration = Duration::from_secs(10);

/// How many boards may be watching at once.
///
/// Each stream is a thread that lives as long as the connection, so this is the ceiling on threads
/// a stranger can ask this server for. Cloud Run's own concurrency limit is lower than this in
/// production; the number exists for every other way the server can be run.
const WARD_STREAMS: usize = 64;

/// How often a stream looks for something new to say.
///
/// Cheap: the payload is the held read, so this costs a lock and a string compare, and a chain
/// read happens at [`WARD_TTL`] whatever the streams do.
const WARD_STREAM_POLL: Duration = Duration::from_secs(3);

/// The most one push from the factory may carry.
///
/// A pack is a few hundred bytes, so this is room for a hundred or so patients at once — well past
/// the twenty the queue is kept at, and small enough that a body arriving from a job nobody is
/// watching cannot become this server's memory problem.
const QUEUE_MAX: usize = 64 * 1024;

/// How often one browser may open a patient on the ward, and how often one address may.
///
/// The key is a person: a shift is minutes long, so six a minute is a loop rather than a learner,
/// and two hundred a day is more patients than anybody will meet.
///
/// The address is a building. A class of thirty opening a patient each minute for a ninety-minute
/// session is two thousand seven hundred opens from one NAT, and every one of them is somebody
/// learning — so the budget is a classroom's. It is still a budget: it is the only thing standing
/// between the ward and a script with no key, and a script with no key cannot take a shift anyway.
const OPENS_PER_KEY_MIN: usize = 6;
const OPENS_PER_KEY_DAY: usize = 200;
const OPENS_PER_ADDR_MIN: usize = 120;
const OPENS_PER_ADDR_DAY: usize = 3_000;

/// A case pack: the scenario, the mark sheet, the voice and the replay proof. The compiler's own
/// run between 15 and 40 KB, and a limit that refused one of those would be a limit that refuses
/// medicine to save bytes.
const CASE_MAX: usize = 512 * 1024;

/// Payouts this process made, by leaf.
///
/// A record of our own transfers, not a second opinion about whether to pay — `settle` still
/// decides from a fresh read of the chain every time. This exists so the display can answer
/// without a round trip: `/api/payout` is polled a dozen times per finished run, and a class
/// finishing together would otherwise be hundreds of chain fan-outs on the one thread this
/// server has, competing with the anchoring those same learners are waiting on.
type Settled = Arc<Mutex<HashMap<String, (u64, String)>>>;

/// What a reconciliation of the local leaf list against the chain's tree decided.
#[derive(Debug, PartialEq, Eq)]
enum Reconciled {
    /// The lists agree, or agreed after dropping `dropped` leaves nobody could hold a proof for.
    Ready { dropped: usize },
    /// The chain holds leaves this server does not. Anchoring must not continue.
    Short { local: usize, chain: u64 },
}

/// Make the local leaf list match the tree the program actually built, or refuse.
///
/// **The chain is the source of truth for the index. The local list is a cache that has to prove
/// itself before every use.**
///
/// `prepare_anchor` takes the index it puts in `ProveAttempt` from the length of *this* list, and
/// the program checks that proof against a tree it appends to itself. The two are not merely
/// related — they must be identical, element for element, or every later proof is rejected. One
/// anchor prepared and never submitted used to leave this list one longer for ever, and
/// `tests/chain_flow.rs` measures the cost: the next player's run anchors and cannot be proven,
/// and so does the player after that, who was in nobody's window.
///
/// The two directions are **not** symmetric, and treating them alike is the mistake this exists
/// to prevent:
///
///   * **Longer than the chain** — the extra leaves are ghosts of anchors prepared and never
///     landed. They are on no chain, so no proof anywhere refers to them and nobody holds
///     anything a truncation would invalidate. Rubbish we generated ourselves: drop it and carry
///     on, but say how much and why, because a silent truncation is somebody's next bug.
///   * **Shorter than the chain** — a leaf that *is* anchored is missing from the list every
///     proof is rebuilt from. Somebody holds a proof of it. Appending now would build a tree
///     that abandons their record, so this refuses to anchor at all and says so loudly. It is
///     not repairable here: the leaf's bytes are gone and only the run they came from could
///     produce them again.
///
/// One direction is our own litter. The other is somebody else's evidence that we lost. They do
/// not get the same treatment.
fn reconcile_leaves(leaves: &mut Vec<[u8; 32]>, chain_len: u64) -> Reconciled {
    let local = leaves.len();
    match (local as u64).cmp(&chain_len) {
        std::cmp::Ordering::Equal => Reconciled::Ready { dropped: 0 },
        std::cmp::Ordering::Greater => {
            leaves.truncate(chain_len as usize);
            Reconciled::Ready { dropped: local - chain_len as usize }
        }
        std::cmp::Ordering::Less => Reconciled::Short { local, chain: chain_len },
    }
}

/// Take back a leaf *this* request pushed — and only while it is still the one on the end.
///
/// `/api/anchor` pushes the leaf, then hands the transaction to the browser to sign. Between
/// that push and the unwind sits a human looking at a wallet prompt. This server handles
/// requests one at a time, so another player's anchor is not racing that wait — it happens
/// *inside* it, start to finish. A bare `pop()` at the end therefore does not necessarily
/// remove the leaf it meant to:
///
/// ```text
/// anchor(A)        push A      len 6, A at index 5
/// anchor(B)        push B      len 7, B at index 6
/// submit(B)   ok               B is on chain at index 6
/// submit(A)   signature refused → pop() removes B
/// ```
///
/// B is anchored and now missing from the list every proof is rebuilt from; A is in the list
/// and anchored nowhere. Both records are wrong, and only one of them is recoverable.
///
/// So: unwind only when our own push is still the last one. If it is not, leave the leaf
/// where it is. That keeps a leaf nobody claims — harmless, and a sweep can find it by asking
/// the chain — rather than dropping one somebody proved.
fn unwind_leaf(leaves: &mut Vec<[u8; 32]>, pushed_at: u64) -> bool {
    if leaves.len() as u64 == pushed_at + 1 {
        leaves.pop();
        return true;
    }
    false
}

#[derive(Serialize)]
struct View {
    scenario: String,
    /// Electrical, and so it survives an arrest — PEA is complexes at a countable rate with no
    /// output behind them, and that disagreement is the finding.
    hr: f64,
    /// `null` whenever there is no pulse to measure them against — see [`reading`]. The rail
    /// prints `--` for a null; it must never print a number that was measured off blood that is
    /// not moving, and the server not sending one is what makes that unarguable.
    sbp: Option<f64>,
    dbp: Option<f64>,
    spo2: Option<f64>,
    /// 0 through an arrest: a patient in cardiac arrest is not breathing, and a calm 28 per
    /// minute over a flat pleth is the screen contradicting itself.
    rr: f64,
    temp: f64,
    gcs: u8,
    /// Whether the heart is producing output. The one fact the rail and the bedside device have
    /// to agree on, so both read it from the same place.
    pulse: bool,
    /// What the ECG is doing, verbatim from the scenario — `sinus`, `pea`, `vf`, `vt`,
    /// `asystole`. The rail draws its trace from this rather than from the status, because a
    /// strip labelled PEA with sinus complexes on it is a clinician's first and last impression.
    rhythm: &'static str,
    /// Whether a defibrillator can do anything for that rhythm. Carried so the rail cannot
    /// re-derive it and get it wrong; shocking PEA costs compressions and adrenaline.
    shockable: bool,
    status: String,
    /// The face to draw at the bedside, on a shift and nowhere else.
    ///
    /// **The server chooses it**, from the same word it publishes in `status` and by the same
    /// ladder the board uses — so the page draws a URL and holds no opinion about states, sizes or
    /// what to do when a picture for this one has not been made. Absent on the Eternal entry: its
    /// stills are the season's, chosen by the Director, and a ward pack has no say in them.
    #[serde(skip_serializing_if = "Option::is_none")]
    portrait: Option<String>,
    beats: Vec<String>,
    /// The display line for each beat above, in the language the page asked for — the language
    /// layer's half of [`View::beats`], and the only part of it a reader ever sees.
    ///
    /// Keyed by the canonical beat, so the page keeps doing all of its *thinking* on `beats`
    /// (which cutscene to roll, which line is a harm, which one to unseal) and uses this only for
    /// the words. That separation is what makes a language switch unable to reach the Director,
    /// the exam seal, or anything else that matters.
    ///
    /// **Only beats this run has already earned appear here.** A pack containing every beat of
    /// every case would be an answer key one devtools tab away — a harm line names the drug, the
    /// disease and the deadline the rubric is paying for. Absent for the default language, where
    /// the page already holds the wording: see [`lang::pack`].
    #[serde(skip_serializing_if = "Option::is_none")]
    tr: Option<std::collections::BTreeMap<String, &'static str>>,
    /// The films ordered so far. Never on the tape, never in the leaf — the page draws a
    /// thumbnail strip from this and nothing else reads it.
    films: Vec<&'static Film>,
    harm: Vec<String>,
    outcome: Option<String>,
    /// Whether the encounter is finished — [`Session::over`], the one predicate.
    ///
    /// **Not the same fact as `outcome`.** A run can be over with no terminal at all: time was
    /// called on a patient the case was never going to resolve, which is what happens on
    /// `osce-b2` and `osce-c` and what used to leave them running for ever. The page reads this
    /// and never `outcome` to decide whether the clock has stopped, the result panel is due and
    /// the mark sheet may be asked for.
    over: bool,
    elapsed: f64,
    /// What the station advertises, in simulated seconds — the same figure the shelf card
    /// prints as `mins`, served so the page has one authority for it rather than two.
    limit: f64,
    /// Only once the run is over — a run in progress has nothing to anchor yet.
    leaf: Option<String>,
    sce_hash: String,
    /// What is on the patient right now, in the order it went on.
    equipment: Vec<Kit>,
    /// Everything that happened, stamped with the scenario clock — the chart.
    chart: Vec<Note>,
    /// NEWS2 — what a ward actually escalates on, computed from the observations above.
    ///
    /// This used to be a "stability" percentage invented here, and it averaged: a patient with
    /// one catastrophic derangement and six normal readings came out looking well. That is the
    /// mistake the real score exists to prevent.
    ///
    /// `None` **once she has died, and for no other reason.** It is an *early warning* score — it
    /// exists to decide whether somebody needs to come and how fast, and there is nothing left to
    /// warn about. The page reads a null here as a death and does considerably more than blank a
    /// number: it raises the result panel and stops painting. A patient NEWS2 does not cover is
    /// not a dead patient, so she gets a [`News`] with `applies: false` instead.
    news: Option<News>,
}

/// The NEWS2 panel, as the page receives it.
///
/// Present for every living patient — `null` means dead and nothing else, and the page's dead
/// branch does more than blank a number, so a paediatric patient may never be sent through it.
#[derive(Serialize)]
struct News {
    /// Whether NEWS2 covers this patient at all. `false` for anyone under 16, where the score is
    /// not validated: [`total`](News::total) and [`worst`](News::worst) are then `null`, `band`
    /// is `"none"`, and `response` carries the sentence to show in the score's place.
    applies: bool,
    /// `null` exactly when `applies` is false. Never a zero standing in for "no score" — a zero
    /// is the best NEWS2 a patient can have, and printing one over a child is the reassurance
    /// this whole field exists to refuse.
    total: Option<u32>,
    worst: Option<u32>,
    /// `"low"`, `"medium"`, `"high"` — or `"none"` when the score does not apply.
    band: &'static str,
    /// What the score asks you to do about it, or why there is no score.
    response: &'static str,
}

#[derive(Serialize, Clone)]
struct Kit {
    id: String,
    setting: Option<f64>,
    since: f64,
}

#[derive(Serialize, Clone)]
struct Note {
    t: f64,
    kind: String,
    text: String,
}


/// How [`vitals_sce::render_beat`] spells a harm, and the prefix a sealed view drops on.
///
/// This used to have a companion, `HARM_SEALED = "harm:sealed"` — the one thing a sealed harm
/// was allowed to say. It said too much. A redacted line is still a line: the feed printed it as
/// "⚠ harm recorded" the instant the candidate acted, which is the verdict the seal exists to
/// withhold, delivered on screen rather than merely on the wire. Both the chart row and the feed
/// line are now absent while sealed, and there is nothing left to redact *to*.
const HARM_BEAT: &str = "harm:";

/// The event kind the automaton stamps on a harm, and the row the sealed chart does not carry.
///
/// A redacted row is still a row. Under seal the chart read
///
/// ```text
/// 0:12 | ORDER | IV-push adrenaline
/// 0:12 | HARM  | ⚠ harm recorded
/// ```
///
/// — the sentence withheld and the *timing* handed over, on the same second as the order that
/// caused it. A candidate does not need to read the sentence to learn what the seal exists to
/// withhold: a marker landing the instant they act says "that one was the mistake", which is the
/// whole of what the mark sheet is going to say later. Redacting the word and leaving its shape
/// is not redaction.
///
/// So under seal the row is filtered out of the chart entirely, before the bytes exist, and the
/// two orders a station is built to tell apart produce charts of the same length, the same kinds
/// and the same clock. After the bell every row comes back in full, because the debrief, the
/// harm list and the mark sheet are what the seal was holding the case open for.
///
/// **Display only, and it must stay that way.** `SceState::harm_events` still records every harm,
/// the tape still carries every order, `replay` recomputes the events from the tape and never
/// from here, and the leaf hashes the replay. A run played under seal and the same run played
/// unsealed anchor byte for byte identically — pinned by `exam_integrity`.
const HARM: &str = "harm";

/// The author's own annotation on a label — the part that grades the order rather than naming it.
///
/// Nineteen labels across twelve stations end in one, in three shapes: `(HARM)`, `(HARM here)`,
/// and one with real text in front of it, `Adrenaline 0.5 mg IM — adult dose (HARM)`. Matched on
/// the opening word of the parenthetical so all three fall to one rule and `— adult dose` — which
/// is a description of the order, not a verdict on it, and is the mirror of the label on the
/// correct dose — survives.
const VERDICT: &str = "HARM";

/// A label with the author's verdict taken off it.
///
/// Commit 52d29e4 replaced the intervention id on the chart with the case author's label, to stop
/// the chart printing the rubric's own needles (`exam_throat`, `adrenaline_undosed`). It was the
/// right move and it carried a second thing across: the labels are the author's working notes,
/// and half of them say what the author thinks of the order.
///
///     0:12 | ORDER | IV-push adrenaline (HARM)
///     0:12 | HARM  | ⚠ harm recorded
///
/// The harm sentence is sealed. The order line above it was not, so the chart — the one surface
/// in an exam that has to stay neutral — told the candidate mid-run that they had just got it
/// wrong. On `osce-d3` that is the whole station: two adrenaline doses, one paediatric and one
/// adult, and the chart named which one was the trap the instant either was given.
///
/// So the verdict comes off before the string reaches the screen, always — after the bell too,
/// because the debrief already has the harm sentence, the harm list and the mark sheet, and a
/// verdict stapled to an order line adds nothing there that is not said better elsewhere.
///
/// **Display only.** The harm classification is a property of the intervention in the scenario
/// file and is untouched: the tape keeps the id, `harm_events` keeps the sentence, the rubric
/// keeps its `no_harm` checks and the leaf hashes the same bytes it always did. A run charted
/// through this function and one charted without it anchor identically.
fn neutral_label(label: &str) -> &str {
    let mut s = label.trim_end();
    // A loop, not a single strip: an author who writes `(HARM) (HARM here)` gets both taken off
    // rather than one, and the result is checked by the test that reads every label off the disk.
    loop {
        let Some(open) = s.rfind('(') else { return s };
        let Some(inner) = s.strip_suffix(')').map(|t| &t[open + 1..]) else { return s };
        if !inner.trim_start().to_ascii_uppercase().starts_with(VERDICT) {
            return s;
        }
        let next = s[..open].trim_end();
        // Never strip a label down to nothing. A label that is *only* a verdict has no neutral
        // form, so the caller falls through to what the player typed rather than to an empty
        // chart line — and the test below fails so the author is told.
        if next.is_empty() {
            return s;
        }
        s = next;
    }
}

impl Session {
    /// Simulated seconds this run has been going. Summed off the tape, never held as a field:
    /// the tape is the truth and everything else is what the tape computes.
    fn elapsed(&self) -> f64 {
        self.tape
            .iter()
            .map(|s| match s {
                Step::Tick(dt) => *dt,
                Step::Do(_) | Step::Act { .. } | Step::Ask(_) | Step::Set(..) | Step::Off(_)
                | Step::Shock(_) => 0.0,
            })
            .sum()
    }

    /// How long this station gives the candidate, in simulated seconds. See [`RUNTIME_MINUTES`].
    fn limit_sec(&self) -> f64 {
        runtime_sec(&self.ep)
    }

    /// **Is this run over?** The one predicate, and the only thing anything may ask.
    ///
    /// It used to be `state.outcome().is_none()`, written out four times — in [`Session::sealed`],
    /// in `/api/marks`, in `/api/debrief` and in `/api/anchor` — and it was wrong in the same way
    /// in all four: it asked whether *the patient* had reached a terminal state, when the question
    /// is whether *the encounter* is finished. Those are not the same question, and two of the
    /// twelve stations are the proof. `osce-b2` and `osce-c` declare no ending a candidate can
    /// reach by standing still, so the patient never reached a terminal state, so the run was
    /// never over, so the mark sheet never opened. The candidate sat in a sealed room for ever.
    ///
    /// A run is over when the patient reached one of the case's own endings, **or** when the
    /// clock the station advertises has run out. Both are read off the tape, so a run rebuilt
    /// from disk and a run still in memory answer identically, and so does a verifier holding
    /// nothing but the tape.
    ///
    /// The second arm is only ever observed *after* [`Session::ring_the_bell`] has run the
    /// encounter forward, because the crossing and the ringing happen inside the same request.
    /// Nothing reads this and finds a run whose patient was left mid-slide.
    fn over(&self) -> bool {
        self.state.outcome().is_some() || self.elapsed() >= self.limit_sec()
    }

    /// **The ending.** Stop taking input, and run the encounter on to its conclusion.
    ///
    /// The whole design of the finish control is in the two words *run on*. A finish that froze
    /// the clock and scored the current state would be a cheat code: a candidate watching a
    /// patient slide toward arrest presses it one second before the arrest, dodges
    /// `vitals_osce::death_cap` and banks a pass on a patient their management was killing.
    /// `osce-d4` is the worked example — a run that treats the sepsis but never starts the
    /// pressor scores 29 of 40 and arrests at sixteen simulated minutes, and the cap takes it to
    /// 27, which is a fail. Frozen at fourteen minutes it would have been a pass.
    ///
    /// So the bell does not stop the patient. It appends ordinary [`Step::Tick`]s — the same 2 s
    /// the live loop sends at a station — until the case reaches one of its own endings, or until
    /// nothing about her is going to change again ([`vitals_replay::bell`] for what bounds it).
    /// The tape it leaves behind is byte-identical to the tape of a candidate who stood at the
    /// bedside and did nothing until the same moment, which is exactly what finishing early is.
    /// Nothing records that the button was pressed: that is a fact about the candidate, and
    /// putting it on the tape would make an early finish score differently from a late one.
    ///
    /// The state and the beats are rebuilt from the tape afterwards rather than carried forward
    /// from the live machine, so the run on screen and the run a verifier recomputes cannot part
    /// company here of all places.
    fn ring_the_bell(&mut self) -> Result<(), String> {
        let until = self.limit_sec();
        let (added, _, _) = vitals_replay::ring(&self.sce_json, &mut self.state, until)?;
        if added.is_empty() {
            return Ok(());
        }
        self.tape.extend(added);
        let (state, r) = resume(&self.sce_json, &self.tape)?;
        self.state = state;
        self.beats = r.beats;
        Ok(())
    }

    /// Is this run sealed *right now*? The one definition, asked by everything that withholds.
    ///
    /// [`Session::view`] asks it before serialising the harm list, the feed and the chart.
    /// `/device/vitals` asks it before handing a device pane any words that interpret a reading.
    /// Two callers and one predicate on purpose: a second copy of "is this sealed" is a second
    /// answer waiting to disagree with the first, and this repo has already paid for that once —
    /// the page's `examMode()` and the server's `exam_mode` disagreed about every station until
    /// the station table was folded into the condition below.
    ///
    /// An exam by declaration (`exam_mode`, which is set from a landed chain commitment and
    /// nowhere else) or an exam by definition (a member of a station set, true even on a bay with
    /// no chain configured) — and only while the clock is still running. [`Session::over`] is the
    /// bell, and the bell is where sealing stops rather than where it starts: the mark sheet and
    /// the debrief are what an unlimited-retry model is for.
    ///
    /// It used to read `outcome.is_none()` here, which sealed two stations for ever: `osce-b2`
    /// and `osce-c` reach no terminal outcome a candidate can get to by standing still, so the
    /// condition never went false and the sheet never opened.
    fn sealed(&self) -> bool {
        (self.exam_mode || set_member(&self.ep).is_some()) && !self.over()
    }

    /// A full snapshot of the run, in the language the page asked for.
    ///
    /// `lang` reaches exactly one field ([`View::tr`]) and nothing else. Every number, every id,
    /// every beat and the leaf itself are computed before it is consulted and are identical
    /// whichever language is passed — which is the property `a_language_never_reaches_the_leaf`
    /// pins, and the reason a Thai run and an English run of the same case can be compared at all.
    fn view(&self, lang: &lang::Language) -> View {
        let v = self.state.vitals;
        let elapsed = self.elapsed();
        let over = self.over();
        let outcome = self.state.outcome().map(|o| format!("{o:?}"));
        // Derived by replaying the tape, not by reading the live state. Assembling a Replay by
        // hand here would show the player a leaf computed one way while the verifier computes it
        // another, and a leaf that depends on which side of the wire you stand on proves nothing.
        // Gated on the run being over rather than on the patient having died or gone home: a
        // station where time was called has a tape, a replay and a leaf like any other, and
        // `leaf()` has always had a spelling for a run with no terminal (`outcome:-`).
        let leaf_hex = over.then(|| {
            let r = replay(&self.sce_json, &self.tape).ok()?;
            Some(hex(&leaf(&sce_hash(&self.sce_json), &self.tape, &r)))
        }).flatten();
        // Supplemental oxygen is worth points of its own: holding 96% on a mask is not the same
        // patient as holding 96% on air, and the score is built to say so.
        let on_oxygen = self.state.has_equipment("o2") || self.state.has_equipment("ett");
        let obs = news2::Obs {
            // Who she is, not what she is doing — and the only thing on this line that decides
            // whether the rest of it may be scored at all. `osce-b3` is three; the adult table
            // charged her 3 for a respiration rate that is normal for three, 2 for a pulse that
            // is normal for three, and 2 for a systolic that is normal for three, and printed
            // "7 · HIGH RISK · emergency response" beside a banner reading "Stable".
            // Hers if the ward placed somebody here, and the case's own only where nobody has
            // been placed at all — the season's cases, which is every caller this had before
            // Vitals World existed. `news2::age_for` is the rule; this is its one call site.
            age_years: news2::age_for(self.ward.as_ref().map(|w| w.age), patient_age(&self.ep)),
            rr: v.rr, spo2: v.spo2, on_oxygen, sbp: v.sbp, hr: v.hr, temp: v.temp, gcs: v.gcs,
        };
        let n = news2::score(&obs);
        // ── the seal, where a seal has to be ────────────────────────────────────
        // It was CSS. `view()` did not read `exam_mode` at all, so every tick of every station
        // shipped the harm sentence in full — "the tongue depressor goes in — she screams, and
        // the stridor doubles" — three times over, in `harm`, in `beats` and in `chart`. The
        // page then greyed one copy of it out. A Network tab reads all three, and a station
        // whose whole lesson is *do not put the depressor in* was telling the candidate what
        // the depressor did, mid-run, in text.
        //
        // The three now part company, because they are three different promises. `harm` — the
        // result panel's list — is emptied. `beats` keeps one line per harm, redacted to
        // [`HARM_SEALED`], because the feed is a live transcript and `unsealHarm()` rewrites
        // those lines from position at the bell. `chart` carries **no harm row at all**: a
        // redacted row on a timestamped record is the timing handed over with the sentence
        // withheld, and the timing is the answer. See [`HARM`].
        //
        // So the withholding happens here, before the bytes exist. It lasts exactly as long as
        // the clock: `outcome.is_none()` is the entire condition, and the moment the bell rings
        // the same call returns every sentence in full, because the mark sheet and the debrief
        // are what an unlimited-retry model is *for*. Practice is never sealed — a practice run
        // is a lesson, and a coach who will not say what went wrong is not coaching.
        //
        // The tape, the replay, the harm list the leaf hashes and the rubric's own `no_harm`
        // checks are all untouched: this is the last step before serialisation, and nothing
        // downstream of a leaf can read it. A run played sealed and a run played unsealed
        // anchor byte for byte identically.
        //
        // "Is this an exam" is asked of the station table, not only of `exam_mode`. `exam_mode`
        // is set from a *landed chain commitment* and nowhere else, so on a bay with no chain
        // configured it is false for every run ever played — and the twelve stations would have
        // gone on shipping the sentence in full on exactly the deployment a visitor reaches
        // first. A station is an exam by definition; that is already the page's own rule
        // (`examMode()` is true for anything with `station` set), and this is the server
        // finally agreeing with it rather than trusting it.
        let sealed = self.sealed();
        // The engine's own word for how she is. Bound rather than inlined because the face at the
        // bedside is chosen from it, and two spellings of one word is how a picture comes to
        // disagree with the line printed beside it.
        let status = format!("{:?}", self.state.status);
        // ── the chart says what was ordered, not what the rubric calls it ───────
        // The engine records an order by intervention id, because an id is what replay and the
        // rubric need. The chart then printed that id: `adrenaline_undosed`, `dx_epiglottitis`,
        // `exam_throat`. Those are the mark sheet's own needles, and they say out loud both what
        // the sheet is looking for and — in `_undosed`, `dx_` — the shape of the mistake it is
        // waiting to catch. So the id is translated on the way out: the case author's label
        // first, and failing that the player's own words off the tape.
        //
        // The tape and the events keep the id. Nothing here is read by replay, the leaf or the
        // scorer; this is the last step before the screen.
        let said: std::collections::HashMap<&str, &str> = self
            .tape
            .iter()
            .filter_map(|s| match s {
                Step::Act { text, id } => Some((id.as_str(), text.as_str())),
                _ => None,
            })
            .collect();
        // ── the feed, on the same rule as the chart ─────────────────────────────
        // A harm beat used to survive the seal as `harm:sealed`, and the feed printed it as
        // "⚠ harm recorded" the second the candidate acted. That is a verdict, delivered on
        // screen, mid-station — the more visible of the two halves of this leak, because the
        // chart's needed a Network tab and this one did not. In a real OSCE the examiner does
        // not lean over and say that. The honest signal is the one the body gives: she gets
        // worse, the numbers move, and reading that is the skill being examined.
        //
        // So a sealed feed carries no harm line at all. **The sealed list is exactly the full
        // list with the `harm:` entries removed, in order** — a subsequence, never a
        // resequencing — which is the invariant `unsealHarm()` reconstructs the transcript
        // from at the bell. Nothing here may reorder, renumber or pad it.
        //
        // Deliberately not an index or an id per beat. Either would have to be the position in
        // the *full* list for the page to key on it, and then the gaps in the sequence a sealed
        // reply carries would spell out where the harms were — the same leak in a form that
        // takes one subtraction to read.
        let beats: Vec<String> = if sealed {
            self.beats.iter().filter(|b| !b.starts_with(HARM_BEAT)).cloned().collect()
        } else {
            self.beats.clone()
        };

        // What a monitor could actually read off her, which is not the same thing as what the
        // model holds. NEWS2 above is deliberately computed from the raw vector: a score is a
        // clinical judgement about the patient, while this is a screen reporting its instruments.
        let m = reading::Reading::of(&v).rounded();
        View {
            scenario: self.scenario.clone(),
            hr: m.hr,
            sbp: m.sbp,
            dbp: m.dbp,
            spo2: m.spo2,
            rr: m.rr,
            temp: m.temp,
            gcs: m.gcs,
            pulse: m.pulse,
            rhythm: m.rhythm,
            shockable: m.shockable,
            status: status.clone(),
            // One word, two uses: what the rail prints and which face to draw. The ladder's keys
            // are the engine's words in lower case — `portrait_for` then answers with hers, or
            // with the nearest milder one, or with nothing at all if no picture of her exists yet.
            portrait: self
                .ward
                .as_ref()
                .and_then(|w| ward::portrait_at_the_bedside(&w.faces, &status.to_lowercase()))
                .map(str::to_string),
            // Read off the sealed copy, not the live one: a translation of a withheld sentence
            // is the withheld sentence.
            tr: beat_lines(lang, &beats),
            beats,
            films: self.films.clone(),
            // The list the result panel prints as "Harm on the record". Empty while the case is
            // running under exam; whole from the bell onwards.
            harm: if sealed { Vec::new() } else { self.state.harm_events.clone() },
            outcome,
            over,
            elapsed,
            limit: self.limit_sec(),
            leaf: leaf_hex,
            sce_hash: hex(&sce_hash(&self.sce_json)),
            equipment: self
                .state
                .equipment()
                .iter()
                .map(|e| Kit { id: e.id.clone(), setting: e.setting, since: e.since_sec })
                .collect(),
            chart: self
                .state
                .events()
                .iter()
                // ── the row goes, not just the sentence ────────────────────────────
                // The seal used to redact the harm line and keep it, on the reasoning that
                // *something went wrong, at this second* is a fact the monitor is showing
                // anyway. It is not: the monitor shows a patient getting worse, and it does not
                // stamp that on the same second as one named order and call it HARM. The kept
                // row did, one line under the order that caused it, which is the answer the
                // sentence was being withheld to protect. See [`HARM`].
                .filter(|e| !(sealed && e.kind == HARM))
                .map(|e| Note {
                    t: e.t_sec,
                    kind: e.kind.clone(),
                    text: if self.state.is_intervention(&e.text) {
                        // An order, recorded by id. Never the id itself: the case's own label,
                        // or what the player typed to reach it, and only then — for a case that
                        // named nothing and an order nobody typed — the id, which by then is the
                        // only word anyone has for it.
                        //
                        // The author's label goes through `neutral_label` first. It is written
                        // for the author's own eye and half of them carry the verdict in the
                        // name — `Look in the throat (HARM)` — so printing it straight told the
                        // candidate they had just made the mistake, on the order line, one line
                        // above the harm sentence the seal had gone to some trouble to withhold.
                        self.state
                            .intervention_label(&e.text)
                            .map(|l| neutral_label(l).to_string())
                            .or_else(|| said.get(e.text.as_str()).map(|t| t.to_string()))
                            .unwrap_or_else(|| e.text.clone())
                    } else {
                        // Already a line rather than an id — the defibrillator writes its own.
                        e.text.clone()
                    },
                })
                .collect(),
            // Absent only for the dead — see [`View::news`]. A living patient always gets the
            // panel, because a panel that vanishes is a panel a reader fills in for themselves.
            news: (self.state.status != vitals_sce::PatientStatus::Dead).then(|| match n {
                Some(n) => News {
                    applies: true,
                    total: Some(n.total),
                    worst: Some(n.worst),
                    band: n.band.as_str(),
                    response: n.band.response(),
                },
                // A child. No score, and a sentence saying which instrument is missing rather
                // than a blank that reads as reassurance.
                None => News {
                    applies: false,
                    total: None,
                    worst: None,
                    band: "none",
                    response: news2::NOT_VALIDATED,
                },
            }),
        }
    }
}

/// Where the scenarios and the story files live.
///
/// CARGO_MANIFEST_DIR is baked in at build time and names the machine that compiled this. In a
/// container that path does not exist, so everything read at runtime goes through here.
fn scenario_root() -> std::path::PathBuf {
    match std::env::var("VITALS_SCENARIOS") {
        Ok(d) => std::path::PathBuf::from(d),
        Err(_) => std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
    }
}

/// One declared member of a station set — which Embla case it was (or will be) converted from,
/// and how the clinic card introduces it. A member is *declared* here even before its files
/// exist: the shelf shows a coming-soon card for it, and playability is a fact about the disk
/// ([`member_playable`]), never a second list to keep in sync. Display fields only — the
/// scenario file, whose hash is the case's identity on chain, is never touched from here.
struct SetMember {
    /// The station id — the `ep` the whole engine already routes on.
    id: &'static str,
    /// The embla-cases id this member is converted from. Provenance, worn on the card.
    case: &'static str,
    /// The clinic display title — **the OSCE stem, never the answer**. A station is a mark
    /// sheet with a rubric item worth 2–4 points for naming the diagnosis, and this string is
    /// on the shelf card, on the title card, and in the player bar for the whole eight to
    /// fourteen minutes of the exam. "Pericarditis — acute chest pain" therefore paid the
    /// candidate before the clock started. What a real circuit puts on the door is the
    /// presentation: age, sex, complaint, the one thing visible from the doorway. Rule for
    /// anything written here: **no disease and no treatment may appear in it.** The diagnosis
    /// is revealed on the debrief instead, where the exam is already over ([`REVEAL`] in
    /// index.html, beside the provenance line).
    title: &'static str,
    /// The Eir specialty. Kept for the record and for the debrief line — it is deliberately
    /// **not** what the shelf card wears any more; see [`SetMember::band`].
    specialty: &'static str,
    /// The OSCE circuit band the card shows instead of the organ specialty. "eir-gastroenterology"
    /// over an epigastric-pain-and-shock stem answers the station's own trap (bleed, not ACS)
    /// before the candidate touches the patient, and "eir-pulmonology" over a clear-chested
    /// hypoxia does the same to the masquerader. A real circuit's door says *Medicine* or
    /// *Paediatrics*; that is the widest label that still tells a player what kind of station
    /// they are picking, and it names no organ the rubric marks.
    band: &'static str,
    /// The tier the case plays at. [`difficulty`] reads this for every member, so adding a
    /// Phase-5b member here is the whole server arm: files land, the member goes live.
    tier: Difficulty,
}

/// A film a station reveals when a specific order is recognised.
///
/// Keyed by the station id ([`Session::ep`]) and the intervention id the matcher resolved, which
/// is why no scenario file is touched to add one: a `.sce.json`'s sha256 is the case's identity
/// on chain, bound in the commitment and carried in the leaf. Everything here hangs off the id
/// the matcher already produces, so every phrasing that reaches the intervention — "chest x-ray",
/// "cxr", "chest film" — reaches the picture too, for free.
///
/// **Presentation only.** A film never enters `Session::tape`, never reaches `replay`, and never
/// touches `leaf` or `sce_hash`. A verifier replaying a tape on a build with no images at all
/// must reach the identical leaf, so nothing below may become an input to one.
#[derive(Serialize, Clone, Copy)]
struct Film {
    station: &'static str,
    intervention: &'static str,
    /// Key under `/img/cases/`, extension included — see [`CASE_IMG`].
    file: &'static str,
    /// What the report says. The case's own words: the picture is the evidence, the caption is
    /// the read.
    caption: &'static str,
    /// Which bank it came from, so the credit under the film names the right licence.
    credit: Credit,
}

/// Whose licence the film is under. Two banks, two obligations — see
/// `static/img/cases/ATTRIBUTION.md`, which is the authority and whose wording is copied rather
/// than reinvented.
#[derive(Serialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Credit {
    /// PTB-XL (PhysioNet). **CC-BY 4.0 — attribution is a licence condition, not a courtesy**:
    /// creator, licence notice, its URI, and a statement that the material was modified. Ours is
    /// modified twice over (rendered as a teaching plot, then palette-quantised for the web), so
    /// the modification notice is not optional. The page carries all four.
    Ptbxl,
    /// NIH ChestX-ray14. No attribution condition, credited anyway — a teaching product that
    /// hides where its films came from has no business asking learners to trust them.
    Nih,
}

/// The films, keyed `(station, intervention)`. A station with no entry behaves exactly as it did
/// before this table existed.
///
/// 🛑 **Two entries are held out pending a clinician's sign-off** and are commented, not coded —
/// see the CLINICAL HOLD in `docs/internal/CASE_MEDIA_WIRING.md`. They are also absent from
/// [`CASE_IMG`], so the bytes are not in the binary and the route cannot serve them even to
/// somebody who guesses the filename. Wiring either one is a content decision, not a code one.
const FILMS: &[Film] = &[
    Film { station: "osce-a", intervention: "ecg", file: "ecg-sinus-tachycardia-04408.png",
           caption: "Sinus tachycardia, rate 118 — no ischaemic changes.", credit: Credit::Ptbxl },
    Film { station: "osce-a", intervention: "cxr", file: "cxr-normal-1.png",
           caption: "Normal heart size, clear lung fields, no effusion or pneumothorax.", credit: Credit::Nih },
    Film { station: "osce-a2", intervention: "ecg", file: "ecg-sinus-tachycardia-04408.png",
           caption: "Sinus tachycardia 124 bpm — no ST changes.", credit: Credit::Ptbxl },
    // HOLD: awaiting clinical sign-off — osce-b / ecg / ecg-st-elevation-anterior-01278.png.
    // The bank's own ecg-mapping.tsv flags it "KOL pick leads+verify acute": the PTB-XL SCP
    // class is ASMI/AMI, which can be an *old* anterior infarct rather than an acute STEMI, and
    // this station teaches reperfusion inside ten minutes. Only an acute trace will do here.
    Film { station: "osce-b2", intervention: "cxr", file: "cxr-normal-3.png",
           caption: "Normal heart size, clear lung fields — no effusion.", credit: Credit::Nih },
    // "Neck and chest films" orders two and we hold one: the source case carries the chest film
    // as an image and the neck film as text, and both scenarios already put the steeple sign in
    // a beat. So the caption says chest — a caption promising a neck film the picture is not
    // would read as a bug.
    Film { station: "osce-b3", intervention: "xray_neck", file: "cxr-normal-3.png",
           caption: "Chest film — normal heart size, clear lung fields.", credit: Credit::Nih },
    Film { station: "osce-c", intervention: "xray_neck", file: "cxr-normal-3.png",
           caption: "Chest film — normal heart size, clear lung fields.", credit: Credit::Nih },
    Film { station: "osce-c2", intervention: "cxr", file: "cxr-normal-1.png",
           caption: "Normal heart size, clear lung fields — no pneumothorax.", credit: Credit::Nih },
    // HOLD: awaiting clinical sign-off — osce-c3 / cxr / cxr-consolidation-pneumonia-1.png.
    // ChestX-ray14's Pneumonia label is NLP-mined from reports and there is no KOL-reviewed CXR
    // mapping in the bank. On inspection the film does not show the wedge of consolidation this
    // station's beat describes, and a learner shown a normal-looking film and told it is
    // pneumonia has been taught something false.
    Film { station: "osce-d2", intervention: "cxr", file: "cxr-normal-4.png",
           caption: "Normal heart size, clear lung fields — a chest this clear does not explain the hypoxia.",
           credit: Credit::Nih },
];

fn film_for(station: &str, intervention: &str) -> Option<&'static Film> {
    (!intervention.is_empty())
        .then(|| FILMS.iter().find(|f| f.station == station && f.intervention == intervention))
        .flatten()
}

/// Every film a tape has already earned, in the order it was ordered.
///
/// Derived, never stored: the resolved intervention id is already on the tape beside the words,
/// so a resumed run re-reads its films from the same bytes the leaf is computed from without
/// films ever becoming an input to that leaf.
fn films_from_tape(station: &str, tape: &[Step]) -> Vec<&'static Film> {
    let mut out: Vec<&'static Film> = Vec::new();
    for s in tape {
        if let Step::Act { id, .. } = s {
            if let Some(f) = film_for(station, id) {
                if !out.iter().any(|x| x.file == f.file) {
                    out.push(f);
                }
            }
        }
    }
    out
}

/// ── the language a beat is read in ──────────────────────────────────────────
///
/// The same idea as [`FILMS`], one shelf along: a table hanging off a key the engine already
/// produces, consulted on the way to the screen and nowhere else. A film hangs off the resolved
/// intervention id; a translated beat hangs off the canonical beat string that
/// `vitals_sce::render_beat` emits and the leaf hashes.
///
/// **Presentation only, and by the same argument.** A verifier replaying this tape on a build
/// that has never heard of Thai must reach the identical leaf, so the table below is read *from*
/// `beats` and never written *to* it. The table itself lives in [`lang`], because a language is a
/// list of strings a translator edits and not something a web server should have opinions about.
///
/// `None` for the default language and for a run whose beats have no rows yet — the field is
/// skipped on the wire and the page shows the original, which is what a case with no translation
/// is supposed to look like.
fn beat_lines(
    l: &lang::Language,
    beats: &[String],
) -> Option<std::collections::BTreeMap<String, &'static str>> {
    let m: std::collections::BTreeMap<String, &'static str> = beats
        .iter()
        .filter_map(|b| lang::beat(l, b).map(|t| (b.clone(), t)))
        .collect();
    (!m.is_empty()).then_some(m)
}

/// The clinical images, compiled in the way [`STILLS`] is, keyed by their path under
/// `/img/cases/`. Content-Type comes from this table rather than from a suffix trim, because the
/// directory mixes PNG and JPEG.
///
/// The two files under CLINICAL HOLD are deliberately absent: not compiled in, not serveable,
/// not guessable. That is the difference between "we did not link it" and "it is not there".
const CASE_IMG: &[(&str, &[u8], &str)] = &[
    ("ecg-sinus-tachycardia-04408.png",
     include_bytes!("../static/img/cases/ecg-sinus-tachycardia-04408.png"), "image/png"),
    ("cxr-normal-1.png", include_bytes!("../static/img/cases/cxr-normal-1.png"), "image/png"),
    ("cxr-normal-3.png", include_bytes!("../static/img/cases/cxr-normal-3.png"), "image/png"),
    ("cxr-normal-4.png", include_bytes!("../static/img/cases/cxr-normal-4.png"), "image/png"),
];

/// Which intervention an order names — the scenario first, then the language layer.
///
/// Two readers, in a fixed order, and the order is the whole safety argument:
///
///   1. **The scenario's own matcher.** Its answer is final. Every keyword a case author wrote,
///      in whatever language they wrote it in, decides what happens on their own case.
///   2. **Only if that declined**, [`lang::canonical_order`] offers the English order a
///      non-English phrase names — and the same matcher rules on *that*. So a translation can
///      add recognition and can never redirect, shadow or override an order a case already
///      understood, and a station with no such intervention still does nothing, exactly as today.
///
/// The empty string means nobody understood it, which is a real answer and goes on the tape as
/// one: replay must stay faithful to a run in which nothing happened.
fn resolve_order(st: &SceState, act: &str) -> String {
    st.resolve(act)
        .or_else(|| lang::canonical_order(act).and_then(|en| st.resolve(en)))
        .unwrap_or_default()
}

/// ── the patient stills a station is shot in ──────────────────────────────────
///
/// EP1 has a frame of its own patient for every state the automaton can put her in, and the
/// bay swaps it as she goes down. A station had nothing of the sort: the biggest panel on the
/// screen carried the stem and then whatever film was ordered, and the patient herself was a
/// name in a line of text. These are the same thing EP1 has, for the stations — one still per
/// state, shot by the same Embla pipeline.
///
/// The four states are the four a station's still is worth shooting for. The automaton reports
/// three more (`improving`, `recovered`, `dead`) and the page folds those onto their neighbours
/// rather than asking the art team for seven shots per station — see `STATIONSTATE` in
/// index.html.
///
/// **These are the one media surface in the build that is read off the disk rather than
/// compiled in.** That is deliberate and it is the whole point: the files are being produced
/// now, and the wiring had to be finished without them. Drop `osce-a_critical.jpg` into the
/// directory, restart, and the station has it — no rebuild, no table to edit, no second list
/// to keep in sync with the disk. It is the same arrangement `/clip/` has had since EP1, and
/// the same rule holds: what is not there is not served, and the page has a stem to fall back
/// to (`renderStage` in index.html), never a black frame.
const STATION_STATES: &[&str] = &["stable", "deteriorating", "critical", "arrest"];

/// Where those files live. `VITALS_STATION_STILLS` in a container (the Dockerfile sets it);
/// the checkout's own `static/` tree in development, which is where the art team commits them.
fn station_stills_dir() -> std::path::PathBuf {
    std::env::var("VITALS_STATION_STILLS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static/img/cases/states")
        })
}

/// The one path a patient still is ever read from — and the only place the name is spelled.
///
/// Both halves are whitelisted against tables the binary owns: `station` has to be a declared
/// set member and `state` one of [`STATION_STATES`], so the filename this composes comes from a
/// finite set of forty-eight strings. A request cannot name a file of its own choosing here,
/// which is what keeps a disk-read route as narrow as the compiled ones beside it — no
/// traversal, and nothing in the images directory above reachable through it either.
fn station_still_path(station: &str, state: &str) -> Option<std::path::PathBuf> {
    if set_member(station).is_none() || !STATION_STATES.contains(&state) {
        return None;
    }
    let f = station_stills_dir().join(format!("{station}_{state}.jpg"));
    f.is_file().then_some(f)
}

/// Which states this station has a still for, on this disk, today.
///
/// Sent with the set table so the page never asks for a picture that is not there: a broken
/// `<img>` in the biggest panel of the bay is the black frame the stem exists to prevent, and
/// the server already knows the answer. Playability is read the same way ([`member_playable`])
/// — a fact about the disk, never a second list.
fn station_states(station: &str) -> Vec<&'static str> {
    STATION_STATES
        .iter()
        .copied()
        .filter(|st| station_still_path(station, st).is_some())
        .collect()
}

/// The station sets (DECISIONS.md "Station Sets", 27 ส.ค.) — the one copy, server side.
/// From EP2 on, an episode door is opened by **its own set's stars and nothing else's**:
/// each member is worth 0–3 stars (best det ≥70% → 1, ≥85% → 2, ≥95% → 3), and farming
/// another gate's stations buys nothing here.
///
/// The needs below are the three-star repricing (27 ส.ค., supersedes 2/4/5/7): against a
/// ceiling of `members × 3` they hold the same climb the two-star prices drew — 50% of the
/// set, then 67%, 78%, 83%. What the third star changes is *how* the late doors are paid for:
/// gate2 and gate3 are still reachable on passes and excellences alone, while gate4 cannot be
/// opened without one flawless run and gate5 without two. That is the escalation, said in
/// stars — and it is bounded, because a door that needed every member flawless would be a
/// door one unlucky rubric item keeps shut.
struct StationSet {
    gate: &'static str,
    /// The episode this gate opens.
    opens: &'static str,
    /// Stars required once the full roster is published. While the set is short, the live
    /// need is capped at what the published members can yield — see [`resolve_sets`].
    need: u32,
    members: &'static [SetMember],
}

const SETS: &[StationSet] = &[
    // gate2 · 2 cases · ceiling 6 · need 3 (50%)
    StationSet { gate: "gate2", opens: "ep2", need: 3, members: &[
        SetMember { id: "osce-a",  case: "ddx-anaphylaxis-1", title: "Rash and facial swelling after a meal — M 71", specialty: "eir-emergency", band: "emergency", tier: Difficulty::Student },
        SetMember { id: "osce-a2", case: "ddx-anaphylaxis-2", title: "Belly cramps, loose stools, swollen face — F 68", specialty: "eir-emergency", band: "emergency", tier: Difficulty::Student },
    ]},
    // gate3 · 3 cases · ceiling 9 · need 6 (67%)
    StationSet { gate: "gate3", opens: "ep3", need: 6, members: &[
        SetMember { id: "osce-b",  case: "ddx-possible-nstemi-stemi-2", title: "Chest pain — M 25", specialty: "eir-cardio", band: "emergency", tier: Difficulty::Intern },
        SetMember { id: "osce-b2", case: "ddx-pericarditis-1", title: "Chest pain — M 14", specialty: "eir-cardio", band: "emergency", tier: Difficulty::Intern },
        SetMember { id: "osce-b3", case: "ddx-croup-1", title: "Barking cough — F 3", specialty: "eir-ent", band: "paediatrics", tier: Difficulty::Intern },
    ]},
    // gate4 · 3 cases · ceiling 9 · need 7 (78%)
    StationSet { gate: "gate4", opens: "ep4", need: 7, members: &[
        SetMember { id: "osce-c",  case: "ddx-croup-2", title: "Barking cough and drooling, worse at night — F 6", specialty: "eir-ent", band: "paediatrics", tier: Difficulty::Resident },
        SetMember { id: "osce-c2", case: "ddx-bronchospasm-acute-asthma-exacerbation-2", title: "Wheeze and breathlessness — F 53", specialty: "eir-pulmonology", band: "emergency", tier: Difficulty::Intern },
        SetMember { id: "osce-c3", case: "ddx-pneumonia-2", title: "A week of cough — F 25", specialty: "eir-pulmonology", band: "emergency", tier: Difficulty::Intern },
    ]},
    // gate5 · 4 cases · ceiling 12 · need 10 (83%)
    StationSet { gate: "gate5", opens: "ep5", need: 10, members: &[
        SetMember { id: "osce-d",  case: "embla-upper-gastrointestinal-bleeding-intern", title: "Vomited blood — M 62", specialty: "eir-gastroenterology", band: "emergency", tier: Difficulty::Intern },
        SetMember { id: "osce-d2", case: "ddx-pulmonary-embolism-2", title: "Sudden breathlessness, clear chest — F 55", specialty: "eir-pulmonology", band: "emergency", tier: Difficulty::Resident },
        SetMember { id: "osce-d3", case: "ddx-p-anaphylaxis-1", title: "Wheals, swollen lips and a wheeze — F 6", specialty: "eir-emergency", band: "paediatrics", tier: Difficulty::Intern },
        SetMember { id: "osce-d4", case: "embla-septic-shock-with-multi-organ-failure-resident", title: "Fever, shaking, pressure of 80 — F 72", specialty: "eir-emergency", band: "emergency", tier: Difficulty::Resident },
    ]},
];

fn set_member(id: &str) -> Option<&'static SetMember> {
    SETS.iter().flat_map(|s| s.members.iter()).find(|m| m.id == id)
}

/// A member is playable when both halves of its identity exist on disk: the scenario (whose
/// hash is the case on chain) and the rubric (without which an exam cannot be scored). A
/// declared member without files is "coming soon" — a card on the shelf, never an error.
fn member_playable(id: &str) -> bool {
    scenario_path(id).exists() && rubric_path(id).is_some()
}

/// A set as it stands on this disk today: which members are live, their on-chain case hashes,
/// and the door's live price. While the roster is short the need is capped at what the
/// published members can actually yield (`STAR_TIERS` each) — **a gate must never be
/// impossible**, only cheaper until the full set ships and the cap stops binding.
struct SetState {
    set: &'static StationSet,
    need_now: u32,
    /// Each declared member, with its scenario hash when playable — the same hash the
    /// commitment binds and the leaf carries, so /api/stars can translate proven attempts
    /// back into set members without a per-request file read.
    members: Vec<(&'static SetMember, Option<[u8; 32]>)>,
}

impl SetState {
    /// The most this set can be worth today — what its *playable* members can yield. The
    /// shelf shows progress against this ("6 / 9 ⭐"), so it must count what can actually be
    /// earned rather than what is declared, or a coming-soon card would read as stars a
    /// player is failing to collect.
    fn ceiling(&self) -> u32 {
        self.members.iter().filter(|(_, h)| h.is_some()).count() as u32 * vitals_progress::STAR_TIERS
    }
}

fn resolve_sets() -> Vec<SetState> {
    SETS.iter()
        .map(|s| {
            let members: Vec<_> = s
                .members
                .iter()
                .map(|m| {
                    let h = member_playable(m.id)
                        .then(|| std::fs::read_to_string(scenario_path(m.id)).ok().map(|j| sce_hash(&j)))
                        .flatten();
                    (m, h)
                })
                .collect();
            let playable = members.iter().filter(|(_, h)| h.is_some()).count() as u32;
            SetState { set: s, need_now: s.need.min(playable * vitals_progress::STAR_TIERS), members }
        })
        .collect()
}

const fn tier_str(d: Difficulty) -> &'static str {
    match d {
        Difficulty::Student => "student",
        Difficulty::Intern => "intern",
        Difficulty::Resident => "resident",
    }
}

fn scenario_path(id: &str) -> std::path::PathBuf {
    let root = scenario_root();
    match id {
        "ep2" => root.join("demo/scenarios/ep2-stemi.json"),
        "ep3" => root.join("demo/scenarios/ep3-epiglottitis.json"),
        "ep4" => root.join("demo/scenarios/ep4-pulmonary-embolism.json"),
        "ep5" => root.join("demo/scenarios/ep5-the-night-the-stars-fell.json"),
        // Any declared set member — the four stations today, their Phase-5b siblings the day
        // their files land — lives under demo/stations by its own id (see the *.sce.json
        // headers for provenance). A declared-only member resolves to a path that does not
        // exist yet, which is exactly what "coming soon" looks like on disk — never the EP1
        // fallback, because playing EP1 under a station's name would anchor the wrong case.
        m if set_member(m).is_some() => root.join("demo/stations").join(format!("{id}.sce.json")),
        _ => root.join("conformance/sce-anaphylaxis-ep1.json"),
    }
}

/// Where the archive of past scenario versions lives on this deployment.
///
/// Under the scenario root, so it moves with `VITALS_SCENARIOS` exactly as the cases do — in the
/// image that is `/app/conformance/sce-archive`, put there by the same `COPY` that ships the
/// conformance vectors. See [`archive`] for why it must not live under `docs/`.
fn sce_archive_dir() -> std::path::PathBuf {
    scenario_root().join(archive::DIR)
}

/// The scenario files this server is playing right now, in shelf order.
///
/// `/api/sce`'s **deny list**, not its second lookup. A file on this list is a mark sheet a
/// candidate can still be marked against, so its hash is refused however many copies of it the
/// archive holds. See [`archive`] for what that costs a verifier and what they do instead.
fn live_scenarios() -> Vec<std::path::PathBuf> {
    every_case().into_iter().map(scenario_path).collect()
}

/// Every case this server can be asked to play, in shelf order: the five episodes and the twelve
/// stations. One list, so "which cases have a voice" is answerable without guessing at ids.
fn every_case() -> Vec<&'static str> {
    let mut v = vec!["ep1", "ep2", "ep3", "ep4", "ep5"];
    v.extend(SETS.iter().flat_map(|s| s.members.iter()).map(|m| m.id));
    v
}

/// How old the patient in each case is, in years.
///
/// **Not in the scenario file.** `sce_hash = sha256(<the whole file>)` is the case's identity on
/// chain, so a field cannot be added to one without minting a different case and orphaning every
/// proof already anchored against it. **Not only in the persona file** either: EP2 through EP5
/// have no authored dialogue anywhere in this repository and so have no persona, and EP3's
/// patient is five years old — precisely one of the four this table exists for. So the ages live
/// here, beside the rest of the case table, with a test that pins every one of them against the
/// persona that does exist and a second that fails if a case is added without one.
///
/// It reaches exactly one decision: whether [`news2`] may report a score at all. The bedside
/// monitor bands its alarm limits by age off the season table on the page (commit ecbdff2); this
/// is the same fact on the server side, and the numbers are the same numbers, because a screen
/// whose monitor says "3–5 YR · HR 80–140 · no alarm" beside a NEWS2 of 7 has already lost the
/// reader whatever the panels individually claim.
const AGES: &[(&str, f64)] = &[
    ("ep1", 19.0),  // Ing · F 19
    ("ep2", 58.0),  // Prasit · M 58
    ("ep3", 5.0),   // Khaopun · M 5
    ("ep4", 34.0),  // Mali · F 34
    ("ep5", 47.0),  // Boonsong · M 47
    ("osce-a", 71.0),   // Somchai · M 71
    ("osce-a2", 68.0),  // Somsri · F 68
    ("osce-b", 25.0),   // Somchai Jaidee · M 25
    ("osce-b2", 14.0),  // Tan · M 14
    ("osce-b3", 3.0),   // Pim · F 3
    ("osce-c", 6.0),    // Fon · F 6
    ("osce-c2", 53.0),  // Wasana · F 53
    ("osce-c3", 25.0),  // Waen · F 25
    ("osce-d", 62.0),   // Somchai Jaiman · M 62
    ("osce-d2", 55.0),  // Somsri Jaidee · F 55
    ("osce-d3", 6.0),   // Beam · F 6
    ("osce-d4", 72.0),  // Pranom · F 72
];

/// How old this case's patient is, if the table says.
///
/// `None` for a case nobody has declared an age for, which [`news2::applies_to_age`] treats as an
/// adult — the published default, and what every screen here did before ages existed. The test
/// that every case is in [`AGES`] is what stops "no age" becoming the way a child is scored as an
/// adult a second time.
fn patient_age(ep: &str) -> Option<f64> {
    AGES.iter().find(|(id, _)| *id == ep).map(|(_, years)| *years)
}

/// Where a case's **persona** lives — the character the model is asked to play.
///
/// Deliberately parallel to [`scenario_path`], arm for arm, because the two must never disagree
/// about which case is which: a session running OSCE-A's automaton and EP1's persona is precisely
/// the bug this file grew the function to fix. `demo/personas/` is a new directory on purpose —
/// a `.sce.json`'s sha256 is the case's identity on chain, so a persona could not be added to one
/// without minting a different case, and none of this is proof-path anyway.
///
/// EP1 keeps `demo/ep1-en.json`: it is the file the language tests read and the conformance case
/// was written against, and moving it would move a hash for no gain. Unknown ids resolve to it
/// exactly as they resolve to its scenario, so an id that plays EP1's automaton speaks with
/// EP1's voice and not somebody else's.
fn persona_path(id: &str) -> std::path::PathBuf {
    let root = scenario_root();
    match id {
        "ep2" | "ep3" | "ep4" | "ep5" => root.join("demo/personas").join(format!("{id}.json")),
        m if set_member(m).is_some() => root.join("demo/personas").join(format!("{id}.json")),
        _ => root.join("demo/ep1-en.json"),
    }
}

/// Read every persona that exists, keyed by case id.
///
/// Missing is normal — EP2 through EP5 have no authored dialogue anywhere in the repository, so
/// they have no persona and their patients stay silent. Malformed is *not* normal and says so on
/// stderr, because a persona that fails to parse looks exactly like one that was never written,
/// and the difference matters to whoever just edited it.
fn load_personas() -> std::collections::BTreeMap<String, serde_json::Value> {
    let mut m = std::collections::BTreeMap::new();
    for id in every_case() {
        let p = persona_path(id);
        let Ok(text) = std::fs::read_to_string(&p) else { continue };
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(v) => {
                m.insert(id.to_string(), v);
            }
            Err(e) => eprintln!("persona {id} ({}) is not valid JSON: {e}", p.display()),
        }
    }
    m
}

/// Where a case's rubric lives — the scorer's inputs, pinned separately from the scenario.
///
/// `None` is a fact, not a fallback: a case with no rubric cannot host an exam, and both the
/// commit gate and the anchor scoring ask this same function, so they cannot disagree about
/// which cases those are.
fn rubric_path(id: &str) -> Option<std::path::PathBuf> {
    let p = match id {
        "ep2" => scenario_root().join("demo/rubrics/ep2-stemi.json"),
        "ep3" => scenario_root().join("demo/rubrics/ep3-epiglottitis.json"),
        "ep4" => scenario_root().join("demo/rubrics/ep4-pulmonary-embolism.json"),
        "ep5" => scenario_root().join("demo/rubrics/ep5-the-night-the-stars-fell.json"),
        // Set members share one naming rule, so a Phase-5b rubric goes live by existing.
        m if set_member(m).is_some() => scenario_root().join("demo/rubrics").join(format!("{id}.json")),
        _ => return None,
    };
    p.exists().then_some(p)
}

/// The name a case wears in the player bar and in the save list.
///
/// Stations wear the stem, not a drama title and not the answer (see [`SetMember::title`]: this
/// string rides the player bar for the whole exam). The `OSCE-x ·` prefix is **built** from the
/// id rather than written out per station, which is why every station has one: four of the twelve
/// were spelled out by hand here and the other eight fell through to the bare stem, so a save
/// list read "Barking cough on the second night — F 3" with nothing to say which station that
/// was, next to three other cases that also start with a cough.
///
/// Display only — no hash is derived from any of this.
fn title(id: &str) -> String {
    match id {
        "ep2" => "EP2 · Time Is Muscle".into(),
        "ep3" => "EP3 · Don't Make Him Cry".into(),
        "ep4" => "EP4 · The Masquerader".into(),
        "ep5" => "EP5 · The Night the Stars Fell".into(),
        _ => match set_member(id) {
            Some(m) => format!("{} · {}", station_label(id), m.title),
            None => "EP1 · The Last Bite".into(),
        },
    }
}

/// `osce-b3` → `OSCE-B3`. The station's own id, in the shape the shelf and the title card print it.
fn station_label(id: &str) -> String {
    id.to_uppercase()
}

/// How long each entry on the shelf advertises, in whole minutes.
///
/// The number was decoration. The card said "a 10-minute station", the page drew a progress bar
/// against it, and nothing on either side of the wire enforced it — which is how `osce-b2` and
/// `osce-c`, neither of which declares an ending edge a candidate can reach by standing still,
/// ran to sixty simulated minutes in an audit with the mark sheet still sealed.
///
/// It is the server's number now, and it means one thing: **how long the candidate gets to
/// work.** It is a floor under the ending and not a guillotine over the case — see
/// [`Session::ring_the_bell`], which stops taking input at this mark and then lets the patient
/// finish going wherever she was going. A station whose failing narrative arrests at 11.6
/// simulated minutes still arrests; it simply does so with nobody left in the room.
///
/// Measured against what the twelve stations actually do, every one of them can be *passed* in
/// under half its advertised time (2.8–5.0 minutes of orders on a competent run). Nothing here
/// stops a case mid-narrative to satisfy a label, and no label was moved to satisfy a case.
///
/// The page keeps its own copy — it is a static file and paints the shelf before it has spoken
/// to the server — and `the_shelf_card_and_the_server_agree_about_the_clock` is what holds the
/// two together, exactly as it does for the stem.
const RUNTIME_MINUTES: &[(&str, u32)] = &[
    ("ep1", 12), ("ep2", 12), ("ep3", 14), ("ep4", 12), ("ep5", 18),
    ("osce-a", 8), ("osce-a2", 8),
    ("osce-b", 10), ("osce-b2", 10), ("osce-b3", 10),
    ("osce-c", 10), ("osce-c2", 10), ("osce-c3", 10),
    ("osce-d", 12), ("osce-d2", 12), ("osce-d3", 10), ("osce-d4", 14),
];

/// The advertised duration of one case, in simulated seconds.
///
/// An id nobody has declared falls back to EP1's twelve minutes, for the same reason [`title`]
/// falls back to EP1: an unknown id already plays the EP1 scenario.
fn runtime_sec(ep: &str) -> f64 {
    let m = RUNTIME_MINUTES.iter().find(|(id, _)| *id == ep).map(|(_, m)| *m).unwrap_or(12);
    m as f64 * 60.0
}

/// The bay's own word for a level, from the compiler's.
///
/// The catalogue's levels are the three the ward offers and the bay's enum is the same three under
/// other names; an unknown word is the middle one rather than a panic, because a case that says
/// something new about its level is still a case somebody is reading.
fn difficulty_named(level: &str) -> Difficulty {
    match level {
        "student" => Difficulty::Student,
        "resident" => Difficulty::Resident,
        _ => Difficulty::Intern,
    }
}

fn difficulty(ep: &str) -> Difficulty {
    // A set member's tier is declared once, in SETS — the shelf chip and the XP weight read
    // the same field, and a Phase-5b member needs no arm here.
    if let Some(m) = set_member(ep) {
        return m.tier;
    }
    match ep {
        "ep2" => Difficulty::Intern,
        "ep3" | "ep4" | "ep5" => Difficulty::Resident,
        _ => Difficulty::Student,
    }
}

fn new_session(ep: &str) -> Result<Session, String> {
    let sce_json = std::fs::read_to_string(scenario_path(ep)).map_err(|e| e.to_string())?;
    let sce = Sce::from_json(&sce_json).map_err(|e| e.to_string())?;
    Ok(Session {
        ep: ep.to_string(),
        owner: None,
        state: SceState::new(sce),
        tape: Vec::new(),
        beats: Vec::new(),
        films: Vec::new(),
        sce_json,
        scenario: title(ep),
        difficulty: difficulty(ep),
        anchored: false,
        said: Vec::new(),
        saved_at: None,
        commit: None,
        exam_mode: false,
        ward: None,
        review: None,
        handed_over: false,
        })
}

/// Open a case the ward holds, to be read rather than played on anybody.
///
/// The person is invented from the case's own patient block — the age and sex it is written about,
/// and a name that is not a name, because nobody is in this bed. The ward's persona pool is not
/// touched: a review run must not consume a face or a person the factory would have placed.
///
/// Everything else is the shift's: the compiled scenario, the case's own content, its voice and its
/// chips, the same monitor. What is missing is what makes a shift a shift — a patient, a head, a
/// lease — and the routes that reach the chain read `review` and refuse.
fn open_review(store: &store::Store, case_id: &str) -> Result<(Session, serde_json::Value), String> {
    let key = ward_case::key_for(case_id);
    let pack: serde_json::Value = store
        .get(ward_case::CASE_STORE, &key)
        .ok_or_else(|| format!("{case_id} is not a case this ward holds"))?;
    let summary = ward_case::validate_case(&pack)?;
    let sce_json = ward_sce(store, case_id)?;
    let sce = Sce::from_json(&sce_json).map_err(|e| e.to_string())?;

    // Nobody is in this bed, and the page must not imply somebody is. The age and the sex are the
    // case's own, because the prose, the examination and the physiology are written about them.
    let who = ward_case::a_patient_of(&summary);

    let review = serde_json::json!({
        "is_review": true,
        "case": summary.case_id,
        // Filled, like every other piece of prose that leaves a pack: the raw title carries
        // `{sex_word}` and `{age}`, and a reviewer reading "a {sex_word} of {age}" is reading the
        // compiler's plumbing rather than the case.
        "title": ward_case::fill_persona(&summary.title, &who),
        "difficulty": summary.difficulty,
        "country": summary.country,
        "country_name": summary.country.as_ref().and_then(|c| ward::persona_pool()
            .into_iter()
            .find(|p| &p.country == c)
            .map(|p| p.place)
            .filter(|p| !p.is_empty())),
        "provisional": summary.provisional,
        "status": ward_chain::catalogue_status(ward_chain::door_here()),
        "withdrawn": summary.withdrawn,
        "endemic": summary.endemic,
        "name": who.name,
        "age": who.age,
        "content": ward_case::case_view(&pack, &who),
        // Said by the server rather than only drawn by the page, so anything reading this answer —
        // a script, a log, the page — is told the same thing.
        "not_on_the_ward": "a review run: not on the ward, not on the chain. Nothing done here is \
                            recorded, counted or anchored",
    });

    Ok((
        Session {
            ep: summary.case_id.clone(),
            owner: None,
            state: SceState::new(sce),
            tape: Vec::new(),
            beats: Vec::new(),
            films: Vec::new(),
            sce_json,
            scenario: summary.title,
            difficulty: difficulty_named(&summary.difficulty),
            anchored: false,
            said: Vec::new(),
            saved_at: None,
            commit: None,
            exam_mode: false,
            ward: None,
            review: Some(case_id.to_string()),
            handed_over: false,
        },
        review,
    ))
}

/// Everything a run in progress needs to be rebuilt on the patient it was played on.
///
/// Chain facts and the tapes they name, gathered once so [`Session::restore`] stays a function of
/// its arguments: a rebuild that reaches for a cluster is a rebuild nobody can test.
struct WardRebuild {
    shifts: Vec<ward::ShiftOnChain>,
    admitted_slot: u64,
}

/// What this server last anchored, per patient: how long her chain then was, and when.
///
/// Kept because a read taken in the seconds after an anchor comes back before the transaction is
/// finalized, and a page that opens her then is handed a chart one shift short of the truth. This
/// is not a cache of the chain — nothing is served *from* it. It is only ever a reason to wait.
fn heads() -> &'static std::sync::Mutex<std::collections::HashMap<u64, (u32, std::time::Instant)>> {
    static HEADS: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<u64, (u32, std::time::Instant)>>,
    > = std::sync::OnceLock::new();
    HEADS.get_or_init(Default::default)
}

/// Read the chain's account of one patient, for a run being restored after a restart.
///
/// `None` when the chain cannot be read or she is not there — and the caller drops the run rather
/// than replaying it onto a patient nobody checked. A stranger's unfinished shift is lost, which
/// is what a crash means; what must not happen is a shift resumed onto the wrong woman.
fn ward_rebuild(store: &store::Store, patient_id: u64) -> Option<WardRebuild> {
    let chain = ward_chain::WardChain::connect().ok()?;
    let her = chain.patient(patient_id).ok()??;
    let mut seen: ward_chain::Seen = store
        .get(ward_chain::SHIFT_CACHE, &format!("p{patient_id}"))
        .unwrap_or_default();
    let _ = chain.refresh(patient_id, &mut seen, store, &ward_chain::Budget::whole_history());
    Some(WardRebuild { shifts: seen.shifts(), admitted_slot: her.admitted_slot })
}

/// The scenario a ward patient's case runs.
///
/// **The ward's own catalogue first.** Her case came through `/api/ward/case`, compiled from
/// embla-cases, and the bytes the admission committed to on chain are the ones stored there — so a
/// stranger playing her is playing what the record says she has.
///
/// The season's files are the fallback and are named as one: three patients were mid-stay on
/// season cases when the case factory landed on 16 ก.ย., and finishing their stays is kinder than
/// stranding them. No patient admitted after that can reach this path — the ticker admits only
/// from the catalogue — and when the last of the three has gone home it can be deleted.
fn ward_sce(store: &store::Store, case: &str) -> Result<String, String> {
    if let Some(json) = ward_case::sce_of(store, case) {
        return Ok(json);
    }
    std::fs::read_to_string(scenario_path(case)).map_err(|e| {
        format!(
            "{case} is not a case this ward holds: {e}. The ward plays what the case factory sends \
             through /api/ward/case"
        )
    })
}

/// Start a shift on a patient the ward is holding.
///
/// Everything this needs is either on the chain or derived from it: she must be a patient this
/// operator admitted, she must still be open, and the case she is being treated for comes from the
/// pack that was queued for her. The state the shift begins on is every tape before it, replayed —
/// the same replay the verifier runs, so what the stranger sees is what a stranger could derive.
///
/// The refusals are sentences because each one is a thing the person standing at her bed can act
/// on: come back to the board, she went home, nobody has described her yet.
fn open_shift(
    store: &store::Store,
    patient_id: u64,
) -> Result<(Session, serde_json::Value), String> {
    let chain = ward_chain::WardChain::connect().map_err(|e| format!("no chain to read: {e}"))?;
    let now_slot = chain.slot().map_err(|e| format!("the chain would not say what slot it is: {e}"))?;
    let her = chain
        .patient(patient_id)
        .map_err(|e| format!("this chart could not be read: {e}"))?
        .ok_or_else(|| format!("no patient {patient_id} has been admitted here"))?;
    // Not behind what this server itself wrote. A chart one shift short, opened seconds after
    // somebody finished a shift, is the page contradicting the record it is made of.
    if let Some(wait) = ward::behind_the_head(
        her.shifts,
        heads().lock().unwrap().get(&patient_id).map(|(n, at)| (*n, at.elapsed())),
    ) {
        return Err(wait);
    }
    if her.state != ward::OPEN {
        // No pronoun: this is the chain's account of a patient whose pack has not been read yet,
        // and the ward admits men. The page that renders her chart a moment later has her persona
        // and says "He died" over a man.
        return Err(format!(
            "patient {patient_id} has left the ward — that stay ended when the patient {}, and a \
             stay that ended is not one anybody can add to",
            if her.state == ward::DISCHARGED { "went home" } else { "died" }
        ));
    }

    let pack = ward_chain::packs(store)
        .remove(&patient_id)
        .ok_or_else(|| format!(
            "we do not know who patient {patient_id} is yet — this patient reached a bed before \
             the details did, and nobody can be treated by a chart with no name on it. Another bed \
             will have somebody in it"
        ))?;

    let sce_json = ward_sce(store, &pack.case)?;

    // Her past, as the chain gives it: the shifts that actually anchored, in slot order, each tape
    // found by the hash its leaf commits to.
    let key = format!("p{patient_id}");
    let mut seen: ward_chain::Seen = store.get(ward_chain::SHIFT_CACHE, &key).unwrap_or_default();
    let read = chain.refresh(patient_id, &mut seen, store, &ward_chain::Budget::whole_history());
    if matches!(read, Ok(r) if r.added > 0) {
        let _ = store.put(ward_chain::SHIFT_CACHE, &key, &seen);
    }
    let (state, played) = ward_chain::resumed(
        &sce_json,
        &seen.shifts(),
        &|h| ward_chain::tape_by_hash(store, h),
        her.admitted_slot,
        now_slot,
        // From the store, and the clock for `now_slot` itself. The refresh above has just filed
        // every slot on her chain from the listing that found them, so nothing here reaches the
        // RPC — the open path used to spend a round trip per slot before a page could be answered.
        &ward_chain::dater_to_now(store, now_slot),
    )?;

    let head = hex(&her.head);
    let shift = WardShift {
        patient_id,
        index: played as u32,
        taken_slot: now_slot,
        head: head.clone(),
        // Her whole set, carried into the session so every view can name the right one without
        // reaching for the store.
        faces: pack.portrait.clone(),
        // Hers, from the pack the factory drew. The case's own patient is a different person.
        age: pack.persona.age,
    };
    let ward_view = serde_json::json!({
        "patient_id": patient_id,
        "case": pack.case,
        "name": pack.persona.name,
        "country": pack.persona.country,
        // The name of the place, from the pool that already holds it. The page says "from
        // Nigeria" rather than "NGA": a code is for matching a map, not for reading aloud.
        "country_name": ward::persona_pool()
            .into_iter()
            .find(|c| c.country == pack.persona.country)
            .map(|c| c.place)
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| pack.persona.country.clone()),
        "age": pack.persona.age,
        // What this shift must extend. Named before the work, because the program refuses a
        // reveal that does not extend the head it was told — and that refusal is the mechanic.
        "head": head,
        "shift": played + 1,
        "shifts_before": played,
        // **The state she is actually in, not the calm one.** This read `"stable"` and so the
        // bedside opened on the base portrait of a woman who might by then have been arrested for
        // hours — the founder, 23 Sep: "ถ้าคนไข้ตายแล้วควรจะเข้าไปเห็นรูปที่อาการไม่ดี". The ladder for
        // this already existed and already resolves a dead patient down to her worst *made*
        // picture; it was simply never asked the question. `portrait_at_the_bedside` rather than
        // `portrait_for` for the reason its own doc gives: at a bedside, the last face there is
        // beats a black frame in front of somebody still in the room with her.
        "portrait": ward::portrait_at_the_bedside(
            &pack.portrait,
            vitals_sce::runtime::PatientStatus::word(state.status),
        ),
        // Her state travels with the picture, because the page has to be able to say what it is
        // showing — a portrait that looks bad is not a sentence, and the founder asked for the
        // sentence too: "โดยต้องมีข้อความบอกคนไข้ตายแล้ว".
        "status": vitals_sce::runtime::PatientStatus::word(state.status),
        "can_speak": state.status.can_speak(),
        "taken_slot": now_slot,
        // The case's own words, told about the person in this bed. The page renders from this and
        // from nothing else: its own table holds the season's sixteen, and a World-case patient
        // rendered from that came out as EP1's.
        "content": store
            .get::<serde_json::Value>(ward_case::CASE_STORE, &ward_case::key_for(&pack.case))
            .map(|held| ward_case::case_view(&held, &pack.persona)),
    });

    Ok((
        Session {
            ep: pack.case.clone(),
            owner: None,
            review: None,
            state,
            tape: Vec::new(),
            beats: Vec::new(),
            films: Vec::new(),
            sce_json,
            scenario: title(&pack.case),
            difficulty: difficulty(&pack.case),
            anchored: false,
            said: Vec::new(),
            saved_at: None,
            commit: None,
            exam_mode: false,
            ward: Some(shift),
            handed_over: false,
        },
        ward_view,
    ))
}

/// What a ward transaction, once signed, is for.
///
/// The ward's program is not Eternal's and its work is kept apart from `PendingWork` deliberately
/// (ruling 7): one map, one submit route, one program. Merging them would put Eternal's anchoring
/// one refactor away from a sprint change.
enum WardWork {
    /// A stranger's first transaction here: an account of their own.
    Open,
    /// Take the head of a patient's chain for the length of a shift.
    Take { patient_id: u64 },
    /// Declare the shift before it is played. The nonce never reaches the chain.
    Declare { session: String, hash: [u8; 32], nonce: [u8; 32] },
    /// Append this shift's leaf to her chain, extending the head it named. The leaf is kept so a
    /// refusal can be read against the head the chain holds afterwards: if that head is this leaf,
    /// the shift is anchored and the "somebody else moved it" sentence would be about us.
    Anchor { session: String, patient_id: u64, leaf: [u8; 32] },
    /// Put the head down with nothing anchored. Her chart is untouched and this shift's tape is
    /// discarded — the next person gets her as this one found her.
    Release { session: String },
}

/// A half-signed ward transaction, waiting for the browser that must finish it.
struct WardPending {
    pending: ward_chain::Pending,
    work: WardWork,
    player: solana_sdk::pubkey::Pubkey,
}

type WardPendings = Arc<Mutex<HashMap<String, WardPending>>>;

/// How many boards are watching right now.
static WARD_WATCHERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Decrements the watcher count however the stream thread ends — a closed tab, a write error, a
/// panic. A counter that only went up would refuse the sixty-fifth watcher for ever.
struct WatcherLeaves;

impl Drop for WatcherLeaves {
    fn drop(&mut self) {
        WARD_WATCHERS.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// A page, with the surface composed in and the build stamped.
///
/// Both pages go through here. The token is not injected into the markup at all any more — it
/// lives in the script, which is served separately — so a page is the same bytes for everybody
/// and only `/bay.js` carries anything that depends on this deployment.
fn compose(page: &str) -> String {
    compose_for(page, ward_mode())
}

/// The bay, composed for the host that is serving it.
///
/// **The brand is the one element of the shared surface that differs by host** (founder, 16 ก.ย.:
/// a logo in the top-left, and pressing it leaves the ward for the globe). On vitals.academy the
/// bar wears the Eternal wordmark a judge may have in a tab; on the ward host it wears the Vitals
/// World mark and is the way back to the globe.
///
/// Done here rather than by a class the script toggles, because a page a visitor is handed already
/// right has nothing to re-render, nothing to flash, and nothing to get wrong on a slow script.
///
/// **A comment that spells out a marker becomes a copy of what the marker stands for** — the one
/// above `<!--BRAND-->` in `world/shift.html` did, and the page carried three marks until it was
/// reworded. Name the marker in the markup and describe it in the prose.
fn compose_for(page: &str, ward: bool) -> String {
    let surface = if ward {
        between(SURFACE, SEASON_ONLY_OPEN, SEASON_ONLY_CLOSE, false)
            .replace(WARD_ONLY_OPEN, "")
            .replace(WARD_ONLY_CLOSE, "")
            .replace(ETERNAL_BRAND, WORLD_BRAND)
    } else {
        between(SURFACE, WARD_ONLY_OPEN, WARD_ONLY_CLOSE, false)
            .replace(SEASON_ONLY_OPEN, "")
            .replace(SEASON_ONLY_CLOSE, "")
    };
    page.replace("<!--BAY-->", &surface)
        // The page's own corner, on the pages that ask for one. The bay's bar carries the same
        // link a row further down; this is the one the founder meant.
        .replace("<!--BRAND-->", if ward { WORLD_BRAND } else { "" })
        .replace(BUILD_STAMP, BUILD)
}

/// The surface is one file and the two hosts are not one product.
///
/// "ผมไม่ได้ให้เอาเคสของ vitals เดิมมาใช้ใน world" — the founder, 16 ก.ย. The ward must carry none
/// of the season: not its episodes, not its stills or films, not its result flow, not its names.
/// So the shared surface marks which parts belong to which host and the server hands each one the
/// page it should have — the same idiom as the brand, for the same reason. A class the script
/// toggles would leave the season's markup in the ward's page, one devtools tab away from a judge,
/// and one missed rule away from rendering.
const SEASON_ONLY_OPEN: &str = "<!--SEASON-->";
const SEASON_ONLY_CLOSE: &str = "<!--/SEASON-->";
const WARD_ONLY_OPEN: &str = "<!--WARD-->";
const WARD_ONLY_CLOSE: &str = "<!--/WARD-->";

/// Everything outside `open`..`close` (or inside, with `keep`), markers included.
fn between(src: &str, open: &str, close: &str, keep: bool) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(i) = rest.find(open) {
        if !keep {
            out.push_str(&rest[..i]);
        }
        let after = &rest[i + open.len()..];
        match after.find(close) {
            Some(e) => {
                if keep {
                    out.push_str(&after[..e]);
                }
                rest = &after[e + close.len()..];
            }
            // An unclosed marker takes the rest of the file rather than leaking half of it: a
            // truncated page is obvious, and half the season's markup on the ward is not.
            None => return out,
        }
    }
    if !keep {
        out.push_str(rest);
    }
    out
}

/// The season's wordmark, exactly as `bay-surface.html` carries it.
const ETERNAL_BRAND: &str = "<span class=\"brand\">Vital<span>s</span></span>";

/// The ward's: the monitor mark and the name, and the whole thing is the way out.
///
/// The mark is `static/world/favicon.svg`'s own elements — one drawing, inlined here so the bar
/// needs no second request, and `the_mark_in_the_bar_is_the_mark_in_the_tab` is what keeps the two
/// from drifting apart. It earned its keep on opening night: the favicon became the globe and this
/// was still the monitor, and the test said so before anybody saw the page.
const WORLD_BRAND: &str = concat!(
    "<a href=\"/\" class=\"brand\" title=\"back to the globe\" ",
    "aria-label=\"Vitals World — back to the globe\">",
    "<svg viewBox=\"0 0 64 64\" width=\"18\" height=\"18\" aria-hidden=\"true\" focusable=\"false\">",
    "<rect x=\"0\" y=\"0\" width=\"64\" height=\"64\" rx=\"14\" fill=\"#0E1719\"/>",
    "<circle cx=\"32\" cy=\"32\" r=\"20.6\" fill=\"none\" stroke=\"#FFFFFF\" stroke-width=\"3.2\"/>",
    "<ellipse cx=\"32\" cy=\"32\" rx=\"8.7\" ry=\"20.6\" fill=\"none\" stroke=\"#FFFFFF\" stroke-width=\"1.8\" opacity=\"0.85\"/>",
    "<ellipse cx=\"32\" cy=\"32\" rx=\"16.1\" ry=\"20.6\" fill=\"none\" stroke=\"#FFFFFF\" stroke-width=\"1.8\" opacity=\"0.85\"/>",
    "<line x1=\"12.6\" y1=\"21.7\" x2=\"51.4\" y2=\"21.7\" stroke=\"#FFFFFF\" stroke-width=\"1.8\" opacity=\"0.85\"/>",
    "<line x1=\"12.6\" y1=\"42.3\" x2=\"51.4\" y2=\"42.3\" stroke=\"#FFFFFF\" stroke-width=\"1.8\" opacity=\"0.85\"/>",
    "<polyline points=\"10.3,32 22.7,32 25.4,28.7 27.9,32 30.4,32 32.4,19.2 35.3,42.3 37.6,32 56.3,32\" fill=\"none\" stroke=\"#0E1719\" stroke-width=\"6.5\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/>",
    "<polyline points=\"10.3,32 22.7,32 25.4,28.7 27.9,32 30.4,32 32.4,19.2 35.3,42.3 37.6,32 56.3,32\" fill=\"none\" stroke=\"#26C0A5\" stroke-width=\"3.2\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/>",
    "</svg> Vitals World</a>",
);

/// One shift's receipt, or the reason there is none.
/// One patient's whole stay, for the page that shows it.
///
/// **Every state except "on the ward right now" used to be a 404.** A patient who went home, died,
/// or reached the chain without a pack answered "Not this bed" — no name, no face, no outcome, and
/// no way to the shifts that treated her. The claim this project makes is that the chart is the
/// chain; a patient whose chart cannot be read the moment her stay ends is that claim with a hole
/// in it, and it is the hole a judge would find first.
///
/// Public and ungated, like the board and the receipts: what it publishes is on chain already, plus
/// the pack the factory sent and the tapes this ward kept.
fn ward_chart(store: &store::Store, patient_id: u64) -> serde_json::Value {
    let bad = |why: &str| serde_json::json!({ "error": why });
    let chain = match ward_chain::WardChain::connect() {
        Ok(c) => c,
        Err(e) => return bad(&e),
    };
    let her = match chain.patient(patient_id) {
        Ok(Some(p)) => p,
        Ok(None) => return bad(&format!("no patient {patient_id} has been admitted here")),
        Err(e) => return bad(&e),
    };
    let as_of = chain.slot().unwrap_or(0);
    // The two slots her stay is bounded by, dated by the chain itself and kept for ever. Asked for
    // here rather than carried from the board's read: a chart is opened one patient at a time, and
    // these are two lookups the store answers from the second time on.
    let times = ward_chain::slot_times(
        &chain,
        store,
        &[her.admitted_slot, her.closed_slot].into_iter().filter(|s| *s > 0).collect(),
    );
    let pack = ward_chain::packs(store).remove(&patient_id);
    let case = pack.as_ref().map(|p| p.case.clone()).unwrap_or_default();
    let held = store
        .get::<serde_json::Value>(ward_case::CASE_STORE, &ward_case::key_for(&case))
        .and_then(|c| ward_case::validate_case(&c).ok());

    // Her shifts, from the cache this ward fills as it reads the chain. Each one is addressed by
    // the leaf where there is one and by its tape's hash otherwise — the same two addresses the
    // receipt page takes, so every row here is a link that resolves.
    let key = format!("p{patient_id}");
    let seen: ward_chain::Seen = store.get(ward_chain::SHIFT_CACHE, &key).unwrap_or_default();
    let mut shifts: Vec<serde_json::Value> = seen
        .shifts()
        .into_iter()
        .map(|s| {
            let hash = hex(&s.run_hash);
            serde_json::json!({
                "run_hash": hash,
                "slot": s.slot,
                "kept": ward_chain::tape_by_hash(store, &hash).is_some(),
                // A key, not a person: there is no signup here, so this is the short form the
                // receipt shows and nothing more.
                "signer": hex(&s.signer).chars().take(8).collect::<String>(),
            })
        })
        .collect();
    shifts.sort_by_key(|s| s.get("slot").and_then(|v| v.as_u64()).unwrap_or(0));

    // Whether this ward can put anybody at her bedside, by the same rule the board uses — one
    // function, so a page and a row cannot disagree about one patient. Her row read `off_ward`
    // with "the ward no longer holds this case" while this endpoint said `on_ward`, and the page
    // read that out as "She is on the ward".
    let openable = her.state == ward::OPEN
        && ward::case_is_held(pack.as_ref(), &ward_case::all(store));
    serde_json::json!({
        "patient_id": patient_id,
        // The board's word, not the chain's. The chain still calls her open and is not wrong;
        // what it cannot know is that nothing here can draw her case.
        "state": if her.state == ward::OPEN && !openable { "caseless" }
                 else { ward::state_word(her.state) },
        "openable": openable,
        "why_not": (her.state == ward::OPEN && !openable).then_some(
            "the ward no longer holds this case, so nothing here can open this bed"),
        "admitted_slot": her.admitted_slot,
        "closed_slot": (her.closed_slot > 0).then_some(her.closed_slot),
        "as_of_slot": as_of,
        // The two slots as wall time, carried the way the board carries its own: a slot is a fact
        // about the chain and a page shows a person when something happened to them.
        // The chain's own dating of the two slots her stay is bounded by, and nothing worked out
        // from this read's slot: that arithmetic put her admission 38 hours early on the board.
        // Null is "this ward has not asked the chain for that slot yet", and the page then shows
        // her state without a date rather than a date nobody can check.
        "admitted_at": ward::at_slot(&times, her.admitted_slot),
        "closed_at": ward::at_slot(&times, her.closed_slot),
        "sex": pack.as_ref().map(|p| p.persona.sex.clone()),
        "shifts_on_chain": her.shifts,
        "name": pack.as_ref().map(|p| p.persona.name.clone()),
        "age": pack.as_ref().map(|p| p.persona.age),
        "country": pack.as_ref().map(|p| p.persona.country.clone()),
        "country_name": pack.as_ref().and_then(|p| ward::persona_pool()
            .into_iter()
            .find(|c| c.country == p.persona.country)
            .map(|c| c.place)
            .filter(|s| !s.is_empty())),
        "portrait": pack.as_ref().and_then(|p| {
            ward::portrait_for(&p.portrait, ward::portrait_state(her.state)).map(str::to_string)
        }),
        "case": (!case.is_empty()).then(|| case.clone()),
        // The case's own words, filled for the patient in the bed rather than the patient the
        // case was authored about — the age beside this sentence is hers, and the two have to be
        // one person or a reader cannot tell which number belongs to whom.
        "case_title": held.as_ref().map(|c| {
            pack.as_ref().map(|p| ward_case::fill_persona(&c.title, &p.persona))
        }),
        "difficulty": held.as_ref().map(|c| c.difficulty.clone()),
        "withdrawn": held.as_ref().map(|c| c.withdrawn),
        "shifts": shifts,
    })
}

fn ward_receipt(store: &store::Store, address: &str) -> serde_json::Value {
    let bad = |why: &str| serde_json::json!({ "error": why });
    if !ward_chain::is_shift_hash(address) {
        return bad("that is not the name of a shift");
    }
    // **Two addresses, one page.** The run hash names the tape — two strangers who did exactly the
    // same things to the same case share one, and the receipt says so. The leaf names this shift
    // and no other, and it is what the chain itself holds as her head, so it is the address a
    // judge can arrive with. Looked up first: a leaf we anchored resolves to its tape's hash, and
    // anything else falls through to being a tape hash itself.
    let run_hash = &ward_chain::run_hash_of_leaf(store, address).unwrap_or_else(|| address.to_string());
    let chain = match ward_chain::WardChain::connect() {
        Ok(c) => c,
        Err(e) => return bad(&e),
    };
    let found = match ward_chain::find_shift(&chain, store, run_hash) {
        Ok(Some(f)) => f,
        Ok(None) => {
            return bad(
                "no shift on this ward has that hash. It may never have been anchored, or it may \
                 belong to another ward — this one does not guess between them",
            )
        }
        Err(e) => return bad(&e),
    };
    let (patient_id, shifts, this) = found;
    let Some(pack) = ward_chain::packs(store).remove(&patient_id) else {
        return bad("the ward has no pack for that patient, so it cannot say which case this shift was");
    };
    let root = scenario_root();
    let Ok(sce_json) = ward_sce(store, &pack.case) else {
        return bad("this ward does not hold that case, so this shift cannot be replayed");
    };
    let admitted = chain.patient(patient_id).ok().flatten().map(|p| p.admitted_slot).unwrap_or(0);
    // From the store, never from the chain. Every slot on this patient's chain was dated by the
    // call that found it — `find_shift` above walks her history, and the walk files the block times
    // the listing hands it — so this reads them rather than asking again, one round trip per slot,
    // with a reader waiting on the answer. A slot nothing has dated yet carries no time and the row
    // says so, which is the same thing this page has always done with a slot the chain would not
    // date.
    let dated = ward_chain::cached_dater(store);
    match ward_chain::receipt(
        &sce_json,
        ward_chain::rubric_for(store, &root, &pack.case).as_deref(),
        &shifts,
        &this,
        &|h| ward_chain::tape_by_hash(store, h),
        &pack,
        admitted,
        &dated,
    ) {
        Ok(mut v) => {
            // The chain's own name for this shift, when the address was one or when this server
            // anchored it. Published so a reader can cite the thing the tree holds rather than the
            // thing the tape hashes to — they are different facts and the page says which is which.
            if address != run_hash {
                v["leaf"] = serde_json::json!(address);
            }
            // The person and the case, in the words a reader knows them by: her face, her age and
            // her country, and the case's own title and level rather than its id. The id is a fact
            // about the filing and belongs with the leaf and the slot.
            let held = store
                .get::<serde_json::Value>(ward_case::CASE_STORE, &ward_case::key_for(&pack.case))
                .and_then(|c| ward_case::validate_case(&c).ok());
            // Filled for the patient this receipt is about. `v["age"]` two lines down is hers,
            // and a receipt whose sentence and whose age are about different people is a receipt
            // nobody can check.
            v["case_title"] = serde_json::json!(held.as_ref().map(|c| {
                ward_case::fill_persona(&c.title, &pack.persona)
            }));
            v["difficulty"] = serde_json::json!(held.as_ref().map(|c| c.difficulty.clone()));
            v["age"] = serde_json::json!(pack.persona.age);
            v["sex"] = serde_json::json!(pack.persona.sex);
            v["country"] = serde_json::json!(pack.persona.country);
            v["country_name"] = serde_json::json!(ward::persona_pool()
                .into_iter()
                .find(|c| c.country == pack.persona.country)
                .map(|c| c.place)
                .filter(|p| !p.is_empty()));
            v["portrait"] = serde_json::json!(ward::portrait_for(&pack.portrait, "stable"));
            // The transaction that anchored it, so a reader can open it in an explorer and see the
            // same thing on somebody else's screen. That is the whole point of anchoring it.
            let key = format!("p{}", patient_id);
            let seen: ward_chain::Seen = store.get(ward_chain::SHIFT_CACHE, &key).unwrap_or_default();
            v["signature"] = serde_json::json!(seen.signature_of(&this.run_hash));
            v
        }
        Err(e) => bad(&e),
    }
}

/// The receipt as a page. Plain on purpose: it is a record, and a record that needs decoration to
/// be believed is not one.
fn receipt_page(r: &serde_json::Value, board: &serde_json::Value) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    if let Some(why) = r["error"].as_str() {
        return format!(
            "<!doctype html><meta charset=utf-8><title>No such shift — Vitals World</title>\
             <link rel=\"icon\" type=\"image/svg+xml\" href=\"/world/favicon.svg\">\
             <meta name=viewport content='width=device-width,initial-scale=1'>\
             <style>body{{font:16px/1.6 ui-sans-serif,system-ui,sans-serif;max-width:34rem;\
             margin:4rem auto;padding:0 1.2rem;color:#16302b;background:#fbfaf7}}a{{color:#0f6e5c}}\
             .k{{font:.72rem/1.5 ui-monospace,monospace;letter-spacing:.1em;text-transform:uppercase;\
             color:#7b8a86;margin:1.6rem 0 .3rem}}ul.beds{{list-style:none;padding:0;margin:0}}\
             ul.beds li{{margin:.35rem 0}}\
             </style><h1>No such shift</h1><p>{why}</p>{beds}<p><a href=/>← the globe</a></p>",
            why = esc(why),
            beds = beds_on_offer(board),
        );
    }
    // ── the shift, as a story a stranger can read ───────────────────────────
    //
    // It said "13 orders · 0 beats" and "8 of 40" over a case id, with the patient's pronoun wrong
    // and no way to check any of it on the chain. A receipt is the thing a player wants to show
    // somebody: who the patient was, what they did, what happened, and where to see for themselves.
    // The hashes are the evidence and they go at the foot, folded, where evidence belongs.
    let did = &r["did"];
    let pid = r["patient_id"].as_u64().unwrap_or(0);
    let sex = r["sex"].as_str().unwrap_or("");
    let (subj, poss) = match sex.chars().next().map(|c| c.to_ascii_lowercase()) {
        Some('m') => ("he", "his"),
        Some('f') => ("she", "her"),
        // The receipt said "what she did" over Yonas Tesfaye until 17 ก.ย.; a page with no persona
        // to read says neither rather than choosing.
        _ => ("the stranger who took it", "the patient's"),
    };
    let cap = |w: &str| {
        let mut c = w.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
    };
    let who = [
        r["age"].as_u64().map(|a| a.to_string()),
        r["country_name"].as_str().map(|c| format!("from {}", esc(c))),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    /* What case this was, in the headline, whichever kind of case it is.
       A compiled case carries its own title and level through the catalogue. A season case carries
       neither — those live in the bay's own tables — and the line collapsed to nothing over
       Yonas's receipt on 00048. `CATALOGUE` is the gate rather than a `title()` call for anything:
       `title()` answers "EP1 · The Last Bite" for an id it does not know, so a compiled case handed
       to it would be named as the wrong case, which is worse than being named as none. */
    let case = r["case"].as_str().unwrap_or("");
    let of_the_season = ward::CATALOGUE.contains(&case);
    let case_line = [
        r["case_title"].as_str().map(esc)
            .or_else(|| of_the_season.then(|| esc(&title(case)))),
        r["difficulty"].as_str().map(esc)
            .or_else(|| of_the_season.then(|| ward::difficulty_of(case).map(esc)).flatten()),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    let face = r["portrait"]
        .as_str()
        .map(|src| format!("<img class=face src=\"{}\" alt=\"\" width=96 height=96>", esc(src)))
        .unwrap_or_default();

    let mins = |at: f64| format!("{}:{:02}", (at as u64) / 60, (at as u64) % 60);
    let timeline = r["timeline"].as_array().map(|steps| {
        if steps.is_empty() {
            "<p class=note>Nothing was ordered or asked on this shift.</p>".to_string()
        } else {
            // The case's own words for what was done, with the id beside them in small type. The
            // words are what a reader came for; the id is what they would need to check it, and
            // `tx_oxygen` alone — which is what this printed — is neither.
            let rows = steps.iter().map(|s| {
                let text = s["text"].as_str().unwrap_or("");
                let said = s["said"].as_str();
                let body = match said {
                    Some(words) => format!("{} <span class=raw>{}</span>", esc(words), esc(text)),
                    None => esc(text),
                };
                format!(
                    "<li><span class=at>{}</span> <span class=kind>{}</span> {body}</li>",
                    mins(s["at"].as_f64().unwrap_or(0.0)),
                    if s["kind"] == "asked" { "asked" } else { "ordered" },
                )
            }).collect::<Vec<_>>().join("");
            format!("<ol class=tl>{rows}</ol>")
        }
    }).unwrap_or_default();

    let harm = did["harm"].as_array().map(|h| h.len()).unwrap_or(0);
    let harm_line = if harm == 0 {
        "no harm was recorded on this shift".to_string()
    } else {
        did["harm"].as_array().unwrap().iter()
            .map(|h| esc(h.as_str().unwrap_or("")))
            .collect::<Vec<_>>().join("<br>")
    };
    let outcome = match r["did"]["outcome"].as_str() {
        Some(o) => format!("the case reached <b>{}</b>", esc(o)),
        None => format!("{} was handed on, still on the ward", subj),
    };
    let marks = match (&r["det"]["earned"], &r["det"]["max"]) {
        (serde_json::Value::Number(e), serde_json::Value::Number(m)) => {
            let rows = r["items"].as_array().map(|items| items.iter().map(|i| {
                /* A `no_unindicated` row is a deduction, not a mark: it can only take. Printed as
                   "0 of 0" it reads as a mark that was there to be earned and was missed, which is
                   what the director read on Yonas's receipt. So it says what it took, and what for
                   — the orders it charged are named, because a deduction a candidate cannot see the
                   reason for teaches nothing. */
                if i["kind"] == "no_unindicated" {
                    let took = i["penalty"].as_i64().unwrap_or(0);
                    let charged = i["charged"].as_array().map(|c| c.len()).unwrap_or(0);
                    return if took == 0 {
                        "<li class=got>no penalty · nothing ordered off the list</li>".to_string()
                    } else {
                        let named = i["charged"].as_array().map(|c| c.iter()
                            .filter_map(|x| x.as_str())
                            .map(esc)
                            .collect::<Vec<_>>().join(", ")).unwrap_or_default();
                        format!(
                            "<li class=missed>−{took} · {charged} order{} off the list: {named}</li>",
                            if charged == 1 { "" } else { "s" },
                        )
                    };
                }
                format!(
                    "<li class=\"{}\"><b>{}</b> of {} · {}</li>",
                    if i["earned"].as_i64().unwrap_or(0) > 0 { "got" } else { "missed" },
                    i["earned"], i["points"], esc(i["label"].as_str().unwrap_or("")),
                )
            }).collect::<Vec<_>>().join("")).unwrap_or_default();
            format!("<p class=score><b>{e}</b> of {m}</p><ul class=marks>{rows}</ul>")
        }
        _ => "<p class=note>This case carries no mark sheet.</p>".to_string(),
    };

    let explorer = r["signature"].as_str().map(|sig| format!(
        "<p><a href=\"https://explorer.solana.com/tx/{}?cluster=devnet\">this shift on the devnet \
         explorer</a> — the transaction that anchored it</p>",
        esc(sig)
    )).unwrap_or_default();
    let ev = |k: &str, v: String| format!("<tr><th>{k}</th><td>{v}</td></tr>");
    let evidence = [
        ev("patient", format!("<a href=/ward/{pid}>{pid}</a>")),
        ev("case", format!("<code>{}</code>", esc(r["case"].as_str().unwrap_or("")))),
        ev("anchored at slot", r["slot"].to_string()),
        ev("leaf", format!("<code>{}</code>", esc(r["leaf"].as_str().unwrap_or("—")))),
        ev("run hash", format!("<code>{}</code>", esc(r["run_hash"].as_str().unwrap_or("")))),
        ev("played by", format!("<code>{}</code>", esc(r["player"].as_str().unwrap_or("")))),
        match r["also_anchored_note"].as_str() {
            Some(n) => ev("also on this ward", esc(n)),
            None => String::new(),
        },
    ].concat();

    format!(
        "<!doctype html><meta charset=utf-8><title>One shift — Vitals World</title>\
         <link rel=\"icon\" type=\"image/svg+xml\" href=\"/world/favicon.svg\">\
         <meta name=viewport content='width=device-width,initial-scale=1'>\
         <style>body{{font:16px/1.6 ui-sans-serif,system-ui,sans-serif;max-width:42rem;\
         margin:3rem auto;padding:0 1.2rem;color:#16302b;background:#fbfaf7}}\
         h1{{font-size:1.45rem;margin:0 0 .1rem}}h2{{font-size:.82rem;letter-spacing:.1em;\
         text-transform:uppercase;color:#5d6f6d;margin:2rem 0 .5rem}}a{{color:#0f6e5c}}\
         .k{{color:#5d6f6d;font:600 .78rem/1.6 ui-monospace,monospace;letter-spacing:.08em;\
         text-transform:uppercase}}.face{{border-radius:12px;object-fit:cover;float:right;\
         margin:0 0 .6rem .9rem}}p.note{{color:#5d6f6d}}\
         ol.tl{{list-style:none;padding:0;margin:0}}ol.tl li{{padding:.15rem 0;border-bottom:1px solid #e7ebe8}}\
         .at{{font:600 .78rem ui-monospace,monospace;color:#5d6f6d;margin-right:.5rem}}\
         .kind{{font:600 .7rem ui-monospace,monospace;letter-spacing:.06em;text-transform:uppercase;\
         color:#0f6e5c;margin-right:.4rem}}\
         .raw{{font:.72rem ui-monospace,monospace;color:#8a9a96;margin-left:.4rem}}\
         p.score{{font-size:1.6rem;margin:.2rem 0}}ul.marks{{list-style:none;padding:0;margin:.4rem 0}}\
         ul.marks li{{padding:.1rem 0}}ul.marks li.missed{{color:#8a9a96}}\
         details{{margin:1.4rem 0}}summary{{cursor:pointer;color:#5d6f6d}}\
         table{{border-collapse:collapse;margin:.6rem 0;width:100%}}\
         th{{text-align:left;font-weight:600;color:#5d6f6d;padding:.3rem 1rem .3rem 0;\
         white-space:nowrap;vertical-align:top;width:11rem}}td{{padding:.3rem 0}}\
         code{{font-size:.85rem;word-break:break-all}}</style>\
         {face}\
         <p class=k>shift {shift} · one shift on a public ward</p>\
         <h1>{name}</h1>\
         <p class=note>{who}</p>\
         <p>{case_line}</p>\
         <h2>what happened</h2>\
         {timeline}\
         <h2>how it ended</h2>\
         <p>{outcome}, and {harm_line}.</p>\
         <h2>what the case paid for</h2>\
         {marks}\
         <p class=note>{omitted}</p>\
         <h2>check it yourself</h2>\
         {explorer}\
         <p><a href=\"{tape}\">download this shift's tape</a> — replay it against the case with \
         vitals-replay and the leaf must come out as the hash below.</p>\
         <details><summary>the addresses this shift is filed under</summary>\
         <table>{evidence}</table></details>\
         <p><a href=/ward/{pid}>← {poss} chart</a> · <a href=/>the ward</a></p>",
        shift = r["shift"],
        name = esc(r["name"].as_str().unwrap_or("a patient")),
        omitted = esc(r["judged_omitted"].as_str().unwrap_or("")),
        tape = esc(r["tape"].as_str().unwrap_or("#")),
        outcome = cap(&outcome),
    )
}

/// The ward as it stands, from the held read — one source for the endpoint and the patient page,
/// so a judge who opens both in one minute cannot be shown two different wards.
/// Is a board being read right now, somewhere behind an answer already given?
///
/// One at a time: every request that finds the board stale would otherwise start its own chain
/// read, and twenty tabs on the globe would be twenty reads of the same thing.
static BOARD_READING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Is a ward pass running right now — in the ticker thread, or inside a scheduler's request?
///
/// One at a time, and a second asker is **refused, never queued**: the pass writes to the chain,
/// and two passes closing the same patient would anchor a closing shift twice. Held through
/// `rebuild::take`, so a pass that panics gives it back.
static TICKING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The session sweep runs once per process, after the first pass's repair — whichever caller ran
/// that pass. A flag rather than a local in the thread, so a scheduler that always wins the gate
/// does not leave the sweep never run.
static SWEPT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The ward as this host last read it — answered now, refreshed behind the answer.
///
/// `ward::board_use` is the decision and the reasoning is there. The shape here is the part that
/// matters: the lock is held to *copy* a board and released before anything slow happens, so a
/// chain read never holds the one thing every request on this server needs to touch.
fn ward_now(held: &WardView, store: &store::Store, state: &str) -> serde_json::Value {
    let (age, cached) = {
        let cell = held.lock().unwrap();
        match cell.as_ref() {
            Some((at, v)) => (Some(at.elapsed()), Some(v.clone())),
            None => (None, None),
        }
    };
    // What the last process to run here left behind, when this one has nothing of its own. Read
    // once, and only when memory is empty: the store is the fallback, never the hot path.
    let kept = cached.is_none().then(|| ward_chain::last_board(store)).flatten();
    let mut v = match (ward::board_use(age, kept.is_some(), WARD_TTL), cached) {
        (ward::Board::Serve, Some(v)) => v,
        (ward::Board::ServeAndRefresh, Some(v)) => {
            refresh_behind(held, state);
            v
        }
        // The board the last process left. Answered now; the chain is read behind it by the same
        // thread the stale-board path uses, so a cold instance costs its first visitor nothing.
        // Memory is seeded with the board's *real* age, so this instance treats it exactly as it
        // would treat its own read of that age — including refreshing it again when it expires.
        (ward::Board::ServeStoredAndRefresh, _) => {
            let kept = kept.expect("ServeStoredAndRefresh means there is one");
            println!(
                "ward       answered from the board {} kept {}s ago",
                if kept.revision.is_empty() { "a previous process" } else { &kept.revision },
                kept.age.as_secs()
            );
            // Stamped once, here, where this board enters the process — not per answer, or the
            // same board would describe itself differently on its second read.
            let mut board = kept.board;
            board["board"] = ward::board_note(
                ward::Origin::Store,
                kept.at_unix,
                Some(&kept.revision).filter(|r| !r.is_empty()).map(String::as_str),
            );
            if let Some(then) = Instant::now().checked_sub(kept.age) {
                *held.lock().unwrap() = Some((then, board.clone()));
            }
            refresh_behind(held, state);
            board
        }
        // Nothing anywhere. The first request on a ward that has never read the chain pays for the
        // read, and it is the only one that ever does.
        _ => {
            let v = match ward_chain::WardChain::connect() {
                Ok(c) => ward_chain::read_ward(&c, store),
                Err(e) => {
                    // No chain to read, and still a door and a queue: both are facts about this
                    // host. A page that branches on the door was getting nothing to branch on.
                    let mut v = ward::ward_unavailable("unconfigured", &e);
                    v["queue"] = ward_chain::queue_block(store);
                    v
                }
            };
            let mut v = v;
            v["board"] = ward::board_note(ward::Origin::Chain, now_ms() / 1000, None);
            *held.lock().unwrap() = Some((Instant::now(), v.clone()));
            v
        }
    };
    // What is true of *this process*, written on the way out: the door it is actually serving
    // behind, the sentence composed from that door, and which deployment answered. A page keeps
    // the first revision it is told and reloads itself when a later board comes from a different
    // one — the tab the founder had open was three hours behind a deploy and had no way to find
    // out — and the door belongs at the same seam for the same reason: a board outlives the
    // revision that composed it, and the door does not travel with it.
    ward::stamp_host(&mut v, ward_chain::door_here(), &revision());
    v
}

/// Read the chain behind an answer that has already gone out.
///
/// Never in front of one. Both callers have just handed a reader a board — this instance's own, or
/// the one the last process left in the store — and this is what makes the next answer fresher. The
/// guard means one read at a time however many requests arrive during it.
///
/// The read's own duration is printed rather than waited on, because it is the number that decides
/// whether the ward is keeping up: measured at 123 s on a cold staging instance with eighteen
/// patients, which is what a visitor used to pay.
fn refresh_behind(held: &WardView, state: &str) {
    // Taken here and moved into the thread, so every way out gives it back: an early return, a
    // panic in the chain read, a poisoned view mutex, or a thread that could not be spawned at all.
    // It used to be given back by the last line of the thread body, which meant one panic left the
    // board unrefreshable for the life of the process, silently.
    let Some(gate) = rebuild::take(&BOARD_READING) else { return };
    let view = Arc::clone(held);
    let dir = state.to_string();
    std::thread::spawn(move || {
        let _gate = gate;
        let began = Instant::now();
        let fresh = match store::Store::open(std::path::PathBuf::from(&dir)) {
            Ok(store) => match ward_chain::WardChain::connect() {
                Ok(c) => Some(ward_chain::read_ward(&c, &store)),
                Err(e) => {
                    let mut v = ward::ward_unavailable("unconfigured", &e);
                    v["queue"] = ward_chain::queue_block(&store);
                    Some(v)
                }
            },
            Err(e) => {
                eprintln!("ward       the board could not be refreshed: {e}");
                None
            }
        };
        if let Some(fresh) = fresh {
            let beds = fresh["patients"].as_array().map(Vec::len).unwrap_or(0);
            // Provenance is stamped by `read_ward` itself now, where the board is made: a board
            // it had to serve from the store (`fall_back`) arrives saying `from: store`, and
            // restamping it here as `chain` would relabel a stale board as fresh.
            println!(
                "ward       board refreshed behind the answer in {:.1}s — {beds} patients",
                began.elapsed().as_secs_f64()
            );
            *view.lock().unwrap() = Some((Instant::now(), fresh));
        }
    });
}

/// Cloud Run's own name for the deployment answering, or this build when it is somewhere else.
fn revision() -> String {
    std::env::var("K_REVISION").unwrap_or_else(|_| BUILD.to_string())
}

/// No such patient, or no readable chain — said in a sentence rather than as a status code alone.
/// What a shared bed looks like when somebody pastes it into a chat.
///
/// UX review G5. The link unfurled as the product's own name and no picture — a card about a
/// website, when what was shared was a person. This is her: the name, the age, the country in
/// words, her face, and the one sentence that says what is happening to her.
///
/// Off the board, so a card cannot say anything the ward is not already saying, and a row with no
/// name on it produces nothing rather than a card about nobody.
fn og_tags(her: &serde_json::Value) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;");
    let Some(name) = her["name"].as_str().filter(|n| !n.is_empty()) else { return String::new() };
    let age = her["age"].as_u64().map(|a| format!(" · {a}")).unwrap_or_default();
    let place = her["country"]
        .as_str()
        .and_then(|c| ward::persona_pool().into_iter().find(|x| x.country == c).map(|x| x.place))
        .filter(|s| !s.is_empty())
        .map(|s| format!(" · {s}"))
        .unwrap_or_default();
    let what = match her["state"].as_str().unwrap_or("") {
        "on_shift" => "Somebody is treating her right now on a public ward.",
        "went_home" | "died" => "Her stay has ended. Her chart is on the chain, and anybody can read it.",
        "off_ward" => "She is on the chain, and this ward has no case for her.",
        _ => "Nobody is with her. Take a shift and treat her — a few minutes at her bedside.",
    };
    let face = her["portrait"]
        .as_str()
        .filter(|u| u.starts_with("https://"))
        .map(|u| format!(
            "<meta property=\"og:image\" content=\"{}\">\
             <meta name=\"twitter:card\" content=\"summary_large_image\">",
            esc(u)))
        // No face yet: a small card rather than a large one with nothing in it.
        .unwrap_or_else(|| "<meta name=\"twitter:card\" content=\"summary\">".to_string());
    format!(
        "<meta property=\"og:title\" content=\"{name}{age}{place}\">\
         <meta property=\"og:description\" content=\"{what}\">\
         <meta property=\"og:type\" content=\"profile\">{face}",
        name = esc(name),
        age = esc(&age),
        place = esc(&place),
        what = esc(what),
    )
}

/// The beds a stranger can take, as rows for a page that has just said no.
///
/// UX review F1: a refusal is read by somebody who came to treat a patient, and a sentence plus a
/// way back to the globe makes them start again. `ward::beds_to_offer` decides which patients those
/// are — off the board this host already holds, so this costs no chain read — and an empty list
/// prints nothing at all rather than an empty heading.
fn beds_on_offer(board: &serde_json::Value) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let open = ward::beds_to_offer(board);
    if open.is_empty() {
        return String::new();
    }
    let rows = open
        .iter()
        .map(|p| {
            let who = p["name"].as_str().unwrap_or("a patient");
            let age = p["age"].as_u64().map(|a| format!(" · {a}")).unwrap_or_default();
            let level = p["difficulty"].as_str().map(|d| format!(" · {d}")).unwrap_or_default();
            let place = p["country"]
                .as_str()
                .and_then(|c| ward::persona_pool().into_iter().find(|x| x.country == c).map(|x| x.place))
                .filter(|s| !s.is_empty())
                .map(|s| format!(" · {s}"))
                .unwrap_or_default();
            format!(
                "<li><a href=\"/ward/{id}\">bed {bed} — {who}{age}{place}{level}</a></li>",
                id = p["patient_id"].as_u64().unwrap_or(0),
                bed = p["bed"].as_u64().unwrap_or(0),
                who = esc(who),
                age = esc(&age),
                place = esc(&place),
                level = esc(&level),
            )
        })
        .collect::<String>();
    format!(
        "<p class=k>beds you can take now</p><ul class=beds>{rows}</ul>"
    )
}

fn ward_page_missing(why: &str, board: &serde_json::Value) -> String {
    format!(
        "<!doctype html><meta charset=utf-8><title>Not a patient — Vitals World</title>\
         <meta name=viewport content='width=device-width,initial-scale=1'>\
         <style>body{{font:16px/1.6 ui-sans-serif,system-ui,sans-serif;max-width:34rem;\
         margin:4rem auto;padding:0 1.2rem;color:#16302b;background:#fbfaf7}}a{{color:#0f6e5c}}\
         .k{{font:.72rem/1.5 ui-monospace,monospace;letter-spacing:.1em;text-transform:uppercase;\
         color:#7b8a86;margin:1.6rem 0 .3rem}}ul.beds{{list-style:none;padding:0;margin:0}}\
         ul.beds li{{margin:.35rem 0}}\
         </style><h1>Nobody here</h1><p>{why}</p>{beds}<p><a href=/>← the globe</a></p>",
        why = why.replace('&', "&amp;").replace('<', "&lt;"),
        beds = beds_on_offer(board),
    )
}


/// The strip a shift page is served with.
///
/// The same ids and the same shape `bay.js`'s `wardBar` builds, because the page *adopts* this one
/// and wires it rather than building a second. It is here so that a stranger on a slow link reads
/// the patient's own strip — the way back and the one thing to press — at first paint rather than
/// two seconds later when the script has parsed. Measured on the globe's panel on 18 ก.ย.: 0.23 s
/// to served words, 2.16 s to script-drawn ones.
///
/// Only for `/ward/<id>`. A review run's strip carries different controls and the markup cannot
/// tell the two paths apart, so that one stays the script's.
const WARD_STRIP: &str = "<div id=\"wardbar\" style=\"display:flex;gap:.8rem;align-items:center;\
     flex-wrap:wrap;padding:.6rem .9rem;margin-bottom:.6rem;border:1px solid var(--rule,#d8ded9);\
     border-radius:.5rem;background:rgba(15,110,92,.06)\">\
     <b id=\"wardwho\">…</b><span id=\"wardsay\" style=\"flex:1\">reading the ward…</span>\
     <span id=\"leaseclock\" class=\"leaseclock\"></span>\
     <button class=\"btn go\" id=\"wardtake\" disabled>take this shift</button>\
     <button class=\"btn quiet\" id=\"wardback-shift\" style=\"display:none\">\
     Leave without recording</button>\
     <a class=\"btn\" id=\"wardback\" href=\"/\">← the globe</a></div>";

/// A patient waiting for the door, as a page.
///
/// Founder's ruling, 18 ก.ย. In preview the board publishes the queue and every row is a link, and
/// what opens is not a bed: she is not admitted, there is no chain account, no head, no lease and
/// no tape. So this is the part of a chart that exists — her face, her name, her age, where she is
/// from, the case she was built for and the level it is written at — and one sentence saying what
/// she is waiting for.
///
/// Nothing on it does anything to her. No take, no clock, no monitor, and nothing of her case
/// beyond its title: a page that offered any of those would be offering a patient nobody may
/// treat. When the door opens she is admitted by the ticker like anybody else and her page becomes
/// a bed, at a different address, with her chain behind it.
fn waiting_page(pack: &ward::Pack, case: Option<&ward_case::CaseSummary>, door: ward_chain::Door) -> String {
    let esc = |t: &str| t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let place = ward::persona_pool()
        .into_iter()
        .find(|c| c.country == pack.persona.country)
        .map(|c| c.place.to_string())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| pack.persona.country.clone());
    let face = ward::portrait_for(&pack.portrait, "stable")
        .map(|u| format!(
            "<img src=\"{}\" alt=\"{}\" width=160 height=160 \
             style=\"border-radius:14px;object-fit:cover;display:block;margin:0 0 1rem\">",
            esc(u), esc(&pack.persona.name)))
        .unwrap_or_default();
    let title = case
        .map(|c| ward_case::fill_persona(&c.title, &ward_case::a_patient_of(c)))
        .map(|t| format!("<p><b>{}</b></p>", esc(&t)))
        .unwrap_or_default();
    let level = pack
        .difficulty
        .clone()
        .or_else(|| case.map(|c| c.difficulty.clone()))
        .map(|d| format!(" · {}", esc(&d)))
        .unwrap_or_default();
    format!(
        "<!doctype html><meta charset=utf-8>\
         <title>{name} — waiting for the ward — Vitals World</title>\
         <meta name=viewport content='width=device-width,initial-scale=1'>\
         <link rel=\"icon\" type=\"image/svg+xml\" href=\"/world/favicon.svg\">\
         <style>body{{font:16px/1.65 ui-sans-serif,system-ui,sans-serif;max-width:34rem;\
         margin:3rem auto;padding:0 1.2rem;color:#16302b;background:#fbfaf7}}\
         a{{color:#0f6e5c}}h1{{font-size:1.4rem;margin:0 0 .2rem}}\
         .k{{font:.72rem/1.5 ui-monospace,monospace;letter-spacing:.1em;text-transform:uppercase;\
         color:#7b8a86}}.note{{color:#4A5B5E}}</style>\
         <p class=k>waiting for the door</p>{face}<h1>{name}</h1>\
         <p class=k>{age} · from {place}{level}</p>{title}\
         <p class=note>This patient is waiting for the ward to open. Nothing has happened to them \
         yet — no shift, no chart, nothing on the chain.</p>\
         <p class=k>the door is {door}</p>\
         <p><a href=/>← the globe</a></p>",
        name = esc(&pack.persona.name),
        age = pack.persona.age,
        place = esc(&place),
        level = level,
        face = face,
        title = title,
        door = door.word(),
    )
}

/// Ask before you use this again.
///
/// Every page this server writes. Not "do not store": the browser keeps it and revalidates, which
/// is one conditional request against the ETag beside it. What it stops is the thing that happened
/// to the founder on 17 ก.ย. — a tab serving him a ward three hours out of date, with a deploy an
/// hour old behind it and nothing in the answer telling his browser to ask.
fn never_kept() -> Header {
    Header::from_bytes(&b"Cache-Control"[..], &b"no-cache"[..]).expect("a static header")
}

/// A year, immutable — for the files whose URL carries the build stamp.
///
/// Safe precisely because of that stamp: `bay.js?v=<build>` cannot mean two different files, so a
/// browser that never asks again is never wrong. Without the stamp this would be the worst header
/// in the file.
fn forever() -> Header {
    Header::from_bytes(&b"Cache-Control"[..], &b"public, max-age=31536000, immutable"[..])
        .expect("a static header")
}

/// Does this client take gzip?
///
/// Asked of the request rather than assumed of the world: a health check, a curl and the odd proxy
/// do not, and sending them a compressed body they cannot read is the one failure this whole path
/// could introduce.
fn takes_gzip(req: &tiny_http::Request) -> bool {
    req.headers()
        .iter()
        .find(|h| h.field.equiv("accept-encoding"))
        .is_some_and(|h| h.value.as_str().to_ascii_lowercase().contains("gzip"))
}

/// Text on the wire, compressed when the client asked and it is worth it.
///
/// The ward is 620 KB of text to a first visitor — the globe 274 KB, `bay.js` 247, `bay.css` 99 —
/// on a service that scales to zero and pays for every byte. gzip takes that to about a tenth.
///
/// **The floor is the point of `SQUEEZE_FLOOR`.** Below it a compressor costs a header, a CPU
/// burst and a round trip's worth of latency to save a few hundred bytes, and the ward answers a
/// great many small JSON requests.
const SQUEEZE_FLOOR: usize = 1400;

fn squeezed(
    req: &tiny_http::Request,
    body: Vec<u8>,
    content_type: &[u8],
) -> Response<std::io::Cursor<Vec<u8>>> {
    let ctype = Header::from_bytes(&b"Content-Type"[..], content_type).expect("a content type");
    if body.len() < SQUEEZE_FLOOR || !takes_gzip(req) {
        return Response::from_data(body).with_header(ctype);
    }
    use std::io::Write;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    // A compressor that fails sends the text uncompressed. There is no version of this worth an
    // error page.
    if gz.write_all(&body).is_err() {
        return Response::from_data(body).with_header(ctype);
    }
    match gz.finish() {
        Ok(packed) => Response::from_data(packed)
            .with_header(ctype)
            .with_header(Header::from_bytes(&b"Content-Encoding"[..], &b"gzip"[..]).unwrap())
            // Caches keyed on the URL alone would hand a compressed body to a client that cannot
            // read one. This is the header that stops that, and it is not optional.
            .with_header(Header::from_bytes(&b"Vary"[..], &b"Accept-Encoding"[..]).unwrap()),
        Err(_) => Response::from_data(body).with_header(ctype),
    }
}

/// A short, stable name for a body: the first eight bytes of its sha256, quoted as an ETag.
///
/// Content-addressed, so two servers behind one URL agree and a restart does not invalidate
/// everybody's copy — which is what a timestamp or a boot id would do.
/// Every answer that changes nothing, in one place.
///
/// `None` for a path this does not serve, so a caller can fall through to the loop it came from.
///
/// One implementation, two callers: the reader pool, and the writing loop's own arms, which
/// delegate here rather than keeping a second copy. Two copies of one page is how the copies come
/// to disagree, and the disagreement is always found by a reader rather than by us.
///
/// Nothing in here opens a session, writes a tape, touches the tree or reaches the chain. That is
/// the property `serve::is_read_only` names path by path, and the reason these may be answered
/// beside a pass rather than behind it.
#[allow(clippy::too_many_arguments)]
fn read_response(
    req: &tiny_http::Request,
    path: &str,
    view: &WardView,
    store: &store::Store,
    state: &str,
    usage: &Arc<Mutex<usage::Usage>>,
    token: &Option<String>,
) -> Option<Response<std::io::Cursor<Vec<u8>>>> {
    match path {
        "/api/ward" => {
            // **A slow read, made askable for.** The board's first read on a cold instance goes to
            // the chain inline and takes sixteen to twenty-one seconds — measured on staging. That
            // is the case the reader pool exists for, and without a way to produce one, the pool
            // would ship as a mechanism nobody had watched work. Inert unless set, and set by no
            // deployed service; it slows this one path so a test can ask whether the others are
            // still answered beside it.
            if let Some(ms) = std::env::var("VITALS_BOARD_SLEEP_MS")
                .ok()
                .and_then(|v| v.trim().parse::<u64>().ok())
            {
                std::thread::sleep(std::time::Duration::from_millis(ms));
            }
            let body = serde_json::to_vec(&ward_now(view, store, state))
                .unwrap_or_else(|_| b"{}".to_vec());
            let tag = etag_of(&body);
            // Asked again with the tag it already has, the ward says "still that" and sends
            // nothing. The board is opened by a room full of people at once during a demo.
            let known = req
                .headers()
                .iter()
                .find(|h| h.field.equiv("if-none-match"))
                .is_some_and(|h| h.value.as_str().split(',').any(|t| t.trim() == tag));
            let resp = if known {
                Response::from_data(Vec::new()).with_status_code(304)
            } else {
                squeezed(req, body, b"application/json")
            };
            Some(
                resp.with_header(Header::from_bytes(&b"ETag"[..], tag.as_bytes()).unwrap())
                    // Fifteen seconds: long enough to absorb a room opening it at once, short
                    // enough that a death is on screen before anybody has stopped looking.
                    .with_header(
                        Header::from_bytes(&b"Cache-Control"[..], &b"public, max-age=15"[..])
                            .unwrap(),
                    ),
            )
        }
        // The bay's own play numbers are not this host's to publish, and that has not changed.
        // What is this host's is who arrived at it, so that is published and nothing else is.
        "/api/usage" => Some(json(serde_json::json!({
            "ward": "not open yet",
            "opens": "week 2 of Crypto World's Fair, 21-27 Sep 2026",
            "arrivals": usage.lock().unwrap().arrivals(),
            "funnel": usage.lock().unwrap().funnel(),
            "usage_for_the_eternal_entry": "https://vitals.academy/api/usage"
        }))),
        "/review" => Some(html(&REVIEW.replace(BUILD_STAMP, BUILD))),
        "/privacy" => Some(html(&PRIVACY.replace(BUILD_STAMP, BUILD))),
        "/stats" => Some(html(WARD_STATS)),
        "/start" | "/start/" => Some(html(WARD_START)),
        "/bay.css" => Some(
            squeezed(req, BAY_CSS.replace(BUILD_STAMP, BUILD).into_bytes(), b"text/css; charset=utf-8")
                .with_header(forever()),
        ),
        "/bay.js" => {
            let js = BAY_JS
                .replace("__VITALS_TOKEN__", token.as_deref().unwrap_or(""))
                .replace(BUILD_STAMP, BUILD);
            Some(
                squeezed(req, js.into_bytes(), b"application/javascript; charset=utf-8")
                    .with_header(forever()),
            )
        }
        "/world/favicon.svg" => Some(
            Response::from_data(FAVICON_WORLD)
                .with_header(Header::from_bytes(&b"Content-Type"[..], &b"image/svg+xml"[..]).unwrap())
                .with_header(forever()),
        ),
        "/world/apple-touch-icon.png" => Some(
            Response::from_data(TOUCH_ICON_WORLD)
                .with_header(Header::from_bytes(&b"Content-Type"[..], &b"image/png"[..]).unwrap())
                .with_header(forever()),
        ),
        p if p.starts_with("/start/img/") => {
            let want = &p["/start/img/".len()..];
            Some(match WARD_START_IMG.iter().find(|(name, _)| *name == want) {
                Some((_, bytes)) => Response::from_data(*bytes)
                    .with_header(Header::from_bytes(&b"Content-Type"[..], &b"image/jpeg"[..]).unwrap())
                    .with_header(forever()),
                // A name not in the list is not a file this server has, and the set is closed.
                None => Response::from_data(b"no such picture".to_vec()).with_status_code(404),
            })
        }
        _ => None,
    }
}

fn etag_of(body: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let h = Sha256::digest(body);
    format!("\"{}\"", h[..8].iter().map(|b| format!("{b:02x}")).collect::<String>())
}

fn html(body: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    html_kept(body, "no-cache")
}

/// A page, and how long a browser may keep it.
///
/// `no-cache` for everything on the ward, and it does not mean "do not store": it means "ask me
/// before you use this again", which is one conditional request against an ETag this server
/// already sends. The founder read a ward three hours out of date in his own tab because the
/// answer said nothing at all and his browser guessed — a document whose content is the state of a
/// ward that changes every minute is not a document to guess about.
///
/// The apex is the exception and says so where it asks: it serves a landing page that changes when
/// somebody writes it, and a proxy holding that for five minutes is right.
fn html_kept(body: &str, cache: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(body)
        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap())
        .with_header(Header::from_bytes(&b"Cache-Control"[..], cache.as_bytes()).unwrap())
}

fn json(v: impl Serialize) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = serde_json::to_string(&v).unwrap_or_else(|_| "{}".into());
    Response::from_string(body)
        .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap())
}

/// A JSON reply that is not a 200. The body still carries the whole story — a metering refusal
/// is a page the front end renders, not a status code it apologises for.
fn json_code(v: impl Serialize, code: u16) -> Response<std::io::Cursor<Vec<u8>>> {
    json(v).with_status_code(code)
}

/// How far from the end of `X-Forwarded-For` the caller's address sits.
///
/// One means the last entry. Two means the entry before it, which is the shape an external
/// load balancer produces: it appends the client and then its own forwarding rule.
///
/// Configurable because the answer is a property of the deployment and not of this code, and
/// because getting it wrong in the safe direction is survivable — see [`client_addr`].
fn client_ip_from_end() -> usize {
    std::env::var("VITALS_CLIENT_IP_FROM_END")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n| *n >= 1)
        .unwrap_or(1)
}

/// The caller's address out of an `X-Forwarded-For` value, counted from the right.
///
/// Split out from [`client_addr`] so it can be tested against the header shapes that actually
/// arrive, rather than only against the one we hope arrives.
fn addr_from_xff(header: &str, from_end: usize) -> Option<String> {
    let parts: Vec<&str> = header.split(',').map(str::trim).collect();
    let at = parts.len().checked_sub(from_end)?;
    parts.get(at).map(|s| s.to_string()).filter(|s| !s.is_empty())
}

/// Who is calling, as far as rate limiting is concerned.
///
/// **Read from the right.** Everything to the left of what our own front end appended is text
/// the caller wrote, and a caller who writes it gets a fresh window for every request.
///
/// This used to take the first entry, and the comment above it said why: the last is the only
/// one Google vouches for, but the first "is the same for one browser and different for two
/// households", which is what a politeness window wants. The reasoning was sound and the
/// conclusion still wrong, for two reasons. The window is not only politeness — it stands in
/// front of `/api/review`, which writes a durable document, and `/api/say`, which spends money.
/// And the household property it was protecting survives the change: behind Cloud Run the entry
/// Google appends *is* the household's public address, so everyone behind one NAT still shares
/// a bucket and two households still get two.
///
/// Measured before the change, against this binary: forty requests to `/api/new`, each with a
/// different invented address in the header, forty accepted — including in the
/// `<invented>, <real>` shape a request through Cloud Run actually carries.
///
/// Measured on production too, on 2026-09-03 against revision `vitals-00045-66d`: a request to
/// `/api/fuel` carrying `X-Forwarded-For: 198.51.100.77` was logged by Cloud Run with the
/// caller's real IPv6 address in `httpRequest.remoteIp`, not the invented one — the platform
/// appends what it observed rather than forwarding what it was handed. Then, with the window
/// already spent, five further requests each carrying a *different* invented value (including
/// the two-entry shape, and one with no header at all as a control) were all refused. None of
/// them bought a fresh allowance.
///
/// [`client_ip_from_end`] stays a setting because the number of trailing entries is a property
/// of the deployment: 1 for a direct service, 2 behind an external load balancer, which appends
/// the client and then its own forwarding rule. Getting it wrong fails safe — everyone shares
/// one window, which is too strict and never bypassable.
///
/// Player keys and session ids are deliberately still not used; a browser mints those for free.
fn client_addr(req: &tiny_http::Request) -> String {
    req.headers()
        .iter()
        .find(|h| h.field.equiv("x-forwarded-for"))
        .and_then(|h| addr_from_xff(h.value.as_str(), client_ip_from_end()))
        .unwrap_or_else(|| {
            req.remote_addr().map(|a| a.ip().to_string()).unwrap_or_else(|| "unknown".into())
        })
}

fn param(url: &str, key: &str) -> Option<String> {
    url.split_once('?')?.1.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then(|| percent_decode(v))
    })
}

/// Enough percent-decoding for a typed clinical order. No dependency for this.
///
/// **Bytes first, then one UTF-8 decode at the end.** Percent-encoding escapes *octets*, and a
/// character outside ASCII is several of them — `แพ้` arrives as nine `%XX` groups. Pushing each
/// decoded octet as a `char` reads those octets as Latin-1 and produces mojibake: the order the
/// learner typed never matches a keyword, never resolves to an intervention, and lands on the
/// tape as garbage that a verifier will faithfully reproduce forever.
///
/// That mattered from the moment a case author wrote a Thai keyword into a scenario — several
/// already have — and it is the whole ballgame now that the page can be played in Thai. ASCII is
/// unaffected either way, which is why every tape already anchored still decodes to exactly what
/// it decoded to before.
///
/// `from_utf8_lossy` rather than a refusal: this is a query parameter from a browser, and the
/// answer to a malformed one is a replacement character in a clinical order nobody will match,
/// not a 500 in the middle of a resuscitation.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    Ok(c) => {
                        out.push(c);
                        i += 3;
                    }
                    Err(_) => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Endpoints that spend something — the server's signature, or the GPU.
///
/// Playing is open because a kiosk should just work. Signing a transaction on request is not,
/// and "whoever can reach the port" is not an authorisation model.
fn guarded(path: &str) -> bool {
    // The player's own routes: this server signing with its own key, on its own sessions, for the
    // page it is showing. The page has to be able to call them, so they answer to the page's
    // token — which is printed into `bay.js` and is therefore public by design.
    matches!(path, "/api/anchor" | "/api/claim" | "/api/commit" | "/api/say")
}

/// The factory's two doors: the patients strangers will be handed, and the faces on them.
///
/// **They take their own secret** (`VITALS_DOOR_TOKEN`) and never the page's. Until 16 ก.ย. they
/// took `VITALS_TOKEN`, which `bay.js` prints for every visitor — so the key to the ward's write
/// side was on a public page, and anybody who opened a shift could fill the beds with patients of
/// their own or put a picture of their choosing on somebody else's patient.
///
/// A secret printed into a page cannot also be a secret that lets somebody write. That is the rule
/// the split exists for, and it is why there is no fallback below: a ward with no door token opens
/// no doors at all, rather than quietly opening them to the token everybody has.
fn door(path: &str) -> bool {
    // `/api/ward/case` is the case factory's, and `/api/ward/cases` — a letter apart — is the
    // catalogue anybody may read. Exact match on the first, which is why this is not a prefix.
    path == "/api/ward/queue"
        // The pass, asked for by Cloud Scheduler every minute — an operator's route, so the door's
        // secret and never the page's. Exact match: nothing else starts with it.
        || path == "/api/ward/tick"
        || path == "/api/ward/case"
        // `/api/ward/case/<id>/withdraw` — taking a case out of service is the factory's door too,
        // and a public one would let a stranger empty the ward's catalogue.
        || (path.starts_with("/api/ward/case/") && path.ends_with("/withdraw"))
        || path.starts_with("/api/ward/pack/")
}

fn bearer_ok(req: &tiny_http::Request, token: &Option<String>) -> bool {
    let Some(want) = token else { return true };
    req.headers()
        .iter()
        .find(|h| h.field.equiv("authorization"))
        .map(|h| h.value.as_str().trim())
        .map(|v| v.strip_prefix("Bearer ").unwrap_or(v) == want)
        .unwrap_or(false)
}

/// Why a request body was refused. Both are the caller's, and neither is a 500.
enum BadBody {
    /// Past the limit it was given. Refused whole rather than cut to fit — see [`REVIEW_MAX`].
    TooLong,
    /// Not UTF-8, or the connection died mid-body. Either way what arrived is not what was
    /// typed, and there is no honest way to store it.
    ///
    /// Deliberately not `String::from_utf8_lossy`, which is what the query-string decoder does
    /// one screen up and is right *there*: a mangled clinical order is a word that matches
    /// nothing and is visible on the tape. Here it would be half a Thai character replaced by
    /// `` in the middle of a physician's ruling, stored as though it were what they wrote.
    NotText,
}

/// Read at most `max` bytes of a request body, as text.
///
/// `take(max + 1)` rather than trusting `Content-Length`: a declared length is a claim the caller
/// makes, and a chunked body declares nothing at all. Reading exactly one byte past the limit is
/// what makes "too long" detectable without ever holding more than the limit plus one.
fn read_body(req: &mut tiny_http::Request, max: usize) -> Result<String, BadBody> {
    use std::io::Read;
    let mut buf = Vec::new();
    if req.as_reader().take(max as u64 + 1).read_to_end(&mut buf).is_err() {
        return Err(BadBody::NotText);
    }
    if buf.len() > max {
        return Err(BadBody::TooLong);
    }
    String::from_utf8(buf).map_err(|_| BadBody::NotText)
}

/// Unix seconds, server-side.
///
/// A client clock is not evidence of anything, which is why `review::Submission::at` is stamped
/// here and not read off the body — and why the key a submission sorts under is derived from it.
/// This host's clock in milliseconds, for the arithmetic that is about this host: how long ago a
/// page beat, and nothing a reader is shown.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn main() {
    // How long this process takes to become answerable, stage by stage.
    //
    // **A request never waits on boot work it does not need.** On Cloud Run at MIN_INSTANCES=0 a
    // request that arrives during a start is *held* until the container listens, so every second
    // spent here is a second in somebody's first `/api/ward`: 118 s of one on 18 September, and
    // 137 s of that boot was restoring sessions nobody had asked for.
    //
    // Every mark carries two numbers — the running total, and how long that step itself took. The
    // first version printed only the total, `boot meter +137.6s` was read as "the meter took two
    // minutes", and the meter reads one document.
    let booted = Instant::now();
    let mut last = booted;
    let mut mark = |what: &str, note: &str| {
        let now = Instant::now();
        println!(
            "boot       {what} +{:.1}s (since the last mark: {:.1}s){note}",
            booted.elapsed().as_secs_f64(),
            now.duration_since(last).as_secs_f64()
        );
        last = now;
    };
    // Not 8090. On the machine this is developed on that port is already three things — a
    // syn-sentry web app, an eir-fhir container publishing on the host, and the service port the
    // hermodr fleet uses inside the cluster. A default that collides with the neighbours is a
    // default that fails on the one day nobody is watching.
    let addr = bind_addr(
        std::env::var("PORT").ok().as_deref(),
        std::env::var("VITALS_WEB_BIND").ok().as_deref(),
    );
    let token = std::env::var("VITALS_TOKEN").ok().filter(|s| !s.is_empty());
    // The factory's own key. Deliberately a second variable rather than a second use of the first:
    // the page's token is printed into `bay.js` and this one must never be.
    let door_token = std::env::var("VITALS_DOOR_TOKEN").ok().filter(|s| !s.is_empty());
    let loopback = addr.starts_with("127.") || addr.starts_with("localhost");
    if !loopback && token.is_none() {
        // Refusing to start is the only honest option. Bound to a public interface with no token,
        // anyone who finds the port can make this process sign transactions with its key.
        eprintln!("refusing to bind {addr} without VITALS_TOKEN — anyone reaching it could make \
                   this server sign with its key. Set VITALS_TOKEN, or bind to 127.0.0.1.");
        std::process::exit(2);
    }
    let server = match Server::http(&addr) {
        Ok(s) => s,
        // The raw panic said "Address already in use" and nothing about who has it.
        Err(e) => {
            eprintln!("cannot bind {addr}: {e}\n\
                       something else is on that port — try `lsof -nP -iTCP:{port} -sTCP:LISTEN`, \
                       or set VITALS_WEB_BIND to a free one.",
                      port = addr.rsplit(':').next().unwrap_or("?"));
            std::process::exit(2);
        }
    };

    let state_dir = std::env::var("VITALS_STATE_DIR").unwrap_or_else(|_| "state".into());
    let store = store::Store::open(std::path::PathBuf::from(&state_dir))
        .unwrap_or_else(|e| panic!("cannot open {state_dir}: {e}"));
    // Timed on its own because on Firestore this is a token fetch against the metadata server
    // before it is a store, and that is a network call on a cold container.
    mark("store", "");
    // Nothing is rebuilt here. A session is replayed when somebody asks for its id — see
    // `rebuild` — because nothing outside a request has ever read one, and 22 of them cost 79.7 s
    // of a boot whose first real reader waited 62 s for the door.
    //
    // The repair below stays where it is. It guards the only copy of a tape for a leaf already on
    // chain, it is not what the 79.7 s was, and the ticker's own call to `repair_tapes` is a minute
    // away rather than in front of a reader.
    let restored: HashMap<String, Session> = HashMap::new();
    // The tape repair and the session sweep used to run here, in that order — 38.3 s and 2.8 s of
    // a 41.6 s boot on staging 00062, with the first reader held 22.2 s behind them. Both are now
    // the ticker's first pass: see the refill thread, which already ran the same repair a minute
    // later. Nothing is read from the chain and nothing is deleted before this process will answer.

    // What is in the store, counted from the records themselves — one list, no chain, no replay.
    // Not "in a bed": a bed is derived from the board, and asking the chain for one here is the
    // work being removed.
    let kinds: Vec<rebuild::Kind> = store
        .list::<Saved>(SESSIONS)
        .into_iter()
        .map(|(_, sv)| match (&sv.review, &sv.ward) {
            (Some(_), _) => rebuild::Kind::Review,
            (None, Some(_)) if sv.anchored || sv.handed_over => rebuild::Kind::WardFinished,
            (None, Some(_)) => rebuild::Kind::WardLive,
            _ => rebuild::Kind::Other,
        })
        .collect();
    mark("sessions", &format!(" · {}", rebuild::census(&kinds)));

    // No counter to carry across a restart any more: ids are random, so a restored run cannot
    // collide with a fresh one and there is nothing to resume from.
    // What boot knows, and nothing it does not. "N run(s) resumed" would print 0 for ever now and
    // read as "there are none"; the census line above says what is actually in the store, and a
    // run that cannot be replayed is discovered by the person who asks for it rather than here.
    println!(
        "state      {} · sessions rebuilt when their id is asked for",
        store.describe(),
    );
    let sessions: Arc<Mutex<HashMap<String, Session>>> = Arc::new(Mutex::new(restored));
    // One rebuild per id at a time, and a memory of the ones that will not come back — so a page
    // retrying every thirty seconds does not replay a chain it cannot use, thirty seconds apart,
    // for ever.
    let rebuilds = rebuild::Rebuilds::default();
    let unrebuildable: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));

    /// When each page holding a head last said it was still there, by patient.
    ///
    /// In memory and nowhere else: a restart is the one case where forgetting is right, because a
    /// server that comes back with a list of heads nobody has beaten for since would free every
    /// bed on the ward at once. After a restart the leases are the net until the pages beat again.
    type Beats = Arc<Mutex<std::collections::BTreeMap<u64, u64>>>;
    let beats: Beats = Arc::new(Mutex::new(std::collections::BTreeMap::new()));

    // What this bay may spend, resumed from the store so a deploy does not reset the month.
    let mut meter = meter::Meter::open(&store);
    mark("meter", "");
    println!("meter      {}", meter.describe());

    // Runs opened and runs finished, resumed from the store. Deliberately not a count of
    // people: there is no signup here, so there is nothing that is a person to count. See
    // `usage::LIMITS`, which travels with every reply the endpoint gives.
    // Shared, because the reader pool answers /api/usage while the writing loop is inside a
    // pass. Every mutation of it still happens on that loop, in the order it always did; the lock
    // is held for the length of one count or one read and never across anything that waits.
    let usage = Arc::new(Mutex::new(usage::Usage::open(&store)));
    println!("usage      {}", usage.lock().unwrap().describe());

    // How many past scenario versions this deployment can hand back to a verifier. Printed
    // because the failure mode is silent: an image built without `conformance/sce-archive`
    // answers /api/sce with 404 for every historical hash and looks perfectly healthy doing it.
    //
    // Two numbers, because they are different and the difference is the endpoint's whole
    // behaviour: what the archive *holds*, and what it will *publish*. A case still on the shelf
    // is withheld however many copies of it the archive has — see `archive` — so a season whose
    // every file is both live and archived publishes nothing, and a line reading "17 archived"
    // would have looked like a working endpoint on exactly that deployment.
    {
        let dir = sce_archive_dir();
        let live = live_scenarios();
        let held = archive::hashes(&dir).len();
        let open = archive::servable(&live, &dir).len();
        println!(
            "archive    {held} archived scenario version(s) at {} · {open} publishable · {} live and withheld{}",
            dir.display(),
            live.iter().filter(|p| p.exists()).count(),
            if open == 0 { " — /api/sce answers 404 for everything until a case is retired" } else { "" },
        );
    }

    // The star bar: an exam-mode case counts as cleared at or above this fraction of the
    // deterministic rubric, in basis points. The default is the canonical constant — the same
    // one the rubric files are test-pinned to — not a literal, so an unset env on any deploy
    // resolves to exactly the number the enforce-test guards and the two cannot drift silently.
    let star_pass_bps: u32 = std::env::var("VITALS_STAR_PASS_BPS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(vitals_progress::STAR_PASS_BPS);
    // The three bars a station's star is read against, carried as one value so the pass mark
    // the env moves and the two published bars above it can never be passed in the wrong order.
    // Only the pass mark is overridable: excellence and flawlessness are the published ladder.
    let star_bars = vitals_progress::StarBars::with_pass(star_pass_bps);

    // Which stations can host an exam — asked once, from the same function the commit gate and
    // the anchor scorer ask, and served to the page so the UI never keeps its own copy. Shelf
    // order, built from SETS: each gate's stations sit between the episode that taught them and
    // the door they open, and a Phase-5b member joins the moment its files land.
    let exam_eps: Vec<&'static str> = {
        let mut v: Vec<&'static str> = vec!["ep1"];
        for s in SETS {
            v.extend(s.members.iter().map(|m| m.id));
            v.push(s.opens);
        }
        v.into_iter().filter(|e| rubric_path(e).is_some()).collect()
    };

    // Station Sets v2 — resolved once against the disk. The hashes here are what let
    // /api/stars translate proven attempts back into set members without re-reading files.
    let set_states = resolve_sets();
    println!(
        "sets       {}",
        set_states
            .iter()
            .map(|st| {
                let p = st.members.iter().filter(|(_, h)| h.is_some()).count();
                format!("{} {}/{} live · need {} of {} (now {})",
                        st.set.gate, p, st.members.len(), st.set.need, st.ceiling(), st.need_now)
            })
            .collect::<Vec<_>>()
            .join(" · ")
    );

    // The gateway, once. Which *character* it plays is decided per request from `personas`
    // below — this used to be `Patient::connect(demo/ep1-en.json)`, one persona loaded at boot
    // and handed to every session in the season.
    let patient = patient::Patient::connect();
    match &patient {
        Some(p) => {
            let via = match p.backend() {
                patient::Backend::Local => "local model via Heimdall",
                patient::Backend::Cloud => "cloud model (local unreachable — fallback)",
            };
            println!("voice      {via}");
        }
        None => println!("voice      none — set HEIMDALL_API_KEY (local) or VITALS_VERTEX_URL (cloud)"),
    }

    // One persona per case, read off the disk through the same root as the scenarios. A case with
    // no file is not an error and never borrows another case's: it plays mute, and the boot line
    // says which ones so nobody has to discover it from a transcript.
    let personas = load_personas();
    let mute: Vec<&str> = every_case().into_iter().filter(|id| !personas.contains_key(*id)).collect();
    println!(
        "personas   {}/{} voiced{}",
        personas.len(),
        every_case().len(),
        if mute.is_empty() { String::new() } else { format!(" · mute: {}", mute.join(" ")) }
    );

    let chain = chain::Chain::connect();
    // Resume the tree this server was filling. Starting a new one every boot would strand every
    // leaf already anchored, because the proof is built from the leaf list and the old list is
    // what those leaves were anchored into.
    //
    // Keyed to this deployment rather than to a fixed name: two servers sharing a store used to
    // share the list and overwrite each other. Without a chain there is nothing to anchor into,
    // so the key is irrelevant and a fixed one keeps offline runs working.
    let tree_key = match &chain {
        Some(c) => {
            let (relay, program, rpc) = c.deployment();
            store::tree_key(&relay, &program, &rpc)
        }
        None => "offline".to_string(),
    };
    let tree = Arc::new(Mutex::new(store.get::<Tree>(TREE, &tree_key).unwrap_or_default()));
    match &chain {
        Some(c) => {
            let mut t = tree.lock().unwrap();
            if t.tree_id == 0 {
                t.tree_id = c.slot();
            }
            println!("chain      connected · slot {} · tree #{} · {} leaf/leaves",
                     c.slot(), t.tree_id, t.leaves.len());
            println!("relay      {} — pays fees, holds no player key", c.relay_pubkey());
        }
        None => println!("chain      not connected — set VITALS_PROGRAM_ID and start a validator to anchor"),
    }
    // What the donate page shows beside the address: the relay's balance and the treasury's,
    // read by the server rather than the browser (CORS, and a public RPC rate-limits per caller)
    // and cached, because this endpoint is public and ungated by design.
    let mut fuel = fuel::Fuel::open();
    mark("chain", "");
    println!("fuel       {}", fuel.describe());
    // Signed halves waiting on the browser, keyed by player. Never persisted: a blockhash goes
    // stale in about a minute, so a pending transaction that outlives the process is worthless.
    let pendings: Arc<Mutex<HashMap<String, PendingWork>>> = Arc::new(Mutex::new(HashMap::new()));
    // The author ledger's chain read, held for AUTHOR_COUNT_TTL. See /api/authors.
    let author_counts: AuthorCounts = Arc::new(Mutex::new(None));
    let ward_view: WardView = Arc::new(Mutex::new(None));
    let ward_pendings: WardPendings = Arc::new(Mutex::new(HashMap::new()));

    // The refill, on its own thread (CWF_PLAN.md ruling 11). The ward keeps running while we are
    // asleep and after 12 Oct, which is the whole point of admitting from a queue rather than by
    // hand — and nothing it decides is a decision this endpoint does not publish.
    //
    // Its own `Store` handle rather than a shared one: the backend is stateless (a directory, or
    // Firestore over REST), so a second handle costs nothing and saves putting the request loop's
    // store behind an Arc for one reader.
    if ward_mode() {
        println!(
            "ward       beds {} · door {} · refill every {}s",
            ward::BEDS,
            match ward_chain::door_here() {
                ward_chain::Door::Open => "open",
                ward_chain::Door::Preview => "preview — packs are taken, nobody is admitted",
                ward_chain::Door::Closed => "closed — packs are refused",
            },
            WARD_TICK.as_secs(),
        );
        let state = state_dir.clone();
        let root = scenario_root();
        std::thread::spawn(move || {
            let store = match store::Store::open(std::path::PathBuf::from(&state)) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("ward       no store, so no refill: {e}");
                    return;
                }
            };
            // The same complaint every minute is noise; a complaint that changed is news.
            let mut last_trouble = String::new();
            loop {
                std::thread::sleep(WARD_TICK);
                // The one pass, shared with `POST /api/ward/tick`. `None` means a scheduler's
                // request holds the gate this minute, which is the pass happening — not a fault.
                match one_pass(&store, &root) {
                    Some(Err(e)) => {
                        if e != last_trouble {
                            eprintln!("ward       no chain, so no refill: {e}");
                            last_trouble = e;
                        }
                    }
                    Some(Ok(_)) => last_trouble.clear(),
                    None => {}
                }
            }
        });

        // The heartbeat's other half. The page beats while it holds a head; this frees the head
        // when the beats stop. Two missed beats and a little for the wire — `ward::heads_to_free`
        // is the whole decision and it is tested away from this thread.
        //
        // The ward's own authority, not a signature kept in a drawer: a release the holder signed
        // in advance cannot be held for them, because a blockhash on this chain is worth about
        // twenty-six seconds and the silence worth acting on is longer than that.
        let heard = Arc::clone(&beats);
        std::thread::spawn(move || loop {
            std::thread::sleep(BEAT_SWEEP);
            let now = now_ms();
            let gone = {
                let map = heard.lock().unwrap();
                ward::heads_to_free(&map, now, BEAT_GRACE_MS)
            };
            if gone.is_empty() {
                continue;
            }
            match ward_chain::WardChain::connect() {
                Ok(chain) => {
                    for patient in gone {
                        match chain.free_shift(patient) {
                            Ok(sig) => {
                                heard.lock().unwrap().remove(&patient);
                                println!("ward       freed the head on {patient} — the page stopped beating · {sig}");
                            }
                            Err(e) => eprintln!("ward       could not free the head on {patient}: {e}"),
                        }
                    }
                }
                Err(e) => eprintln!("ward       no chain, so no head can be freed: {e}"),
            }
        });
    }
    let settled: Settled = Arc::new(Mutex::new(HashMap::new()));
    // Where AUTHORS.json and the archive index live, resolved once.
    let authors_root = scenario_root();
    // The author payout, or nothing at all. `from_env` refuses rather than degrades: a rate with
    // no wallet, a key inside the repository, a cluster that is not devnet, or a mistyped cap
    // stops the server here instead of paying somebody by accident later.
    // The same URL `chain` uses, read the same way, so the payout and the proof can never end up
    // looking at different clusters.
    let rpc_url = std::env::var("VITALS_RPC").unwrap_or_else(|_| "http://127.0.0.1:8899".into());
    let payer: Option<Arc<payout::Payer>> = match payout::Payer::from_env(&rpc_url) {
        Ok(p) => p.map(Arc::new),
        Err(e) => {
            eprintln!("payout    refusing to start: {e}");
            std::process::exit(1);
        }
    };
    match &payer {
        Some(p) => println!(
            "payout     {} lamports per proven replay · {} bps to the platform · wallet {} · {} authorised",
            p.rate, p.platform_bps, p.address(), p.allowlist.len()
        ),
        None => println!("payout     off — VITALS_PAYOUT_LAMPORTS is 0 or unset"),
    }

    match (&token, loopback) {
        (Some(_), _) => println!("auth       bearer token required on anchor · claim · say"),
        (None, true) => println!("auth       none — loopback only, so the blast radius is this machine"),
        (None, false) => unreachable!("refused to start above"),
    }
    // The address it actually got, not the one it asked for. Binding to :0 is how a test gets a
    // port nobody else has, and that is useless if the server then reports the zero back.
    let bound = server
        .server_addr()
        .to_ip()
        .map(|a| a.to_string())
        .unwrap_or_else(|| addr.clone());
    mark("listening", "");
    println!("Vitals — play at http://{bound}");

    // One slow local model, and /api/say holds a worker for as long as it takes. Without a
    // ceiling a single caller can occupy the GPU indefinitely.
    let mut said: Vec<Instant> = Vec::new();
    const SAY_PER_MIN: usize = 20;

    // `mut` for exactly one route: reading a request body needs the request mutably, and the
    // match below borrows it for the whole of its scrutinee. See `/api/review`.
    // ── the readers ─────────────────────────────────────────────────────────────────────
    //
    // A pass holds the loop below for eleven to fifteen seconds at the census the ward runs at,
    // and about eighty on a cold start. Cloud Run is told this container takes eight requests at
    // once and answers one; during a pass it counts eight in hand with seven unserved and refuses
    // the ninth — which is what a stranger opening a bedside met three times today. max-instances
    // stays 1: the anchoring tree is in memory and a second instance would hold another.
    //
    // So the reads leave the loop and nothing else does. Every take, every anchor, every step is
    // still answered in the order it arrived, by the one thread that has always answered them; a
    // write arriving mid-pass should wait for the pass rather than race it. Reads wait for
    // nothing, because they change nothing — `serve::is_read_only` is that claim, path by path.
    //
    // Ward only. The Eternal entry is not redeployed for this sprint and has none of this
    // problem; changing how it serves to fix something it does not have is a risk with no return.
    const READERS: usize = 4;
    let (read_tx, read_rx) = std::sync::mpsc::channel::<tiny_http::Request>();
    let read_rx = Arc::new(Mutex::new(read_rx));
    if ward_mode() {
        for _ in 0..READERS {
            let rx = Arc::clone(&read_rx);
            let view = Arc::clone(&ward_view);
            let counts = Arc::clone(&usage);
            let page_token = token.clone();
            let state = state_dir.clone();
            std::thread::spawn(move || {
                // Its own handle on the same files. Opened here rather than shared so a reader
                // cannot be waiting on a lock the writing loop is holding.
                let store = match store::Store::open(std::path::PathBuf::from(&state)) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("readers    no store, so this reader is not serving: {e}");
                        return;
                    }
                };
                loop {
                    // The lock is held only while waiting for and taking one request; the answer
                    // is built after it is released, which is what lets four of these overlap.
                    let got = { rx.lock().unwrap().recv() };
                    let Ok(req) = got else { return };
                    let path = req.url().split('?').next().unwrap_or("/").to_string();
                    let resp = read_response(&req, &path, &view, &store, &state, &counts, &page_token);
                    let _ = match resp {
                        Some(r) => req.respond(r),
                        // Routed here by the table but not served by it: answer rather than drop,
                        // and the table's test is what stops the two disagreeing.
                        None => req.respond(
                            Response::from_data(b"not found".to_vec()).with_status_code(404),
                        ),
                    };
                }
            });
        }
        // Announced on stderr, not stdout. Every harness in this repository reads the server's
        // stdout only until the line that names the port and then drops the pipe; a `println!`
        // after that point kills the server with a broken pipe, and the test sees a connection
        // that failed rather than a server that was told to stop talking. This line cost an hour
        // of looking at a concurrency result that was really a dead process.
        eprintln!("readers    {READERS} answering the board and the pages beside a pass");
    }

    for mut req in server.incoming_requests() {
        let url = req.url().to_string();
        let path = url.split('?').next().unwrap_or("/").to_string();

        // Off the writing loop, if it changes nothing. A send that finds no reader is answered
        // here instead — a request is never dropped for want of a thread.
        if ward_mode() && serve::is_read_only(req.method().as_str(), &path) {
            if let Err(std::sync::mpsc::SendError(back)) = read_tx.send(req) {
                let resp =
                    read_response(&back, &path, &ward_view, &store, &state_dir, &usage, &token);
                let _ = match resp {
                    Some(r) => back.respond(r),
                    None => back
                        .respond(Response::from_data(b"not found".to_vec()).with_status_code(404)),
                };
            }
            continue;
        }

        // A session is rebuilt here, the first time its id is asked for, rather than all of them at
        // boot. One place instead of the twenty handlers that take an `id`, so no route can forget.
        //
        // An id nobody saved falls through and is answered "no such session" as it always was. An
        // id that *is* saved and cannot be replayed is answered here, because the answer has to
        // reach the page whatever it was asking for: it tells it to stop beating, which turns a bed
        // held for the ten-minute lease into one freed in seventy-five seconds.
        if let Some(id) = param(&url, "id") {
            let why = {
                let known = unrebuildable.lock().unwrap().get(&id).cloned();
                match known {
                    Some(why) => Some(why),
                    None => {
                        rebuilds.once(
                            &id,
                            || sessions.lock().unwrap().contains_key(&id),
                            || {
                                let Some(saved) = store.get::<Saved>(SESSIONS, &id) else { return };
                                let prior = saved
                                    .ward
                                    .as_ref()
                                    .and_then(|w| ward_rebuild(&store, w.patient_id));
                                match Session::restore(
                                    saved,
                                    prior.as_ref(),
                                    &|h| ward_chain::tape_by_hash(&store, h),
                                    &ward_chain::cached_dater(&store),
                                ) {
                                    Ok(s) => {
                                        sessions.lock().unwrap().insert(id.clone(), s);
                                    }
                                    Err(e) => {
                                        // Kept rather than deleted. The tape is the only copy of
                                        // what somebody did, and a run that will not replay is a
                                        // thing to look at, not to tidy away at three in the
                                        // morning. The sweep collects it when nobody comes back.
                                        eprintln!("session {id} will not replay: {e}");
                                        unrebuildable.lock().unwrap().insert(id.clone(), e);
                                    }
                                }
                            },
                        );
                        unrebuildable.lock().unwrap().get(&id).cloned()
                    }
                }
            };
            if let Some(why) = why {
                let _ = req.respond(json_code(rebuild::cannot_rebuild(&why), 409));
                continue;
            }
        }

        // The apex answers with the front door — the landing, and the two documents about the
        // company — and anything deeper moves permanently to the game origin. 301 on purpose:
        // the split is a recorded decision, not a phase. Everything the apex serves carries its
        // own short cache life so a proxy caching the apex never holds anything of the game's.
        if host_of(&req) == APEX {
            let resp = match apex_target(&url) {
                None => html_kept(&front_door(&path), "public, max-age=300"),
                Some(to) => Response::from_string("")
                    .with_status_code(301)
                    .with_header(Header::from_bytes(&b"Location"[..], to.as_bytes()).unwrap()),
            };
            let _ = req.respond(resp);
            continue;
        }

        if guarded(&path) && !bearer_ok(&req, &token) {
            let _ = req.respond(
                Response::from_string(r#"{"error":"unauthorised"}"#)
                    .with_status_code(401)
                    .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap()),
            );
            continue;
        }
        if door(&path) {
            // No secret of their own, no doors. Never the page's token as a fallback: that is the
            // bug this split exists for, and a fallback is how it would come back.
            let Some(want) = door_token.clone() else {
                let _ = req.respond(json_code(serde_json::json!({
                    "error": "this ward has no VITALS_DOOR_TOKEN, so the factory's doors are \
                              shut. They take their own secret and never the page's — the page's \
                              is printed into bay.js for every visitor"
                }), 503));
                continue;
            };
            if !bearer_ok(&req, &Some(want)) {
                let _ = req.respond(json_code(serde_json::json!({
                    "error": "unauthorised — the factory's doors take VITALS_DOOR_TOKEN, which \
                              is not the token the page carries"
                }), 401));
                continue;
            }
        }
        // ── the reviewer's answers: the one route in this server that reads a body ──────────
        //
        // Everything else is a GET with query parameters, including the player's own free text
        // (`/api/say?q=…`). This one cannot be. A reviewer's answers run to eight thousand Thai
        // characters in a box, percent-encoding Thai costs nine bytes a character, and a full
        // submission would be roughly 72KB in one URL — past every browser's limit and most
        // proxies'. Splitting it per question only moves the failure into the notes box.
        //
        // It sits *here*, ahead of the match, because that is the entire architectural change:
        // `Request::as_reader` needs `&mut req`, and the match below borrows `req` immutably for
        // as long as its scrutinee lives. One block in front of the loop's match, rather than a
        // second shape of handler inside it.
        //
        // **There is no caller identity, deliberately, and none is invented here.** Every other
        // notion of "who is this" in this server answers a question a review does not ask: the
        // session id says which run in progress this is, and `player` says whose run it is —
        // both are about a run, and a review is not attached to one. The two people this exists
        // for have no player key, no account and no wallet, and requiring either would mean the
        // review does not arrive. `role` and `name` in the body are self-declared attribution,
        // not authentication; `name` is optional so an uncomfortable answer can still be sent.
        // The two identity mechanisms that *do* apply are reused rather than duplicated: the
        // route is declared public in `guarded` alongside every other route, pinned by the test
        // that reads that table, and the caller is counted by the same `client_addr` window
        // `/api/new` already uses.
        //
        // Nothing here touches a session, the tape, the tree or a rubric. A reviewer's opinion is
        // data recorded alongside a run and never an input to one; `tests/review.rs` proves the
        // leaf and the mark sheet are byte-identical across a submission.
        if req.method() == &Method::Post && path == "/api/review" {
            // Storage, not inference, so the window and not the ceiling — the same call
            // `/api/new` makes. Generous next to the two or three submissions a real reviewer
            // sends, and the only thing standing between a public endpoint and a full disk.
            if let meter::Verdict::SlowDown { retry_secs } =
                meter.allow_free(&format!("review:{}", client_addr(&req)), &store)
            {
                let _ = req.respond(json_code(
                    serde_json::json!({
                        "error": "too many submissions from this address — give it a minute",
                        "retry_in": retry_secs,
                    }),
                    429,
                ));
                continue;
            }
            let resp = match read_body(&mut req, REVIEW_MAX) {
                Err(BadBody::TooLong) => json_code(
                    serde_json::json!({ "error": "too long", "limit": REVIEW_MAX }),
                    413,
                ),
                Err(BadBody::NotText) => {
                    json_code(serde_json::json!({ "error": "unreadable" }), 400)
                }
                Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
                    Err(_) => json_code(serde_json::json!({ "error": "not json" }), 400),
                    // Every refusal `review.rs` can make is the caller's, and each says which
                    // one it was: a form that posts the wrong shape has to be debuggable by
                    // whoever is holding it, and "empty" is a real answer to a real mistake.
                    Ok(v) => match review::Submission::from_json(&v, now_secs()) {
                        Err(e) => json_code(serde_json::json!({ "error": e }), 400),
                        // `file`, not a bare put: a reviewer who taps Send twice on a bad
                        // connection gets one record, and the id it quotes back is the record
                        // that is actually on disk — on a resend that is the first one's.
                        Ok(s) => match s.file(&store) {
                            Ok(filed) => json(serde_json::json!({
                                "ok": true, "id": filed.id, "answers": filed.answers.len(),
                            })),
                            // The page hands the reviewer their answers to copy out on any
                            // non-200. That is the whole reason this is allowed to fail.
                            Err(_) => json_code(
                                serde_json::json!({ "error": "could not store" }),
                                500,
                            ),
                        },
                    },
                },
            };
            let _ = req.respond(resp);
            continue;
        }
        if path == "/api/say" {
            said.retain(|t| t.elapsed() < Duration::from_secs(60));
            if said.len() >= SAY_PER_MIN {
                let _ = req.respond(json(serde_json::json!({ "error": "too many questions — give the patient a moment" })));
                continue;
            }
            said.push(Instant::now());
        }

        let resp = match (req.method(), path.as_str()) {
            (Method::Get, "/") => {
                // Where the people on the ward came from, on the ward host only. A `src` is a
                // string we handed out on a link, and the tally holds our own string or nothing —
                // see `usage::channel`. Nothing about the person is read on the way here.
                if ward_mode() {
                    usage.lock().unwrap().arrived(param(&url, "src").as_deref().and_then(usage::channel), &store);
                }
                let page = compose(if ward_mode() { WORLD } else { LANDING });
                let resp = squeezed(&req, page.into_bytes(), b"text/html; charset=utf-8")
                    .with_header(never_kept());
                let _ = req.respond(resp);
                continue;
            }
            (Method::Get, "/play") => {
                // The page is served by the same process that holds the token, so handing it over
                // does not widen anything: reaching the page and reaching the API are one
                // boundary. The token itself now rides on /bay.js, which this same process serves.
                let page = compose(PAGE);
                let _ = req.respond(
                    Response::from_string(page).with_header(
                        Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..])
                            .unwrap(),
                    ),
                );
                continue;
            }
            // ── the pitch ───────────────────────────────────────────────────────
            // Unguarded, like the bay itself. Everything in the deck is already public in the
            // repository, so a token here would protect nothing and only stop it opening.
            //
            // The deck, and only the deck. `/slides/script` served the speaking notes through the
            // same open door and is gone; the 404 at the bottom of this match is the right answer
            // for it, and `session.rs` holds it to that.
            (Method::Get, "/slides") | (Method::Get, "/slides/") => {
                let _ = req.respond(html(&format!("{DECK}{PRESENT}")));
                continue;
            }

            // ── film ────────────────────────────────────────────────────────────
            // Story Mode does not show a still and call it a patient: it loops a per-state clip
            // and cuts to a full-frame cutscene on a beat. Both are already rendered.
            (Method::Get, p) if p.starts_with("/clip/") => {
                let name = p.trim_start_matches("/clip/");
                // Nothing but a bare clip name — no traversal into the rest of the disk.
                let safe = name
                    .strip_suffix(".mp4")
                    .filter(|n| n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
                match safe.and_then(|n| std::fs::read(clips_dir().join(format!("{n}.mp4"))).ok()) {
                    Some(bytes) => {
                        let _ = req.respond(
                            Response::from_data(bytes)
                                .with_header(Header::from_bytes(&b"Content-Type"[..], &b"video/mp4"[..]).unwrap())
                                .with_header(Header::from_bytes(&b"Cache-Control"[..], &b"public, max-age=86400"[..]).unwrap()),
                        );
                        continue;
                    }
                    None => Response::from_string("no such clip").with_status_code(404),
                }
            }
            // Above the `/img/cases/` arm for the same reason that one is above `/img/`: a longer
            // prefix has to be tried first or the shorter one answers for it. This is the only
            // image route that reads the disk — the files are shot per station and arrive after
            // the wiring — and the name is whitelisted on both halves before anything is opened.
            (Method::Get, p) if p.starts_with("/img/cases/states/") => {
                let name = p.trim_start_matches("/img/cases/states/");
                let hit = name
                    .strip_suffix(".jpg")
                    .and_then(|n| n.rsplit_once('_'))
                    .and_then(|(station, state)| station_still_path(station, state))
                    .and_then(|f| std::fs::read(f).ok());
                match hit {
                    Some(bytes) => {
                        let _ = req.respond(
                            Response::from_data(bytes)
                                .with_header(Header::from_bytes(&b"Content-Type"[..], &b"image/jpeg"[..]).unwrap())
                                .with_header(Header::from_bytes(&b"Cache-Control"[..], &b"public, max-age=86400"[..]).unwrap()),
                        );
                        continue;
                    }
                    // Not shot yet, or not a name this build recognises. The bay's stem is the
                    // answer to both, and it is already on the stage.
                    None => Response::from_string("no such still").with_status_code(404),
                }
            }
            // Above the `/img/` arm on purpose, and it has to stay there. That arm strips `.jpg`
            // and searches STILLS only, so `/img/cases/cxr-normal-1.png` would match it first and
            // 404 with the files sitting right there in the binary. The Content-Type comes from
            // the table rather than from the suffix, because this directory mixes PNG and JPEG.
            (Method::Get, p) if p.starts_with("/img/cases/") => {
                let key = p.trim_start_matches("/img/cases/");
                match CASE_IMG.iter().find(|(k, _, _)| *k == key) {
                    Some((_, bytes, mime)) => {
                        let _ = req.respond(
                            Response::from_data(*bytes)
                                .with_header(Header::from_bytes(&b"Content-Type"[..], mime.as_bytes()).unwrap())
                                .with_header(Header::from_bytes(&b"Cache-Control"[..], &b"public, max-age=86400"[..]).unwrap()),
                        );
                        continue;
                    }
                    None => Response::from_string("no such film").with_status_code(404),
                }
            }
            (Method::Get, p) if p.starts_with("/img/") => {
                let key = p.trim_start_matches("/img/").trim_end_matches(".jpg");
                match STILLS.iter().chain(KEY_ART.iter()).find(|(k, _)| *k == key) {
                    Some((_, bytes)) => {
                        let _ = req.respond(
                            Response::from_data(*bytes)
                                .with_header(Header::from_bytes(&b"Content-Type"[..], &b"image/jpeg"[..]).unwrap())
                                .with_header(Header::from_bytes(&b"Cache-Control"[..], &b"public, max-age=86400"[..]).unwrap()),
                        );
                        continue;
                    }
                    None => Response::from_string("no such still").with_status_code(404),
                }
            }
            (Method::Get, p @ ("/device/monitor" | "/device/vent" | "/device/pump")) => {
                let page = match p {
                    "/device/vent" => VENT,
                    "/device/pump" => PUMP,
                    _ => MONITOR,
                };
                let _ = req.respond(Response::from_string(page).with_header(
                    Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap(),
                ));
                continue;
            }
            (Method::Get, "/device/vitals") => {
                // The monitor identifies the bed by header, the way the device page already does.
                let sid = req
                    .headers()
                    .iter()
                    .find(|h| h.field.equiv("x-embla-session"))
                    .map(|h| h.value.as_str().to_string())
                    .unwrap_or_default();
                let map = sessions.lock().unwrap();
                match map.get(&sid) {
                    None => json(serde_json::json!({})),
                    Some(s) => {
                        // The same reading the bay's own rail is served, from the same rule: a
                        // saturation and a cuff pressure need flowing blood, so in an arrest they
                        // are absent rather than stale. The device page has always keyed on
                        // `pulse`; now it cannot disagree with the rail even if it stopped.
                        let m = reading::Reading::of(&s.state.vitals);
                        let mut body = serde_json::json!({
                            "hr": m.hr, "spo2": m.spo2, "sbp": m.sbp, "dbp": m.dbp,
                            "rr": m.rr, "temp": m.temp, "gcs": m.gcs,
                            "status": format!("{:?}", s.state.status),
                            // The scenario clock, which is the only clock anything at this
                            // bedside may date a reading against. The bay ticks two to three
                            // scenario seconds every 700 ms of wall time, so a pane left to
                            // `Date.now()` ages its reading against a clock nobody else on the
                            // screen is watching: the cuff printed the pressure the patient
                            // walked in with, "24 s ago", beside a bay clock reading 0:56.
                            // `pump.html` has read this field since it was written and has been
                            // getting `undefined` — which is its 0 mL infused, on every run.
                            "t_sec": s.state.t_sec(),
                            "rhythm": m.rhythm,
                            // A monitor that invents a pulse is worse than one that misses an
                            // arrest, so this comes from the rhythm rather than from the numbers.
                            "pulse": m.pulse,
                            "shockable": m.shockable,
                            "paused": false,
                        });
                        // A device pane holds no words it is not allowed to show, so the words
                        // are sent to it — or they are not. The numbers always travel: an exam
                        // hides no instrument, and a ventilator that would not show its own
                        // pressures is not a ventilator. What travels only outside the seal is
                        // the sentence that says what the pressures mean, because reading them
                        // is the thing being marked. Absent, not null and not conditional: a
                        // sealed reply has no such key, so there is nothing to notice and
                        // nothing to flip. See [`VENT_READ_WIDE`].
                        if !s.sealed() {
                            body["vent_read"] = serde_json::json!({
                                "wide": VENT_READ_WIDE,
                                "narrow": VENT_READ_NARROW,
                            });
                        }
                        json(body)
                    }
                }
            }
            (Method::Get, "/api/new") => {
                // A run is a stored document, and a loop hammering "new" is a bill with no
                // learner attached. The window only, never the ceiling: opening a run must
                // survive the month's voice budget running out.
                //
                // **What is counted differs by host.** The Eternal entry counts an address, which
                // is one learner at one bay. A public ward counts the browser's own key: a school
                // is one address, and thirty students opening patients in the same minute are not
                // one abuser — the seventh of them was refused until 16 ก.ย. The address is still
                // counted behind the key, for browsers that have none yet, against a classroom's
                // budget rather than a reader's.
                let counted = match (ward_mode(), param(&url, "player").and_then(|p| pubkey(&p))) {
                    (true, Some(key)) => Some((format!("new:key:{key}"), OPENS_PER_KEY_MIN, OPENS_PER_KEY_DAY, "this browser")),
                    (true, None) => Some((format!("new:{}", client_addr(&req)), OPENS_PER_ADDR_MIN, OPENS_PER_ADDR_DAY, "this address")),
                    (false, _) => None,
                };
                let verdict = match &counted {
                    Some((key, per_min, per_day, _)) => meter.allow_budget(key, *per_min, *per_day, &store),
                    None => meter.allow_free(&format!("new:{}", client_addr(&req)), &store),
                };
                if let meter::Verdict::SlowDown { retry_secs } = verdict {
                    let (what, per_min) = counted
                        .as_ref()
                        .map(|(_, m, _, what)| (*what, *m))
                        .unwrap_or(("this address", 0));
                    let _ = req.respond(json_code(serde_json::json!({
                        "error": if per_min > 0 {
                            format!("{what} has opened {per_min} runs in a minute, which is all \
                                     this ward counts for one — give it a minute")
                        } else {
                            "too many new runs from this address — give it a minute".to_string()
                        },
                        "retry_in": retry_secs,
                    }), 429));
                    continue;
                }
                // A shift on the ward: the same bay, started on the patient the chain says is
                // in that bed (producer's ruling, 16 ก.ย. — a parameter, never a second page).
                // `patient` on the ward host is the whole request, well formed or not. It used to
                // be read as "a patient if it parses, otherwise never mind", and never-minding
                // fell through to `ep`, defaulted to ep1 and opened a practice run of the season's
                // first episode on the host whose front page is a globe (ruling 13: nothing of the
                // season lives here). A mistyped id is a mistyped id, and it is answered as one.
                if ward_mode() && param(&url, "patient").is_some_and(|p| p.parse::<u64>().is_err()) {
                    let _ = req.respond(json_code(serde_json::json!({
                        "error": "a patient id is a whole number, and this host has no episodes to \
                                  open instead",
                        "the_ward_is": "/api/ward"
                    }), 404));
                    continue;
                }
                if let Some(patient_id) = param(&url, "patient").and_then(|p| p.parse::<u64>().ok()) {
                    if !ward_mode() {
                        let _ = req.respond(json_code(serde_json::json!({
                            "error": "this host has no ward",
                            "the_ward_is": "https://world.vitals.academy"
                        }), 404));
                        continue;
                    }
                    match open_shift(&store, patient_id) {
                        Ok((mut s, ward)) => {
                            s.owner = param(&url, "player").and_then(|p| pubkey(&p)).map(|k| k.to_string());
                            let id = fresh_id();
                            let view = s.view(lang::language(param(&url, "lang").as_deref()));
                            let mut map = sessions.lock().unwrap();
                            map.insert(id.clone(), s);
                            persist(&store, &id, map.get_mut(&id).expect("just inserted"), true);
                            drop(map);
                            let _ = req.respond(json(serde_json::json!({
                                "id": id, "view": view, "ward": ward
                            })));
                        }
                        // Said in words rather than as a code: every one of these is a thing the
                        // person standing at her bed can understand and act on.
                        Err(why) => {
                            let _ = req.respond(json_code(serde_json::json!({ "error": why }), 409));
                        }
                    }
                    continue;
                }
                // A review run: a held case, opened to be read. Before the episode path, because
                // `?review=` with no `ep` used to fall through to it and open EP1 — on the host
                // whose front page is a globe and whose ruling is that nothing of the season lives
                // here.
                if let Some(case_id) = param(&url, "review") {
                    if !ward_mode() {
                        let _ = req.respond(json_code(serde_json::json!({
                            "error": "this host has no ward and no catalogue to review",
                            "the_ward_is": "https://world.vitals.academy"
                        }), 404));
                        continue;
                    }
                    match open_review(&store, &case_id) {
                        Ok((mut s, review)) => {
                            s.owner = param(&url, "player").and_then(|p| pubkey(&p)).map(|k| k.to_string());
                            let id = fresh_id();
                            let view = s.view(lang::language(param(&url, "lang").as_deref()));
                            let mut map = sessions.lock().unwrap();
                            map.insert(id.clone(), s);
                            persist(&store, &id, map.get_mut(&id).expect("just inserted"), true);
                            drop(map);
                            // Not counted in `usage`: nobody played a case here, somebody read one.
                            // The month's figures are about learners at bedsides.
                            let _ = req.respond(json(serde_json::json!({
                                "id": id, "view": view, "review": review
                            })));
                        }
                        Err(why) => {
                            let _ = req.respond(json_code(serde_json::json!({ "error": why }), 404));
                        }
                    }
                    continue;
                }
                let ep = param(&url, "ep").unwrap_or_else(|| "ep1".into());
                // A case id this server does not have is refused here, before anything is
                // created or counted.
                //
                // `scenario_path` answers an unknown id with EP1's file, so without this line
                // `?ep=<anything>` opened a real, playable run of EP1 filed under whatever the
                // caller typed. Two things went wrong at once: the shelf's numbers gained a case
                // that does not exist, and `usage.by_case` — a durable map with no bound on its
                // keys — took a key from the query string on a public endpoint that needs no
                // account. `no-such-ep-at-all` is in the live tally today because somebody
                // tried it.
                //
                // The hazard was already written down one match arm above the fallback, for
                // station ids: "never the EP1 fallback, because playing EP1 under a station's
                // name would anchor the wrong case." It is the same sentence for every other id.
                if !every_case().contains(&ep.as_str()) {
                    let _ = req.respond(json_code(serde_json::json!({
                        "error": "no such case",
                    }), 404));
                    continue;
                }
                match new_session(&ep) {
                    Ok(mut s) => {
                        s.owner = param(&url, "player").and_then(|p| pubkey(&p)).map(|k| k.to_string());
                        // One run opened. The key is a browser's, not a person's — it is folded
                        // into a month-salted fingerprint and never stored as itself.
                        usage.lock().unwrap().started(&ep, s.owner.as_deref(), &store);
                        let id = fresh_id();
                        let view = s.view(lang::language(param(&url, "lang").as_deref()));
                        let mut map = sessions.lock().unwrap();
                        map.insert(id.clone(), s);
                        persist(&store, &id, map.get_mut(&id).expect("just inserted"), true);
                        drop(map);
                        json(serde_json::json!({ "id": id, "view": view }))
                    }
                    Err(e) => json(serde_json::json!({ "error": e })),
                }
            }
            (Method::Get, "/api/step") => {
                let id = param(&url, "id").unwrap_or_default();
                let caller = param(&url, "player");
                let mut map = sessions.lock().unwrap();
                match map.get_mut(&id).filter(|s| s.answers_to(caller.as_deref())) {
                    None => no_such_session(),
                    // A shift nobody has taken is a chart you may read and not a patient you may
                    // treat. The page has refused this since the morning of 16 ก.ย.; this is the
                    // server refusing it, which is the half a scripted client cannot skip.
                    Some(s) if ward::may_step(s.ward.is_some(), s.commit.is_some(), s.handed_over).is_err() => {
                        let why = ward::may_step(s.ward.is_some(), s.commit.is_some(), s.handed_over)
                            .unwrap_err();
                        drop(map);
                        let _ = req.respond(json_code(serde_json::json!({ "error": why }), 409));
                        continue;
                    }
                    Some(s) => {
                        // Read before anything moves: the increment belongs to the transition
                        // into a finished run, not to every request made after it. It is also
                        // what decides whether this request is the one that rings the bell.
                        let was_over = s.over();
                        let acted = !was_over && param(&url, "do").is_some();
                        // ── nothing lands on a run that is already over ─────────────────────
                        // Post-bell orders and ticks used to go on the tape: the engine ignored
                        // them, but `Step::Tick` was pushed unconditionally, so a client that
                        // kept polling grew the tape — and the tape is the leaf. A finished run
                        // has to hash the same however long the browser is left open, and a
                        // candidate must not be able to keep working after time is called.
                        if !was_over {
                            if let Some(act) = param(&url, "do") {
                                // Recognition happens here, once, and its answer goes on the tape beside the
                                // words. An order nobody understood is recorded as exactly that — an
                                // empty resolution — so replay stays faithful to a run in which nothing
                                // happened, even after the matcher learns the phrase.
                                //
                                // The scenario's own matcher answers first and its answer is final. Only
                                // when *it* declines does the language layer get a turn, and all it may do
                                // is offer the English order a non-English phrase names — which the same
                                // matcher then rules on. So a translation can add recognition and can
                                // never redirect an order a case author already spelled out.
                                let id = resolve_order(&s.state, &act);
                                // ── the defibrillator, when the case did not claim the words ──
                                // Second, never first, so the rule above holds for it too: a
                                // station that defines its own shock intervention keeps it. Only
                                // when the scenario has declined does the order reach the
                                // physiology — which is what makes typing "shock" on `ep4` chart
                                // the same thing that pressing the button on `ep4` charts, on a
                                // case whose author never wrote the word.
                                if let Some(j) = id.is_empty().then(|| shock_order(&act)).flatten() {
                                    let (_, emitted) = s.state.defibrillate(j);
                                    s.beats.extend(emitted.iter().map(render_beat));
                                    // The joules, not the phrase. Recognition happened here,
                                    // once, exactly as it does for `Step::Act`.
                                    s.tape.push(Step::Shock(j));
                                } else {
                                    // By id, not by text: the id is what the tape carries and what
                                    // replay re-runs, so the run on screen and the run a verifier
                                    // recomputes are the same run even when the words that started
                                    // it were in another language.
                                    let emitted = if id.is_empty() {
                                        s.state.apply(&act)
                                    } else {
                                        s.state.apply_id(&id)
                                    };
                                    s.beats.extend(emitted.iter().map(render_beat));
                                    s.tape.push(Step::acted(&act, &id));
                                    // The picture the order asked for, if this station has one. It
                                    // hangs off the id the tape already carries and goes nowhere
                                    // near it — the line above is the whole of what replay sees.
                                    if let Some(f) = film_for(&s.ep, &id) {
                                        if !s.films.iter().any(|x| x.file == f.file) {
                                            s.films.push(f);
                                        }
                                    }
                                }
                            }
                            if let Some(dt) = param(&url, "tick").and_then(|v| v.parse::<f64>().ok()) {
                                let emitted = s.state.tick(dt);
                                s.beats.extend(emitted.iter().map(render_beat));
                                s.tape.push(Step::Tick(dt));
                            }
                        }
                        // ── the announced time limit, actually ringing the bell ─────────────
                        // The tick that carries the clock past what the card advertises is the
                        // tick that ends the station, in the same request, so nothing ever reads
                        // a run that is `over` and has not been resolved. On the crossing only:
                        // once `over` the branch above stops the tape, so this cannot fire twice.
                        if !was_over && s.over() {
                            if let Err(e) = s.ring_the_bell() {
                                eprintln!("could not ring the bell on {id}: {e}");
                            }
                        }
                        let v = s.view(lang::language(param(&url, "lang").as_deref()));
                        count_finish(&mut usage.lock().unwrap(), s, was_over, &store);
                        // Nothing to write for a run that was already over: it took nothing on
                        // this request, so the bytes on disk are the bytes already there. A poll
                        // left running against a finished case must not be a write per second.
                        if !was_over {
                            persist(&store, &id, s, acted || s.over());
                        }
                        json(v)
                    }
                }
            }
            // ── the candidate says they are done ────────────────────────────────
            // The control the bay did not have. Embla's `← จบเคส` is not it: that abandons the
            // encounter without submitting or scoring, which is an escape hatch and not a finish.
            //
            // This ends the attempt the only honest way — see [`Session::ring_the_bell`]. It
            // takes no argument beyond the run: there is nothing to choose, because choosing
            // when to stop the patient is the cheat this exists to make impossible.
            (Method::Get, "/api/finish") => {
                let id = param(&url, "id").unwrap_or_default();
                let caller = param(&url, "player");
                let mut map = sessions.lock().unwrap();
                match map.get_mut(&id).filter(|s| s.answers_to(caller.as_deref())) {
                    None => no_such_session(),
                    Some(s) => {
                        let was_over = s.over();
                        if !was_over {
                            if let Err(e) = s.ring_the_bell() {
                                let _ = req.respond(json(serde_json::json!({ "error": e })));
                                continue;
                            }
                        }
                        let v = s.view(lang::language(param(&url, "lang").as_deref()));
                        count_finish(&mut usage.lock().unwrap(), s, was_over, &store);
                        if !was_over {
                            persist(&store, &id, s, true);
                        }
                        json(v)
                    }
                }
            }
            // ── the kit ─────────────────────────────────────────────────────────
            // Attaching a device is not a free-text order. It is a pick from a catalogue with a
            // setting, and the flowmeter has to read what the learner actually chose — the same
            // shape Embla's device tray uses, so the chart and a debrief can quote the number.
            (Method::Get, "/api/kit") => {
                let id = param(&url, "id").unwrap_or_default();
                let caller = param(&url, "player");
                let dev = param(&url, "dev").unwrap_or_default();
                let set = param(&url, "set").and_then(|v| v.parse::<f64>().ok());
                let off = param(&url, "off").is_some();
                let mut map = sessions.lock().unwrap();
                match map.get_mut(&id).filter(|s| s.answers_to(caller.as_deref())) {
                    None => no_such_session(),
                    Some(s) => {
                        // The picker goes through the same matcher a typed order does, so it can
                        // end a run the same way. Counted from the same edge — and, like `/api/step`,
                        // it lands nothing at all on a run the bell has already ended.
                        let was_over = s.over();
                        if was_over {
                            // Time has been called. Nothing more goes on the patient, and
                            // nothing more goes on the tape.
                        } else if dev == "defib" {
                            // ── not a device, and never was ──────────────────────────────
                            // It attaches nothing, it has no `off`, and what it does depends on
                            // a rhythm rather than on a catalogue. Everything below this line
                            // asks the equipment list what is already on the patient, and for a
                            // defibrillator every one of those answers is meaningless.
                            //
                            // The phrase is still minted, and then read back by the same
                            // recogniser a typed order goes through — so the button cannot drift
                            // away from the words. `kit_phrase("defib", Some(200))` is
                            // "defibrillate 200 j", and typing that produces this identical step.
                            let j = kit_phrase(&dev, set)
                                .as_deref()
                                .and_then(shock_order)
                                .unwrap_or(DEFIB_JOULES);
                            let (_, emitted) = s.state.defibrillate(j);
                            s.beats.extend(emitted.iter().map(render_beat));
                            s.tape.push(Step::Shock(j));
                        } else if off {
                            s.state.detach(&dev);
                            s.tape.push(Step::Off(dev.clone()));
                        } else if s.state.has_equipment(&dev)
                            && (set.is_none() || s.state.equipment_setting(&dev) == set)
                        {
                            // Already on, at that number. Re-picking it is not a second dose —
                            // and re-running the intervention would re-attach at the scenario's
                            // canonical setting, so the chart would log a change that never
                            // happened, then log changing it back.
                        } else if s.state.has_equipment(&dev) {
                            // On already, different number: turn the dial, do not re-dose.
                            if let Some(v) = set {
                                s.state.attach(&dev, Some(v));
                                s.tape.push(Step::Set(dev.clone(), v));
                            }
                        } else if let Some(phrase) = kit_phrase(&dev, set) {
                            // Go through the matcher, so the physiology moves exactly as it would
                            // for someone who typed it. The picker is a convenience, not a bypass.
                            let emitted = s.state.apply(&phrase);
                            s.beats.extend(emitted.iter().map(render_beat));
                            s.tape.push(Step::did(&phrase));
                            // Then correct the reading to what was actually dialled in. attach()
                            // records it too, so the chart quotes the learner's number rather
                            // than the scenario's canonical dose.
                            if let Some(v) = set {
                                if s.state.has_equipment(&dev) && s.state.equipment_setting(&dev) != Some(v) {
                                    s.state.attach(&dev, Some(v));
                                    // On the tape too. Without this the correction lived only in
                                    // this process: the player saw 6 L/min, the tape replayed to
                                    // the scenario's 10, and the leaf certified the wrong run.
                                    s.tape.push(Step::Set(dev.clone(), v));
                                }
                            }
                        }
                        if !was_over && s.over() {
                            if let Err(e) = s.ring_the_bell() {
                                eprintln!("could not ring the bell on {id}: {e}");
                            }
                        }
                        let v = s.view(lang::language(param(&url, "lang").as_deref()));
                        count_finish(&mut usage.lock().unwrap(), s, was_over, &store);
                        if !was_over {
                            persist(&store, &id, s, true);
                        }
                        json(v)
                    }
                }
            }
            // What the run is told back. Derived from the tape, so anyone holding the tape and the
            // scenario re-derives the same debrief — it is evidence, not commentary.
            (Method::Get, "/api/debrief") => {
                let id = param(&url, "id").unwrap_or_default();
                let caller = param(&url, "player");
                let mut map = sessions.lock().unwrap();
                match map.get_mut(&id).filter(|s| s.answers_to(caller.as_deref())) {
                    None => no_such_session(),
                    // ── sealed until the case is over, exactly like the mark sheet ─────────
                    // This endpoint was not sealed at all, and it gives away more than any
                    // other: `expected` is the scenario's own model answer — every intervention
                    // the case wanted, with its label, its reason and the second it wanted it by
                    // — and `harms` carries the full harm sentence with the intervention id that
                    // caused it. A GET mid-run was the whole station, in order, with timings.
                    //
                    // The page only ever asks after the bell, but the page is a file anyone can
                    // read and edit; this is the refusal that holds. The wording matches
                    // /api/marks so the two seals read as one rule rather than two accidents.
                    Some(s) if !s.over() => json(serde_json::json!({
                        "sealed": true,
                        "error": "the debrief opens when the case is over",
                    })),
                    Some(s) => match vitals_replay::debrief(&s.sce_json, &s.tape) {
                        Err(e) => json(serde_json::json!({ "error": e })),
                        Ok(d) => json(serde_json::json!({
                            "outcome": d.outcome,
                            "seconds": d.sim_seconds,
                            "expected": d.expected.iter().map(|e| serde_json::json!({
                                "id": e.id, "label": e.label, "why": e.why,
                                "within": e.within, "done_at": e.done_at,
                                "late": e.late, "late_by": e.late_by,
                            })).collect::<Vec<_>>(),
                            "avoided": d.avoided.iter().filter(|a| a.done_at.is_some())
                                .map(|a| serde_json::json!({
                                    "id": a.id, "label": a.label, "why": a.why, "done_at": a.done_at,
                                })).collect::<Vec<_>>(),
                            "harms": d.harms.iter().map(|h| serde_json::json!({
                                "text": h.text, "at": h.at, "caused_by": h.caused_by,
                            })).collect::<Vec<_>>(),
                            "statuses": d.statuses.iter().map(|sp| serde_json::json!({
                                "status": sp.status, "from": sp.from, "seconds": sp.seconds,
                            })).collect::<Vec<_>>(),
                        })),
                    },
                }
            }
            // ── the mark sheet ──────────────────────────────────────────────────
            // What the rubric actually paid for, item by item, from the same tape and the same
            // scorer that produce the number the chain carries (`vitals_osce::sheet_for_run` is
            // `det_for_run`'s own body). The debrief is therefore the arithmetic behind the star
            // rather than a second opinion about it.
            //
            // **Sealed until the case is over, on this side of the wire.** A mark sheet mid-run
            // is the answer key — it names every action the rubric pays for, with its window.
            // The page also refuses to ask for one, but the page is a file anyone can read and
            // edit; this is the refusal that holds. It is the same seal the harm text gets
            // (Phase 9), applied to the thing that would give away more.
            (Method::Get, "/api/marks") => {
                let id = param(&url, "id").unwrap_or_default();
                let caller = param(&url, "player");
                let map = sessions.lock().unwrap();
                match map.get(&id).filter(|s| s.answers_to(caller.as_deref())) {
                    None => no_such_session(),
                    Some(s) if !s.over() => json(serde_json::json!({
                        "sealed": true,
                        "error": "the mark sheet opens when the case is over",
                    })),
                    // A case with no rubric — EP1, or a member whose files have not landed — has
                    // no mark sheet to open. That is a fact about the case, not a failure.
                    Some(s) => {
                        // The rubric of a compiled case is in the pack, in the store, because that
                        // is how it arrived: the compiler sends the mark sheet with the scenario
                        // through the case door. Only the season's cases have a file. `s.ep` is the
                        // case id for a ward shift and for a review run alike, so one lookup
                        // answers for both.
                        let held = store
                            .get::<serde_json::Value>(ward_case::CASE_STORE, &ward_case::key_for(&s.ep))
                            .and_then(|pack| pack.get("rubric").cloned())
                            .map(|r| r.to_string());
                        let on_disk = || rubric_path(&s.ep).and_then(|p| std::fs::read_to_string(p).ok());
                        match held.or_else(on_disk) {
                        None => json(serde_json::json!({ "case": s.ep, "items": [] })),
                        Some(rj) => {
                            let sheet = vitals_osce::sheet_for_run(&s.sce_json, &s.tape, &rj);
                            match sheet {
                                Err(e) => json(serde_json::json!({ "error": e })),
                                Ok((rubric, det)) => json(serde_json::json!({
                                    "case": rubric.case,
                                    // Where the case came from, and what field it belongs to.
                                    // These used to ride on /api/chain, where a GET before the
                                    // exam started read them for all twelve stations at once —
                                    // a bank id names the diagnosis and a specialty names the
                                    // organ. They live here now because this endpoint is
                                    // already sealed behind an outcome: by the time anyone can
                                    // read them, the clock has stopped and the leaf is fixed,
                                    // which is exactly when a provenance line is worth reading.
                                    "bank_case": set_member(&s.ep).map(|m| m.case),
                                    "specialty": set_member(&s.ep).map(|m| m.specialty),
                                    "score": det.earned,
                                    "max": det.max,
                                    "bps": det.bps(),
                                    "pass_bps": rubric.pass_bps,
                                    "cleared": det.cleared(&rubric),
                                    // What the items added to before the death cap, or absent.
                                    // The sheet below still shows every point that was earned,
                                    // so without this the head and the rows would disagree and
                                    // the player would be right to think the sheet was broken.
                                    // See `vitals_osce::death_cap`.
                                    "capped_from": det.capped_from,
                                    // What over-ordering took off the items' own total, before
                                    // the cap. The rows below carry every point that was
                                    // earned, so a sheet that did not publish this would show a
                                    // column of ticks adding to more than the score at the top
                                    // — the same disagreement `capped_from` exists to prevent.
                                    // See `vitals_osce::Check::NoUnindicated`.
                                    "penalty": det.penalty,
                                    "exam": s.exam_mode,
                                    // Costliest first — the top of the sheet is what to fix
                                    // before sitting it again, which is the whole point of
                                    // showing it. Sorted here so every reader agrees.
                                    "items": det.by_loss().iter().map(|i| serde_json::json!({
                                        // ── the deduction has to be readable on the page ────
                                        // A row renders as `earned/points`, and a deduction has
                                        // no points — it takes them. So a charged row would
                                        // print "0/0" while the total at the head was three
                                        // lower than the rows add to, which is the exact
                                        // head-and-rows disagreement `capped_from` exists to
                                        // stop. Until the page renders `penalty` and `charged`
                                        // itself, the row says it in words. The structured
                                        // fields below are the ones to build on; this suffix
                                        // comes out the day they are used.
                                        "label": if i.penalty > 0 {
                                            format!("{} — {} marks off: {}", i.label, i.penalty, i.charged.join(", "))
                                        } else {
                                            i.label.clone()
                                        },
                                        "kind": i.kind,
                                        "mark": i.mark.as_str(),
                                        "points": i.points,
                                        "earned": i.earned_points(),
                                        "lost": i.lost(),
                                        "at": i.at,
                                        "within": i.within,
                                        // Non-zero only on the deduction row, with the orders
                                        // it charged for named: a candidate who is told three
                                        // marks went and not which order took them has been
                                        // marked at, not taught.
                                        "penalty": i.penalty,
                                        "charged": i.charged,
                                    })).collect::<Vec<_>>(),
                                })),
                            }
                        }
                        }
                    }
                }
            }
            (Method::Get, "/api/tape") => {
                let id = param(&url, "id").unwrap_or_default();
                let caller = param(&url, "player");
                let map = sessions.lock().unwrap();
                match map.get(&id).filter(|s| s.answers_to(caller.as_deref())) {
                    None => no_such_session(),
                    // The tape in the same shape vitals-replay takes, so a player can hand it to
                    // someone else and have the leaf re-derived off this machine entirely.
                    Some(s) => json(serde_json::json!({
                        "scenario": s.scenario,
                        "sce_hash": hex(&sce_hash(&s.sce_json)),
                        "tape": s.tape.iter().map(|st| match st {
                            Step::Tick(dt) => serde_json::json!({"tick": dt}),
                            Step::Do(t) => serde_json::json!({"do": t}),
                            Step::Act { text, id } => serde_json::json!({"do": text, "act": id}),
                            Step::Ask(t) => serde_json::json!({"ask": t}),
                            Step::Set(id, v) => serde_json::json!({"set": id, "to": v}),
                            Step::Off(id) => serde_json::json!({"off": id}),
                            Step::Shock(j) => serde_json::json!({"shock": j}),
                        }).collect::<Vec<_>>()
                    })),
                }
            }
            (Method::Get, "/api/say") => {
                let id = param(&url, "id").unwrap_or_default();
                let caller = param(&url, "player");
                let q = param(&url, "q").unwrap_or_default();
                // Whose question this is, before anything that costs money or needs a gateway.
                // A ward patient answers out of her own case file: the compiler wrote her words
                // against every `ask_` in it, the ward host runs with no model at all, and
                // refusing at the gateway check meant every question at every bed came back "no
                // gateway — the patient has no voice here" from a server holding the answer.
                //
                // `ep` comes out with the rest of it, because *which patient is in this bed* is a
                // fact about the session. It was not read at all before: one persona was loaded
                // at boot and every case in the season borrowed it, so asking OSCE-A's
                // seventy-one-year-old man anything got an answer from a nineteen-year-old woman
                // about her shrimp allergy — in her name, on her allergy, at her age.
                let (hist, status, pulse, spo2, ep, shift) = {
                    let mut map = sessions.lock().unwrap();
                    let Some(s) = map.get_mut(&id).filter(|s| s.answers_to(caller.as_deref())) else {
                        let _ = req.respond(no_such_session());
                        continue;
                    };
                    // The question goes on the tape. The answer never will.
                    s.tape.push(Step::asked(&q));
                    (
                        s.said.clone(),
                        format!("{:?}", s.state.status),
                        // The engine's own value, not the string above. `status` is a Debug format
                        // built for a prompt; a decision about whether a person can speak is not
                        // something to make by matching on formatted text.
                        s.state.status,
                        s.state.vitals.spo2,
                        s.ep.clone(),
                        s.ward.clone(),
                    )
                };
                let want = lang::language(param(&url, "lang").as_deref());

                // ── a patient with no pulse does not answer ───────────────────────
                //
                // Before either voice below, because there are two and both of them would speak.
                // The ward's voice is `ward_case::answer`, a lookup in the case file's own string
                // table that never had a notion of status, so a dead patient answered *certainly*
                // rather than probably — nine questions, on 22 Sep, from the first stranger ever to
                // take a shift here. The bay's voice is a model, and `patient::brief` tells it "you
                // are {status}" and then asks anyway, which is a different mechanism with the same
                // result. One gate above both, so neither can be fixed alone and left broken.
                //
                // The ask is already on the tape a few lines above and stays there: what a stranger
                // asked a dead patient is part of what happened, and the receipt is not a place to
                // be tactful. What changes is only that nobody answers in her name.
                //
                // Her name rather than a pronoun, because `plain_words` forbids server sentences
                // composed out of pronouns — the ward does not know how to refer to a person it has
                // only a case file for, and "she" in the wrong place is worse than a name.
                if !pulse.can_speak() {
                    // "The patient" and not her name, although the page says her name a line away:
                    // the session knows her id and her faces, not what she is called — the name
                    // lives in the pack the ward branch below loads. A sentence that is true from
                    // here beats one that needs a store read to be polite, and `plain_words`
                    // forbids composing it out of a pronoun instead.
                    let _ = req.respond(json(serde_json::json!({
                        "reply": "The patient has no pulse and cannot answer. Start CPR, or hand over.",
                        "asked": serde_json::Value::Null,
                        "cannot_answer": true,
                    })));
                    continue;
                }

                // ── the ward: her words, out of the case the ward is holding ──────
                if let Some(w) = shift {
                    let held = ward_chain::packs(&store).remove(&w.patient_id);
                    let case = held.as_ref().and_then(|pack| {
                        store
                            .get::<serde_json::Value>(ward_case::CASE_STORE, &ward_case::key_for(&pack.case))
                            .map(|c| (c, pack))
                    });
                    let Some((case, pack)) = case else {
                        // The bed exists and its case does not: a patient admitted on a case this
                        // ward no longer holds. Said plainly, because the alternative on a page
                        // whose whole claim is that the chart is the chain is a confident sentence
                        // from nobody.
                        let _ = req.respond(json(serde_json::json!({
                            "error": "this patient's case is not on this ward — examine, order and treat instead",
                        })));
                        continue;
                    };
                    let said = ward_case::answer(&case, &pack.persona, &q);
                    {
                        let mut map = sessions.lock().unwrap();
                        if let Some(s) = map.get_mut(&id) {
                            s.said.push(("user".into(), q));
                            s.said.push(("assistant".into(), said.words.clone()));
                            persist(&store, &id, s, true);
                        }
                    }
                    // Her words are the case's, in the language the case was written in. A learner
                    // who asked for Thai is told that rather than left to wonder — the same note
                    // the bay puts on a model's reply that drifted, for the same reason.
                    let off = !lang::reply_is_in(want, &said.words);
                    let _ = req.respond(json(serde_json::json!({
                        "reply": said.words,
                        "who": pack.persona.name,
                        "asked": said.matched,
                        "off_language": off,
                    })));
                    continue;
                }

                let Some(pt) = patient.as_ref() else {
                    let _ = req.respond(json(serde_json::json!({ "error": "no gateway — the patient has no voice here" })));
                    continue;
                };
                // The bay is free and the inference is paid for by donations, so the spend is
                // metered per address and capped per month. The ceiling reply carries the whole
                // meter: the page turns it into "what this month funded", not a bare 429.
                match meter.allow(&client_addr(&req), &store) {
                    meter::Verdict::Ok => {}
                    meter::Verdict::SlowDown { retry_secs } => {
                        let _ = req.respond(json_code(serde_json::json!({
                            "error": "the patient needs a moment — you are asking faster than the bay allows",
                            "retry_in": retry_secs,
                        }), 429));
                        continue;
                    }
                    meter::Verdict::Ceiling => {
                        let _ = req.respond(json_code(serde_json::json!({
                            "error": "this month's compute is spent",
                            "ceiling": meter.view(),
                        }), 429));
                        continue;
                    }
                }
                // A case with no persona is mute, and stays mute. Answering it out of another
                // case's file is the failure this whole path exists to prevent: a wrong answer in
                // a confident voice is worse for a candidate than no answer at all, because there
                // is nothing on the screen to tell them it was the wrong patient talking.
                let Some(persona) = personas.get(&ep) else {
                    let _ = req.respond(json(serde_json::json!({
                        "error": "this patient has no voice here — examine, order and treat instead",
                    })));
                    continue;
                };
                // No hint on this path yet — the reveal-gate wiring passes one when it lands.
                match pt.say(persona, &q, &hist, &status, spo2, None, want) {
                    Ok(reply) => {
                        // Counted only when she actually answered — a failed call is not billed
                        // to the month or to the visitor.
                        meter.spend(&store);
                        let mut map = sessions.lock().unwrap();
                        if let Some(s) = map.get_mut(&id) {
                            s.said.push(("user".into(), q));
                            s.said.push(("assistant".into(), reply.clone()));
                            persist(&store, &id, s, true);
                        }
                        // She was asked for Thai and answered in English. The answer is still her
                        // answer and it is still true about the case, so it is shown — with a note
                        // beside it, because a learner who chose a language deserves to be told
                        // when the model did not hold to it rather than left wondering. Swallowing
                        // it or retrying would cost the learner her only reply, or the bay a second
                        // inference, to fix a wording problem the learner can already see.
                        let off = !lang::reply_is_in(want, &reply);
                        json(serde_json::json!({
                            "reply": reply,
                            // The name comes off the persona that actually answered, so it can
                            // never again say "Ing" over a reply from somebody else's case.
                            "who": persona["patient"]["name"].as_str().unwrap_or("the patient"),
                            "off_language": off,
                        }))
                    }
                    Err(e) => json(serde_json::json!({ "error": e })),
                }
            }
            // ── the language layer ──────────────────────────────────────────────
            // What languages the bay speaks, and the pack of strings for one of them. Unguarded
            // and cacheable: it is a table compiled into the binary, the same for every visitor,
            // and it carries no case content — a station's own beats reach the page one at a time
            // through the view, as the run earns them, so this endpoint is not an answer key even
            // during an exam. Absent or unknown `lang` ⇒ the language the cases are written in.
            (Method::Get, "/api/lang") => json(lang::pack(lang::language(
                param(&url, "lang").as_deref(),
            )))
            .with_header(
                Header::from_bytes(&b"Cache-Control"[..], &b"public, max-age=300"[..]).unwrap(),
            ),
            // ── the scenario, addressed by its own hash ─────────────────────────
            // Every leaf on chain names the scenario it was played against by sha256. Until this
            // route existed, that name resolved to nothing: the disk held the current file, the
            // old versions were nowhere, and "deterministic, re-derivable by anyone" meant
            // "re-derivable by whoever has our repository and guesses the right commit". A leaf
            // can now hand a stranger the exact bytes it was computed over.
            //
            // **Retired versions only.** The first cut of this route resolved through the shelf
            // as well, "so today's runs are re-derivable before anyone remembers to archive
            // them" — and a scenario file is the answer key. A candidate could open a station,
            // read `sce_hash` off their own view, and GET every intervention id, every matcher
            // keyword, every `(HARM)` beside a wrong turn, the trigger thresholds that decide the
            // outcome, and the `_note` that names the diagnosis — mid-run, unauthenticated,
            // while the seal below was carefully withholding one sentence at a time. It is the
            // same leak `bank_case` was pulled off `/api/chain` to stop, in a worse form, and it
            // reached further than that one: `/api/marks` and `/api/debrief` open at the bell,
            // and this opened before it.
            //
            // So `archive::answer` treats the shelf as a deny list. A case that can still be sat
            // is refused whether or not the archive holds a copy — and it does hold one for every
            // case in the season, which is exactly why "serve from the archive" is not by itself
            // the fix. What is left is what has been retired, and a case that cannot be sat costs
            // nobody a mark. `VERIFICATION.md` §5 says so, and says what to do instead: a live
            // case's bytes are in the repository, and `shasum` on your own clone proves the same
            // thing this endpoint would have.
            //
            // Public and ungated, like the chain it explains — a proof only we can serve the
            // inputs for is not a proof. Verified before it is sent: `archive::answer` re-hashes
            // what it read and refuses anything that does not match, because bytes served under
            // the wrong hash would make a verifier conclude the *chain* was lying.
            // One patient's page — where the globe's "take a shift" points. The bay that plays
            // her opens in week 2; until then this answers with what the chain says about her,
            // which is more than a 404 and is true today.
            // A patient in the queue, at the address the board publishes her under. Before the
            // generic /ward/ route, which reads a patient id out of the path and would call a pack
            // id "not a patient id".
            (Method::Get, p) if ward_mode() && p.starts_with("/ward/waiting/") => {
                let id = p.trim_start_matches("/ward/waiting/");
                let door = ward_chain::door_here();
                let held = store.get::<ward::Pack>(ward_chain::QUEUE_STORE, id);
                let resp = match held {
                    // A closed ward publishes no queue, so it has no pages about one either.
                    Some(pack) if door != ward_chain::Door::Closed => {
                        let case = ward_case::all(&store)
                            .into_iter()
                            .find(|c| c.case_id == pack.case);
                        html(&waiting_page(&pack, case.as_ref(), door))
                    }
                    _ => html(&ward_page_missing(
                        "no patient is waiting under that name. The queue is published on the \
                         board while the ward is opening, and every row on it is a link",
                        &ward_now(&ward_view, &store, &state_dir),
                    ))
                    .with_status_code(404),
                };
                let _ = req.respond(resp);
                continue;
            }
            (Method::Get, p) if ward_mode() && p.starts_with("/ward/") => {
                // Her page is **the bay** (producer's ruling, 16 ก.ย.): one page, one engine, one
                // tape, with a start-state parameter. The page reads the id out of its own path,
                // rebuilds her from the chain and shows a strip saying whose shift this is.
                //
                // Every refusal a patient can carry — she went home, no pack describes her, the
                // chain cannot be read — arrives through `/api/new` and is shown on that strip, so
                // there is one place a stranger reads bad news rather than two pages that disagree
                // about which of them is the ward.
                let body = match ward::patient_id_in_path(p) {
                    // The ward's own page (founder, 16 ก.ย.: "ทำให้แยกกันเลยสิ"). The same play
                    // surface as the Eternal entry, composed from the same file, and none of the
                    // season around it — plus the tags that make a shared link carry the patient
                    // rather than the product (UX review G5). The board is already in hand and the
                    // card is built from her row on it, so a card cannot say what the ward does not.
                    Some(id) => {
                        // Somebody got past the globe to a person. Counted here, where the page is
                        // actually served, so the gap between this and `shifts_taken` says whether
                        // a quiet ward is one nobody found or one nobody knew how to start.
                        usage.lock().unwrap().opened_a_bedside(&store);
                        let board = ward_now(&ward_view, &store, &state_dir);
                        let her = board["patients"]
                            .as_array()
                            .and_then(|rows| rows.iter().find(|p| p["patient_id"] == id).cloned())
                            .unwrap_or(serde_json::Value::Null);
                        // The strip goes *inside* the cockpit, where the script would have put
                        // it, and the cockpit is served in its waiting state rather than hidden:
                        // both are true about this page from the moment it is served, and `hide`
                        // on `#game` is what made the served strip invisible when it was first
                        // tried here. The page then drops `waiting` as the case lands.
                        compose(SHIFT)
                            .replace("<!--OG-->", &og_tags(&her))
                            .replace(
                                "<div class=\"app hide\" id=\"game\">",
                                &format!("<div class=\"app waiting\" id=\"game\">{WARD_STRIP}"),
                            )
                    }
                    // The reviewer's two: the list of cases the ward holds, and one case opened to
                    // be read. The run is the same play surface as a shift — one page, one engine,
                    // one tape — and the page reads which it is out of its own path.
                    None if p == "/ward/review" || p == "/ward/review/" => compose(WARD_CASES),
                    None if ward::review_case_in_path(p).is_some() => compose(SHIFT),
                    None if p.starts_with("/ward/review/") => {
                        ward_page_missing("that is not a case id", &ward_now(&ward_view, &store, &state_dir))
                    }
                    None => ward_page_missing("that is not a patient id",
                                              &ward_now(&ward_view, &store, &state_dir)),
                };
                let _ = req.respond(html(&body));
                continue;
            }
            (Method::Get, p) if p.starts_with("/api/sce/") => {
                let want = p.trim_start_matches("/api/sce/");
                match archive::answer(want, &live_scenarios(), &sce_archive_dir()) {
                    archive::Answer::Retired(text) => {
                        let _ = req.respond(
                            Response::from_string(text)
                                .with_header(Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap())
                                // Content-addressed: these bytes cannot change without changing
                                // the URL, which is the one case `immutable` is simply true.
                                .with_header(Header::from_bytes(&b"Cache-Control"[..], &b"public, max-age=31536000, immutable"[..]).unwrap()),
                        );
                        continue;
                    }
                    // A case that is still being sat. Said plainly rather than folded into "no
                    // such hash", because the caller is a verifier holding a leaf and silence
                    // would send them looking for a bug in the chain. It discloses nothing: they
                    // already hold the hash, and the reply says nothing whatever about the file.
                    archive::Answer::InPlay => json_code(
                        serde_json::json!({
                            "error": "that scenario is in active use",
                            "detail": "This hash names a case that can be sat right now, and the \
                                       file is the mark sheet — every matcher, every harm, every \
                                       threshold. It is published when the case is retired, and \
                                       not before.",
                            "verify_now": "Until then the bytes are in the repository and prove \
                                           the same thing: clone it and run \
                                           `shasum -a 256 <the scenario file>` — see \
                                           VERIFICATION.md §5.",
                        }),
                        404,
                    ),
                    // A past version of a case that is still being sat. The distinction from
                    // `InPlay` is the whole of the fix: the caller's hash is real, it is in the
                    // archive, and it is still withheld — because the case it belongs to has
                    // not retired, only rotated. Told plainly, or a verifier holding a leaf
                    // from an older version concludes their leaf names nothing.
                    archive::Answer::Superseded => json_code(
                        serde_json::json!({
                            "error": "that scenario is a version of a case in active use",
                            "detail": "This hash names an earlier version of a case that can be \
                                       sat right now. Editing a case rotates its hash; it does \
                                       not retire it — and an old version is the same mark \
                                       sheet, carrying every matcher, every harm and every \
                                       threshold the live one does. Every version of a case is \
                                       published together, on the day the case leaves the shelf.",
                            "verify_now": "Your leaf is still checkable today: the bytes are in \
                                           the repository's committed archive. Clone it and run \
                                           `shasum -a 256 conformance/sce-archive/<hash>.json` \
                                           — see VERIFICATION.md §5.",
                        }),
                        404,
                    ),
                    // In the archive, unattributed. An operator problem, not a caller problem,
                    // so the reply says what is missing instead of pretending the file is not
                    // there — and it still withholds, because an unattributed version cannot be
                    // shown not to be a live case's answer key.
                    archive::Answer::Unattributed => json_code(
                        serde_json::json!({
                            "error": "that scenario cannot be attributed to a case",
                            "detail": "The archive holds this version and no INDEX.json row says \
                                       which case it is a version of, so this deployment cannot \
                                       tell whether the case is still being sat. It is withheld \
                                       until it can. Adding the row publishes it.",
                            "verify_now": "The bytes are in the repository's committed archive \
                                           either way: `shasum -a 256 \
                                           conformance/sce-archive/<hash>.json` — see \
                                           VERIFICATION.md §5.",
                        }),
                        404,
                    ),
                    // One answer for "not a hash", "no such hash" and "the file under that name
                    // is not that file". The caller's next move is the same in all three, and
                    // the alternative is a probe that tells a stranger which files exist.
                    archive::Answer::Unknown => json_code(
                        serde_json::json!({
                            "error": "no scenario with that hash",
                            "want": "GET /api/sce/<64 hex sha256 of a retired scenario file>",
                        }),
                        404,
                    ),
                }
            }
            // ── how much this bay is used ───────────────────────────────────────
            // Runs opened, runs finished, and how many distinct browsers were seen this month.
            // Public and read-only, because a project that sells "check it yourself" and keeps
            // its own usage private is arguing against itself.
            //
            // **It is not a count of people and it may never be quoted as one.** There is no
            // signup here by design, so there is nothing that is a person: one shared box in a
            // faculty is one device with fifty students behind it, and one person with a phone
            // and a laptop is two. `usage::LIMITS` says so in the payload, every time — the
            // numbers and the caveats are built in the same call so a bare integer cannot leave
            // this server without them.
            //
            // No consent gate, and none needed: this is the server counting its own work. No IP,
            // no user-agent, no identifier that follows a reader anywhere. That is what makes it
            // a better instrument than the analytics tag, which loses everyone who declines.
            (Method::Get, "/api/usage") => {
                // On the ward host these would answer for vitals.academy's play, not for a ward
                // that has not opened — a number that is true elsewhere is still a wrong answer
                // here. Say so, and point at where the real one lives.
                if ward_mode() {
                    // One implementation, in `read_response`, which the reader pool also calls.
                    match read_response(&req, &path, &ward_view, &store, &state_dir, &usage, &token) {
                        Some(r) => { let _ = req.respond(r); }
                        None => { let _ = req.respond(json_code(serde_json::json!({}), 404)); }
                    }
                    continue;
                }
                let t = tree.lock().unwrap();
                let mut v = usage.lock().unwrap().view();
                // The one figure on this page an outsider can verify without trusting us: the
                // runs anchored on chain. Read from the same lock /api/chain and /api/fuel read,
                // so the three can never disagree about how many there are.
                v["anchored_on_chain"] = serde_json::json!(t.leaves.len());
                v["anchored_on_chain_note"] =
                    serde_json::json!("Anchoring is opt-in, so this is a floor, not a total — \
                                       but it is the only number here anyone can check for themselves.");
                json(v)
            }
            // The month's spend, the ceiling and where donations go — public, because the
            // ceiling being visible is the point. Anyone can check what the bay has left.
            (Method::Get, "/api/meter") => json(meter.view()),
            // What the money has bought, in one reading: the month's patient turns against the
            // ceiling, the relay's balance and the runs it still pays for, the treasury, and the
            // count already anchored. Public and ungated on purpose — a fuel gauge only anyone
            // can read is a fuel gauge, and one only we can read is a claim.
            //
            // The numbers are joined here rather than in the page because three of the four
            // already have a single source in this process (the meter, the leaf list, the
            // relay's own key) and the fourth needs an RPC call the browser must not make.
            // Who wrote the cases, and how many proven replays each one has.
            //
            // The ledger a payment would read, with no payment in it: no currency, no "earned",
            // no rate. SYSTEM_DESIGN §7 says authors are paid per replay; this is the counting
            // that has to be true and checkable before that sentence means anything, and
            // TOKENOMICS.md keeps its "designed, not built" label untouched.
            //
            // Nothing here comes from /api/usage. Those are this server's own tallies of its own
            // work, and 56 of the 174 runs on them were curl and Python. A replay of somebody's
            // case is one the chain accepted, and nothing else.
            (Method::Get, "/api/authors") => {
                let tree_id = tree.lock().unwrap().tree_id;
                // **One chain fan-out a minute for this whole handler**, not per read.
                //
                // Everything the chain can tell this endpoint is taken together and held: the
                // proven counts, what has been paid, and which leaf each payment was for. Three
                // separate caches would let a paid count be a minute newer than the proven count
                // printed beside it, and show a case paid more often than it was played.
                //
                // The bound is the point. `get_program_accounts` and a signature scan behind an
                // endpoint anyone can curl is the RPC budget that anchoring needs, spent by the
                // page that only displays it — and on the one thread this server has, every
                // round trip here stalls a learner mid-run. `settle` still reads fresh and
                // uncached, because that one is the payment decision; this one is display.
                let view = {
                    let mut cached = author_counts.lock().unwrap();
                    let fresh = cached.as_ref().is_some_and(|(at, _): &(Instant, ChainView)| {
                        at.elapsed() < AUTHOR_COUNT_TTL
                    });
                    if !fresh {
                        if let Some(fresh_view) = chain_view(&chain, &payer, tree_id) {
                            *cached = Some((Instant::now(), fresh_view));
                        }
                    }
                    cached.clone()
                };
                let (view, counted, age) = match &view {
                    Some((at, v)) => (v.clone(), true, at.elapsed().as_secs()),
                    None => (ChainView::default(), false, 0),
                };

                let root = scenario_root();
                let table = match authors::load(&root.join(authors::AUTHORS_PATH)) {
                    Ok(t) => t,
                    Err(e) => {
                        let _ = req.respond(json(serde_json::json!({ "error": e })));
                        continue;
                    }
                };
                let index = authors::archive_entries(&root.join(authors::INDEX_PATH))
                    .unwrap_or_default();
                // Which hash is the file on the shelf right now, and which card it is. Computed
                // where both facts live rather than guessed from a path on the page.
                let mut live = std::collections::BTreeSet::new();
                let mut eps = std::collections::BTreeMap::new();
                for id in every_case() {
                    if let Ok(json) = std::fs::read_to_string(scenario_path(id)) {
                        let h = hex(&sce_hash(&json));
                        eps.insert(h.clone(), id.to_string());
                        live.insert(h);
                    }
                }

                let led = authors::ledger(&authors::Inputs {
                    table: &table,
                    index: &index,
                    eps: &eps,
                    live: &live,
                    proven: &view.per_case,
                    paid: &view.paid_by_case,
                    payable: payer.as_ref().map(|p| &p.allowlist),
                });
                json(serde_json::json!({
                    "counts": "proven replays — attempts that passed the Merkle check on chain. \
                               Anchoring alone carries no case, so it cannot be counted per case.",
                    "lineage": "a case is every archive entry sharing a path; a signature is over \
                                one version's bytes. Replays never move between versions.",
                    "tree_id": tree_id,
                    "counted_from_chain": counted,
                    "counts_age_secs": age,
                    "counts_bound": "one pass over the chain a minute at most, however often \
                                     this is asked — an account sweep and a signature scan, \
                                     taken together so the proven and paid figures are from the \
                                     same moment",
                    "attributed_versions": table.len(),
                    "payouts": match payer.as_ref() {
                        Some(p) => serde_json::json!({
                            "on": true,
                            "lamports_per_proven_replay": p.rate,
                            "cluster": "devnet — illustrative devnet SOL, not money",
                            "wallet": p.address(),
                        }),
                        None => serde_json::json!({ "on": false }),
                    },
                    "cases": led.cases,
                    "authors": led.authors,
                }))
            }
            // Was this run's leaf paid for, and with which transaction.
            //
            // Per leaf rather than per case, because a learner is being shown the payment *their
            // own run* caused. It answers "not yet" rather than "no": the transfer is a second
            // transaction landing a moment after the proof, and the honest answer in that gap is
            // that it has not happened yet.
            (Method::Get, "/api/payout") => {
                // **No chain read on this path.** The page polls it a dozen times per finished
                // run; a class finishing together would be hundreds of fan-outs in half a
                // minute, on the one thread that is also anchoring their runs. So it answers
                // from what this process already knows it did, and falls back to the snapshot
                // the authors endpoint holds — never from a fresh scan.
                let leaf = param(&url, "leaf").unwrap_or_default().to_lowercase();
                if payer.is_none() || leaf.is_empty() {
                    let _ = req.respond(json(serde_json::json!({ "paid": false, "unknown": true })));
                    continue;
                }
                // What this process paid, which is the ordinary case: the run just finished here.
                let mine = settled.lock().unwrap().get(&leaf).cloned();
                // Otherwise whatever the last chain read saw — a restart, or another instance.
                // Up to a minute stale, and the debrief's silence already handles "not yet"
                // honestly, so a leaf paid elsewhere reads as unpaid for at most that long.
                let known = mine.or_else(|| {
                    author_counts
                        .lock()
                        .unwrap()
                        .as_ref()
                        .and_then(|(_, v): &(Instant, ChainView)| v.paid_leaves.get(&leaf).cloned())
                });
                match known {
                    Some((lamports, signature)) => json(serde_json::json!({
                        "paid": true,
                        "lamports": lamports,
                        "signature": signature,
                        // Said on the same line as the number, everywhere the number goes.
                        "currency": "devnet SOL — illustrative, not money",
                    })),
                    // Not "no": not yet, as far as anything here has been told.
                    None => json(serde_json::json!({ "paid": false })),
                }
            }
            (Method::Get, "/api/fuel") => {
                let t = tree.lock().unwrap();
                let mut v = fuel.view(chain.as_ref().map(|c| c.relay_pubkey()).as_deref());
                // The expensive one, and the one the page leads with: a patient turn is paid
                // inference, and the ceiling is what stops a stranger spending the month.
                v["turns"] = meter.view();
                // Runs already on chain. The same count /api/chain serves, from the same lock,
                // so the two pages can never disagree about how many there are.
                v["anchored"] = serde_json::json!(t.leaves.len());
                v["tree_id"] = serde_json::json!(t.tree_id);
                v["connected"] = serde_json::json!(chain.is_some());
                json(v)
            }
            // The treasury page, always — no env var in this handler, so there is nothing to
            // misconfigure into a loop. The visit is counted: page views of /donate are the
            // conversion this side can measure honestly; the money itself is audited on chain.
            // VITALS_DONATE_URL has exactly one remaining job, elsewhere: the sentinel that
            // shows the donate button in the UI.
            (Method::Get, "/donate") => {
                meter.click(&store);
                html(DONATE)
            }
            // ── what we hold, and what nobody can take back ──────────────────────
            //
            // Served like the deck and the donate page: compiled in, ungated, no click counted.
            // The visit is deliberately *not* metered — /donate counts its own views because a
            // donation link's conversion is a number worth having, and counting who reads the
            // privacy policy would be the one measurement this page has no business making.
            //
            // Both hosts answer them. `apex_target` keeps these two paths on the apex rather
            // than redirecting to the game origin, because they are the company's documents and
            // because the URL handed to an OAuth consent screen or a reviewer should resolve at
            // the name it was written as, not one hop later.
            (Method::Get, "/stats") => read_response(&req, &path, &ward_view, &store, &state_dir, &usage, &token)
                .unwrap_or_else(|| html(WARD_STATS)),
            (Method::Get, "/privacy") => read_response(&req, &path, &ward_view, &store, &state_dir, &usage, &token)
                .unwrap_or_else(|| html(&PRIVACY.replace(BUILD_STAMP, BUILD))),
            (Method::Get, "/terms") => html(&TERMS.replace(BUILD_STAMP, BUILD)),
            // ── the form itself ─────────────────────────────────────────────────
            // One URL and nothing else. The two reviewers this is for are a final-year student
            // and a physician: a link that opens and works is the entire brief, and any step
            // between the link and the first question is a step at which the review does not
            // happen.
            //
            // Stamped on the way out. The stamp is both the build the answers were written
            // about and the page's own evidence that there is a server behind it — an unstamped
            // copy (mailed, opened off disk, published) falls back to handing the reviewer their
            // answers to send by hand, which is what kept this usable before the route existed.
            //
            // Ungated, like the bay. A token here would protect nothing — everything on the page
            // is a question we are asking — and would stop the page opening for the two people
            // it was written for. `guarding_covers_everything_that_spends_or_signs` holds it.
            (Method::Get, "/review") => read_response(&req, &path, &ward_view, &store, &state_dir, &usage, &token)
                .unwrap_or_else(|| html(&REVIEW.replace(BUILD_STAMP, BUILD))),
            // The board, pushed (CWF_PLAN.md ruling 12). SSE rather than a socket: the page only
            // ever listens, and a browser reconnects an EventSource by itself — which matters
            // here because Cloud Run ends a request at its timeout however healthy it is.
            //
            // This is the one route that leaves the request loop. Everything else this server does
            // answers and returns; a stream that stayed on the loop would hold the only thread for
            // as long as somebody kept a tab open, and the ward would serve nobody else.
            (Method::Get, "/api/ward/stream") if ward_mode() => {
                if WARD_WATCHERS.load(std::sync::atomic::Ordering::Relaxed) >= WARD_STREAMS {
                    let _ = req.respond(
                        json(serde_json::json!({
                            "error": format!("{WARD_STREAMS} boards are already watching"),
                            "poll_instead": "/api/ward"
                        }))
                        .with_status_code(503),
                    );
                    continue;
                }
                WARD_WATCHERS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let view = Arc::clone(&ward_view);
                let state = state_dir.clone();
                std::thread::spawn(move || {
                    let _leave = WatcherLeaves;
                    let store = match store::Store::open(std::path::PathBuf::from(&state)) {
                        Ok(s) => s,
                        Err(_) => return,
                    };
                    let mut w = req.into_writer();
                    // Written by hand because the body has no end: tiny_http's Response wants a
                    // length or a reader, and what this needs is a socket it can keep talking on.
                    // `Connection: close` is what makes a bodiless-length response legal, and
                    // `X-Accel-Buffering` asks proxies not to hold the events until they have a
                    // pageful — which is the difference between a live board and a stuttering one.
                    let head = "HTTP/1.1 200 OK\r\n\
                                Content-Type: text/event-stream\r\n\
                                Cache-Control: no-cache\r\n\
                                X-Accel-Buffering: no\r\n\
                                Connection: close\r\n\r\n\
                                retry: 5000\n\n";
                    if w.write_all(head.as_bytes()).is_err() {
                        return;
                    }
                    let mut last = String::new();
                    let mut quiet = Duration::ZERO;
                    loop {
                        let v = ward_now(&view, &store, &state);
                        // Compared without `as_of_slot`, so a re-read that found nothing new is
                        // not an event. A board that flashed every thirty seconds because the
                        // clock moved would teach its watcher to stop looking.
                        let now = format!(
                            "{}|{}|{}",
                            v["census"], v["patients"], v["queue"]
                        );
                        let send = if now != last {
                            last = now;
                            quiet = Duration::ZERO;
                            serde_json::to_string(&v)
                                .map(|body| format!("event: ward\ndata: {body}\n\n"))
                                .unwrap_or_default()
                        } else if quiet >= Duration::from_secs(15) {
                            quiet = Duration::ZERO;
                            // A comment. It keeps the connection and the proxies awake and says
                            // nothing, which is exactly what has happened.
                            ": still here\n\n".to_string()
                        } else {
                            String::new()
                        };
                        if !send.is_empty() && (w.write_all(send.as_bytes()).is_err() || w.flush().is_err()) {
                            // The reader closed their tab. Not an error, and not worth a log line.
                            return;
                        }
                        std::thread::sleep(WARD_STREAM_POLL);
                        quiet += WARD_STREAM_POLL;
                    }
                });
                continue;
            }
            // The factory's door (CWF_PLAN.md ruling 10). Packs only — an existing case, a
            // person, a portrait — and never a key. Guarded by the same token the signing routes
            // use, because what arrives here becomes the patients strangers are handed.
            // The case factory's door. Packs of *cases* rather than of patients: the scenario the
            // engine runs, the mark sheet, and the patient's own words for what she is asked.
            // Behind the same secret as the queue, because what arrives here is what strangers
            // will be asked to treat.
            (Method::Post, "/api/ward/case") => {
                if !ward_mode() {
                    let _ = req.respond(json_code(serde_json::json!({
                        "error": "this host has no ward",
                        "the_ward_is": "https://world.vitals.academy/api/ward/case"
                    }), 404));
                    continue;
                }
                let body = match read_body(&mut req, CASE_MAX) {
                    Ok(b) => b,
                    Err(BadBody::TooLong) => {
                        let _ = req.respond(json_code(serde_json::json!({
                            "refused": format!("a case pack is at most {CASE_MAX} bytes")
                        }), 413));
                        continue;
                    }
                    Err(BadBody::NotText) => {
                        let _ = req.respond(json_code(serde_json::json!({
                            "refused": "that body is not UTF-8 text"
                        }), 400));
                        continue;
                    }
                };
                let pack: serde_json::Value = match serde_json::from_str(&body) {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = req.respond(json_code(serde_json::json!({
                            "refused": format!("that is not a case pack: {e}")
                        }), 400));
                        continue;
                    }
                };
                let summary = match ward_case::validate_case(&pack) {
                    Ok(s) => s,
                    // 422 and not 400: the JSON was fine and the case is not. The compiler is the
                    // only thing that can act on the difference, and it reads this in a log.
                    Err(why) => {
                        let _ = req.respond(json_code(serde_json::json!({ "refused": why }), 422));
                        continue;
                    }
                };
                // Add or replace while it is provisional; add-only once it has been reviewed. A
                // reviewed case is one the chain carries shifts against, and replacing it under the
                // same id would rewrite what those shifts were about.
                let key = ward_case::key_for(&summary.case_id);
                if let Some(held) = store.get::<serde_json::Value>(ward_case::CASE_STORE, &key) {
                    let reviewed = !held.get("provisional").and_then(|p| p.as_bool()).unwrap_or(true);
                    if reviewed {
                        let _ = req.respond(json_code(serde_json::json!({
                            "refused": format!(
                                "{} is already here and reviewed, so it is add-only now: somebody \
                                 has played it and the chain carries shifts against this case",
                                summary.case_id
                            )
                        }), 409));
                        continue;
                    }
                }
                match store.put(ward_case::CASE_STORE, &key, &pack) {
                    Ok(()) => {
                        let _ = req.respond(json(serde_json::json!({
                            "stored": summary.case_id,
                            "provisional": summary.provisional,
        "status": ward_chain::catalogue_status(ward_chain::door_here()),
                            "version": summary.version,
                        })));
                    }
                    Err(e) => {
                        let _ = req.respond(json_code(serde_json::json!({
                            "refused": format!("the case could not be stored: {e}")
                        }), 503));
                    }
                }
                continue;
            }
            // Taking a case out of service. **Not a delete**: patients are mid-stay on these and
            // their charts are rebuilt from the pack, so it stays in the store and stays readable
            // for as long as anybody is on it. What is withdrawn is its future.
            //
            // It exists because the compiler's own rules can change under a catalogue that is
            // already full: it stopped accepting non-English cases on 17 ก.ย., and sixty of the
            // cases this ward held were Thai ones with English placeholders filled into them —
            // "womanวัยกลางคน…" over a Japanese patient. Without this the only ways to stop that
            // are deleting the packs the chain's patients depend on, or emptying the ward.
            (Method::Post, p) if p.starts_with("/api/ward/case/") && p.ends_with("/withdraw") => {
                if !ward_mode() {
                    let _ = req.respond(json_code(serde_json::json!({
                        "error": "this host has no ward"
                    }), 404));
                    continue;
                }
                let id = p
                    .trim_start_matches("/api/ward/case/")
                    .trim_end_matches("/withdraw")
                    .trim_matches('/')
                    .to_string();
                let key = ward_case::key_for(&id);
                let Some(mut pack) = store.get::<serde_json::Value>(ward_case::CASE_STORE, &key)
                else {
                    let _ = req.respond(json_code(serde_json::json!({
                        "refused": format!("{id} is not a case this ward holds")
                    }), 404));
                    continue;
                };
                // The same rule as replacing one, and for the same reason: somebody has played a
                // reviewed case and the chain carries what they did.
                if !pack.get("provisional").and_then(|p| p.as_bool()).unwrap_or(true) {
                    let _ = req.respond(json_code(serde_json::json!({
                        "refused": format!(
                            "{id} is reviewed, and a reviewed case is not withdrawn: somebody has \
                             played it and the chain carries shifts against it"
                        )
                    }), 409));
                    continue;
                }
                pack["withdrawn"] = serde_json::Value::Bool(true);
                match store.put(ward_case::CASE_STORE, &key, &pack) {
                    Ok(()) => {
                        let _ = req.respond(json(serde_json::json!({
                            "withdrawn": id,
                            "kept": "the pack stays in the store and stays readable: the \
                                     patients already on this case are rebuilt from it",
                        })));
                    }
                    Err(e) => {
                        let _ = req.respond(json_code(serde_json::json!({
                            "refused": format!("the case could not be withdrawn: {e}")
                        }), 503));
                    }
                }
                continue;
            }
            // What the ward is holding, without the scenarios: a pack is twenty kilobytes and
            // nobody reading the catalogue needs one.
            (Method::Get, "/api/ward/cases") => {
                let mut cases: Vec<serde_json::Value> = ward_case::all(&store)
                    .into_iter()
                    .map(|c| serde_json::json!({
                        "case_id": c.case_id,
                        "archetype": c.archetype,
                        // Filled from the case's own patient. The catalogue is a list of cases and
                        // the case's own patient is who its title is written about, so a reader
                        // gets the sentence the author wrote rather than "{sex_word} of {age}".
                        "title": ward_case::fill_persona(&c.title, &ward_case::a_patient_of(&c)),
                        "country": c.country,
                        "difficulty": c.difficulty,
                        "endemic": c.endemic,
                        "provisional": c.provisional,
                        "version": c.version,
                        // Who the case is written about: the sex its dialogue and examination
                        // assume, and the age its physiology was tuned for. The factory places a
                        // person against these.
                        "patient": { "age": c.patient_age, "sex": c.patient_sex },
                        // Out of service: still here, still readable for the patients on it, never
                        // placed again. A reader deciding what can be played asks `?placeable=1`.
                        "withdrawn": c.withdrawn,
                    }))
                    .collect();
                if param(&url, "placeable").is_some_and(|v| v == "1" || v == "true") {
                    cases.retain(|c| c["withdrawn"] != serde_json::Value::Bool(true));
                }
                cases.sort_by(|a, b| a["case_id"].as_str().cmp(&b["case_id"].as_str()));
                let _ = req.respond(json(serde_json::json!({
                    "cases": cases,
                    // The catalogue page shows this beneath its counts; it never types the date.
                    "status": ward_chain::catalogue_status(ward_chain::door_here()),
                    "derivations": {
                        "cases": "every pack the case factory has put through /api/ward/case and \
                                  this ward accepted. `provisional` is the compiler's own word for \
                                  a case that has been compiled and not clinically reviewed; a \
                                  reviewed one cannot be replaced under the same id. `withdrawn` is \
                                  a case taken out of service: it stays here and stays readable, \
                                  because patients are mid-stay on it and their charts are rebuilt \
                                  from it, and nobody new is ever placed on it. `?placeable=1` \
                                  leaves the withdrawn ones out",
                    },
                })));
                continue;
            }
            (Method::Post, "/api/ward/tick") => {
                if !ward_mode() {
                    let _ = req.respond(json_code(serde_json::json!({
                        "ward": "not on this host",
                        "the_ward_is": "https://world.vitals.academy/api/ward/tick"
                    }), 404));
                    continue;
                }
                // **The occupation, made askable for.** A pass holds this loop for eleven to
                // fifteen seconds at the census the ward now runs at, and about eighty on a cold
                // start. Nothing in the test suite can occupy it that long — there is no chain to
                // read — so a claim about what happens to other requests during a pass had
                // nothing able to check it, which is how Cloud Run came to be refusing visitors
                // all day without a single test noticing.
                //
                // Inert unless the variable is set, and no deployed service sets it. It lives
                // inside the pass's own thread below, so that holding the pass no longer holds
                // the loop — which is the thing being fixed.
                // The pass runs *inside* this request on purpose: with min-instances 0, Cloud Run
                // gives the container CPU only while a request is being served, so a pass spawned
                // and answered 202 would stall the moment the response went out. The budget in
                // `Budget::pass` is what makes a synchronous pass tolerable on a server that
                // answers one request at a time.
                // **The pass runs on its own thread, holding this request open.**
                //
                // It used to run right here, and "here" is the loop that pulls requests off the
                // socket. While it ran, nothing else was even *collected*, let alone answered —
                // which is why the reader pool alone did not help: the readers were idle behind a
                // loop that never reached the point of handing them anything.
                //
                // The request is carried into the thread and answered when the pass finishes, so
                // Cloud Run still sees a request in flight for the whole of it. That matters at
                // min-instances 0, where CPU is given only while a request is being served: a
                // pass that answered first and worked after would stall the moment the response
                // went out. This keeps the original reason for running it synchronously and drops
                // the cost nobody had noticed — that it also stopped the ward answering anybody.
                //
                // `one_pass` takes the `TICKING` gate, so two scheduler ticks still cannot overlap:
                // the second is answered 409 by the gate rather than by this loop being busy.
                let state = state_dir.clone();
                std::thread::spawn(move || {
                    let store = match store::Store::open(std::path::PathBuf::from(&state)) {
                        Ok(s) => s,
                        Err(e) => {
                            let _ = req.respond(json_code(
                                serde_json::json!({ "error": format!("no store for the pass: {e}") }),
                                503,
                            ));
                            return;
                        }
                    };
                    if let Some(ms) = std::env::var("VITALS_TICK_SLEEP_MS")
                        .ok()
                        .and_then(|v| v.trim().parse::<u64>().ok())
                    {
                        std::thread::sleep(std::time::Duration::from_millis(ms));
                    }
                    let began = Instant::now();
                    let ran = match one_pass(&store, &scenario_root()) {
                        None => None,
                        Some(Ok(t)) => Some(t),
                        // No chain is a pass that ran and admitted nobody, with the reason in its
                        // notes — 200, because the route did its job and the ward is saying why.
                        Some(Err(e)) => Some(ward_chain::Ticked {
                            notes: vec![format!(
                                "the chain could not be read, so nobody was admitted: {e}"
                            )],
                            ..Default::default()
                        }),
                    };
                    let (code, body) = ward_chain::tick_response(ran, began.elapsed());
                    let _ = req.respond(json_code(body, code));
                });
                continue;
            }
            (Method::Post, "/api/ward/queue") => {
                if !ward_mode() {
                    let _ = req.respond(json(serde_json::json!({
                        "ward": "not on this host",
                        "the_ward_is": "https://world.vitals.academy/api/ward/queue"
                    })));
                    continue;
                }
                if !ward_chain::door_open_here() {
                    // Answered as "come back later" rather than as a refusal of the caller: the
                    // factory is right to be pushing, the ward is simply not open yet, and a 4xx
                    // would read in its log as a pack it should stop building.
                    let _ = req.respond(
                        json(serde_json::json!({
                            "door": ward_chain::door_here().word(),
                            "why": "the ward is not open yet. It opens when the founder says so, \
                                    not when a deploy lands — this build carries the code with \
                                    the door shut on purpose",
                            "queued": 0,
                            "duplicates": 0,
                            "rejected": [],
                            "depth": 0
                        }))
                        .with_status_code(503),
                    );
                    continue;
                }
                let body = match read_body(&mut req, QUEUE_MAX) {
                    Ok(b) => b,
                    Err(BadBody::TooLong) => {
                        let _ = req.respond(json(serde_json::json!({
                            "error": format!("a page of packs is at most {QUEUE_MAX} bytes — send \
                                              fewer per push, the queue keeps what it is given")
                        })));
                        continue;
                    }
                    Err(BadBody::NotText) => {
                        let _ = req.respond(json(serde_json::json!({
                            "error": "that body is not UTF-8 text"
                        })));
                        continue;
                    }
                };
                #[derive(serde::Deserialize)]
                struct Push {
                    packs: Vec<ward::Pack>,
                }
                match serde_json::from_str::<Push>(&body) {
                    // Every count and every refusal goes back in words: the factory is a job
                    // nobody watches, and a door that answered "ok" while dropping half of what it
                    // was sent would look exactly like a factory that was working.
                    Ok(push) => {
                        let report = ward_chain::enqueue(&store, push.packs);
                        let _ = req.respond(json(report));
                    }
                    Err(e) => {
                        let _ = req.respond(json(serde_json::json!({
                            "error": format!("that is not a page of packs: {e}"),
                            "shape": {"packs": [{
                                "case": "one of the ids in /api/ward's policy.catalogue",
                                "persona": {"name": "as the ward renders it",
                                            "country": "ISO 3166-1 alpha-3",
                                            "age": "inside the case's own band"},
                                "portrait": format!("{}/<sha256>.webp, or null",
                                                    ward_chain::PORTRAITS),
                                "endemic": "true only when the endemic list pairs that country \
                                            with that case"
                            }]}
                        })));
                    }
                }
                continue;
            }
            // ── the chain, at both ends of a shift ──────────────────────────────
            //
            // Four transactions, and the browser signs every one of them: an account of their
            // own, the head, the declaration, and the anchor. The relay pays for all four and can
            // produce none of the signatures, which is what makes the record theirs. Kept apart
            // from the Eternal bay's own chain routes on purpose — a different program, a
            // different map, a different submit (ruling 7).
            (Method::Get, p) if ward_mode() && p.starts_with("/api/ward/")
                && matches!(p, "/api/ward/open" | "/api/ward/take" | "/api/ward/declare"
                               | "/api/ward/anchor" | "/api/ward/release") =>
            {
                // A door that is not open answers before anything else here: no key is read, no
                // chain is reached, and nothing is prepared. In preview the ward is filling its
                // queue and showing who is in it, and the one thing it does not do is let anybody
                // put their hands on a patient.
                let door = ward_chain::door_here();
                if !door.plays() {
                    let _ = req.respond(json_code(serde_json::json!({
                        "refused": door.refusal(), "door": door.word()
                    }), 409));
                    continue;
                }
                let Some(who) = param(&url, "player").and_then(|k| pubkey(&k)) else {
                    let _ = req.respond(json_code(serde_json::json!({
                        "error": "no player key — the browser signs its own shift, so it has to \
                                  say which key it will sign with"
                    }), 400));
                    continue;
                };
                // What kind of run this is, before reaching for a cluster. A review run has no
                // patient, no lease and no head, and asking the chain about it first meant a ward
                // with no chain answered "no chain to read" to a question that was never about the
                // chain. The sentence a reviewer gets says what their run is instead.
                if p != "/api/ward/open" {
                    let kind = sessions
                        .lock()
                        .unwrap()
                        .get(&param(&url, "id").unwrap_or_default())
                        .map(|s| (s.ward.is_some(), s.review.clone()));
                    if let Some((false, review)) = kind {
                        let _ = req.respond(json_code(serde_json::json!({
                            "error": if review.is_some() {
                                "this is a review run — the case is being read, not played on \
                                 anybody. Nothing here is on the ward or on the chain"
                            } else {
                                "this run is not a shift on the ward"
                            }
                        }), 409));
                        continue;
                    }
                }
                let chain = match ward_chain::WardChain::connect() {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = req.respond(json_code(serde_json::json!({ "error": e }), 503));
                        continue;
                    }
                };
                // Everything below needs the session except opening an account, which is a
                // stranger's first transaction and happens before they have one.
                let built = if p == "/api/ward/open" {
                    Ok((
                        ward_chain::open_account_ix(chain.program_id(), &chain.operator(), &who),
                        WardWork::Open,
                    ))
                } else {
                    let id = param(&url, "id").unwrap_or_default();
                    let map = sessions.lock().unwrap();
                    // The same rule the rest of the bay uses: an owned run answers to its owner
                    // and an anonymous one to whoever holds the id.
                    match map.get(&id).filter(|s| s.answers_to(Some(&who.to_string()))) {
                        None => Err("no such session".to_string()),
                        Some(s) => match (&s.ward, p) {
                            (None, _) if s.review.is_some() => Err(
                                "this is a review run — the case is being read, not played on \
                                 anybody. Nothing here is on the ward or on the chain"
                                    .into(),
                            ),
                            (None, _) => Err("this run is not a shift on the ward".into()),
                            (Some(w), "/api/ward/release") => Ok((
                                ward_chain::release_shift_ix(
                                    chain.program_id(), &chain.operator(), &who, w.patient_id),
                                WardWork::Release { session: id.clone() },
                            )),
                            (Some(w), "/api/ward/take") => Ok((
                                ward_chain::take_shift_ix(
                                    chain.program_id(), &chain.operator(), &who, w.patient_id),
                                WardWork::Take { patient_id: w.patient_id },
                            )),
                            (Some(w), "/api/ward/declare") => {
                                // Declared only by the key the chain says is holding her. The
                                // declaration is what makes a shift playable at all — `may_step`
                                // gates every tick on it — so the lease is checked here, once,
                                // rather than on every step, and a client that skipped the take
                                // is refused before it has a tape rather than at the anchor.
                                match chain.patient(w.patient_id) {
                                    Ok(Some(p))
                                        if ward::may_declare(p.lease_holder, who.to_bytes()) => {}
                                    Ok(Some(_)) => {
                                        drop(map);
                                        let _ = req.respond(json_code(serde_json::json!({
                                            "refused": "her head is not yours — take the shift \
                                                        before declaring it"
                                        }), 409));
                                        continue;
                                    }
                                    Ok(None) => {
                                        drop(map);
                                        let _ = req.respond(json_code(serde_json::json!({
                                            "error": "no such patient on this ward"
                                        }), 404));
                                        continue;
                                    }
                                    Err(e) => {
                                        drop(map);
                                        let _ = req.respond(json_code(serde_json::json!({
                                            "error": format!("her chart could not be read: {e}")
                                        }), 503));
                                        continue;
                                    }
                                }
                                // The nonce keeps the case hidden from chain observers until the
                                // reveal, and never leaves this process.
                                use solana_sdk::signature::Signer;
                                let nonce = solana_sdk::signature::Keypair::new().pubkey().to_bytes();
                                let case = sce_hash(&s.sce_json);
                                let hash = vitals_progress::record::commitment_hash(
                                    &case, &who.to_bytes(), &nonce, 0);
                                Ok((
                                    ward_chain::commit_ix(
                                        chain.program_id(), &chain.operator(), &who, hash),
                                    WardWork::Declare { session: id.clone(), hash, nonce },
                                ))
                            }
                            (Some(w), _) => {
                                // A shift anchors once, and the server is where that is settled —
                                // not the button. The page latches its two Hand over buttons, and
                                // a reload gets past a latch; this is before any chain is read,
                                // so a second anchor costs a sentence rather than a transaction
                                // the program will refuse.
                                if s.anchored {
                                    drop(map);
                                    let _ = req.respond(json_code(serde_json::json!({
                                        "refused": ward_chain::ALREADY_ANCHORED
                                    }), 409));
                                    continue;
                                }
                                // The anchor. Everything it carries is rebuilt rather than
                                // remembered: the reduction comes from the tape through the shared
                                // reducer, so what lands on chain is what a verifier recomputes.
                                let Some((chash, cslot, _)) = s.commit else {
                                    drop(map);
                                    let _ = req.respond(json_code(serde_json::json!({
                                        "error": "this shift was never declared — the chain \
                                                  refuses a shift that was not declared before it \
                                                  was played"
                                    }), 409));
                                    continue;
                                };
                                let Some(prev_head) = hex32(&w.head) else {
                                    drop(map);
                                    let _ = req.respond(json_code(serde_json::json!({
                                        "error": "this session does not know which head it extends"
                                    }), 500));
                                    continue;
                                };
                                let rebuild = ward_rebuild(&store, w.patient_id);
                                let r = match rebuild.as_ref().map(|b| ward_chain::resumed(
                                    &s.sce_json, &b.shifts,
                                    &|h| ward_chain::tape_by_hash(&store, h),
                                    b.admitted_slot, w.taken_slot,
                                    &ward_chain::cached_dater(&store))) {
                                    Some(Ok((mut st, _))) => vitals_replay::shift(&mut st, &s.tape, 0.0),
                                    Some(Err(e)) => { drop(map);
                                        let _ = req.respond(json_code(
                                            serde_json::json!({ "error": e }), 409));
                                        continue; }
                                    None => { drop(map);
                                        let _ = req.respond(json_code(serde_json::json!({
                                            "error": "the chain could not be read, so this shift \
                                                      cannot be reduced against the patient it \
                                                      was played on"
                                        }), 503));
                                        continue; }
                                };
                                let sce = sce_hash(&s.sce_json);
                                match record_for(who.to_bytes(), sce, sce, s.difficulty,
                                                 s.exam_mode, &s.tape, &r, chash, cslot) {
                                    // The tape is filed under this record's own run hash, here,
                                    // before the instruction exists. The hand-over files one too,
                                    // and on 16 ก.ย. the two disagreed — the page's clock kept
                                    // ticking between them — so the chain took this hash and the
                                    // store held the other, and the patient could not be rebuilt
                                    // by anybody. A leaf on chain whose tape was never kept is
                                    // worse than a shift that did not anchor, so a failure here
                                    // refuses the instruction rather than going ahead.
                                    Ok(rec) => match ward_chain::keep_for_anchor(
                                        &store, w.patient_id, &rec, &s.tape,
                                    ) {
                                        Err(e) => Err(e),
                                        Ok(_) => Ok((
                                            ward_chain::anchor_shift_ix(
                                                chain.program_id(), &chain.operator(), &who,
                                                w.patient_id, ward_chain::WARD_TREE,
                                                ward_chain::wire(&rec), prev_head),
                                            WardWork::Anchor { session: id.clone(), patient_id: w.patient_id,
                                                               leaf: rec.leaf() },
                                        )),
                                    },
                                    Err(e) => Err(e),
                                }
                            }
                        },
                    }
                };
                match built {
                    Err(e) => {
                        let _ = req.respond(json_code(serde_json::json!({ "error": e }), 409));
                    }
                    Ok((ix, work)) => match chain.prepare(ix, &who) {
                        Ok(pending) => {
                            let msg = hex_bytes(&pending.message());
                            ward_pendings.lock().unwrap()
                                .insert(who.to_string(), WardPending { pending, work, player: who });
                            let _ = req.respond(json(serde_json::json!({ "sign": msg })));
                        }
                        Err(e) => {
                            let _ = req.respond(json_code(serde_json::json!({ "error": e }), 503));
                        }
                    },
                }
                continue;
            }
            // The other half: the browser signed the bytes, we drop the signature into its slot
            // and send. A refusal from the program comes back as a sentence, because this is the
            // moment the ward is most worth watching — the chain deciding, in public, against
            // somebody who wanted a different answer.
            // ── the heartbeat ───────────────────────────────────────────────────────
            // A page holding a head says so every thirty seconds. POST, because that is what
            // `sendBeacon` sends and the leaving half of this pair is a beacon; no body and no
            // signature, because the only thing being said is "still here" and the session id is
            // already the secret every other control on that page is guarded by.
            (Method::Post, "/api/ward/beat") | (Method::Post, "/api/ward/left") if ward_mode() => {
                let leaving = path == "/api/ward/left";
                // Leaving frees a head, which is a hand on a patient; beating is a page saying it
                // is still there, which is true whatever the door says.
                let door = ward_chain::door_here();
                if leaving && !door.plays() {
                    let _ = req.respond(json_code(serde_json::json!({
                        "refused": door.refusal(), "door": door.word()
                    }), 409));
                    continue;
                }
                let patient = param(&url, "id")
                    .and_then(|id| sessions.lock().unwrap().get(&id).and_then(|s| s.ward.as_ref().map(|w| w.patient_id)));
                let Some(patient) = patient else {
                    let _ = req.respond(json_code(serde_json::json!({
                        "error": "no shift of that name is running here"
                    }), 404));
                    continue;
                };
                if leaving {
                    // The fast path: the tab is closing and the bed should be free before the next
                    // stranger refreshes the globe, not two missed beats later. Best effort by
                    // nature — a beacon can be dropped — and the heartbeat is the net under it.
                    beats.lock().unwrap().remove(&patient);
                    let freed = match ward_chain::WardChain::connect() {
                        Ok(chain) => chain.free_shift(patient).map(|_| true),
                        Err(e) => Err(e),
                    };
                    if let Err(e) = &freed {
                        eprintln!("ward       a page left patient {patient} and the head did not come back: {e}");
                    }
                    let _ = req.respond(json(serde_json::json!({ "freed": freed.is_ok() })));
                } else if param(&url, "done").is_some() {
                    // The shift ended properly — handed over, or handed back. The head is already
                    // back on chain, so the ward forgets this page rather than sweeping a head
                    // nobody is holding and paying for a transaction to say so.
                    beats.lock().unwrap().remove(&patient);
                    let _ = req.respond(json(serde_json::json!({ "heard": true, "watching": false })));
                } else {
                    beats.lock().unwrap().insert(patient, now_ms());
                    let _ = req.respond(json(serde_json::json!({
                        "heard": true,
                        "beat_again_in_seconds": BEAT_EVERY.as_secs(),
                    })));
                }
                continue;
            }
            (Method::Get, "/api/ward/submit") if ward_mode() => {
                let Some(who) = param(&url, "player").and_then(|k| pubkey(&k)) else {
                    let _ = req.respond(json_code(serde_json::json!({ "error": "no player key" }), 400));
                    continue;
                };
                let Some(sig) = param(&url, "sig").and_then(|h| sig64(&h)) else {
                    let _ = req.respond(json_code(serde_json::json!({ "error": "no signature" }), 400));
                    continue;
                };
                let Some(work) = ward_pendings.lock().unwrap().remove(&who.to_string()) else {
                    let _ = req.respond(json_code(serde_json::json!({
                        "error": "nothing of yours is waiting to be signed"
                    }), 409));
                    continue;
                };
                let chain = match ward_chain::WardChain::connect() {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = req.respond(json_code(serde_json::json!({ "error": e }), 503));
                        continue;
                    }
                };
                let tx = match work.pending.signed(&sig) {
                    Ok(tx) => tx,
                    Err(e) => {
                        let _ = req.respond(json_code(serde_json::json!({ "error": e }), 400));
                        continue;
                    }
                };
                let sent = chain.submit(&tx);
                let answer = match (&work.work, sent) {
                    (WardWork::Anchor { patient_id, leaf, .. }, Err(e)) => {
                        // A stale head at the end of an anchor is either somebody else's shift or
                        // our own, and only the chain can say which. Read once, here, at the
                        // moment of the refusal — the head it holds now is the answer.
                        let head_now = chain.patient(*patient_id).ok().flatten().map(|h| h.head);
                        match ward_chain::anchor_refusal(&e, head_now, *leaf) {
                            Some(said) => json_code(serde_json::json!({
                                "refused": said, "by": "the ward's program, on chain"
                            }), 409),
                            None => json_code(serde_json::json!({ "error": e }), 503),
                        }
                    }
                    (_, Err(e)) => {
                        // A refusal is the program saying no and is answered in words; anything
                        // else is an outage and is answered as one.
                        match ward_chain::refusal(&e) {
                            Some(said) => json_code(serde_json::json!({
                                "refused": said, "by": "the ward's program, on chain"
                            }), 409),
                            None => json_code(serde_json::json!({ "error": e }), 503),
                        }
                    }
                    (WardWork::Open, Ok(sig)) => json(serde_json::json!({ "opened": true, "tx": sig })),
                    (WardWork::Take { patient_id }, Ok(sig)) => {
                        // A shift taken, counted here and nowhere earlier: the program has accepted
                        // it, the head is theirs and the lease is running. Counting the press would
                        // have counted every take the program refused as a shift, and told us
                        // people were playing on a night nobody was.
                        usage.lock().unwrap().took_a_shift(&store);
                        let until = chain.patient(*patient_id).ok().flatten()
                            .map(|p| p.lease_until_slot);
                        json(serde_json::json!({
                            "took": true, "tx": sig, "lease_until_slot": until,
                            "note": "the head is yours until you anchor or the lease runs out"
                        }))
                    }
                    (WardWork::Declare { session, hash, nonce }, Ok(sig)) => {
                        // Read back rather than assumed: the slot was assigned on chain, and the
                        // record anchored later must carry the same one or the leaf the server
                        // builds is not the leaf the program checks.
                        match chain.commitment(&work.player) {
                            Some(c) if c.open && c.hash == *hash => {
                                let mut map = sessions.lock().unwrap();
                                if let Some(s) = map.get_mut(session) {
                                    s.commit = Some((*hash, c.slot, *nonce));
                                    persist(&store, session, s, true);
                                }
                                json(serde_json::json!({ "declared": true, "tx": sig, "slot": c.slot }))
                            }
                            _ => json_code(serde_json::json!({
                                "error": "the declaration landed but could not be read back — try again"
                            }), 503),
                        }
                    }
                    (WardWork::Release { session }, Ok(sig)) => {
                        // The tape goes with it. A shift that was put down is not a shift that
                        // happened, and a tape left lying about could be anchored later onto a
                        // patient somebody else has since moved.
                        let mut map = sessions.lock().unwrap();
                        if let Some(s) = map.get_mut(session) {
                            s.tape.clear();
                            s.beats.clear();
                            persist(&store, session, s, true);
                        }
                        drop(map);
                        json(serde_json::json!({
                            "released": true,
                            "tx": sig,
                            "recorded": "nothing — the head is back and her chart is as you found it"
                        }))
                    }
                    (WardWork::Anchor { session, patient_id, .. }, Ok(sig)) => {
                        let mut map = sessions.lock().unwrap();
                        let now_long = map.get(session).and_then(|s| s.ward.as_ref()).map(|w| w.index + 1);
                        if let Some(s) = map.get_mut(session) {
                            s.anchored = true;
                            persist(&store, session, s, true);
                        }
                        drop(map);
                        // How long her chain is now, from the shift that just landed rather than
                        // from a read that may not see it yet. `open_shift` waits on this.
                        if let Some(n) = now_long {
                            heads().lock().unwrap().insert(*patient_id, (n, std::time::Instant::now()));
                        }
                        let her = chain.patient(*patient_id).ok().flatten();
                        json(serde_json::json!({
                            "anchored": true,
                            "tx": sig,
                            "head": her.map(|h| hex_bytes(&h.head)),
                            "shifts": her.map(|h| h.shifts),
                            "state": her.map(|h| match h.state {
                                ward::DISCHARGED => "went_home",
                                ward::DIED => "died",
                                _ => "on_ward",
                            }),
                        }))
                    }
                };
                let _ = req.respond(answer);
                continue;
            }
            // The end of a shift: hand her over. Not the end of her stay — that is the engine's
            // to decide and the chain's to record. This reduces what this stranger did, keeps the
            // tape under the hash their leaf will commit to, and hands back what the anchor needs.
            (Method::Get, "/api/handover") => {
                let id = param(&url, "id").unwrap_or_default();
                let caller = param(&url, "player");
                let mut map = sessions.lock().unwrap();
                let Some(s) = map.get_mut(&id).filter(|s| s.answers_to(caller.as_deref())) else {
                    let _ = req.respond(no_such_session());
                    continue;
                };
                let Some(w) = s.ward.clone() else {
                    let _ = req.respond(json_code(serde_json::json!({
                        "error": "this run is not a shift on the ward — the bay's own ending is \
                                  /api/finish, and it ends a case rather than handing a patient on"
                    }), 409));
                    continue;
                };
                // A shift the chain has already taken is not reduced again: the same minutes
                // cannot be filed twice, and the sentence that says so is the ward's own.
                if s.anchored {
                    let _ = req.respond(json_code(serde_json::json!({
                        "refused": ward_chain::ALREADY_ANCHORED
                    }), 409));
                    continue;
                }
                // Reduced from the patient this shift walked into, so the harm and the beats are
                // this stranger's own and not the ones they inherited.
                let Some(rebuild) = ward_rebuild(&store, w.patient_id) else {
                    let _ = req.respond(json_code(serde_json::json!({
                        "error": "the chain could not be read, so this shift cannot be reduced \
                                  against the patient it was played on — nothing is recorded"
                    }), 503));
                    continue;
                };
                let base = ward_chain::resumed(
                    &s.sce_json,
                    &rebuild.shifts,
                    &|h| ward_chain::tape_by_hash(&store, h),
                    rebuild.admitted_slot,
                    w.taken_slot,
                    &ward_chain::cached_dater(&store),
                );
                let r = match base {
                    Ok((mut st, _)) => vitals_replay::shift(&mut st, &s.tape, 0.0),
                    Err(e) => {
                        let _ = req.respond(json_code(serde_json::json!({ "error": e }), 409));
                        continue;
                    }
                };
                let run_hash = hex(&leaf(&sce_hash(&s.sce_json), &s.tape, &r));
                let kept = ward_chain::keep_tape(&store, &ward_chain::StoredTape {
                    patient_id: w.patient_id,
                    run_hash: run_hash.clone(),
                    steps: s.tape.clone(),
                });
                // Frozen from here. The tape has been reduced and the leaf named; anything more
                // would be a step onto a tape that has already been counted.
                s.handed_over = true;
                persist(&store, &id, s, true);
                let out = serde_json::json!({
                    "patient_id": w.patient_id,
                    // Said in the payload rather than inferred by the page from a field's absence:
                    // the page has to stop its own clock on this, and a flag it has to guess at is
                    // a clock that keeps running.
                    "handed_over": true,
                    // What the anchor must name, and what it must extend. Both go on chain; the
                    // program refuses a reveal that does not extend the head it was told.
                    "run_hash": run_hash,
                    "prev_head": w.head,
                    "shift": {
                        "beats": r.beats.len(),
                        "harm": r.harm_events,
                        "outcome": r.outcome,
                        "sim_seconds": r.sim_seconds,
                        "steps": r.steps,
                    },
                    // Said plainly because it is not done yet: the tape is kept and reduced, and
                    // nothing is on chain until the browser signs the anchor.
                    "anchored": false,
                    "tape_kept": kept.is_ok(),
                    "next": "declare and anchor from the browser — this shift is on nobody's \
                             record until its leaf extends her head on chain",
                });
                if let Err(e) = kept {
                    let _ = req.respond(json_code(serde_json::json!({
                        "error": e,
                        "shift": out["shift"].clone(),
                    }), 500));
                    continue;
                }
                let _ = req.respond(json(out));
                continue;
            }
            // The factory again, after admission: the rest of her portraits (ruling 10, and the
            // producer's 16 ก.ย. state-keyed set). Same token, same door switch, same validation —
            // and add-only, so a face the board has shown cannot be changed underneath it.
            (Method::Post, p) if p.starts_with("/api/ward/pack/") => {
                if !ward_mode() {
                    let _ = req.respond(json(serde_json::json!({
                        "ward": "not on this host",
                        "the_ward_is": "https://world.vitals.academy/api/ward/pack/<patient_id>"
                    })));
                    continue;
                }
                if !ward_chain::door_open_here() {
                    let _ = req.respond(
                        json(serde_json::json!({
                            "door": ward_chain::door_here().word(),
                            "why": "the ward is not open yet, so the factory has nothing to do \
                                    here either"
                        }))
                        .with_status_code(503),
                    );
                    continue;
                }
                // Two kinds of address arrive here, and they carry different rules. A patient id
                // is digits and her faces are add-only — the board has shown them. A pack id is a
                // content address and hers may be replaced, because nobody has seen her yet.
                let who = p.strip_prefix("/api/ward/pack/").unwrap_or("");
                let patient_id = who
                    .bytes()
                    .all(|b| b.is_ascii_digit())
                    .then(|| who.parse::<u64>().ok())
                    .flatten();
                let queued = (who.len() == 64
                    && who.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
                    .then_some(who);
                if patient_id.is_none() && queued.is_none() {
                    let _ = req.respond(
                        json(serde_json::json!({
                            "error": "that is neither a patient id nor a pack id"
                        }))
                        .with_status_code(404),
                    );
                    continue;
                }
                let body = match read_body(&mut req, QUEUE_MAX) {
                    Ok(b) => b,
                    Err(_) => {
                        let _ = req.respond(json(serde_json::json!({
                            "error": "that body is not UTF-8 text, or is past the limit"
                        })));
                        continue;
                    }
                };
                #[derive(serde::Deserialize)]
                struct Fill {
                    portrait: std::collections::BTreeMap<String, String>,
                }
                match serde_json::from_str::<Fill>(&body) {
                    Ok(fill) => {
                        let r = match (patient_id, queued) {
                            (Some(id), _) => ward_chain::fill_portraits(&store, id, fill.portrait),
                            (None, Some(pack)) => {
                                ward_chain::replace_queued_portraits(&store, pack, fill.portrait)
                            }
                            _ => unreachable!("one of the two was matched above"),
                        };
                        let _ = req.respond(json(r));
                    }
                    Err(e) => {
                        let _ = req.respond(json(serde_json::json!({
                            "error": format!("that is not a set of portraits: {e}"),
                            "shape": {"portrait": {
                                "<one of stable improving deteriorating critical arrest recovered>":
                                    format!("{}/<sha256>.webp", ward_chain::PORTRAITS)
                            }}
                        })));
                    }
                }
                continue;
            }
            // The bay's own two files, shared by the Eternal entry and the ward's shift page.
            // Served by the same process that holds the token, for the same reason the page is:
            // reaching the script and reaching the API are one boundary.
            // The ward host's mark. Public, unguarded and cached for a year: it is on every page
            // of the ward, it never changes without a deploy, and nothing about it is a secret.
            (Method::Get, "/world/apple-touch-icon.png") => {
                let _ = req.respond(
                    Response::from_data(TOUCH_ICON_WORLD)
                        .with_header(
                            Header::from_bytes(&b"Content-Type"[..], &b"image/png"[..]).unwrap(),
                        )
                        .with_header(
                            Header::from_bytes(
                                &b"Cache-Control"[..],
                                &b"public, max-age=31536000, immutable"[..],
                            )
                            .unwrap(),
                        ),
                );
                continue;
            }
            // The guide a first-time stranger is pointed at from the bedside. Ward only: it
            // teaches this ward's controls, and the Eternal entry has its own season around it.
            //
            // `/start` and `/start/` are the same page because both are what a person types, and
            // a trailing slash deciding whether somebody gets help is not a rule worth having.
            (Method::Get, p) if ward_mode() && (p == "/start" || p == "/start/") => {
                let _ = req.respond(html(WARD_START));
                continue;
            }
            (Method::Get, p) if ward_mode() && p.starts_with("/start/img/") => {
                let want = &p["/start/img/".len()..];
                match WARD_START_IMG.iter().find(|(name, _)| *name == want) {
                    Some((_, bytes)) => {
                        let _ = req.respond(
                            Response::from_data(*bytes)
                                .with_header(
                                    Header::from_bytes(&b"Content-Type"[..], &b"image/jpeg"[..])
                                        .unwrap(),
                                )
                                .with_header(forever()),
                        );
                    }
                    // A name not in the list is not a file this server has, and saying so is the
                    // whole of the answer: the set is closed, so nothing a stranger types reaches
                    // past it.
                    None => {
                        let _ = req.respond(
                            Response::from_string("no such picture").with_status_code(404),
                        );
                    }
                }
                continue;
            }
            (Method::Get, "/world/favicon.svg") => {
                let _ = req.respond(
                    Response::from_data(FAVICON_WORLD)
                        .with_header(
                            Header::from_bytes(&b"Content-Type"[..], &b"image/svg+xml"[..]).unwrap(),
                        )
                        .with_header(
                            Header::from_bytes(
                                &b"Cache-Control"[..],
                                &b"public, max-age=31536000, immutable"[..],
                            )
                            .unwrap(),
                        ),
                );
                continue;
            }
            (Method::Get, p) if p == "/bay.css" || p.starts_with("/bay.css?") => {
                let css = BAY_CSS.replace(BUILD_STAMP, BUILD);
                let resp = squeezed(&req, css.into_bytes(), b"text/css; charset=utf-8")
                    .with_header(forever());
                let _ = req.respond(resp);
                continue;
            }
            (Method::Get, p) if p == "/bay.js" || p.starts_with("/bay.js?") => {
                let js = BAY_JS
                    .replace("__VITALS_TOKEN__", token.as_deref().unwrap_or(""))
                    .replace(BUILD_STAMP, BUILD);
                let resp = squeezed(&req, js.into_bytes(), b"application/javascript; charset=utf-8")
                    .with_header(forever());
                let _ = req.respond(resp);
                continue;
            }
            // One shift, for somebody who never played it. Public and unguarded: a receipt only
            // the people we hand a token to can read is not a receipt, it is a claim.
            // One patient's whole stay: who she is, what happened to her, and every shift that
            // treated her with the address of its receipt. The page for a patient who is no longer
            // in a bed reads this — until it existed, her page was "Not this bed".
            (Method::Get, p) if ward_mode() && p.starts_with("/api/ward/patient/") => {
                let id = p.trim_start_matches("/api/ward/patient/").trim_end_matches('/');
                match id.parse::<u64>() {
                    Ok(patient_id) => {
                        let _ = req.respond(json(ward_chart(&store, patient_id)));
                    }
                    Err(_) => {
                        let _ = req.respond(json_code(serde_json::json!({
                            "error": "a patient id is a whole number"
                        }), 404));
                    }
                }
                continue;
            }
            (Method::Get, p) if ward_mode() && p.starts_with("/api/shift/") => {
                let hash = p.trim_start_matches("/api/shift/");
                let _ = req.respond(json(ward_receipt(&store, hash)));
                continue;
            }
            // The tape itself, by the hash its leaf commits to. This is what makes the rest
            // checkable rather than merely readable: a stranger replays these bytes against the
            // pinned engine and arrives at the same numbers, or we are wrong.
            (Method::Get, p) if ward_mode() && p.starts_with("/api/tape/") => {
                let hash = p.trim_start_matches("/api/tape/");
                match ward_chain::tape_by_hash(&store, hash) {
                    Some(steps) => {
                        let _ = req.respond(json(serde_json::json!({
                            "run_hash": hash,
                            "steps": steps,
                            "how_to_check": "replay these steps against the case's scenario with \
                                             vitals-replay and the leaf must come out as this hash",
                        })));
                    }
                    None => {
                        let _ = req.respond(json_code(serde_json::json!({
                            "error": "this ward does not hold that tape"
                        }), 404));
                    }
                }
                continue;
            }
            (Method::Get, p) if ward_mode() && p.starts_with("/shift/") => {
                let hash = p.trim_start_matches("/shift/").to_string();
                let answer = ward_receipt(&store, &hash);
                // A refusal here offers the beds that are open, off the board this host already
                // holds — the person reading it came to treat somebody (UX review F1).
                let board = answer["error"].is_string().then(|| ward_now(&ward_view, &store, &state_dir));
                let _ = req.respond(html(&receipt_page(&answer, &board.unwrap_or(serde_json::Value::Null))));
                continue;
            }
            // The ward's census. Public, and every figure on it carries where it came from —
            // `vitals_web::ward` builds the payload, `ward_chain` does the reading, and neither
            // of them can report a number this server kept for itself.
            (Method::Get, "/api/ward") => {
                if !ward_mode() {
                    // vitals.academy is the Eternal entry and is never redeployed for the sprint
                    // (CWF_PLAN.md ruling 8). Answering here with an empty ward would be a
                    // number about a thing this host does not run.
                    let _ = req.respond(json(serde_json::json!({
                        "ward": "not on this host",
                        "the_ward_is": "https://world.vitals.academy/api/ward",
                        "why": "the ward is its own host and its own service, so this entry is \
                                never redeployed for it"
                    })));
                    continue;
                }
                // One implementation, in `read_response`. The reader pool answers this path when
                // the ward is serving; this is the same answer for the Eternal entry and for a
                // send that found no reader. Within an hour of writing "one implementation, two
                // callers" there were two implementations, and the drift was found by a test hook
                // that existed in one of them — so the delegation is the point, not the tidiness.
                match read_response(&req, &path, &ward_view, &store, &state_dir, &usage, &token) {
                    Some(r) => { let _ = req.respond(r); }
                    None => { let _ = req.respond(Response::from_data(b"not found".to_vec()).with_status_code(404)); }
                }
                continue;
            }

            (Method::Get, "/api/chain") => {
                // On the ward host these would answer for vitals.academy's play, not for a ward
                // that has not opened — a number that is true elsewhere is still a wrong answer
                // here. Say so, and point at where the real one lives.
                if ward_mode() {
                    let _ = req.respond(json(serde_json::json!({
                        "ward": "not open yet",
                        "opens": "week 2 of Crypto World's Fair, 21-27 Sep 2026",
                        "chain_for_the_eternal_entry": "https://vitals.academy/api/chain"
                    })));
                    continue;
                }
                let t = tree.lock().unwrap();
                let who = param(&url, "player").and_then(|p| pubkey(&p));
                json(serde_json::json!({
                    "connected": chain.is_some(),
                    // Which cluster the records anchor to, read off the RPC url — the page shows
                    // this string, and a label the server derives cannot drift from where the
                    // transactions actually go. It said "localnet" on the public demo once.
                    "cluster": chain.as_ref().map(|c| cluster_of(&c.deployment().2)),
                    // A gateway with no persona for a case is not a voice in that case. The
                    // page shows the chat affordance off this flag, and offering a microphone to
                    // a patient who cannot answer is a worse first impression than not offering
                    // one — so it reports the cases that can actually speak.
                    "voice": patient.is_some(),
                    "voiced": if patient.is_some() { personas.keys().collect::<Vec<_>>() } else { Vec::new() },
                    // Which stations can sit an exam — the server's rubric map is the only copy.
                    "exam_eps": exam_eps,
                    // The three bars a station's star is read against. Served here as well as
                    // on /api/stars because the shelf must be able to *say* what a star costs
                    // ("70% pass · 85% excellent · 95% flawless") to a visitor with no account
                    // and no chain — a rule nobody can read is not a rule they can aim at.
                    "star_bars": { "pass": star_bars.pass, "excellent": star_bars.excellent,
                                   "flawless": star_bars.flawless, "tiers": vitals_progress::STAR_TIERS },
                    // Station Sets v2, the shape without the player: which sets exist, who is
                    // in them, what each door costs today. The page draws the shelf from this
                    // — declared-but-unpublished members become coming-soon cards — and joins
                    // it with the per-player tiers from /api/stars. One copy, this one.
                    "sets": set_states.iter().map(|st| serde_json::json!({
                        "gate": st.set.gate,
                        "opens": st.set.opens,
                        "need": st.set.need,
                        "need_now": st.need_now,
                        // What this set is worth today (playable members × 3). The shelf's
                        // "6 / 9 ⭐" strip and the season ring are drawn from this, so the
                        // page never multiplies by a 3 of its own.
                        "ceiling": st.ceiling(),
                        "complete": st.members.iter().all(|(_, h)| h.is_some()),
                        // ── what a member may say before the bell ────────────────────
                        // `case` and `specialty` are NOT here, and this is the whole point of
                        // the shape. The bank id spells the diagnosis out loud
                        // ("ddx-anaphylaxis-1"), and the Eir specialty names the organ the
                        // rubric is marking — so this one unauthenticated GET used to hand a
                        // candidate the answer to all twelve stations before they sat any of
                        // them, undoing every stem, band and nudge fix that came before it.
                        // Both now travel on `/api/marks`, which opens only once the case has
                        // an outcome. Nothing on this endpoint may name a disease again.
                        "members": st.members.iter().map(|(m, h)| serde_json::json!({
                            "id": m.id,
                            "title": m.title,
                            // What the card wears: the circuit band, never the organ — an
                            // organ name over a stem is a free rubric point (see SetMember).
                            "band": m.band,
                            "tier": tier_str(m.tier),
                            // Which patient stills this station has on disk right now. The bay
                            // hangs one of these in the frame and swaps it as the patient goes
                            // down; an empty list is a station whose art has not landed, and its
                            // frame keeps the stem. See STATION_STATES.
                            "states": station_states(m.id),
                            "playable": h.is_some(),
                        })).collect::<Vec<_>>(),
                    })).collect::<Vec<_>>(),
                    "tree_id": t.tree_id,
                    "anchored": t.leaves.len(),
                    "relay": chain.as_ref().map(|c| c.relay_pubkey()),
                    // How many of the tree's leaves are *this* player's. Before the relay split
                    // there was one number here because there was one identity.
                    "proven": match (chain.as_ref(), who) {
                        (Some(c), Some(k)) => Some(c.proven_count(&k, t.tree_id)),
                        _ => None,
                    },
                }))
            }
            // Declare the run before it is played. The player signs the declaration; the chain
            // stamps the slot; the session keeps all of it so the record built at anchor time
            // carries exactly what the program will stamp into the leaf.
            (Method::Get, "/api/commit") => {
                let Some(c) = &chain else {
                    let _ = req.respond(json(serde_json::json!({ "error": "no chain configured" })));
                    continue;
                };
                let Some(id) = param(&url, "id") else {
                    let _ = req.respond(json(serde_json::json!({ "error": "no session" })));
                    continue;
                };
                let Some(who) = param(&url, "player").and_then(|p| pubkey(&p)) else {
                    let _ = req.respond(json(serde_json::json!({ "error": "no player key" })));
                    continue;
                };
                let person = param(&url, "account").and_then(|p| pubkey(&p)).unwrap_or(who);
                let (case, ep) = {
                    let map = sessions.lock().unwrap();
                    let Some(s) = map.get(&id) else {
                        drop(map);
                        let _ = req.respond(no_such_session());
                        continue;
                    };
                    (sce_hash(&s.sce_json), s.ep.clone())
                };
                // The nonce is what keeps the case hidden from chain observers until reveal. It
                // never leaves this process except inside a debrief the player asks for.
                let nonce = {
                    use solana_sdk::signature::Signer;
                    solana_sdk::signature::Keypair::new().pubkey().to_bytes()
                };
                // Exam-ness is part of the declaration, bound into the hash the chain stamps —
                // decided here, before play, and never re-chosen after the outcome is known.
                let mode: u8 = param(&url, "exam").map(|v| v == "1").unwrap_or(false) as u8;
                // Refused before anything binds: letting a player commit "exam" on a station
                // with no rubric would promise a star that can never be scored into existence.
                if mode == 1 && rubric_path(&ep).is_none() {
                    let _ = req.respond(json(serde_json::json!({
                        "error": "this station has no rubric yet — an exam here could never be scored; play it as practice"
                    })));
                    continue;
                }
                let hash = vitals_progress::record::commitment_hash(&case, &person.to_bytes(), &nonce, mode);
                match c.prepare_commit(&who, &person, hash) {
                    Ok(p) => {
                        let msg = hex_bytes(&p.message());
                        pendings.lock().unwrap().insert(
                            who.to_string(),
                            PendingWork { pending: p, session: id.clone(), account: person,
                                          // Not an anchor: no leaf of its own, and nothing to pay for.
                                          leaf: String::new(), case: String::new(),
                                          prove: None, commit: Some((hash, nonce, mode)),
                                          index: 0, score: 0, det: None, level: None, link: false },
                        );
                        json(serde_json::json!({ "sign": msg }))
                    }
                    Err(e) => json(serde_json::json!({ "error": e })),
                }
            }
            (Method::Get, "/api/anchor") => {
                let id = param(&url, "id").unwrap_or_default();
                let caller = param(&url, "player");
                let Some(c) = chain.as_ref() else {
                    let _ = req.respond(json(serde_json::json!({ "error": "no chain connected" })));
                    continue;
                };
                let mut map = sessions.lock().unwrap();
                let Some(s) = map.get_mut(&id).filter(|s| s.answers_to(caller.as_deref())) else {
                    let _ = req.respond(no_such_session());
                    continue;
                };
                if !s.over() {
                    let _ = req.respond(json(serde_json::json!({ "error": "the run has not finished" })));
                    continue;
                }
                if s.anchored {
                    let _ = req.respond(json(serde_json::json!({ "error": "already anchored" })));
                    continue;
                }
                // Rebuild the run from the tape through the shared reducer rather than from the
                // live session, so what gets anchored is exactly what a verifier would recompute.
                let r = match replay(&s.sce_json, &s.tape) {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = req.respond(json(serde_json::json!({ "error": e })));
                        continue;
                    }
                };
                // Whose run this is. It arrives from the browser and it is the key the browser
                // will sign with — the server has no way to produce that signature, which is what
                // makes the credential the player's rather than the server's.
                let Some(who) = param(&url, "player").and_then(|p| pubkey(&p)) else {
                    let _ = req.respond(json(serde_json::json!({ "error": "no player key" })));
                    continue;
                };
                // The machine signs; the person owns. Absent an account the two are the same, which
                // is exactly what the very first run on a brand new browser is.
                let person = param(&url, "account").and_then(|p| pubkey(&p)).unwrap_or(who);
                let sce = sce_hash(&s.sce_json);
                // The commitment made before this run started. Anchoring without one is refused
                // here with a sentence rather than by the program with an error code — the
                // program will refuse it anyway, since it reads the commitment account and finds
                // nothing open, but the person typing deserves to know what was missing.
                let Some((chash, cslot, _nonce)) = s.commit else {
                    let _ = req.respond(json(serde_json::json!({
                        "error": "this run was never committed — the chain refuses runs that were not declared before play"
                    })));
                    continue;
                };
                let mut rec: AttemptRecord =
                    match record_for(person.to_bytes(), sce, sce, s.difficulty, s.exam_mode, &s.tape, &r, chash, cslot) {
                        Ok(rec) => rec,
                        Err(e) => {
                            let _ = req.respond(json(serde_json::json!({ "error": e })));
                            continue;
                        }
                    };
                // An exam run is marked by the pinned rubric, recomputed here from the same tape
                // the leaf commits to — never accepted from a client. Practice runs stay zero:
                // det_score only means something for an exam. The rubric gate at commit means an
                // exam without a rubric cannot reach this line; the error path guards the seam
                // anyway, because "cannot happen" is a claim, not a property.
                if s.exam_mode {
                    let scored = rubric_path(&s.ep)
                        .ok_or_else(|| "this station has no rubric — the exam cannot be scored".to_string())
                        .and_then(|p| std::fs::read_to_string(&p).map_err(|e| e.to_string()))
                        .and_then(|rj| vitals_osce::det_for_run(&s.sce_json, &s.tape, &rj));
                    match scored {
                        Ok((det, max, rh)) => {
                            rec.det_score = det;
                            rec.det_max = max;
                            rec.rubric_hash = rh;
                        }
                        Err(e) => {
                            let _ = req.respond(json(serde_json::json!({ "error": e })));
                            continue;
                        }
                    }
                }
                let mut t = tree.lock().unwrap();
                let tree_id = t.tree_id;
                // Ask the chain how long its tree is before trusting ours. `reconcile_leaves`
                // says why the two answers may not differ, and why the two ways they can differ
                // are not treated alike.
                let chain_len = match c.tree_len(tree_id) {
                    // No tree account yet is a real answer: nothing anchored, so zero.
                    Ok(n) => n.unwrap_or(0),
                    // "Could not ask" is not zero. Refusing costs one player one attempt;
                    // reading it as zero would truncate every leaf this server holds.
                    Err(e) => {
                        drop(t);
                        let _ = req.respond(json(serde_json::json!({
                            "error": format!("cannot anchor without reading the tree first: {e}"),
                        })));
                        continue;
                    }
                };
                match reconcile_leaves(&mut t.leaves, chain_len) {
                    Reconciled::Ready { dropped } => {
                        if dropped > 0 {
                            // Out loud, with the arithmetic: a list that quietly changed length
                            // is the beginning of the next investigation.
                            eprintln!(
                                "tree #{tree_id}: dropped {dropped} leaf/leaves prepared and \
                                 never anchored — the local list was {} against {chain_len} on \
                                 chain",
                                chain_len as usize + dropped
                            );
                            let _ = store.put(TREE, &tree_key, &*t);
                        }
                    }
                    Reconciled::Short { local, chain } => {
                        drop(t);
                        let _ = req.respond(json(serde_json::json!({
                            "error": "this server cannot prove what it has already anchored, \
                                      so it will not anchor more",
                            "leaves_here": local,
                            "leaves_on_chain": chain,
                            "why": "a leaf that is on chain is missing from the list every \
                                    proof is rebuilt from. Someone holds a proof of it, and \
                                    anchoring now would build a tree that abandons it.",
                        })));
                        continue;
                    }
                }
                t.leaves.push(rec.leaf());
                let leaves = t.leaves.clone();
                let index = leaves.len() as u64 - 1;
                drop(t);
                match c.prepare_anchor(&who, &person, tree_id, &rec, &leaves) {
                    Ok((anchor, prove)) => {
                        let msg = hex_bytes(&anchor.message());
                        let msg2 = hex_bytes(&prove.message());
                        pendings.lock().unwrap().insert(
                            who.to_string(),
                            PendingWork { pending: anchor, prove: Some(prove),
                                          session: id.clone(), account: person, commit: None,
                                          index, leaf: hex(&rec.leaf()),
                                          case: hex(&sce_hash(&s.sce_json)),
                                          score: rec.score(),
                                          det: s.exam_mode.then_some((rec.det_score, rec.det_max)),
                                          level: None, link: false },
                        );
                        json(serde_json::json!({ "sign": msg, "sign2": msg2 }))
                    }
                    Err(e) => {
                        // Building it failed, so the leaf was never anchored. Take it back off the
                        // list or every later proof is built against a tree that does not exist.
                        // Nothing else has run since the push a few lines up — this request has
                        // not let go of the loop — so the guard always passes here. It goes
                        // through `unwind_leaf` anyway, because the day someone moves this unwind
                        // past a point where the browser answers is the day that stops being true.
                        unwind_leaf(&mut tree.lock().unwrap().leaves, index);
                        json(serde_json::json!({ "error": e }))
                    }
                }
            }
            (Method::Get, "/api/claim") => {
                let level: u8 = param(&url, "level").and_then(|v| v.parse().ok()).unwrap_or(2);
                let Some(c) = chain.as_ref() else {
                    let _ = req.respond(json(serde_json::json!({ "error": "no chain connected" })));
                    continue;
                };
                let Some(who) = param(&url, "player").and_then(|p| pubkey(&p)) else {
                    let _ = req.respond(json(serde_json::json!({ "error": "no player key" })));
                    continue;
                };
                let id = param(&url, "account").and_then(|p| pubkey(&p)).unwrap_or(who);
                let tree_id = tree.lock().unwrap().tree_id;
                match c.prepare_claim(&who, &id, tree_id, level) {
                    Ok(p) => {
                        let msg = hex_bytes(&p.message());
                        pendings.lock().unwrap().insert(
                            who.to_string(),
                            PendingWork { pending: p, session: String::new(), account: id, prove: None, commit: None,
                                          // Not an anchor: no leaf of its own, and nothing to pay for.
                                          leaf: String::new(), case: String::new(),
                                          index: 0, score: 0, det: None, level: Some(level), link: false },
                        );
                        json(serde_json::json!({ "sign": msg }))
                    }
                    Err(e) => json(serde_json::json!({ "error": e })),
                }
            }
            // Who this machine is, and whose record it may write to.
            (Method::Get, "/api/account") => {
                let Some(c) = chain.as_ref() else {
                    let _ = req.respond(json(serde_json::json!({ "error": "no chain connected" })));
                    continue;
                };
                let Some(dev) = param(&url, "device").and_then(|p| pubkey(&p)) else {
                    let _ = req.respond(json(serde_json::json!({ "error": "no device key" })));
                    continue;
                };
                let person = param(&url, "account").and_then(|p| pubkey(&p)).unwrap_or(dev);
                let acct = c.account(&person);
                json(serde_json::json!({
                    "account": person.to_string(),
                    "open": acct.is_some(),
                    // Whether *this* machine may act for that person. A machine that has been
                    // named but not yet linked is a machine that can watch and not play.
                    "linked": acct.as_ref().map(|a| a.allows(&dev)).unwrap_or(person == dev),
                    "devices": acct.as_ref().map(|a| a.authorities.len()).unwrap_or(0),
                    // How many runs this person has ever declared, as the chain counted them —
                    // the number the commit-reveal design exists to make undeniable: five
                    // practice runs need five visible commitments.
                    "started": c.commitment(&person).map(|cm| cm.started),
                }))
            }
            // Reading somebody's level is not a privileged act, and that is the whole point:
            // a score you can only see on the machine that earned it is not a credential.
            (Method::Get, "/api/progress") => {
                let Some(c) = chain.as_ref() else {
                    let _ = req.respond(json(serde_json::json!({ "error": "no chain connected" })));
                    continue;
                };
                let Some(person) = param(&url, "account").and_then(|p| pubkey(&p)) else {
                    let _ = req.respond(json(serde_json::json!({ "error": "no account" })));
                    continue;
                };
                match c.progress(&person) {
                    Some(pr) => json(serde_json::json!({
                        "account": person.to_string(),
                        "level": pr.level,
                        "level_name": chain::level_name(pr.level),
                        "attempts": pr.attempts_counted,
                        "distinct": pr.distinct_cases,
                        "xp": pr.xp,
                    })),
                    None => json(serde_json::json!({
                        "account": person.to_string(), "level": serde_json::Value::Null,
                        "message": "nothing claimed yet",
                    })),
                }
            }
            // Stars — distinct exam-mode cases cleared at or above the pass bar. Read-only and
            // additive: the level path never consults this. The bar rides in the reply, so what
            // was asked sits next to what was answered and a verifier can re-derive the count.
            (Method::Get, "/api/stars") => {
                let Some(c) = chain.as_ref() else {
                    let _ = req.respond(json(serde_json::json!({ "error": "no chain connected" })));
                    continue;
                };
                let Some(person) = param(&url, "account")
                    .or_else(|| param(&url, "player"))
                    .and_then(|p| pubkey(&p))
                else {
                    let _ = req.respond(json(serde_json::json!({ "error": "no account" })));
                    continue;
                };
                let tree_id = tree.lock().unwrap().tree_id;
                // Station Sets v2: the three-tier star per member, from the best proven det of
                // that case — one claim-buffer read answers every set. The playable members'
                // hashes were resolved at boot; a declared-only member is tier 0 by definition,
                // present in the reply so the shape never shifts when Phase 5b publishes it.
                let cases: Vec<[u8; 32]> = set_states
                    .iter()
                    .flat_map(|st| st.members.iter().filter_map(|(_, h)| *h))
                    .collect();
                let tiers = c.star_tiers(&person, tree_id, &cases, star_bars);
                let mut next = tiers.iter().copied();
                let sets: Vec<serde_json::Value> = set_states
                    .iter()
                    .map(|st| {
                        let mut total = 0u32;
                        let members: serde_json::Map<String, serde_json::Value> = st
                            .members
                            .iter()
                            .map(|(m, h)| {
                                let t = if h.is_some() { next.next().unwrap_or(0) } else { 0 };
                                total += t;
                                (m.id.to_string(), t.into())
                            })
                            .collect();
                        serde_json::json!({
                            "gate": st.set.gate,
                            "opens": st.set.opens,
                            "need": st.set.need,
                            "need_now": st.need_now,
                            "ceiling": st.ceiling(),
                            "total": total,
                            "tiers": members,
                        })
                    })
                    .collect();
                json(serde_json::json!({
                    // The original fields, exactly as they were — the verify page and the old
                    // scripts read these, and a door that opened on them keeps opening.
                    "account": person.to_string(),
                    "stars": c.star_count(&person, tree_id, star_pass_bps),
                    "pass_bps": star_pass_bps,
                    "excellent_bps": star_bars.excellent,
                    // The third bar, added with the three-star repricing. The two above it keep
                    // their names and their meanings, so a reader that predates this field sees
                    // exactly what it saw before.
                    "flawless_bps": star_bars.flawless,
                    "tiers_max": vitals_progress::STAR_TIERS,
                    "sets": sets,
                }))
            }
            // Link or unlink a machine. Signed by one that is already trusted.
            (Method::Get, "/api/link") => {
                let Some(c) = chain.as_ref() else {
                    let _ = req.respond(json(serde_json::json!({ "error": "no chain connected" })));
                    continue;
                };
                let Some(dev) = param(&url, "player").and_then(|p| pubkey(&p)) else {
                    let _ = req.respond(json(serde_json::json!({ "error": "no player key" })));
                    continue;
                };
                let person = param(&url, "account").and_then(|p| pubkey(&p)).unwrap_or(dev);
                let Some(other) = param(&url, "device").and_then(|p| pubkey(&p)) else {
                    let _ = req.respond(json(serde_json::json!({ "error": "no device to link" })));
                    continue;
                };
                let on = param(&url, "off").is_none();
                // prepare_link opens the account itself when there is not one yet, so the
                // transaction always does what the button said.
                match c.prepare_link(&dev, &person, &other, on) {
                    Ok(p) => {
                        let msg = hex_bytes(&p.message());
                        pendings.lock().unwrap().insert(
                            dev.to_string(),
                            PendingWork { pending: p, session: String::new(), account: person, prove: None, commit: None,
                                          // Not an anchor: no leaf of its own, and nothing to pay for.
                                          leaf: String::new(), case: String::new(),
                                          index: 0, score: 0, det: None, level: None, link: true },
                        );
                        json(serde_json::json!({ "sign": msg }))
                    }
                    Err(e) => json(serde_json::json!({ "error": e })),
                }
            }
            // The other half. The browser signed the bytes we handed it; we drop the signature
            // into its slot and send. If it does not verify, nothing is sent.
            (Method::Get, "/api/submit") => {
                let Some(c) = chain.as_ref() else {
                    let _ = req.respond(json(serde_json::json!({ "error": "no chain connected" })));
                    continue;
                };
                let Some(who) = param(&url, "player").and_then(|p| pubkey(&p)) else {
                    let _ = req.respond(json(serde_json::json!({ "error": "no player key" })));
                    continue;
                };
                let Some(sig) = param(&url, "sig").and_then(|h| sig64(&h)) else {
                    let _ = req.respond(json(serde_json::json!({ "error": "no signature" })));
                    continue;
                };
                let Some(work) = pendings.lock().unwrap().remove(&who.to_string()) else {
                    let _ = req.respond(json(serde_json::json!({ "error": "nothing waiting to be signed" })));
                    continue;
                };
                // Only an anchor put a leaf on the list, so only an anchor takes one back off.
                let speculative = work.level.is_none() && !work.link && work.commit.is_none();
                let tx = match work.pending.signed(&sig) {
                    Ok(tx) => tx,
                    Err(e) => {
                        if speculative {
                            unwind_leaf(&mut tree.lock().unwrap().leaves, work.index);
                        }
                        let _ = req.respond(json(serde_json::json!({ "error": e })));
                        continue;
                    }
                };
                let id = work.account;
                if let Some((hash, nonce, mode)) = work.commit {
                    let _ = req.respond(match c.submit(&tx) {
                        Ok(()) => match c.commitment(&id) {
                            // Read back rather than assumed: the slot was assigned on chain, and
                            // the record built at anchor time must carry the same one the program
                            // will stamp into the leaf — a guessed slot forks the server's leaf
                            // list from the tree.
                            Some(cm) if cm.open && cm.hash == hash => {
                                let mut map = sessions.lock().unwrap();
                                if let Some(s) = map.get_mut(&work.session) {
                                    s.commit = Some((hash, cm.slot, nonce));
                                    // From the landed commitment, never re-chosen later: the
                                    // anchor stamps the record's exam flag from this field.
                                    s.exam_mode = mode == 1;
                                    persist(&store, &work.session, s, true);
                                }
                                json(serde_json::json!({ "committed": true, "started": cm.started, "exam": mode == 1 }))
                            }
                            _ => json(serde_json::json!({
                                "error": "the commit landed but could not be read back — try again"
                            })),
                        },
                        Err(e) => json(serde_json::json!({ "error": e })),
                    });
                    continue;
                }
                if work.link {
                    let _ = req.respond(match c.submit(&tx) {
                        Ok(()) => {
                            let n = c.account(&id).map(|a| a.authorities.len()).unwrap_or(0);
                            json(serde_json::json!({ "linked": true, "devices": n }))
                        }
                        Err(e) => json(serde_json::json!({ "error": e })),
                    });
                    continue;
                }
                match (c.submit(&tx), work.level) {
                    (Ok(()), Some(_)) => match c.claimed(&id) {
                        Ok(m) => json(serde_json::json!({ "granted": true, "message": m })),
                        Err(m) => json(serde_json::json!({ "granted": false, "message": m })),
                    },
                    (Ok(()), None) => {
                        // The tree really changed, so write it before anything else can fail.
                        let t = tree.lock().unwrap();
                        let _ = store.put(TREE, &tree_key, &*t);
                        drop(t);
                        // The proof rides as a second transaction — the pair stopped fitting in
                        // one packet when the record grew. The anchor is already in; a proof that
                        // fails here leaves an intact state (anchored, provable later), so the
                        // error is reported rather than unwound.
                        if let Some(prove) = work.prove {
                            let sent = param(&url, "sig2")
                                .and_then(|h| sig64(&h))
                                .ok_or_else(|| "anchored, but no second signature for the proof".to_string())
                                .and_then(|s2| prove.signed(&s2))
                                .and_then(|tx2| c.submit(&tx2));
                            if let Err(e) = sent {
                                let _ = req.respond(json(serde_json::json!({
                                    "anchored": true, "proven": false,
                                    "error": format!("anchored, but the proof did not land: {e}"),
                                })));
                                continue;
                            }
                            // The proof is on chain. Pay the author, off this thread.
                            //
                            // Spawned here and returns nothing. The reply below is built from
                            // `c.anchored` — from the chain — and not from anything this does,
                            // which is what keeps a learner's proof theirs whatever happens to
                            // our wallet. (An earlier version of this comment said the payout
                            // ran *after* the response was built. It does not: it is started a
                            // dozen lines before. The guarantee is the spawn and the ignored
                            // return, not the ordering, and a reader who moved code trusting the
                            // old wording would have been surprised.)
                            settle(&payer, &settled, &authors_root, &work.case, &work.leaf);
                        }
                        let mut map = sessions.lock().unwrap();
                        if let Some(s) = map.get_mut(&work.session) {
                            s.anchored = true;
                            persist(&store, &work.session, s, true);
                        }
                        drop(map);
                        // Read the id out and let go of the lock. A `tree.lock()` written inside
                        // the match scrutinee stays alive for the whole match, so taking it again
                        // in an arm deadlocks the one thread this server has — which is not a slow
                        // request, it is every request from then on.
                        let tree_id = tree.lock().unwrap().tree_id;
                        match c.anchored(&id, tree_id, work.index) {
                            Ok(a) => json(serde_json::json!({
                                "index": a.index, "root": a.root, "leaves": a.leaves,
                                "proven": a.proven, "score": work.score,
                                "det": work.det.map(|(s, m)| serde_json::json!({"score": s, "max": m})),
                                "counted": c.proven_count(&id, tree_id),
                            })),
                            Err(e) => json(serde_json::json!({ "error": e })),
                        }
                    }
                    (Err(e), Some(_)) => json(serde_json::json!({ "granted": false, "message": e })),
                    (Err(e), None) => {
                        // Nothing landed on chain, so the leaf is not in the tree — ours, that
                        // is. The player spent seconds at a wallet prompt before this request,
                        // and another anchor can have been served whole inside that gap.
                        unwind_leaf(&mut tree.lock().unwrap().leaves, work.index);
                        json(serde_json::json!({ "error": e }))
                    }
                }
            }
            _ => Response::from_string("not found").with_status_code(404),
        };
        let _ = req.respond(resp);
    }
}

/// What the browser is waiting to sign, and what to do once it has.
struct PendingWork {
    pending: chain::Pending,
    /// The proof transaction that follows a successful anchor. Two transactions because the pair
    /// stopped fitting in one packet when the record grew — see `prepare_anchor`. The player
    /// signs both messages together; the server submits them in order.
    prove: Option<chain::Pending>,
    /// Which run this anchors. Empty for a claim.
    session: String,
    /// Set when this transaction is a pre-run commitment: (hash, nonce, mode). On success the
    /// slot is read back from the account — the program assigned it, so only the chain knows
    /// it — and everything lands in the session for the record to use at anchor time. The mode
    /// rides here so the session's exam flag is written only when the commitment actually lands.
    commit: Option<([u8; 32], [u8; 32], u8)>,
    /// Whose record this lands on — not necessarily the key that signs it.
    account: solana_sdk::pubkey::Pubkey,
    index: u64,
    /// The leaf and the case behind it, carried so the payout does not have to re-derive either
    /// from a tree that has moved on by the time the proof confirms.
    leaf: String,
    case: String,
    score: u32,
    /// The deterministic exam mark stamped into the record at anchor time — carried here only
    /// so the submit reply can show it; `None` for practice runs and for non-anchor work.
    det: Option<(u16, u16)>,
    /// Set for a claim, `None` for an anchor.
    level: Option<u8>,
    /// True when this transaction only moves devices around and touches no tree.
    link: bool,
}

/// The apex is the company's front door and nothing else (decided 2026-08-25): the game, its
/// APIs and its session state live on the devnet host. One instance serves both names, so the
/// split is the Host header — an allowlist of exactly one special name, with every other name
/// (devnet, run.app, localhost) keeping the full app unchanged.
/// What the ventilator pane says the peak-to-plateau gap means, held here rather than there.
///
/// `static/device/vent.html` is served whole to anyone who asks for it, so every string in it is
/// readable in view-source whether or not the pane is willing to render it. These two sentences
/// were in that file, behind `const EXAM = P.get('exam') === '1'` — a gate the reader holds. A
/// candidate did not have to attack it: dropping `&exam=1` off the iframe URL was enough, and
/// reading the source was enough even without that.
///
/// So the pane holds no interpretation at all now. It is handed these two lines, or it is handed
/// nothing and prints the number it measured. Under [`Session::sealed`] it is handed nothing, and
/// the key is absent from the reply rather than empty — there is no field to notice, no branch to
/// flip, and nothing in the bytes to read.
///
/// The gap itself is not secret and is not withheld: an exam does not hide the instrument, and a
/// real ventilator displays both pressures. What is withheld is the sentence that reads them,
/// because reading them is the mark.
const VENT_READ_WIDE: &str = "Ppeak high but <b>Pplat normal</b> → airway resistance, \
     not stiff lungs — think bronchospasm or a blocked tube";
/// The other half of [`VENT_READ_WIDE`] — the same rule, the reassuring branch.
const VENT_READ_NARROW: &str = "Ppeak and Pplat are close — airway resistance is not the problem";

const APEX: &str = "vitals.academy";
const GAME_ORIGIN: &str = "https://devnet.vitals.academy";

fn host_of(req: &tiny_http::Request) -> String {
    req.headers()
        .iter()
        .find(|h| h.field.equiv("host"))
        .map(|h| h.value.as_str().trim().to_ascii_lowercase())
        .and_then(|h| h.split(':').next().map(str::to_string))
        .unwrap_or_default()
}

/// What the apex does with a URL: `None` serves the landing, otherwise the permanent home of
/// that path on the game origin — a deep link pasted against the apex still lands somewhere
/// real, query string and all. Decided on the path alone so `/?utm_source=...` stays a landing.
/// The page the apex serves for a path [`apex_target`] keeps: the landing, or one of the two
/// documents about the company, stamped with the build they describe.
///
/// One function so the two hosts cannot drift: the game origin serves the same bytes from its own
/// match arms, and a policy that differed between `vitals.academy/privacy` and
/// `devnet.vitals.academy/privacy` would be two policies.
fn front_door(path: &str) -> String {
    match path {
        "/privacy" => PRIVACY.replace(BUILD_STAMP, BUILD),
        "/terms" => TERMS.replace(BUILD_STAMP, BUILD),
        _ => LANDING.to_string(),
    }
}

fn apex_target(url: &str) -> Option<String> {
    let path = url.split('?').next().unwrap_or("/");
    // The landing and the two documents about the company stay on the front door. Everything
    // else moves to the game origin.
    //
    // A redirect would work for a reader and is wrong for the two callers that matter: Google's
    // OAuth consent screen wants a privacy-policy URL it can fetch, and a link mailed to a
    // reviewer or printed in a footer is quoted at the name it was written as. `vitals.academy/privacy`
    // answering with the policy — rather than with a 301 to a host called `devnet` — is the
    // difference between a URL that reads as the company's and one that reads as an artefact of
    // our hosting.
    (!matches!(path, "/" | "/privacy" | "/terms")).then(|| format!("{GAME_ORIGIN}{url}"))
}

/// Where to listen, with the platform's word winning over ours.
///
/// Cloud Run assigns `PORT` and health-probes exactly that port; a container that binds anything
/// else boots perfectly and never becomes healthy — which is precisely how the first public
/// deploy failed, the app on its own 8474 while the probe watched 8080. So `PORT`, when given,
/// is not a preference to weigh against `VITALS_WEB_BIND`; it is the address the platform will
/// judge us by. Everywhere else `VITALS_WEB_BIND` decides, and the default stays loopback so a
/// laptop never opens a public port by accident.
fn bind_addr(platform_port: Option<&str>, configured: Option<&str>) -> String {
    match platform_port {
        Some(p) if !p.is_empty() => format!("0.0.0.0:{p}"),
        _ => configured.map(str::to_string).unwrap_or_else(|| "127.0.0.1:8474".into()),
    }
}

/// The cluster an RPC url points at, for the label on screen. Substring matching is enough:
/// the public endpoints all carry their cluster's name, and anything unrecognised is reported
/// as what it is rather than guessed.
fn cluster_of(rpc: &str) -> &'static str {
    fuel::cluster_of(rpc)
}

/// A player key as the browser sends it: base58, and it has to be a real curve point or the
/// transaction it is put into can never be signed.
fn pubkey(s: &str) -> Option<solana_sdk::pubkey::Pubkey> {
    let raw = bs58_to_32(s)?;
    let k = solana_sdk::pubkey::Pubkey::new_from_array(raw);
    (k.to_string() == s).then_some(k)
}

fn hex_bytes(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// A 32-byte hash as hex, the other way round.
fn hex32(h: &str) -> Option<[u8; 32]> {
    if h.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(h.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// A 64-byte signature as hex.
fn sig64(h: &str) -> Option<[u8; 64]> {
    if h.len() != 128 {
        return None;
    }
    let mut out = [0u8; 64];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(h.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// Decode a base58 pubkey into raw bytes.
fn bs58_to_32(s: &str) -> Option<[u8; 32]> {
    const A: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let mut out = vec![0u8; 32];
    for ch in s.bytes() {
        let mut carry = A.iter().position(|&c| c == ch)?;
        for b in out.iter_mut().rev() {
            carry += 58 * (*b as usize);
            *b = (carry & 0xff) as u8;
            carry >>= 8;
        }
        if carry != 0 {
            return None;
        }
    }
    out.try_into().ok()
}

/// The phrase a device pick sends through the intervention matcher.
///
/// One catalogue, and it carries the number: a learner who sets the flowmeter to 15 should see
/// 15 in the chart, not the scenario's canonical dose.
fn kit_phrase(dev: &str, set: Option<f64>) -> Option<String> {
    Some(match dev {
        "o2" => format!("oxygen face mask {} lpm", set.unwrap_or(10.0) as i64),
        "iv" => format!("iv access normal saline {} ml/hr", set.unwrap_or(999.0) as i64),
        "ett" => "intubate, secure the airway".to_string(),
        "supine" => "lay her flat, legs up".to_string(),
        "defib" => format!("defibrillate {} j", set.unwrap_or(DEFIB_JOULES) as i64),
        _ => return None,
    })
}

// ── the defibrillator ───────────────────────────────────────────────────────
//
// The one order on the tray that is not an order. Everything else in `kit_phrase` is a sentence
// the *scenario* rules on: the case says what oxygen does to this patient, and a case that never
// mentions oxygen is entitled to ignore it. Shockability is not like that. Whether a rhythm can
// be shocked is physiology — `Rhythm::shockable` — and routing it through each case's own
// `interventions` list is how the shelf ended up where it was: `ep2` branched on the *name of a
// state*, and `ep3`, `ep4` and `ep5` declared no `defibrillate` at all, so pressing the button on
// a five-year-old in cardiac arrest charted nothing, scored nothing and appeared in no debrief.
//
// So the button and the typed order both end at `SceState::defibrillate`, and both write the
// same `Step::Shock` on the tape. What is deliberately *not* changed is who answers first: the
// scenario's matcher still rules on the text, and only when it declines does the engine's own route open.
// A station that defines its own shock intervention keeps it, unchanged, and every rubric in the
// repo scores exactly as it did.

/// What the defibrillator delivers when nobody said. The page's own default preset.
const DEFIB_JOULES: f64 = 200.0;

/// Words that mean "deliver a shock", and the ones that mean the *other* kind of shock.
///
/// Shaped like a scenario `Matcher` because that is the shape this repo already argues about:
/// `any_kw` plus `not_kw`. The exclusions are the whole reason it is not a bare substring test —
/// "cardiogenic shock", "septic shock", "she is in shock" and "shock index" are all things a
/// candidate types on a station where nobody wants 200 joules delivered, and on `ep5`, whose
/// patient is exsanguinating, they are things a candidate types *often*.
///
/// The keywords are `ep2`'s own, taken off the intervention this replaced, so a learner who
/// reached the old one reaches this.
const SHOCK_KW: [&str; 5] = ["defib", "shock", "cardiovert", "joule", "200j"];
const NOT_SHOCK_KW: [&str; 11] = [
    "cardiogenic", "septic", "hypovol", "haemorrhagic", "hemorrhagic", "distributive",
    "neurogenic", "spinal shock", "obstructive shock", "shock index", "in shock",
];

/// Does this text name a defibrillator?
fn names_a_shock(text: &str) -> bool {
    let t = vitals_sce::text::canon(text).to_lowercase();
    SHOCK_KW.iter().any(|k| t.contains(k)) && !NOT_SHOCK_KW.iter().any(|k| t.contains(k))
}

/// The energy an order asks for, in joules, if it names one a defibrillator could deliver.
///
/// Read off the text the learner actually typed rather than off the language layer's canonical
/// English, so "ช็อกไฟฟ้า 360" keeps its 360 — `lang::canonical_order` answers *whether* a phrase
/// is a shock, and would flatten every one of them to the same headword.
fn joules_in(text: &str) -> Option<f64> {
    let t = vitals_sce::text::canon(text);
    let mut best = None;
    let mut n = String::new();
    for c in t.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_digit() {
            n.push(c);
            continue;
        }
        if !n.is_empty() {
            // A defibrillator's dial: nothing outside this is an energy, so a year, a bed
            // number or a saturation cannot be read as one.
            if let Ok(v) = n.parse::<f64>() {
                if (1.0..=1000.0).contains(&v) && best.is_none() {
                    best = Some(v);
                }
            }
            n.clear();
        }
    }
    best
}

/// The shock an order asks for, if it is one — through the language layer, like every order.
fn shock_order(act: &str) -> Option<f64> {
    let named = names_a_shock(act)
        || lang::canonical_order(act).is_some_and(names_a_shock);
    named.then(|| joules_in(act).unwrap_or(DEFIB_JOULES))
}

/// Everything `/api/authors` reads from the chain, in one pass.
///
/// Two calls, both once: `Chain::proven` (one `get_program_accounts`) and `Payer::ledger` (one
/// signature scan plus a lookup per memo not seen before). Called only when the cache is stale,
/// so the endpoint costs at most this once a minute however hard it is asked for.
///
/// Returns `None` when there is no chain at all — an absent answer rather than an empty one, so
/// the endpoint can say `counted_from_chain: false` instead of reporting zeroes as facts.
fn chain_view(
    chain: &Option<chain::Chain>,
    payer: &Option<Arc<payout::Payer>>,
    tree_id: u64,
) -> Option<ChainView> {
    let proven = chain.as_ref()?.proven(tree_id).ok()?;
    let mut view = ChainView { per_case: proven.per_case, ..Default::default() };
    // Payments are optional: payouts may be off, or the wallet unreadable. Neither makes the
    // proven counts less true, so the view is returned either way.
    if let Some(led) = payer.as_ref().and_then(|p| p.ledger().ok()) {
        for (leaf, lamports) in &led.paid_amounts {
            if let Some(case) = proven.case_of_leaf.get(leaf) {
                let e = view.paid_by_case.entry(case.clone()).or_default();
                e.paid += 1;
                e.paid_lamports += lamports;
            }
            if let Some(sig) = led.paid_signatures.get(leaf) {
                view.paid_leaves.insert(leaf.clone(), (*lamports, sig.clone()));
            }
        }
    }
    Some(view)
}

/// Pay a case's author for a proof that has just landed, on a thread of its own.
///
/// **Nothing this does can change what the learner is told.** The proof is theirs the moment the
/// chain accepted it; an empty wallet or a reached cap is ours to have quietly. This is spawned
/// and its return is ignored, and the reply the caller gets is built from the chain rather than
/// from anything here — that, not the order the two happen in, is the guarantee.
///
/// The decision reads a fresh ledger from the chain every time rather than a cached one. The
/// whole point of reading memos is that they are true across restarts and instances, and a cache
/// consulted here would be a second opinion about money.
fn settle(
    payer: &Option<Arc<payout::Payer>>,
    settled: &Settled,
    root: &std::path::Path,
    case: &str,
    leaf: &str,
) {
    let Some(payer) = payer.clone() else { return };
    let settled = settled.clone();
    if case.is_empty() || leaf.is_empty() {
        return;
    }
    let (case, leaf) = (case.to_string(), leaf.to_string());
    let root = root.to_path_buf();
    std::thread::spawn(move || {
        // Who the side table says wrote it. No entry is not an error and never a guess.
        // Who the side table says wrote it — **and whether that entry's signature holds**.
        //
        // `authors::load` parses; it does not verify. Verification runs in `audit`, which is the
        // check over the shipped file, in a test. This is the one place lamports actually leave,
        // so the signature is checked here too: the allowlist bounds who can be paid, and this
        // bounds whether the entry naming them was really signed by them. It costs microseconds.
        let entry = authors::load(&root.join(authors::AUTHORS_PATH))
            .unwrap_or_default()
            .into_iter()
            .find(|a| a.sce_hash == case);
        let author = match entry {
            Some(a) => match a.verify() {
                Ok(()) => Some(a.author),
                Err(why) => {
                    eprintln!(
                        "payout: leaf {leaf} not paid — attribution signature does not verify: {why}"
                    );
                    return;
                }
            },
            None => None,
        };

        let led = match payer.ledger() {
            Ok(l) => l,
            Err(e) => {
                eprintln!("payout: not paying for leaf {leaf} — {e}");
                return;
            }
        };
        let ask = payout::Ask {
            rate: payer.rate,
            platform_bps: payer.platform_bps,
            allowlist: &payer.allowlist,
            author: author.as_deref(),
            leaf: &leaf,
            paid: &led.paid,
            spent_today: led.spent_today,
            daily_cap: payer.daily_cap,
            balance: payer.balance(),
        };
        match payout::decide(&ask) {
            payout::Verdict::Skip(why) => eprintln!("payout: leaf {leaf} not paid — {why}"),
            payout::Verdict::Pay(split) => {
                let Some(author) = author else { return };
                match payer.pay(&leaf, &author, split) {
                    Ok(paid) => {
                        // Ours to report, because we just made it. Not a decision — `pay` has
                        // already happened — only a record so the debrief does not have to ask
                        // the chain what this process did a second ago.
                        if let Ok(mut done) = settled.lock() {
                            done.insert(leaf.clone(), (paid.author_lamports, paid.signature.clone()));
                        }
                        eprintln!(
                            "payout: {} lamports to {} for leaf {leaf} — {}",
                            paid.author_lamports, paid.author, paid.signature
                        );
                    }
                    Err(e) => eprintln!("payout: leaf {leaf} — {e}"),
                }
            }
        }
    });
}

/// One pass of the ward — the ticker's and the scheduler's, the same function.
///
/// `None`: the gate is held, a pass is already running; the caller does not wait. `Some(Err)`: no
/// chain to read. `Some(Ok)`: the pass, with its notes already printed here so both callers log
/// identically. Defined after the refill thread on purpose — `tests/boot.rs` reads the straight-line
/// boot as everything before that thread and must not find the sweep or the tick on it — and
/// before the test module, because clippy refuses items after one.
fn one_pass(store: &store::Store, root: &std::path::Path) -> Option<Result<ward_chain::Ticked, String>> {
    let _gate = rebuild::take(&TICKING)?;
    let chain = match ward_chain::WardChain::connect() {
        Ok(c) => c,
        Err(e) => return Some(Err(e)),
    };
    // What this server still holds, for the repair: every stored run's scenario and tape. A leaf
    // the chain names whose tape is lost is recoverable only from here, and only while these are
    // still on disk.
    let held: Vec<(String, Vec<Step>)> = store
        .list::<Saved>(SESSIONS)
        .into_iter()
        .filter_map(|(_, sv)| {
            let p = scenario_path(&sv.ep);
            std::fs::read_to_string(p).ok().map(|sce| (sce, sv.tape))
        })
        .collect();
    let began = Instant::now();
    let t = ward_chain::tick(&chain, store, root, now_secs(), &held);
    for note in &t.notes {
        println!("ward       {note}");
    }
    // **Repair first, then sweep, and never the other way.** `tick` repairs at its top; the sweep
    // runs only after it has returned. The repair recovers a lost tape *from* the stored runs and
    // the sweep deletes stored runs older than a day — so sweeping first can delete the only copy
    // of a tape for a leaf already on chain, which is the thing the repair exists to put back.
    // Boot did them in that wrong order until 20 ก.ย. Once per process, whichever caller got here.
    if !SWEPT.swap(true, std::sync::atomic::Ordering::SeqCst) {
        let swept = store.sweep(SESSIONS, std::time::Duration::from_secs(24 * 60 * 60));
        println!(
            "ward       first pass · {} of {} patients needed a signature listing · swept {swept} \
             expired, after the repair and never before it",
            t.listed, t.checked
        );
    }
    // **A pass that changed the ward re-keeps the board.**
    //
    // `keep_board` is called from one place — `read_ward` — and `tick` does not call it, so until
    // now a pass admitted a patient on chain and left the board in the store exactly as it found
    // it. A fresh instance then served a census taken before the arrival, which at min-instances 0
    // is the next visitor: the first person through the door after a patient arrives would not see
    // that patient. For a ward whose whole point is that the census keeps moving, an arrival
    // nobody can see is an arrival that did not happen.
    //
    // Only when something changed. `read_ward` is the expensive call this ticker was rebuilt to
    // avoid — 100 seconds to about four — and paying it once per actual arrival or death is a
    // trade worth making, while paying it every minute for a pass that moved nothing is not.
    if !t.admitted.is_empty() || !t.closed.is_empty() {
        let kept = ward_chain::read_ward(&chain, store);
        println!(
            "ward       the board was re-kept after a pass that admitted {} and closed {} — \
             a census a fresh instance can read without waiting for a visitor{}",
            t.admitted.len(),
            t.closed.len(),
            if kept["readable"] == serde_json::json!(true) { "" } else { " (unreadable board)" }
        );
    }
    // Any pass that took more than ten seconds, not only the first; the sentence is `ward_chain`'s
    // so the shape can be tested without a deploy.
    if let Some(note) = ward_chain::slow_pass_note(began.elapsed(), t.pace, &t.spans) {
        println!("ward       {note}");
    }
    Some(Ok(t))
}

#[cfg(test)]
mod tests {
    /// The page as a browser gets it: the markup, the play surface composed into it, and the
    /// script.
    ///
    /// It was one file until 16 ก.ย., when the bay's surface, stylesheet and script were pulled
    /// out so the ward's shift page could share them and carry none of the season. These tests ask
    /// what a *player* is shown, so they ask the composition rather than the shell.
    fn served() -> String {
        [PAGE, SURFACE, BAY_JS].concat()
    }

    use super::*;

    // ── the face at the bedside ─────────────────────────────────────────────

    /// **A shared link carries the patient, not the product.**
    ///
    /// UX review G5. `/ward/<id>` pasted into LINE, Slack or a tweet unfurls as "A shift on the
    /// ward — Vitals World" with no picture: a card about a website, when what was shared was a
    /// person somebody wants treated. The card should say who she is, where she is from, and what
    /// is happening to her, with her own face on it.
    ///
    /// Built from the board — the same row the panel draws — so a card cannot say something the
    /// ward does not. A patient the board has nothing about gets no tags at all rather than a card
    /// about a stranger.
    #[test]
    fn a_shared_bed_unfurls_as_the_patient() {
        let her = serde_json::json!({
            "patient_id": 11, "name": "Nusrat Jahan", "age": 64, "country": "BGD",
            "state": "on_ward", "bed": 1, "difficulty": "intern",
            "portrait": "https://storage.googleapis.com/vitals-world-portraits/aa.webp"
        });
        let tags = og_tags(&her);
        assert!(tags.contains("og:title\" content=\"Nusrat Jahan · 64 · Bangladesh"), "{tags}");
        assert!(tags.contains("og:image\" content=\"https://storage.googleapis.com/vitals-world-portraits/aa.webp"),
                "her own face, and an absolute URL because a card is fetched by somebody else: {tags}");
        assert!(tags.contains("Nobody is with her"), "what is happening to her: {tags}");
        assert!(tags.contains("summary_large_image"), "the face is the point of the card: {tags}");

        // On shift, and after the stay: the same card, a different sentence.
        let busy = serde_json::json!({ "name": "Park Ji-woo", "age": 8, "country": "KOR",
                                       "state": "on_shift" });
        assert!(og_tags(&busy).contains("Somebody is treating"), "{}", og_tags(&busy));
        let gone = serde_json::json!({ "name": "Lee Seo-yeon", "age": 54, "country": "KOR",
                                       "state": "died" });
        assert!(og_tags(&gone).contains("stay has ended"), "{}", og_tags(&gone));

        // The page has somewhere to put them, and a composed page that never met a patient keeps
        // the marker empty rather than carrying somebody else's card.
        assert!(SHIFT.contains("<!--OG-->"), "the shift page has no slot for these any more");
        assert!(!compose(SHIFT).replace("<!--OG-->", "").contains("og:title"),
                "a page composed for nobody in particular claims nothing about anybody");

        // Nothing known, nothing claimed.
        assert_eq!(og_tags(&serde_json::Value::Null), "");
        assert_eq!(og_tags(&serde_json::json!({ "state": "on_ward" })), "",
                   "a row with no name is not a person to put on a card");
    }

    /// **A refusal page offers the beds, as links a thumb can hit.**
    ///
    /// The rows themselves are `ward::beds_to_offer`'s and tested there. What is here is the
    /// markup: one link per bed, addressed by patient id, carrying the bed number first because
    /// that is how the ward is arranged, and the country's name rather than its code — a stranger
    /// who mistyped an id is not owed "PAK".
    #[test]
    fn a_refusal_page_offers_the_open_beds_as_links() {
        let board = serde_json::json!({
            "patients": [
                { "patient_id": 77, "state": "on_ward", "bed": 2, "name": "Ayesha Malik",
                  "age": 57, "country": "PAK", "difficulty": "intern" },
                { "patient_id": 11, "state": "on_ward", "bed": 1, "name": "Nusrat Jahan",
                  "age": 64, "country": "BGD", "difficulty": "resident" },
                { "patient_id": 22, "state": "on_shift", "bed": 3, "name": "Park Ji-woo", "age": 8 }
            ]
        });
        let html = beds_on_offer(&board);
        assert!(html.contains("beds you can take now"), "{html}");
        assert!(html.contains("href=\"/ward/11\">bed 1 — Nusrat Jahan · 64 · Bangladesh · resident"),
                "bed first, then who she is, then where she is from in words: {html}");
        assert!(html.contains("href=\"/ward/77\">bed 2 — Ayesha Malik · 57 · Pakistan · intern"), "{html}");
        assert!(!html.contains("Park Ji-woo"),
                "somebody is in the room with her, so she is not a bed on offer: {html}");
        assert!(html.find("bed 1").unwrap() < html.find("bed 2").unwrap(), "in bed order");

        // A board nobody could read prints nothing — not an empty heading over nothing.
        assert_eq!(beds_on_offer(&serde_json::json!({ "readable": false })), "");
        assert_eq!(beds_on_offer(&serde_json::Value::Null), "");
    }

    /// **"ไม่มีรูปผู้ป่วยหรอ"** — the founder, looking at Park Ji-woo on staging, 16 ก.ย.
    ///
    /// The shift page showed no face. The pack had one; `/api/new` even carried it; nothing drew
    /// it, and nothing would have changed it if it had, because the payload carried her base
    /// picture and no other.
    ///
    /// The rule: the face at the bedside is the face for the state she is in **now**, chosen by
    /// the server's own ladder and never by a copy of it in the page — so every view a ward
    /// session paints carries the URL to draw, and the page draws whatever arrives. The Eternal
    /// bay carries no such field: its stills are the season's, chosen by the Director, and a
    /// portrait from a ward pack has no business on them.
    #[test]
    fn a_ward_view_carries_the_face_for_the_state_she_is_in() {
        let base = "https://storage.googleapis.com/vitals-world-portraits/";
        let faces: std::collections::BTreeMap<String, String> = [
            ("stable", "a"), ("deteriorating", "b"), ("critical", "c"),
        ]
        .iter()
        .map(|(k, n)| (k.to_string(), format!("{base}{}.webp", n.repeat(64))))
        .collect();

        // ep5 rather than Ji-woo's own case: the picture has to be shown moving, and osce-c is one
        // of the two in the catalogue that never leaves stable untended (the mortality table in
        // CWF_PLAN.md). The faces below are synthetic either way.
        let mut s = new_session("ep5").expect("ep5 is in the repository");
        s.ward = Some(WardShift {
            patient_id: 1789528326,
            index: 0,
            taken_slot: 1,
            head: "00".repeat(32),
            faces: faces.clone(),
            age: 8,
        });

        // The engine spells its states `Stable`; the ladder's keys are the same words in lower
        // case, which is the one place the two vocabularies meet — and the reason the view binds
        // the word once rather than formatting it twice.
        let seen = |s: &Session| {
            let v = s.view(lang::language(None));
            (v.status.to_lowercase(), v.portrait.clone())
        };
        let (status, portrait) = seen(&s);
        assert_eq!(portrait.as_deref(), ward::portrait_for(&faces, &status),
                   "the face is the server's own choice for the state it is publishing beside it");
        assert!(portrait.is_some(), "she has a picture for {status}, so one must be sent");

        // Move her. The picture moves with her, because both come from the same word.
        let before = seen(&s);
        for _ in 0..600 {
            s.state.tick(1.0);
            if seen(&s).0 != before.0 {
                break;
            }
        }
        let after = seen(&s);
        assert_ne!(after.0, before.0, "ep5 left alone must reach another state, or this proves nothing");
        assert_eq!(after.1.as_deref(), ward::portrait_for(&faces, &after.0),
                   "and the face follows the state rather than staying at the one she arrived in");

        // The season's bay is untouched: no field, so nothing to draw.
        let eternal = new_session("ep1").expect("ep1");
        assert!(eternal.view(lang::language(None)).portrait.is_none(),
                "the Eternal entry's stills are the Director's and a ward pack has no say in them");
    }

    /// **"No picture has been made for her" and "this run has no such thing as a picture" are
    /// different sentences, and the wire says only one of them.**
    ///
    /// `View.portrait` is `skip_serializing_if = "Option::is_none"`, which was written for the
    /// season: the Eternal entry's stills are the Director's, a ward pack has no say in them, and
    /// the honest thing is to send no field at all. Then the ward borrowed the same absence. A
    /// patient nobody has photographed yet resolves to `None` through the ladder, the field is
    /// omitted, and the page receives exactly what an Eternal run sends.
    ///
    /// The page's guard is `if (v.portrait !== undefined)`, so an omitted field skips the whole
    /// block — including the line that draws the stand-in. That is why Somsak Faceless has no
    /// `.face-none` element on his bedside at all rather than a visible one: b9b72af promised an
    /// outline with her initials wherever there is no photograph, and it has only ever been
    /// delivered when the server had something to say about the photograph.
    ///
    /// This is the blank-frame bug one layer down, and it is the same lesson: an absence says
    /// nothing, so it is read as whatever the reader already assumes. On 22 ก.ย. the reader was
    /// the founder. Here it is my own code.
    ///
    /// So the two absences are made to look different on the wire, and the test is about the wire
    /// rather than the type — a browser sees JSON, and `Option::is_none` on the Rust side is not
    /// the fact that matters.
    #[test]
    fn a_ward_patient_with_no_picture_says_so_and_the_season_says_nothing_at_all() {
        let mut s = new_session("ep5").expect("ep5 is in the repository");
        s.ward = Some(WardShift {
            patient_id: 1790144220,
            index: 0,
            taken_slot: 1,
            head: "00".repeat(32),
            // Nobody has photographed him. Not a missing pack, not a closed door — a patient on
            // the ward whose face has not been made yet, which is the ordinary state of a patient
            // the factory has only just admitted.
            faces: std::collections::BTreeMap::new(),
            age: 62,
        });

        let on_the_wire = serde_json::to_value(s.view(lang::language(None)))
            .expect("the view a browser is sent");
        assert!(
            on_the_wire.get("portrait").is_some(),
            "a ward bedside says something about the picture even when there is none, or the \
             page cannot tell 'no photograph' from 'this is not a ward': {on_the_wire:#}"
        );
        assert!(
            on_the_wire["portrait"].is_null(),
            "and what it says is null — there is a bedside, and it has no face to draw"
        );

        // The season is untouched, and this half is why the skip exists at all. Nothing about the
        // Eternal entry changed when the ward learned to say "none".
        let eternal = serde_json::to_value(
            new_session("ep1").expect("ep1").view(lang::language(None)),
        )
        .expect("the season's view");
        assert!(
            eternal.get("portrait").is_none(),
            "the Eternal entry sends no such field: its stills are the Director's and a ward \
             pack has no say in them"
        );
    }

    /// **The top-left is the way out** (founder, 16 ก.ย.: a logo, and pressing it leaves for the
    /// globe). On the ward host that is the Vitals World mark and a link to `/`; on vitals.academy
    /// the bar keeps the Eternal wordmark exactly as it is.
    ///
    /// The brand is the one element of the shared surface that differs by host, so the server
    /// composes it rather than the script toggling a class: the page a visitor gets is already
    /// right, with nothing to re-render and nothing to get wrong on a slow script.
    #[test]
    fn the_ward_host_wears_its_own_mark_and_the_season_keeps_its_wordmark() {
        let ward = compose_for(SHIFT, true);
        assert!(ward.contains("Vitals World</a>"),
                "the ward's bar says which world this is: {}", &ward[..0]);
        // Two now, and deliberately: the page's own corner, which is what the founder asked for,
        // and the bay's bar a row below it. One drawing and one link — what must not happen is two
        // different marks, or two different destinations.
        let marks: Vec<&str> = ward.match_indices("class=\"brand\"").map(|(i, _)| &ward[i..]).collect();
        assert_eq!(marks.len(), 2, "the corner and the bar");
        let brand = marks[0].split("</a>").next().expect("closed");
        assert_eq!(brand, marks[1].split("</a>").next().expect("closed"),
                   "the two marks on the page must be the same mark, pointing at the same door");
        assert!(brand.contains("<svg"), "and it carries the mark, not only the words: {brand}");
        assert!(ward.contains("href=\"/\" class=\"brand\"") || brand.starts_with(" href=\"/\"")
                || ward.contains("<a href=\"/\" class=\"brand\""),
                "pressing it leaves the ward for the globe");
        assert!(!ward.contains("Vital<span>s</span>"),
                "and the Eternal wordmark is not on the ward host");

        let eternal = compose_for(PAGE, false);
        assert!(eternal.contains("Vital<span>s</span>"),
                "vitals.academy keeps the wordmark a judge has in a tab");
        assert!(!eternal.contains("Vitals World"),
                "and the ward's name is not on it");
    }

    /// Two exits, saying the same thing.
    #[test]
    fn the_strip_offers_the_globe_as_well() {
        assert!(BAY_JS.contains("← the globe"),
                "the strip's own way back is the globe, in the same words as the mark");
    }

    // ── unwinding a leaf that was never anchored ────────────────────────────

    fn leaf_n(n: u8) -> [u8; 32] {
        let mut l = [0u8; 32];
        l[0] = n;
        l
    }

    #[test]
    fn unwinding_takes_back_our_own_leaf() {
        let mut leaves = vec![leaf_n(1), leaf_n(2), leaf_n(3)];
        assert!(unwind_leaf(&mut leaves, 2));
        assert_eq!(leaves, vec![leaf_n(1), leaf_n(2)]);
    }

    /// The one that matters. Two players anchor seconds apart; the first refuses the wallet
    /// prompt after the second has already landed on chain. The unwind must not reach past its
    /// own push — the leaf on the end is somebody's anchored run, and nothing puts it back.
    #[test]
    fn unwinding_never_takes_back_a_leaf_somebody_else_anchored() {
        let ours = 1u64;
        let mut leaves = vec![leaf_n(1), leaf_n(2)];
        leaves.push(leaf_n(3)); // the other player's anchor, submitted and on chain
        assert!(!unwind_leaf(&mut leaves, ours));
        assert_eq!(leaves, vec![leaf_n(1), leaf_n(2), leaf_n(3)]);
    }

    /// Two failed unwinds in a row must not walk the list down. The second one has already lost
    /// the end, so it does nothing — the guard is on our index, not on a "did I pop yet" flag.
    #[test]
    fn a_second_unwind_of_the_same_work_is_inert() {
        let mut leaves = vec![leaf_n(1), leaf_n(2), leaf_n(3)];
        assert!(unwind_leaf(&mut leaves, 2));
        assert!(!unwind_leaf(&mut leaves, 2));
        assert_eq!(leaves, vec![leaf_n(1), leaf_n(2)]);
    }

    #[test]
    fn unwinding_an_empty_list_does_nothing() {
        let mut leaves: Vec<[u8; 32]> = vec![];
        assert!(!unwind_leaf(&mut leaves, 0));
        assert!(leaves.is_empty());
    }

    // ── things that arrive from the network ─────────────────────────────────
    //
    // Every one of these turns an attacker-controlled string into something the rest of the
    // server trusts — a public key, a signature, a query value. They had no tests at all.

    // ── the local leaf list against the chain's tree ────────────────────────

    /// Agreement is the ordinary case and must change nothing.
    #[test]
    fn lists_that_agree_are_left_alone() {
        let mut leaves = vec![leaf_n(1), leaf_n(2), leaf_n(3)];
        assert_eq!(reconcile_leaves(&mut leaves, 3), Reconciled::Ready { dropped: 0 });
        assert_eq!(leaves.len(), 3);
    }

    /// Longer than the chain: our own litter, and safe to sweep.
    ///
    /// The extra leaves were prepared and never submitted, so they are on no chain and no proof
    /// anywhere refers to them. Dropping them is what puts the index back in step.
    #[test]
    fn leaves_the_chain_never_saw_are_dropped() {
        let mut leaves = vec![leaf_n(1), leaf_n(2), leaf_n(3), leaf_n(4)];
        assert_eq!(reconcile_leaves(&mut leaves, 2), Reconciled::Ready { dropped: 2 });
        assert_eq!(leaves, vec![leaf_n(1), leaf_n(2)], "it dropped from the wrong end");
    }

    /// Shorter than the chain: somebody else's evidence, and not ours to paper over.
    ///
    /// A missing leaf is one that IS anchored, that somebody holds a proof of, and that we
    /// cannot regenerate. Anchoring past it would build a tree abandoning their record, so the
    /// answer is to stop — and to leave the list exactly as it is, because a half-repair
    /// destroys the evidence of what went wrong.
    #[test]
    fn a_list_shorter_than_the_chain_refuses_and_changes_nothing() {
        let mut leaves = vec![leaf_n(1), leaf_n(2)];
        assert_eq!(reconcile_leaves(&mut leaves, 5), Reconciled::Short { local: 2, chain: 5 });
        assert_eq!(leaves, vec![leaf_n(1), leaf_n(2)], "it tried to repair itself");
    }

    /// A tree nothing has been anchored to is length zero, and a local list against it is still
    /// only ghosts.
    #[test]
    fn a_tree_with_nothing_on_it_still_reconciles() {
        let mut empty: Vec<[u8; 32]> = vec![];
        assert_eq!(reconcile_leaves(&mut empty, 0), Reconciled::Ready { dropped: 0 });

        let mut ghosts = vec![leaf_n(1), leaf_n(2)];
        assert_eq!(reconcile_leaves(&mut ghosts, 0), Reconciled::Ready { dropped: 2 });
        assert!(ghosts.is_empty());
    }

    // ── who the rate-limit window belongs to ────────────────────────────────

    /// The shapes that actually arrive, and the one that used to walk straight through.
    #[test]
    fn the_caller_is_read_from_the_end_of_the_forwarded_header() {
        // Cloud Run, honest client: one entry, and it is the visitor.
        assert_eq!(addr_from_xff("203.0.113.7", 1).as_deref(), Some("203.0.113.7"));
        // Cloud Run, client that wrote its own header. The invented value is on the left and
        // must be ignored; this is the shape measured against the live binary.
        assert_eq!(addr_from_xff("198.51.100.9, 203.0.113.7", 1).as_deref(), Some("203.0.113.7"));
        // A caller who writes a whole convincing chain still cannot reach past the end.
        assert_eq!(
            addr_from_xff("1.1.1.1, 2.2.2.2, 3.3.3.3, 203.0.113.7", 1).as_deref(),
            Some("203.0.113.7")
        );
        // Whitespace is the norm in this header, not an oddity.
        assert_eq!(addr_from_xff("198.51.100.9,   203.0.113.7  ", 1).as_deref(), Some("203.0.113.7"));
        // Behind an external load balancer: client, then the forwarding rule.
        assert_eq!(addr_from_xff("203.0.113.7, 34.34.34.34", 2).as_deref(), Some("203.0.113.7"));
    }

    /// Nothing usable means nothing, so the caller falls back to the socket peer rather than to
    /// a bucket every stranger shares by name.
    #[test]
    fn an_unusable_forwarded_header_yields_no_address_at_all() {
        assert_eq!(addr_from_xff("", 1), None);
        assert_eq!(addr_from_xff("   ", 1), None);
        assert_eq!(addr_from_xff("203.0.113.7", 2), None, "asking past the front of the list");
        assert_eq!(addr_from_xff("203.0.113.7, ", 1), None, "a trailing comma is not an address");
    }

    /// A window keyed on something the caller can change is not a window.
    ///
    /// Forty invented addresses used to buy forty windows. They now buy one, because the entry
    /// this reads is the one the caller cannot write.
    #[test]
    fn invented_addresses_all_land_in_one_window() {
        let seen: std::collections::HashSet<String> = (0..40)
            .map(|i| addr_from_xff(&format!("198.51.100.{i}, 203.0.113.7"), 1).expect("an address"))
            .collect();
        assert_eq!(seen.len(), 1, "the caller still controls the key: {seen:?}");
    }

    /// What this fix does **not** close, written down so it is a known limit and not a surprise.
    ///
    /// Reading from the right only helps if something to the right of the caller's text is there
    /// to read. If a request arrives carrying a lone invented address and the platform forwards
    /// it untouched, the last entry *is* the invented one and the window is still the caller's
    /// to choose. Against this binary, twenty such requests were accepted where the two-entry
    /// shape was cut to six.
    ///
    /// Whether that request can *arrive* is a fact about Cloud Run rather than about this
    /// code, and it has since been settled: it cannot. Measured on 2026-09-03 against revision
    /// `vitals-00045-66d` — Cloud Run appends what it observed, so a header the caller wrote is
    /// never the last entry, and five requests carrying five different invented values were all
    /// refused after the window was spent rather than each buying a fresh one.
    ///
    /// The assertion below is kept, and is about the parser rather than the platform: given
    /// that shape it does hand back twenty different keys. It stands as the description of what
    /// the guard rests on — if Cloud Run's behaviour ever changes, or this runs behind
    /// something that forwards the header untouched, this is the sentence that says what breaks
    /// and `VITALS_CLIENT_IP_FROM_END` is the lever.
    #[test]
    fn a_lone_invented_address_is_still_the_callers_to_choose() {
        let seen: std::collections::HashSet<String> = (0..20)
            .map(|i| addr_from_xff(&format!("198.51.100.{i}"), 1).expect("an address"))
            .collect();
        assert_eq!(
            seen.len(),
            20,
            "if this ever fails, the platform question was settled and this test should say how"
        );
    }

    /// The setting is a deployment fact, and a nonsense value must not disable the guard.
    #[test]
    fn the_position_setting_never_reads_past_the_end() {
        for bad in ["0", "-1", "", "two", "99999999999999999999"] {
            // SAFETY: single-threaded test, and the value is read back immediately.
            unsafe { std::env::set_var("VITALS_CLIENT_IP_FROM_END", bad) };
            assert!(client_ip_from_end() >= 1, "{bad:?} produced a position before the first entry");
        }
        unsafe { std::env::remove_var("VITALS_CLIENT_IP_FROM_END") };
        assert_eq!(client_ip_from_end(), 1, "the default stopped being the last entry");
    }

    #[test]
    fn a_pubkey_must_round_trip_exactly() {
        // A real one, and the check that makes it real: base58 has no canonical padding, so a
        // string that decodes to 32 bytes is not necessarily the string those bytes encode to.
        let real = "AStZxZ8XgH9nSKarLT4MzUrY8HM5LtExzDaN9SDoaKiq";
        assert_eq!(pubkey(real).map(|k| k.to_string()).as_deref(), Some(real));
    }

    /// Forty-three characters is a real address, not a truncated one. A pubkey is 32 bytes and
    /// base58 spends 43 or 44 characters on them depending on the leading byte — so a length
    /// check would reject roughly one address in 256, and only for the people who happen to hold
    /// one. The round-trip is the check; the length never was.
    #[test]
    fn a_forty_three_character_key_is_valid() {
        let short = "AStZxZ8XgH9nSKarLT4MzUrY8HM5LtExzDaN9SDoaKi";
        assert_eq!(short.len(), 43);
        assert_eq!(pubkey(short).map(|k| k.to_string()).as_deref(), Some(short));
    }

    #[test]
    fn pubkey_refuses_everything_that_is_not_one() {
        for bad in [
            "",
            "notapubkey",
            "0OIl",                                          // the four base58 excludes
            "AStZxZ8XgH9nSKarLT4MzUrY8HM5LtExzDaN9SDoaKiqq", // one char long
            "1AStZxZ8XgH9nSKarLT4MzUrY8HM5LtExzDaN9SDoaKiq", // leading zero byte, different string
            "../../etc/passwd",
            "AStZxZ8XgH9nSKarLT4MzUrY8HM5LtExzDaN9SDoaKi ",  // trailing space
        ] {
            assert!(pubkey(bad).is_none(), "accepted {bad:?}");
        }
    }

    #[test]
    fn a_signature_is_exactly_sixty_four_bytes_of_hex() {
        let ok = "ab".repeat(64);
        assert_eq!(sig64(&ok).map(|s| s[0]), Some(0xab));
        assert_eq!(sig64(&ok).map(|s| s.len()), Some(64));
        for bad in [
            String::new(),
            "ab".repeat(63),          // short
            "ab".repeat(65),          // long
            "zz".repeat(64),          // not hex
            format!("{}gg", "ab".repeat(63)),
            format!(" {}", "ab".repeat(64)).trim_end().to_string(), // leading space, right length
        ] {
            assert!(sig64(&bad).is_none(), "accepted {:?}", &bad[..bad.len().min(12)]);
        }
    }

    #[test]
    fn hex_round_trips_through_sig64() {
        let bytes: [u8; 64] = std::array::from_fn(|i| (i * 7 % 251) as u8);
        assert_eq!(sig64(&hex_bytes(&bytes)), Some(bytes));
    }

    #[test]
    fn hex_is_lowercase_and_zero_padded() {
        assert_eq!(hex_bytes(&[0, 1, 15, 16, 255]), "00010f10ff");
    }

    #[test]
    fn base58_decodes_the_known_all_zero_key() {
        // 32 zero bytes encode as 32 '1's, and nothing else does.
        assert_eq!(bs58_to_32(&"1".repeat(32)), Some([0u8; 32]));
    }

    // ── query parsing ───────────────────────────────────────────────────────

    #[test]
    fn param_reads_values_and_decodes_them() {
        let u = "/api/step?id=s1&do=oxygen%20face%20mask&tick=30";
        assert_eq!(param(u, "id").as_deref(), Some("s1"));
        assert_eq!(param(u, "do").as_deref(), Some("oxygen face mask"));
        assert_eq!(param(u, "tick").as_deref(), Some("30"));
        assert_eq!(param(u, "missing"), None);
    }

    /// A percent-escape is an octet, and a Thai character is three of them. Decoding each octet
    /// as a `char` reads them as Latin-1 and hands the matcher mojibake — so a Thai order matches
    /// nothing, resolves to nothing, and is written to the tape as rubbish a verifier then
    /// reproduces forever. Several scenarios have carried Thai keywords since before the language
    /// layer existed; none of them could ever have fired.
    #[test]
    fn a_typed_order_survives_the_url_in_any_alphabet() {
        let u = "/api/step?id=s1&do=%E0%B8%9F%E0%B8%B1%E0%B8%87%E0%B8%9B%E0%B8%AD%E0%B8%94";
        assert_eq!(param(u, "do").as_deref(), Some("ฟังปอด"));
        // And the whole point of getting it right: it now names an order a station understands.
        assert_eq!(lang::canonical_order(&param(u, "do").unwrap()), Some("listen to the chest"));
        // ASCII decodes exactly as it always did — every tape already anchored still reads the
        // same, which is the property that makes this safe to change at all.
        let a = "/api/step?do=oxygen%20face%20mask+15+lpm&x=100%25";
        assert_eq!(param(a, "do").as_deref(), Some("oxygen face mask 15 lpm"));
        assert_eq!(param(a, "x").as_deref(), Some("100%"));
    }

    #[test]
    fn param_does_not_match_a_key_that_merely_ends_with_the_one_asked_for() {
        // `account` must not be answered by `subaccount`, or a caller can aim a request at
        // somebody else's record by naming a parameter carefully.
        let u = "/api?subaccount=theirs&account=mine";
        assert_eq!(param(u, "account").as_deref(), Some("mine"));
    }

    // ── the apex ────────────────────────────────────────────────────────────

    /// The front door serves the landing and the company's two documents, and moves everything
    /// else — path, query and all — to the game origin. A deep link shared from the game must
    /// survive being pasted on the apex.
    #[test]
    fn the_apex_keeps_the_landing_and_redirects_the_rest() {
        assert_eq!(apex_target("/"), None);
        assert_eq!(apex_target("/?utm_source=colosseum"), None, "campaign links are landings");
        assert_eq!(apex_target("/play").as_deref(), Some("https://devnet.vitals.academy/play"));
        assert_eq!(
            apex_target("/api/chain?player=abc").as_deref(),
            Some("https://devnet.vitals.academy/api/chain?player=abc"),
        );
    }

    /// The privacy URL is quoted to an OAuth consent screen and printed in footers, so the apex
    /// answers it rather than bouncing to a host called `devnet`. Both hosts serve the same
    /// bytes: `front_door` is the one copy.
    #[test]
    fn the_apex_answers_the_policy_and_the_terms_itself() {
        assert_eq!(apex_target("/privacy"), None, "the policy must not 301 off the apex");
        assert_eq!(apex_target("/terms"), None, "the terms must not 301 off the apex");
        assert!(front_door("/privacy").contains("Anchoring a run is permanent"));
        assert!(front_door("/terms").contains("not a clinical qualification"));
        assert_eq!(front_door("/"), LANDING, "the landing is still the landing");
        // The stamp is replaced on the way out, or a reader cannot tell which build the page
        // is describing — which is the only thing that makes it checkable against the code.
        for p in ["/privacy", "/terms"] {
            assert!(!front_door(p).contains(BUILD_STAMP), "{p} went out unstamped");
            assert!(front_door(p).contains(BUILD), "{p} does not say which build it describes");
        }
    }

    // ── where to listen ─────────────────────────────────────────────────────

    /// Cloud Run probes the port it assigned, not the one we prefer. An app that boots
    /// perfectly on the wrong port fails in the way that is hardest to see from inside.
    #[test]
    fn the_platforms_port_wins_and_binds_publicly() {
        assert_eq!(bind_addr(Some("8080"), None), "0.0.0.0:8080");
        assert_eq!(bind_addr(Some("8080"), Some("0.0.0.0:8474")), "0.0.0.0:8080");
    }

    #[test]
    fn without_a_platform_port_the_operator_decides_and_the_default_is_loopback() {
        assert_eq!(bind_addr(None, Some("0.0.0.0:8474")), "0.0.0.0:8474");
        assert_eq!(bind_addr(None, None), "127.0.0.1:8474");
        // An empty PORT is no PORT — some shells export the variable before it has a value.
        assert_eq!(bind_addr(Some(""), None), "127.0.0.1:8474");
    }

    // ── which routes need a token ───────────────────────────────────────────

    #[test]
    fn guarding_covers_everything_that_spends_or_signs() {
        for p in ["/api/anchor", "/api/claim", "/api/commit", "/api/say"] {
            assert!(guarded(p), "{p} makes the server sign or spend");
        }
        // The factory's three doors are on the other guard, and deliberately not on this one:
        // this one's token is printed into `bay.js` for every visitor.
        for p in ["/api/ward/queue", "/api/ward/pack/42", "/api/ward/case"] {
            assert!(door(p), "{p} writes to the ward and takes the ward's own secret");
            assert!(!guarded(p),
                    "{p} must not answer to the page's token — it is on a public page, and this \
                     is the bug of 16 ก.ย.");
        }
        for p in ["/", "/play", "/api/new", "/api/step", "/api/finish", "/api/kit", "/api/tape", "/api/chain",
                  "/api/meter", "/api/fuel", "/api/stars", "/api/lang", "/api/usage", "/donate",
                  // The ward's census. The endpoint is the source the weekly card photographs
                  // and the thing a judge is invited to re-derive; a token on it would mean
                  // "checkable by anyone we gave a token to", which is not the claim.
                  "/api/ward", "/api/ward/stream",
                  // What cases the ward holds. The same kind of fact as the census, and a
                  // catalogue only the people we hand a token to can read is a claim.
                  "/api/ward/cases",
                  // One shift, and the tape it is checked against. A receipt only the people we
                  // hand a token to can read is not a receipt, it is a claim.
                  "/api/shift/0000000000000000000000000000000000000000000000000000000000000000",
                  "/api/tape/0000000000000000000000000000000000000000000000000000000000000000",
                  // Who wrote what, and how often it has been proven. A ledger nobody can read
                  // is not one anybody can check, and checkable is the entire claim.
                  "/api/authors", "/api/payout",
                  // The policy and the terms. A token on either would be a policy nobody can
                  // read, which is the same as not having one — and Google's consent screen has
                  // to be able to fetch the privacy URL without credentials.
                  "/privacy", "/terms",
                  // The reviewer's form and the route it posts to. A student and a physician
                  // were handed one link; a token on either is a review that never arrives, and
                  // "tell us what is wrong" is not a thing anyone should need an account for.
                  "/review", "/api/review",
                  // Re-derivation is the product's whole claim. A token on it would mean
                  // "re-derivable by anyone we gave a token to", which is not the claim.
                  "/api/sce/0000000000000000000000000000000000000000000000000000000000000000"] {
            assert!(!guarded(p), "{p} is play, and a kiosk must not need a token to play");
            assert!(!door(p), "{p} is play, and the factory's key opens the factory's doors only");
        }
    }

    // ── the cluster label ───────────────────────────────────────────────────

    #[test]
    fn the_label_names_the_cluster_the_rpc_actually_points_at() {
        assert_eq!(cluster_of("https://api.devnet.solana.com"), "devnet");
        assert_eq!(cluster_of("https://api.testnet.solana.com"), "testnet");
        assert_eq!(cluster_of("https://api.mainnet-beta.solana.com"), "mainnet");
        assert_eq!(cluster_of("http://127.0.0.1:8899"), "localnet");
        assert_eq!(cluster_of("http://localhost:8899"), "localnet");
        assert_eq!(cluster_of("https://rpc.example.com"), "custom");
    }

    // ── scenario table ──────────────────────────────────────────────────────

    #[test]
    fn every_episode_has_a_title_a_difficulty_and_a_file() {
        for ep in ["ep1", "ep2", "ep3", "ep4", "ep5", "osce-a", "osce-b", "osce-c", "osce-d"] {
            assert!(title(ep).starts_with(&ep.to_uppercase()), "{ep} title is {}", title(ep));
            let p = scenario_path(ep);
            assert!(p.exists(), "{ep}: {} is missing", p.display());
        }
        // The ladder is meant to climb — and the stations sit on the tier they rehearse.
        assert_eq!(difficulty("ep1"), Difficulty::Student);
        assert_eq!(difficulty("ep2"), Difficulty::Intern);
        assert_eq!(difficulty("ep5"), Difficulty::Resident);
        assert_eq!(difficulty("osce-a"), Difficulty::Student);
        assert_eq!(difficulty("osce-b"), Difficulty::Intern);
        assert_eq!(difficulty("osce-c"), Difficulty::Resident);
        assert_eq!(difficulty("osce-d"), Difficulty::Intern);
    }

    /// **The clock the card advertises and the clock the server keeps are one number.**
    ///
    /// It was never one number, because it was never a number the server had. `mins` lived only
    /// in the page, where it drew a progress bar; nothing on this side had heard of it, so
    /// "a 10-minute station" was a caption. It is enforced now — [`Session::over`] reads
    /// [`RUNTIME_MINUTES`] — and the moment a caption becomes a rule, the two copies of it have
    /// to be held together or a card will advertise one duration while the bell rings on
    /// another.
    ///
    /// Neither copy can go. The page is served as a static file and paints its shelf before it
    /// has spoken to the server; the server needs the figure with no page in the room (the bell
    /// itself). So they stay two copies with one value, and this is what says so — the same
    /// arrangement, and the same reason, as `the_shelf_card_and_the_server_print_the_same_stem`.
    /// The landing carousel is a third copy of the same clock, and the newest one.
    ///
    /// EP1's card was typed by hand when it was added, and "12 min" was right by luck rather than
    /// by construction. The shelf inside the app has had this guard since it existed; the front
    /// door is the page more people see, and it had none.
    ///
    /// It also counts: five cards and five dots, because a dot with no card behind it is a
    /// destination the ring can never reach, and a card with no dot is one a keyboard cannot.
    #[test]
    fn the_landing_carousel_agrees_with_the_server_about_the_clock() {
        let cards: Vec<&str> = LANDING.match_indices("<article class=\"cf-card\" data-ep=\"")
            .map(|(at, pat)| {
                let rest = &LANDING[at + pat.len()..];
                &rest[..rest.find('"').expect("unterminated data-ep")]
            })
            .collect();
        assert_eq!(
            cards,
            ["ep1", "ep2", "ep3", "ep4", "ep5"],
            "season one is five numbered cases, in order, and the carousel is what says so"
        );

        // "EP1 &middot; Ing &middot; F 19 &middot; 12 min" — the last figure on the card's own line.
        // Scoped to the card's own <article>: the hero above it prints "EP1 &middot; The Last Bite
        // &middot; waiting", which comes first in the file and is not a duration at all.
        for ep in &cards {
            let card_at = LANDING
                .find(&format!("<article class=\"cf-card\" data-ep=\"{ep}\""))
                .unwrap_or_else(|| panic!("{ep} has no card"));
            let card = &LANDING[card_at..];
            let card = &card[..card.find("</article>").expect("unterminated card")];
            let at = card
                .find("cf-who\">")
                .unwrap_or_else(|| panic!("{ep}'s card has no who line"));
            let line = &card[at..at + card[at..].find("</p>").expect("unterminated card line")];
            let mins = line
                .rsplit("&middot; ")
                .next()
                .and_then(|tail| tail.trim().strip_suffix(" min"))
                .and_then(|n| n.trim().parse::<u32>().ok())
                .unwrap_or_else(|| panic!("{ep}'s card line does not end in a duration: {line}"));
            let want = RUNTIME_MINUTES
                .iter()
                .find(|(id, _)| id == ep)
                .unwrap_or_else(|| panic!("{ep} is on the landing and not in RUNTIME_MINUTES"))
                .1;
            assert_eq!(mins, want, "{ep}: the landing card and the bell disagree about the clock");
        }

        let dots = LANDING.matches("data-ga=\"landing_cases_dot_").count();
        assert_eq!(dots, cards.len(), "the carousel has {} cards and {dots} dots", cards.len());
    }

    #[test]
    fn the_shelf_card_and_the_server_agree_about_the_clock() {
        // The array itself, and not the rest of the file after it: `{id:'…'` occurs elsewhere,
        // and a count taken over the tail would be counting something else.
        let page = served();
        let season = page
            .split_once("const SEASON=[")
            .map(|(_, rest)| rest)
            .and_then(|rest| rest.split_once("\n];"))
            .map(|(arr, _)| arr)
            .expect("SEASON is gone from the page");
        let card_mins = |id: &str| -> u32 {
            let at = season
                .find(&format!("{{id:'{id}',"))
                .unwrap_or_else(|| panic!("{id} has no card in SEASON"));
            let rest = &season[at..];
            let from = rest.find(",mins:").unwrap_or_else(|| panic!("{id}'s card has no mins")) + 6;
            let to = from + rest[from..].find(|c: char| !c.is_ascii_digit()).unwrap_or(0);
            rest[from..to].parse().unwrap_or_else(|_| panic!("{id}'s mins is not a number"))
        };
        for (id, mins) in RUNTIME_MINUTES {
            assert_eq!(card_mins(id), *mins, "{id}: the card and the bell disagree about the clock");
        }
        // And the table covers the whole shelf. A case the server has no duration for would
        // fall back to twelve minutes and ring a bell nobody advertised.
        let cards = season.matches("{id:'").count();
        assert_eq!(
            cards,
            RUNTIME_MINUTES.len(),
            "the shelf has {cards} entries and the server times {}",
            RUNTIME_MINUTES.len()
        );
    }

    /// A duration that cannot be met is a lie on the card, so this is measured rather than
    /// asserted from memory: every station has to be *passable* well inside what it advertises.
    ///
    /// It is the check that matters for the bell, and it is not the same as asking whether the
    /// case *ends* inside it. Four stations' failing narratives arrest one to four minutes after
    /// their card's mark, and that is correct — the candidate's time is up, the patient's is not,
    /// and `Session::ring_the_bell` lets her finish going where she was going. What would be
    /// wrong is a station a candidate could not complete in the time on the door.
    #[test]
    fn every_station_can_be_passed_inside_the_time_its_card_advertises() {
        // The definitive order for each station — what turns the case around — given at once.
        // Enough to reach the win; the point is the clock, not the mark sheet.
        const CURE: &[(&str, &[&str])] = &[
            ("osce-a", &["adrenaline_im"]),
            ("osce-a2", &["adrenaline_im"]),
            ("osce-b", &["ecg", "cath_lab"]),
            ("osce-b2", &["nsaid"]),
            ("osce-b3", &["dexamethasone", "observe_child"]),
            ("osce-c", &["dexamethasone", "observe_child"]),
            ("osce-c2", &["neb_salbutamol", "prednisolone", "ipratropium", "pefr"]),
            ("osce-c3", &["antibiotics", "admit_ward"]),
            ("osce-d", &["two_lines", "crystalloid", "type_screen", "transfuse", "endoscopy"]),
            ("osce-d2", &["wells", "ctpa", "heparin"]),
            ("osce-d3", &["adrenaline_child"]),
            ("osce-d4", &["two_lines", "cultures", "fluids", "antibiotics", "norepinephrine",
                          "source_control", "icu_bed"]),
        ];
        for (ep, cure) in CURE {
            let j = std::fs::read_to_string(scenario_path(ep)).expect("scenario");
            let mut tape: Vec<Step> = Vec::new();
            for o in *cure {
                tape.push(Step::Act { text: (*o).into(), id: (*o).into() });
                for _ in 0..5 {
                    tape.push(Step::Tick(2.0));
                }
            }
            let limit = runtime_sec(ep);
            let (whole, _) = vitals_replay::rung(&j, &tape, limit).expect("ring");
            let r = replay(&j, &whole).expect("replay");
            assert_eq!(
                r.outcome.as_deref().map(|o| o.starts_with("Win")),
                Some(true),
                "{ep}: the model answer does not reach a win — {:?}",
                r.outcome
            );
            assert!(
                r.sim_seconds <= limit,
                "{ep} advertises {:.0} minutes and cannot be completed inside them: the win \
                 lands at {:.1}",
                limit / 60.0,
                r.sim_seconds / 60.0
            );
        }
    }

    /// Every station is nameable in the save list, not just the four somebody typed out.
    ///
    /// `title()` used to spell the `OSCE-x ·` prefix by hand for A, B, C and D, and the other
    /// eight fell through to the bare stem. A save list then read "Barking cough on the second
    /// night — F 3" with nothing to say whether that was B3, C, or one of the other coughs, which
    /// is the one thing the save list exists to tell you.
    #[test]
    fn every_station_wears_its_own_name_in_the_save_list() {
        for m in SETS.iter().flat_map(|s| s.members.iter()) {
            let t = title(m.id);
            let want = format!("{} · ", m.id.to_uppercase());
            assert!(t.starts_with(&want), "{} is saved as {t:?} — no station id in front", m.id);
            assert!(t.ends_with(m.title), "{}: the stem was dropped or rewritten: {t:?}", m.id);
        }
        // Episodes are unchanged: they have drama titles, not station ids.
        assert_eq!(title("ep1"), "EP1 · The Last Bite");
        assert_eq!(title("ep3"), "EP3 · Don't Make Him Cry");
        // Twelve distinct names, which is the property that failed.
        let names: std::collections::BTreeSet<String> =
            SETS.iter().flat_map(|s| s.members.iter()).map(|m| title(m.id)).collect();
        assert_eq!(names.len(), 12, "two stations save under the same name");
    }

    /// A station title is on screen from the shelf card through the title card and then in the
    /// player bar for every minute of the exam — while the mark sheet is paying 2–4 points for
    /// naming the diagnosis. So the rule is mechanical, and so is the check: no station's display
    /// copy may contain a disease or a treatment. This test is the reason the rule survives the
    /// next person who adds a member and reaches for the case name.
    #[test]
    fn no_station_title_names_the_answer_it_is_marking() {
        // Diseases the rubrics name, and the drugs their `expected` items pay for.
        const GIVEAWAYS: &[&str] = &[
            "anaphyla", "stemi", "infarct", "coronary", "pericarditis", "myocarditis", "croup",
            "epiglott", "asthma", "bronchospasm", "pneumonia", "embolism", "sepsis", "septic",
            "gi bleed", "gastrointestinal", "peptic", "ulcer", "shock",
            "adrenaline", "epinephrine", "steroid", "dexamethasone", "antibiotic", "aspirin",
            "heparin", "thrombolys", "salbutamol", "nebulis",
            // The rest of what these twelve rubrics pay for. The list was written against the
            // titles as they stood and stopped there, so a rewrite could reach for a synonym of
            // the answer and land inside the gap: "melaena" is the diagnosis of a GI bleed said
            // in one word, "urticaria" is anaphylaxis said in one word, and every drug below is
            // an `expected` item on some sheet. A stem names what the doorway shows — a rash, a
            // cough, vomited blood — and none of these is that.
            "melaena", "melena", "haematemes", "hematemes", "varice", "urticaria",
            "angio-oedema", "angioedema", "pneumothorax", "tuberculosis", "bronchiolitis",
            "tracheitis", "urosepsis", "hydrocortisone", "prednisolone", "chlorphen",
            "ipratropium", "amoxi", "ceftriaxone", "pantoprazole", "colchicine", "ibuprofen",
            "clopidogrel", "noradrenaline", "endoscopy", "intubat", "defibrillat",
        ];
        let titles = SETS
            .iter()
            .flat_map(|s| s.members.iter())
            .map(|m| (m.id, m.title.to_string()))
            // The save-list copy is the same string with an id in front of it; it leaks the same.
            .chain(SETS.iter().flat_map(|s| s.members.iter()).map(|m| (m.id, title(m.id))));
        for (id, t) in titles {
            let low = t.to_lowercase();
            for bad in GIVEAWAYS {
                assert!(!low.contains(bad), "{id} title says the answer out loud: {t:?} contains {bad:?}");
            }
            assert!(!t.is_empty(), "{id} has no title");
        }
    }

    /// **The same stem, written twice, and twice it has drifted.**
    ///
    /// A station title lives in two files: [`SETS`] here, which is what the server puts in the
    /// save list, the player bar and `/api/sets`, and `SEASON` in `static/index.html`, which is
    /// what the shelf card and the title card print. Nothing joined them, so the fix for
    /// "the card is answering the mark sheet" was applied to one copy and not the other — twice.
    /// The visible result the second time: the card on the shelf still read
    /// "Wheals, swollen lips and a wheeze — F 6, 20 kg" — the weight is the paediatric dose
    /// calculation, and `osce-d3`'s sheet pays for getting it right — while the server had
    /// already dropped it.
    ///
    /// Neither copy can simply be deleted: the page is served as a static file and reads its own
    /// table before it has spoken to the server, and the server needs the stem with no page in
    /// the room at all (the save list, the CLI, the mark sheet). So they stay two copies with one
    /// value, and this is what says so. It reads the page rather than keeping a third list, for
    /// the same reason `every_alias_names_an_order_a_case_could_recognise` does in `lang.rs`.
    #[test]
    fn the_shelf_card_and_the_server_print_the_same_stem() {
        // The one table in the page that carries a station card. Anchored so a stray `{id:'…'`
        // somewhere else in the file can never be read as the shelf.
        let page = served();
        let season = page
            .split_once("const SEASON=[")
            .map(|(_, rest)| rest)
            .expect("SEASON is gone from the page");
        let card = |id: &str| -> &str {
            let at = season
                .find(&format!("{{id:'{id}',"))
                .unwrap_or_else(|| panic!("{id} has no card in SEASON"));
            let rest = &season[at..];
            let from = rest.find(",t:'").unwrap_or_else(|| panic!("{id}'s card has no title")) + 4;
            let to = from + rest[from..].find('\'').unwrap_or_else(|| panic!("{id}'s title never ends"));
            &rest[from..to]
        };
        for m in SETS.iter().flat_map(|s| s.members.iter()) {
            assert_eq!(
                card(m.id),
                m.title,
                "{}: the shelf card and the set table disagree about the stem",
                m.id
            );
        }
        // And the page holds exactly these twelve — a card the server has never heard of would
        // be a station nobody can score, and it would pass the loop above by not being in it.
        assert_eq!(
            season.matches("station:true").count(),
            SETS.iter().map(|s| s.members.len()).sum::<usize>(),
            "the page shows a different number of stations than the server declares"
        );
    }

    /// **A comment is served with the page.** `static/index.html` ships whole — markup, script
    /// and every comment in it — so a comment is public copy that happens to be addressed to the
    /// next engineer. A scored number has now escaped that way twice: the shelf card carried the
    /// paediatric weight until the test above was written, and the two comments explaining `who`
    /// went on printing the very same string as their worked example for another release after
    /// the card was fixed. `osce-d3` pays three points for asking that weight (`ask_weight`) and
    /// six more for the dose drawn off it, and the whole station is about dosing a child by the
    /// kilo — nine points, collectable with view-source and no clinical thought at all.
    ///
    /// Two mechanical rules, checked against the page exactly as the browser receives it:
    ///
    ///   * a patient descriptor is two fields — `Name · SEX AGE` — and stops. A third `·`
    ///     segment is where the weight got in both times, in the card and in the comment.
    ///   * a body weight in kilograms appears nowhere in the file. No card, caption or comment
    ///     has a use for one; the candidate is paid to ask for it. A dose written *per* kilogram
    ///     (`saline 20 ml/kg`) is a chip label offered on screen, not a weight, and stays legal.
    ///
    /// Deliberately narrower than [`no_station_title_names_the_answer_it_is_marking`]: that
    /// test's GIVEAWAYS list cannot be run over the whole page, because `REVEAL` and `CHIPS`
    /// legitimately hold all twelve diagnoses and every drug on every differential — the page
    /// has to print the differential to offer it. The leak this catches is not a disease being
    /// named in this file, it is a scored *number* written down where nobody had to ask.
    #[test]
    fn the_page_never_writes_down_a_number_the_rubric_pays_to_ask_for() {
        // Every `· M`/`· F` marker in the file is inside a patient descriptor — a `who` field, a
        // bay caption, or a comment quoting one. Read from the marker to the end of whatever is
        // holding it and fail on a second separator.
        let ends = |c: char| c == '\'' || c == '"' || c == '<' || c == '\n';
        let page = served();
        for (i, _) in page.match_indices("· M").chain(page.match_indices("· F")) {
            let rest = &page[i..];
            let field = &rest[..rest.find(ends).unwrap_or(rest.len())];
            assert_eq!(
                field.matches('·').count(),
                1,
                "a patient descriptor carries a third field — {field:?} — and everything past the \
                 age is a fact the candidate is supposed to have to ask the patient for"
            );
        }
        // And no body weight, in any of them or anywhere else.
        let page = served();
        for (i, _) in page.match_indices("kg") {
            let head = page[..i].trim_end_matches(' ');
            if !head.ends_with(|c: char| c.is_ascii_digit()) {
                continue; // `mg/kg`, `ml/kg`, `20 ml/kg` — a rate per kilo, not a weight.
            }
            let mut ctx: Vec<char> = head.chars().rev().take(60).collect();
            ctx.reverse();
            let ctx: String = ctx.into_iter().collect();
            panic!(
                "the page states a weight in kilograms — ...{ctx}kg... — which is the one number \
                 `osce-d3` pays a candidate to ask for"
            );
        }
    }

    /// A device pane may not hold text it is not always allowed to show.
    ///
    /// `vent.html` shipped the interpretation of the peak-to-plateau gap — the reading a
    /// ventilator station exists to mark — and decided whether to render it from
    /// `P.get('exam') === '1'`. Both halves of that were wrong. The string was in the served
    /// file whatever the branch did, so view-source read it; and the branch was steered by a
    /// query parameter on an iframe URL, so dropping `&exam=1` was not even an attack.
    ///
    /// The sentences live in [`VENT_READ_WIDE`] and [`VENT_READ_NARROW`] now and reach a pane
    /// only on the feed, only when [`Session::sealed`] is false. This test is the one that
    /// fails if either ever comes back into a file the candidate is served: it checks the
    /// panes exactly as the browser receives them, and it checks that no pane has re-hung a
    /// gate on something the reader controls.
    #[test]
    fn a_device_pane_holds_no_reading_it_may_have_to_withhold() {
        const PANES: &[(&str, &str)] =
            &[("vent", VENT), ("monitor", MONITOR), ("pump", PUMP)];
        // The sentences themselves, and the phrases that carry the answer even paraphrased.
        const READS: &[&str] = &[
            VENT_READ_WIDE,
            VENT_READ_NARROW,
            "think bronchospasm",
            "not stiff lungs",
            "airway resistance is not the problem",
        ];
        for (name, page) in PANES {
            for needle in READS {
                assert!(
                    !page.contains(needle),
                    "device/{name}.html ships an interpretation — {needle:?} — and every string \
                     in that file is one view-source away from the candidate reading it"
                );
            }
            // A gate the reader holds is not a gate. No pane may decide what to withhold from
            // its own URL: the seal is the server's answer, and it arrives on the feed.
            for gate in ["P.get('exam')", "get('exam')", "exam=1"] {
                assert!(
                    !page.contains(gate),
                    "device/{name}.html gates on {gate:?}, which is a query parameter the \
                     candidate can edit off the end of the iframe URL"
                );
            }
        }
        // And the bay does not offer one either — the parameter is gone from the URL it builds,
        // so there is nothing for a pane to start reading again.
        assert!(
            !served().contains("&exam=1"),
            "the page still hangs an exam flag on a device URL"
        );
    }

    /// Same rule, the other half of the card: the band is what a circuit prints on the door, and
    /// it must stay wider than the organ the station is about.
    #[test]
    fn a_station_card_wears_a_circuit_band_not_an_organ() {
        const BANDS: &[&str] = &["emergency", "paediatrics", "medicine", "surgery"];
        for m in SETS.iter().flat_map(|s| s.members.iter()) {
            assert!(BANDS.contains(&m.band), "{} wears {:?}, which is not a circuit band", m.id, m.band);
            assert!(!m.band.starts_with("eir-"), "{} is wearing the Eir specialty on the card", m.id);
        }
    }

    /// The commit gate and the anchor scorer both ask this function, so the set of cases that
    /// can host an exam has exactly one definition.
    #[test]
    fn only_rubricd_cases_can_host_exams() {
        for ep in ["ep2", "ep3", "ep4", "ep5", "osce-a", "osce-b", "osce-c", "osce-d"] {
            assert!(rubric_path(ep).is_some(), "{ep} has an authored rubric");
        }
        // ep1 stays the story-only intro — the door a stranger walks through unexamined.
        for ep in ["ep1", "nonsense"] {
            assert!(rubric_path(ep).is_none(), "{ep} hosts no exam");
        }
    }

    #[test]
    fn an_unknown_episode_falls_back_rather_than_panicking() {
        assert!(scenario_path("../../etc/passwd").exists(), "unknown ids fall back to EP1");
        assert_eq!(difficulty("nonsense"), Difficulty::Student);
    }

    // ── station sets ────────────────────────────────────────────────────────

    /// The set table is the one copy of the gate design — so the design's own invariants are
    /// pinned here: satisfiable needs, unique members, a published lead per set, and gates
    /// keyed to the episodes they open.
    #[test]
    fn station_sets_are_well_formed_and_lead_members_are_live() {
        let mut seen = std::collections::HashSet::new();
        for (s, opens) in SETS.iter().zip(["ep2", "ep3", "ep4", "ep5"]) {
            assert_eq!(s.opens, opens, "{} opens the wrong door", s.gate);
            let ceiling = vitals_progress::STAR_TIERS * s.members.len() as u32;
            assert!(s.need >= 1 && s.need <= ceiling,
                "{}: need {} can never be met by {} members", s.gate, s.need, s.members.len());
            for m in s.members {
                assert!(seen.insert(m.id), "{} is declared in two sets", m.id);
                assert!(m.id.starts_with("osce-"), "{} is not a station id", m.id);
                assert!(!m.case.is_empty() && !m.title.is_empty() && !m.specialty.is_empty());
            }
            assert!(member_playable(s.members[0].id),
                "{}: lead member {} must be playable today", s.gate, s.members[0].id);
        }
    }

    /// The repriced ladder, pinned as numbers rather than as a ratio: a gate whose price drifts
    /// is a season whose difficulty drifts, and neither shows up in any other test.
    #[test]
    fn the_three_star_gate_prices_hold_the_published_climb() {
        let priced: Vec<(&str, u32, u32)> = SETS
            .iter()
            .map(|s| (s.gate, s.need, vitals_progress::STAR_TIERS * s.members.len() as u32))
            .collect();
        assert_eq!(
            priced,
            vec![("gate2", 3, 6), ("gate3", 6, 9), ("gate4", 7, 9), ("gate5", 10, 12)],
            "the doors are priced 3/6/7/10 against ceilings 6/9/9/12 (DECISIONS.md 27 ส.ค.)"
        );
        // The climb itself, as a fraction of each set's ceiling: 50% → 67% → 78% → 83%.
        let pct: Vec<u32> = priced.iter().map(|(_, need, ceil)| need * 100 / ceil).collect();
        assert_eq!(pct, vec![50, 66, 77, 83], "each door must ask for more of its set than the last");
        assert!(pct.windows(2).all(|w| w[0] < w[1]), "the season must get harder, never easier");
        // And no door may demand a flawless run of *every* member: one item a tape did not
        // catch would then shut an episode for good, which is not a difficulty curve, it is a
        // wall. Two stars per member is always enough to leave headroom somewhere.
        for (gate, need, ceil) in &priced {
            assert!(need < ceil, "{gate}: a door priced at its own ceiling can never forgive a slip");
        }
    }

    /// While a set is short of its roster, the door's live price is capped at what the
    /// published members can yield — a gate must never be impossible, only cheaper until
    /// Phase 5b ships the rest. Resolved against the real files, so the day new members land
    /// this test re-prices the doors by itself.
    #[test]
    fn a_short_set_caps_its_need_at_what_its_members_can_yield() {
        for st in resolve_sets() {
            let playable = st.members.iter().filter(|(_, h)| h.is_some()).count() as u32;
            assert!(playable >= 1, "{}: no playable member at all", st.set.gate);
            assert_eq!(st.need_now, st.set.need.min(playable * vitals_progress::STAR_TIERS));
            assert_eq!(st.ceiling(), playable * vitals_progress::STAR_TIERS);
            assert!(st.need_now <= st.ceiling(), "{}: an unreachable door", st.set.gate);
            // every playable member carries the hash the chain will see for it
            for (m, h) in &st.members {
                assert_eq!(h.is_some(), member_playable(m.id), "{} hash/playability disagree", m.id);
            }
        }
    }

    /// A declared-but-unpublished member resolves to its own absent file — never to the EP1
    /// fallback, because playing EP1 under a station's name would anchor the wrong case hash.
    /// And with no rubric it can host no exam, so a coming-soon card can never cost a star.
    #[test]
    fn a_coming_soon_member_is_a_card_not_an_error_and_never_an_exam() {
        for st in resolve_sets() {
            for (m, h) in st.members.iter().filter(|(_, h)| h.is_none()) {
                assert!(scenario_path(m.id).ends_with(format!("demo/stations/{}.sce.json", m.id)),
                    "{} must resolve under demo/stations", m.id);
                assert!(rubric_path(m.id).is_none(), "{} without files cannot host an exam", m.id);
                assert!(h.is_none());
            }
        }
    }

    // ── case films ──────────────────────────────────────────────────────────

    /// 🛑 The CLINICAL HOLD, as a test rather than as a promise.
    ///
    /// Two images are held pending a clinician's read (`docs/internal/CASE_MEDIA_WIRING.md`):
    /// the ChestX-ray14 pneumonia film, whose label is NLP-mined and which does not obviously
    /// show the consolidation osce-c3's beat describes, and the PTB-XL anterior-ST trace, which
    /// may be an old infarct on a station that teaches acute reperfusion. Neither may be named by
    /// a film or compiled into the binary — a route that cannot find the bytes cannot serve them
    /// to somebody who guesses the URL. There is no third door to close: the shelf wears no
    /// clinical image at all any more (a station card says who the patient is, not what one of
    /// its investigations came back as), and the patient stills are read off the disk under a
    /// name this table can never spell — see [`station_still_path`].
    #[test]
    fn the_films_under_clinical_hold_are_nowhere_in_the_build() {
        const HELD: &[&str] = &["cxr-consolidation-pneumonia-1", "ecg-st-elevation-anterior-01278"];
        for h in HELD {
            assert!(!FILMS.iter().any(|f| f.file.contains(h)), "{h} is wired to a station");
            assert!(!CASE_IMG.iter().any(|(k, _, _)| k.contains(h)), "{h} is compiled in and serveable");
            assert!(station_still_path(h, "stable").is_none(), "{h} is reachable as a patient still");
        }
    }

    /// Every film names a file the route can actually serve, and every compiled image is one
    /// something asks for. A caption over a 404 is worse than no picture.
    #[test]
    fn every_film_resolves_to_bytes_in_the_binary() {
        for f in FILMS {
            assert!(CASE_IMG.iter().any(|(k, _, _)| *k == f.file), "{}: {} is not served", f.station, f.file);
            assert!(!f.caption.is_empty(), "{} has a picture and no read", f.station);
            assert!(set_member(f.station).is_some(), "{} is not a declared station", f.station);
        }
        for (k, bytes, mime) in CASE_IMG {
            assert!(!bytes.is_empty(), "{k} is empty");
            assert!(FILMS.iter().any(|f| f.file == *k), "{k} is compiled in and nothing shows it");
            // The mixed-suffix trap the route arm exists to avoid, pinned.
            let want = if k.ends_with(".png") { "image/png" } else { "image/jpeg" };
            assert_eq!(*mime, want, "{k} is served as the wrong type");
        }
    }

    /// A film is presentation. It is read off the tape, never written to it — so a station
    /// ordered twice shows one picture, and a resumed run shows what the run had already seen
    /// without the leaf knowing images exist.
    #[test]
    fn films_are_read_off_the_tape_and_never_repeat() {
        let tape = vec![
            Step::acted("12-lead ecg", "ecg"),
            Step::Tick(30.0),
            Step::acted("chest x-ray", "cxr"),
            Step::acted("another ecg", "ecg"),
            // An order nobody understood resolves to an empty id; it must not match a station
            // whose table happens to hold an entry keyed on the empty string later.
            Step::acted("do something clever", ""),
        ];
        let got = films_from_tape("osce-a", &tape);
        assert_eq!(got.len(), 2, "one film per distinct order");
        assert_eq!(got[0].file, "ecg-sinus-tachycardia-04408.png");
        assert_eq!(got[1].file, "cxr-normal-1.png");
        // A station with no table entry stays exactly as it was before FILMS existed.
        assert!(films_from_tape("osce-d", &tape).is_empty());
        assert!(film_for("osce-a", "").is_none(), "an unresolved order shows nothing");
    }

    // ── the language layer ──────────────────────────────────────────────────

    /// Drive a run the way `/api/step` does, so these tests exercise the real path rather than a
    /// convenient one: recognise, apply by id, record text *and* id, advance the clock.
    fn play(s: &mut Session, orders: &[&str], tick: f64) {
        for act in orders {
            let id = resolve_order(&s.state, act);
            let emitted = if id.is_empty() { s.state.apply(act) } else { s.state.apply_id(&id) };
            s.beats.extend(emitted.iter().map(render_beat));
            s.tape.push(Step::acted(act, &id));
            let emitted = s.state.tick(tick);
            s.beats.extend(emitted.iter().map(render_beat));
            s.tape.push(Step::Tick(tick));
        }
    }

    /// Long enough for a scenario to reach a terminal state if it is going to — the same drift
    /// `vitals-replay`'s own liveness tests use.
    fn drift(s: &mut Session) {
        for _ in 0..6 {
            let emitted = s.state.tick(300.0);
            s.beats.extend(emitted.iter().map(render_beat));
            s.tape.push(Step::Tick(300.0));
        }
    }

    /// **The load-bearing test of the whole language layer.**
    ///
    /// A case's identity on chain is the sha256 of its file, and its run's identity is the leaf
    /// over the tape. If choosing Thai could move either of those, a Thai learner would be
    /// playing a different case from an English one — the stars would not be comparable, the
    /// cohort statistics would be meaningless, and "anybody can re-verify this run" would become
    /// "anybody holding the same translation can". So: same session, two languages, and the only
    /// thing on the wire that may differ is the line the beats are *read* in.
    #[test]
    fn a_language_never_reaches_the_leaf() {
        let mut s = new_session("ep1").expect("ep1 is the case the season opens on");
        play(&mut s, &["adrenaline im", "oxygen", "supine", "admit"], 30.0);
        drift(&mut s);

        let en = s.view(lang::language(Some("en")));
        let th = s.view(lang::language(Some("th")));

        assert_eq!(en.sce_hash, th.sce_hash, "the case changed identity when the page changed language");
        assert_eq!(en.leaf, th.leaf, "the run changed identity when the page changed language");
        assert!(en.leaf.is_some(), "the run has to have ended for the leaf to prove anything");
        assert_eq!(en.beats, th.beats, "the canonical beats are the leaf's own input");
        assert_eq!(en.harm, th.harm);
        assert_eq!(en.status, th.status);
        assert_eq!(en.outcome, th.outcome);

        // And on the wire: byte-identical apart from the one presentation field.
        let a = serde_json::to_value(&en).expect("view serialises");
        let mut b = serde_json::to_value(&th).expect("view serialises");
        assert!(a.get("tr").is_none(), "the default language sends no translation at all");
        assert!(b.get("tr").is_some(), "Thai asked for a translation and got none");
        b.as_object_mut().expect("an object").remove("tr");
        assert_eq!(a, b, "language reached something other than the beat lines");

        // The tape is the evidence, and it never learned what language anybody was reading in.
        let tape = serde_json::to_string(&s.tape).expect("the tape serialises");
        for l in lang::LANGUAGES {
            assert!(!tape.contains(&format!("\"{}\"", l.id)), "{} is on the tape", l.id);
        }
        assert!(!tape.contains("lang"), "the tape carries a language field");
    }

    /// Only beats the run has actually earned are translated, and only for a language that has
    /// rows. This is the exam seal's problem restated: a table of every beat in the case, handed
    /// to the page up front, would name the drug and the deadline the rubric is about to pay for.
    #[test]
    fn a_translation_carries_only_what_the_run_has_already_seen() {
        let th = lang::language(Some("th"));
        let mut s = new_session("ep1").expect("ep1");
        assert!(beat_lines(th, &s.beats).is_none(), "a run that has done nothing has nothing to read");

        play(&mut s, &["stand up and walk to the toilet"], 5.0);
        let lines = beat_lines(th, &s.beats).expect("standing a hypotensive patient up is a harm");
        for k in lines.keys() {
            assert!(s.beats.contains(k), "{k} was translated and never happened");
        }
        assert!(
            !lines.contains_key("terminal:DeathBiphasic"),
            "an ending this run has not reached was sent to the page",
        );
    }

    /// A station reads in the language it was asked in, and the run underneath does not move.
    ///
    /// This used to assert the other half — that OSCE-B3's scripted lines came back
    /// *untranslated* — which was true while `BEATS` held three rows against the season. It is
    /// not any more: every scripted beat of every case on the shelf now has a Thai line, pinned
    /// against the scenario files themselves by
    /// `lang::tests::every_scripted_beat_of_every_case_has_a_thai_line`, and the fallback for a
    /// case that has none is pinned there too.
    ///
    /// What survives is the half that always mattered here, and it is this file's half rather
    /// than that one's: the translation is a coat over the run. Same beats, same spelling, same
    /// order, same leaf — whichever language the page asked in.
    #[test]
    fn a_case_with_no_translation_still_plays() {
        let th = lang::language(Some("th"));
        let mut s = new_session("osce-b3").expect("a station");
        play(&mut s, &["dexamethasone syrup", "score her from the doorway"], 30.0);
        let v = s.view(th);
        let en = s.view(lang::language(Some("en")));
        assert!(!v.beats.is_empty(), "the station still speaks");
        // The canonical beats are the run. They are what replay re-derives and what the leaf
        // hashes, so they must be byte-identical either side of the picker — the translation
        // rides beside them in `tr` and never in place of them.
        assert_eq!(v.beats, en.beats, "the beats themselves changed language");
        assert_eq!(v.leaf, en.leaf);
        assert!(en.tr.is_none(), "the language the case was written in sent a translation");
        for b in &v.beats {
            if b.starts_with("threshold:") {
                assert!(
                    v.tr.as_ref().is_some_and(|t| t.contains_key(b)),
                    "{b} reached a Thai bedside in English",
                );
            }
        }
    }

    /// A learner who reads Thai buttons types Thai orders. Those must reach the same intervention
    /// the English words reach — on the episodes *and* on the stations, whose keyword lists carry
    /// only a scattering of Thai.
    #[test]
    fn a_thai_order_reaches_the_intervention_the_english_one_does() {
        let ep1 = new_session("ep1").expect("ep1").state;
        assert_eq!(resolve_order(&ep1, "ฉีดอะดรีนาลีนเข้ากล้าม"), "adrenaline_im");
        assert_eq!(resolve_order(&ep1, "ให้ออกซิเจน"), resolve_order(&ep1, "oxygen"));
        // The harmful route stays its own order — a translation that collapsed the two would put
        // a learner's IV push on the record as the rescue dose.
        assert_eq!(resolve_order(&ep1, "อะดรีนาลีนเข้าเส้น 1:1000"), "adrenaline_iv_push");

        let a = new_session("osce-a").expect("osce-a").state;
        assert_eq!(resolve_order(&a, "ฟังปอด"), resolve_order(&a, "listen to the chest"));
        assert_ne!(resolve_order(&a, "ฟังปอด"), "", "the station heard nothing");

        // Nobody understood it: still the empty answer the tape is entitled to.
        assert_eq!(resolve_order(&ep1, "ยาหอมสักซอง"), "");
    }

    /// The same case, played identically, once through English chips and once by typing Thai.
    /// The words on the tape are the learner's own and differ; everything the score, the debrief
    /// and the chain are computed from is the same run.
    #[test]
    fn the_same_care_in_two_languages_is_the_same_run() {
        let mut en = new_session("ep1").expect("ep1");
        let mut th = new_session("ep1").expect("ep1");
        play(&mut en, &["adrenaline im", "oxygen", "supine", "admit"], 30.0);
        play(&mut th, &["ฉีดอะดรีนาลีน", "ให้ออกซิเจน", "นอนราบยกขาสูง", "admit"], 30.0);
        drift(&mut en);
        drift(&mut th);

        let ids = |s: &Session| -> Vec<String> {
            s.tape
                .iter()
                .filter_map(|x| match x {
                    Step::Act { id, .. } => Some(id.clone()),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(ids(&en), ids(&th), "the same care resolved to different interventions");

        let a = en.view(lang::language(Some("en")));
        let b = th.view(lang::language(Some("th")));
        assert_eq!(a.outcome, b.outcome);
        assert_eq!(a.beats, b.beats);
        // The leaf hashes the tape, and the tape keeps the words the learner actually typed — so
        // these two leaves are *not* equal, and that is correct. What must be equal is everything
        // the rubric and the debrief read, which is the ids above and the beats here.
        assert_eq!(a.sce_hash, b.sce_hash, "they played the same case");
    }

    // ── the stations' own patient stills ────────────────────────────────────

    /// The disk-read route is as narrow as the compiled ones beside it. Both halves of the name
    /// are whitelisted before a path is composed, so nothing outside the forty-eight filenames
    /// this build recognises can be asked for — no traversal, no neighbouring directory, and no
    /// clinical image from the bank (whose names are not station ids).
    #[test]
    fn a_patient_still_can_only_ever_be_asked_for_by_a_name_the_binary_owns() {
        for bad in ["../ecg-sinus-tachycardia-04408", "osce-zz", "ep1", "", "card/x"] {
            assert!(station_still_path(bad, "stable").is_none(), "{bad} resolved to a path");
        }
        for bad in ["../../Cargo", "dead", "recovered", "improving", "stable.jpg", ""] {
            assert!(station_still_path("osce-a", bad).is_none(), "state {bad} resolved to a path");
        }
        // And where a name *is* legal, the path it composes is the one file it is allowed to be.
        for m in SETS.iter().flat_map(|s| s.members.iter()) {
            for st in STATION_STATES {
                let want = station_stills_dir().join(format!("{}_{st}.jpg", m.id));
                assert_eq!(station_still_path(m.id, st), want.is_file().then_some(want));
            }
        }
    }

    /// What the set table advertises is what the route will actually serve. The page hangs a
    /// still in the biggest panel of the bay on the strength of this list, and a name in it that
    /// 404s is the black frame the stem exists to prevent.
    #[test]
    fn a_station_advertises_exactly_the_stills_it_has() {
        for m in SETS.iter().flat_map(|s| s.members.iter()) {
            let advertised = station_states(m.id);
            for st in &advertised {
                assert!(station_still_path(m.id, st).is_some(), "{} advertises a missing {st}", m.id);
                assert!(STATION_STATES.contains(st));
            }
            // Order matters: the page walks this list to find a substitute when the exact state
            // has not been shot, and "worse than asked for" is the wrong way to fall back.
            let want: Vec<_> = STATION_STATES.iter().filter(|st| advertised.contains(st)).collect();
            assert_eq!(advertised.iter().collect::<Vec<_>>(), want, "{} lists its states out of order", m.id);
        }
    }

    /// The key art is reachable through the same arm the stills use, and both crops of each
    /// episode ship — `<picture>` falls back to the wide one when a source is missing, so a
    /// dropped 3:2 file would silently send a phone the billboard.
    #[test]
    fn every_episode_with_key_art_ships_both_crops() {
        for ep in ["ep2_prasit", "ep3_khaopun", "ep4_mali", "ep5_boonsong"] {
            for k in [ep.to_string(), format!("{ep}_3x2")] {
                let found = KEY_ART.iter().find(|(n, _)| *n == k);
                assert!(found.is_some_and(|(_, b)| !b.is_empty()), "{k}.jpg is missing");
            }
        }
        // One namespace, so a key art file may not shadow a clinical status.
        for (k, _) in KEY_ART {
            assert!(!STILLS.iter().any(|(s, _)| s == k), "{k} collides with a still");
        }
    }

    // ── the device picker ───────────────────────────────────────────────────

    #[test]
    fn kit_phrases_carry_the_number_the_learner_dialled() {
        assert_eq!(kit_phrase("o2", Some(6.0)).as_deref(), Some("oxygen face mask 6 lpm"));
        assert_eq!(kit_phrase("iv", Some(250.0)).as_deref(), Some("iv access normal saline 250 ml/hr"));
        assert_eq!(kit_phrase("defib", Some(200.0)).as_deref(), Some("defibrillate 200 j"));
        assert_eq!(kit_phrase("nothing", None), None);
    }

    #[test]
    fn kit_phrases_without_a_setting_still_read_as_orders() {
        assert_eq!(kit_phrase("ett", None).as_deref(), Some("intubate, secure the airway"));
        assert!(kit_phrase("o2", None).unwrap().contains("10 lpm"), "falls back to the scenario dose");
    }

    // ── the chart's own words ───────────────────────────────────────────────
    //
    // The chart prints the case author's label for an order. The labels are the author's working
    // notes and nineteen of them carry the author's verdict in the name — so the chart, the one
    // surface in an exam that has to stay neutral, was marking the candidate's work in front of
    // them, one line above the harm sentence the seal was withholding. See `neutral_label`.

    /// `(case id, intervention id, the author's label)` for every case on this disk.
    ///
    /// Read from the scenario files rather than from a list here, so a station added tomorrow —
    /// or a label edited tomorrow — is covered without anyone remembering to come back.
    fn every_authored_label() -> Vec<(&'static str, String, String)> {
        let mut out = Vec::new();
        for ep in every_case() {
            let path = scenario_path(ep);
            // A declared-but-unpublished member is a coming-soon card, not a missing file.
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let v: serde_json::Value =
                serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            for iv in v["interventions"].as_array().into_iter().flatten() {
                let (Some(id), Some(label)) = (iv["id"].as_str(), iv["label"].as_str()) else { continue };
                out.push((ep, id.to_string(), label.to_string()));
            }
        }
        assert!(out.len() > 200, "the scenario files stopped being read: {} labels", out.len());
        out
    }

    /// **The regression, pinned against the disk.** A future case author who writes `(HARM)` into
    /// a label — the natural thing to do, and what nineteen of them already do — cannot make that
    /// verdict reach a candidate's chart without this failing.
    #[test]
    fn no_authored_label_reaches_the_chart_carrying_a_verdict() {
        let mut annotated = 0;
        for (ep, id, label) in every_authored_label() {
            let shown = neutral_label(&label);
            assert!(
                !shown.to_ascii_uppercase().contains(VERDICT),
                "{ep}/{id}: the chart would print {shown:?} — that is the author grading the \
                 candidate's order on the order line. Write the verdict in `harm:`, which is \
                 sealed until the bell, not in `label`, which is not."
            );
            assert!(!shown.is_empty(), "{ep}/{id}: {label:?} was stripped down to nothing");
            assert!(
                !shown.ends_with('(') && !shown.ends_with('—') && !shown.ends_with('-'),
                "{ep}/{id}: {label:?} rendered as {shown:?}, which is a sentence cut in half"
            );
            if shown != label {
                annotated += 1;
            }
        }
        assert!(
            annotated >= 19,
            "only {annotated} annotated labels were found and stripped; the audit found 19 across \
             twelve stations plus EP1's two. If the count fell because the labels were rewritten, \
             lower it — if it fell because the stripping stopped matching them, do not."
        );
    }

    /// The nineteen, written out, because "it no longer contains HARM" is not the same claim as
    /// "it still names the order the candidate gave". Every one of these is a chart line a real
    /// run produces.
    #[test]
    fn the_annotated_labels_render_as_the_order_and_nothing_else() {
        for (before, after) in [
            ("IV-push adrenaline (HARM)", "IV-push adrenaline"),
            ("Discharge home (HARM)", "Discharge home"),
            ("Reassure and discharge (HARM)", "Reassure and discharge"),
            ("Aspirin (HARM here)", "Aspirin"),
            ("Activate the cath lab (HARM)", "Activate the cath lab"),
            ("Thrombolysis (HARM)", "Thrombolysis"),
            ("Antibiotics (HARM)", "Antibiotics"),
            ("Look in the throat (HARM)", "Look in the throat"),
            ("A drip and bloods first (HARM)", "A drip and bloods first"),
            ("Sedative for the panic (HARM)", "Sedative for the panic"),
            ("Home with tablets (HARM)", "Home with tablets"),
            ("Aspirin (HARM)", "Aspirin"),
            ("Full-dose lytics (HARM)", "Full-dose lytics"),
            ("Reassure — anxiety (HARM)", "Reassure — anxiety"),
            ("Send her home (HARM)", "Send her home"),
            ("IV-push 1:1000 adrenaline (HARM)", "IV-push 1:1000 adrenaline"),
            ("stand / walk the hypotensive patient (HARM)", "stand / walk the hypotensive patient"),
            // OSCE D3's pair, which *is* the station: 0.2 mg to the kilo, or the adult 0.5. The
            // dose stays on both — it is what was ordered — and only the grade comes off, so the
            // chart cannot be read to find out which one was the trap.
            ("Adrenaline 0.5 mg IM — adult dose (HARM)", "Adrenaline 0.5 mg IM — adult dose"),
            ("Adrenaline 0.2 mg IM — 0.01/kg", "Adrenaline 0.2 mg IM — 0.01/kg"),
        ] {
            assert_eq!(neutral_label(before), after, "{before:?}");
        }
    }

    // ── how old the patient is ──────────────────────────────────────────────
    //
    // One decision hangs off this: whether NEWS2 may report a score. It is an adult score, and
    // it is not validated under 16 — see `news2`. `osce-b3` is three years old and was being
    // handed "7 · HIGH RISK · emergency response" on vitals that are normal for three.

    /// The ward and the bay must mean the same case by the same name.
    ///
    /// They did not. The ward's catalogue took its episode ids from the scenario **filenames** —
    /// `ep2-stemi` — and the bay has always keyed on the shelf ids `ep2` to `ep5`. So
    /// `scenario_path("ep2-stemi")` fell through to its last arm and answered with **EP1's file**,
    /// and a shift opened on an episode patient would have played anaphylaxis under her name,
    /// anchored it against her chain, and looked entirely healthy doing it.
    ///
    /// Two resolvers is the underlying fault — `ward_chain::case_path` for the refill, this one
    /// for the bay — so the test pins that they agree on every case the ward can admit, rather
    /// than only that each one resolves to something.
    #[test]
    fn the_ward_and_the_bay_resolve_a_case_to_the_same_file() {
        let root = scenario_root();
        for case in vitals_web::ward::CATALOGUE {
            let bay = scenario_path(case);
            let ward = ward_chain::case_path(&root, case);
            assert_eq!(bay, ward,
                       "{case}: the bay reads {} and the ward reads {} — one of them is playing a \
                        different patient under the same name",
                       bay.display(), ward.display());
            assert!(bay.exists(), "{case} resolves to {}, which is not there", bay.display());
            assert_ne!(bay.file_name(), std::path::Path::new("sce-anaphylaxis-ep1.json").file_name(),
                       "{case} fell through to EP1's file, which is the fallback this catalogue \
                        must never reach: it would anchor anaphylaxis under another case's name");
        }
    }

    /// Nothing may be added to the shelf without saying how old its patient is. This is the test
    /// that makes "no age declared means adult" a safe default rather than a back door.
    #[test]
    fn every_case_declares_how_old_its_patient_is() {
        for ep in every_case() {
            assert!(
                patient_age(ep).is_some(),
                "{ep} has no age in AGES, so NEWS2 would score its patient on the adult table \
                 whoever that patient is"
            );
        }
        for (id, _) in AGES {
            assert!(every_case().contains(id), "AGES names {id}, which is not a case any more");
        }
    }

    /// The age in the table is the age the patient gives when she is asked. A server that scores
    /// a fourteen-year-old as an adult while her own persona says fourteen is one screen
    /// disagreeing with itself, which is how this bug reached production in the first place.
    #[test]
    fn the_declared_age_is_the_age_the_patient_says_she_is() {
        let mut checked = 0;
        for ep in every_case() {
            let Ok(text) = std::fs::read_to_string(persona_path(ep)) else { continue };
            let v: serde_json::Value = serde_json::from_str(&text).expect("a persona");
            let Some(said) = v["patient"]["age"].as_f64() else { continue };
            assert_eq!(
                patient_age(ep),
                Some(said),
                "{ep}: the persona says {said} and AGES says {:?}",
                patient_age(ep)
            );
            checked += 1;
        }
        assert!(checked >= 13, "only {checked} personas were read; the cross-check stopped working");
    }

    /// The five the score must refuse, named, so a case cannot quietly leave the list.
    #[test]
    fn the_children_on_the_shelf_are_not_given_an_adult_score() {
        let under: Vec<&str> = every_case()
            .into_iter()
            .filter(|ep| !news2::applies_to_age(patient_age(ep)))
            .collect();
        assert_eq!(
            under,
            vec!["ep3", "osce-b2", "osce-b3", "osce-c", "osce-d3"],
            "the set of paediatric cases changed — b2 is fourteen, which the original audit missed"
        );
    }

    /// A parenthesis is not a verdict. These are labels that say what the order *was*, and the
    /// stripping may not reach into them — a chart that prints "Risk-stratified" where the case
    /// said "Risk-stratified (Wells / PERC)" has lost the order, not a grade.
    #[test]
    fn a_parenthetical_that_is_not_a_verdict_survives() {
        for keep in [
            "Risk-stratified (Wells / PERC)",
            "Confirmed the diagnosis (CTPA)",
            "Decompressed the chest (tension pneumothorax)",
            "Transfused blood (not crystalloid)",
            "Did not distress the child (no forced cannulation)",
            "Haemorrhage control first (tourniquet / pressure)",
            "Called the airway team (ENT / anaesthesia) early",
            "Patient reperfused (survives)",
            "Adrenaline — no dose named",
        ] {
            assert_eq!(neutral_label(keep), keep, "a plain label was cut");
        }
        // Lower case, and a label that is only a verdict: neither may end as an empty line.
        assert_eq!(neutral_label("Aspirin (harm)"), "Aspirin", "the check is on the word, not its case");
        assert_eq!(neutral_label("(HARM)"), "(HARM)", "a label with no order in it has no neutral form");
    }


    /// **A receipt on a season case still names the case, and a penalty row says what it took.**
    ///
    /// Three things the director found on Yonas's receipt on 00048:
    ///
    ///   * the headline read "Yonas Haile · 16 · from Ethiopia" and nothing about the case. He is
    ///     on a season case, whose title lives in the bay's own table rather than in the catalogue,
    ///     so `case_title` is null and the line collapsed to nothing. The ruling was title and
    ///     level in the headline, and where the title comes from is our problem, not a reader's.
    ///   * a `no_unindicated` row read "0 of 0 · Ordered nothing this pericardium did not need".
    ///     It is a deduction, not a mark: on a run that ordered nothing off the list it took
    ///     nothing, and "0 of 0" reads as a mark that was available and missed.
    ///   * the judged-score line was the long explanation of why a mid-stay shift has no AI marks.
    ///     On a receipt it is one line.
    #[test]
    fn a_season_receipt_names_its_case_and_a_penalty_row_says_what_it_took() {
        let r = serde_json::json!({
            "patient_id": 1789528998,
            "shift": 1,
            "name": "Yonas Haile",
            "sex": "m",
            "age": 16,
            "country_name": "Ethiopia",
            // A season case: the catalogue has no title or level for it.
            "case": "ep3",
            "case_title": serde_json::Value::Null,
            "difficulty": serde_json::Value::Null,
            "run_hash": "77aa",
            "slot": 499000000,
            "player": "7FAEbbbb",
            "did": { "steps": 5, "beats": 2, "harm": [], "outcome": serde_json::Value::Null },
            "det": { "earned": 8, "max": 40 },
            "items": [
                { "label": "Pericardiocentesis", "kind": "action", "points": 6, "earned": 6,
                  "penalty": 0, "charged": [] },
                { "label": "Ordered nothing this pericardium did not need", "kind": "no_unindicated",
                  "points": 0, "earned": 0, "penalty": 0, "charged": [] }
            ],
            "timeline": [{ "at": 30.0, "kind": "order", "text": "pericardiocentesis" }],
            "tape": "/api/tape/77aa",
            "judged_omitted": "No AI-judged marks on a mid-stay shift.",
        });
        let page = receipt_page(&r, &serde_json::Value::Null);

        // The case, from the season's own table, with its level beside it.
        assert!(page.contains("EP3 · Don't Make Him Cry"),
                "a season case's title comes from the bay's table when the catalogue has none");
        assert!(page.contains("resident"), "and its level with it: {page}");

        // The deduction row, as a deduction.
        assert!(page.contains("no penalty · nothing ordered off the list"),
                "a no_unindicated row that took nothing says so: {page}");
        assert!(!page.contains("0</b> of 0"),
                "and never as a mark that was available and missed");

        // One line about the judged score.
        assert!(page.contains("No AI-judged marks on a mid-stay shift."), "{page}");

        // A compiled case must not borrow the season's fallback title — `title()` answers EP1 for
        // anything it does not know, which on a receipt would name the wrong case entirely.
        let mut compiled = r.clone();
        compiled["case"] = serde_json::json!("embla-dengue-shock-student");
        let page = receipt_page(&compiled, &serde_json::Value::Null);
        assert!(!page.contains("The Last Bite"),
                "a case the season does not know is not EP1: {page}");
    }

    /// **The receipt tells a stranger what happened, in the right person's pronoun.**
    ///
    /// It read "13 orders · 0 beats" and "8 of 40" under a case id, with no list of what was done,
    /// no mark it was earned against, nothing to open on the chain — and it said "what she did"
    /// over Yonas Tesfaye, because the page had one pronoun written into it. A receipt is the thing
    /// a player wants to show somebody.
    #[test]
    fn the_receipt_tells_the_shift_and_takes_the_patients_pronoun() {
        let r = serde_json::json!({
            "patient_id": 1789528999,
            "shift": 2,
            "name": "Yonas Tesfaye",
            "sex": "m",
            "age": 41,
            "country_name": "Ethiopia",
            "case": "embla-severe-falciparum-malaria-cerebral-resident",
            "case_title": "Man of 41 from Ethiopia with fever and confusion",
            "difficulty": "resident",
            "portrait": "https://storage.googleapis.com/vitals-world-portraits/aa.webp",
            "run_hash": "9f2c",
            "leaf": "11aa",
            "slot": 498100000,
            "player": "7FAEaaaa",
            "signature": "5xTxSig",
            "did": { "steps": 3, "beats": 1, "harm": [], "outcome": "death_arrest" },
            "det": { "earned": 8, "max": 40 },
            "items": [
                { "label": "Blood cultures before antibiotics", "points": 3, "earned": 3 },
                { "label": "Antibiotics within the hour", "points": 6, "earned": 0 }
            ],
            "timeline": [
                { "at": 12.0, "kind": "asked", "text": "how long has the fever been going?" },
                { "at": 48.0, "kind": "order", "text": "blood cultures", "id": "ix_blood_cultures" }
            ],
            "tape": "/api/tape/9f2c",
            "judged_omitted": "AI-judged marks are not shown on a mid-stay shift",
        });
        let page = receipt_page(&r, &serde_json::Value::Null);

        // The story, in order.
        assert!(page.contains("Yonas Tesfaye"), "{page}");
        assert!(page.contains("Man of 41 from Ethiopia with fever and confusion"),
                "the case's own title, not its id, in the headline");
        assert!(page.contains("what happened") && page.contains("how long has the fever been going?")
                    && page.contains("blood cultures"),
                "the tape read out: what was asked and what was ordered");
        assert!(page.contains("0:12") && page.contains("0:48"), "with the clock beside each");
        assert!(page.contains("8</b> of 40") && page.contains("Antibiotics within the hour"),
                "the marks, and the rows they are made of");

        // The evidence is present and folded: the id, the leaf, the slot, the explorer, the tape.
        assert!(page.contains("<details>") && page.contains("the addresses this shift is filed under"));
        assert!(page.contains("embla-severe-falciparum-malaria-cerebral-resident"),
                "the case id is in the evidence rather than the headline");
        let head = page.split("<h2>").next().unwrap_or("");
        assert!(!head.contains("embla-severe-falciparum"), "and never above it: {head}");
        assert!(page.contains("explorer.solana.com/tx/5xTxSig?cluster=devnet"),
                "the transaction a stranger can open on somebody else's screen");
        assert!(page.contains("/api/tape/9f2c"));

        // The pronoun is the patient's.
        assert!(page.contains("his chart"), "{page}");
        assert!(!page.contains("her chart") && !page.contains("what she did"),
                "the ward admits men, and this page said otherwise over Yonas");

        // A patient with no persona to read gets neither pronoun rather than a guess.
        let mut anon = r.clone();
        anon["sex"] = serde_json::json!("");
        anon["did"]["outcome"] = serde_json::Value::Null;
        let page = receipt_page(&anon, &serde_json::Value::Null);
        assert!(page.contains("handed on, still on the ward"), "{page}");
        assert!(!page.contains(" she ") && !page.contains(" he "), "{page}");
    }
}
