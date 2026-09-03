//! Play EP1 in a browser.
//!
//! One process: the same `vitals-sce` automaton the verifier runs, a session per player, and a
//! tape that is recorded as you play. Reach a terminal state and the tape reduces to a leaf —
//! the same bytes `vitals-replay` would produce from the same tape, because it is the same code.
//!
//! Deliberately small. No framework, no database, no build step: tiny_http, a single HTML page,
//! and sessions in a map. The point is to make the automaton playable, not to ship a platform.

mod chain;
use vitals_web::{archive, fuel, lang, meter, news2, patient, reading, review, store, usage};

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
/// The front door. The product page lives at `/` and the game one click behind it at `/play`,
/// because the first visitor a public URL meets is as likely to be a reviewer deciding what this
/// company is as a learner deciding whether to press play — and the bay answers only the second.
const LANDING: &str = include_str!("../static/landing.html");
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
        }
    }

    /// Rebuild a run from disk by replaying its tape.
    fn restore(saved: Saved) -> Result<Session, String> {
        let sce_json = std::fs::read_to_string(scenario_path(&saved.ep)).map_err(|e| e.to_string())?;
        let want = hex(&sce_hash(&sce_json));
        if want != saved.sce_hash {
            return Err(format!("scenario {} changed under this run", saved.ep));
        }
        let (state, r) = resume(&sce_json, &saved.tape)?;
        Ok(Session {
            ep: saved.ep.clone(),
            owner: saved.owner.clone(),
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
        Some(o) => u.finished(s.state.outcome_id().unwrap_or("unknown"), o.is_death(), store),
        // A run the clock ended. It has no outcome id to borrow, because the case never reached
        // one of its own endings — so it is counted under a name of its own rather than folded
        // into somebody else's ending or, as before this existed, not counted at all.
        None => u.finished(TIME_CALLED, false, store),
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
            age_years: patient_age(&self.ep),
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
            status: format!("{:?}", self.state.status),
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
        })
}

fn html(body: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_string(body).with_header(
        Header::from_bytes(&b"Content-Type"[..], &b"text/html; charset=utf-8"[..]).unwrap(),
    )
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

/// Who is calling, as far as rate limiting is concerned.
///
/// Behind Cloud Run the socket peer is the load balancer, and the visitor is the first entry of
/// `X-Forwarded-For` — later entries are whatever proxies appended, and the *last* one is the
/// only one Google vouches for, but for a politeness window the first is the honest choice: it
/// is the same for one browser and different for two households. Player keys and session ids are
/// deliberately not used here; a browser mints those for free.
fn client_addr(req: &tiny_http::Request) -> String {
    req.headers()
        .iter()
        .find(|h| h.field.equiv("x-forwarded-for"))
        .and_then(|h| h.value.as_str().split(',').next().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
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
    matches!(path, "/api/anchor" | "/api/claim" | "/api/commit" | "/api/say")
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
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn main() {
    // Not 8090. On the machine this is developed on that port is already three things — a
    // syn-sentry web app, an eir-fhir container publishing on the host, and the service port the
    // hermodr fleet uses inside the cluster. A default that collides with the neighbours is a
    // default that fails on the one day nobody is watching.
    let addr = bind_addr(
        std::env::var("PORT").ok().as_deref(),
        std::env::var("VITALS_WEB_BIND").ok().as_deref(),
    );
    let token = std::env::var("VITALS_TOKEN").ok().filter(|s| !s.is_empty());
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
    // A run nobody has touched in a day is a closed tab, not a patient.
    let swept = store.sweep(SESSIONS, std::time::Duration::from_secs(24 * 60 * 60));

    let mut restored = HashMap::new();
    let mut broken = 0usize;
    for (id, saved) in store.list::<Saved>(SESSIONS) {
        match Session::restore(saved) {
            Ok(s) => {
                restored.insert(id, s);
            }
            Err(e) => {
                // Loud, and dropped. A run that will not replay is exactly the thing this repo
                // must not paper over: it means the tape and the automaton disagree.
                eprintln!("dropping session {id}: {e}");
                store.del(SESSIONS, &id);
                broken += 1;
            }
        }
    }
    // No counter to carry across a restart any more: ids are random, so a restored run cannot
    // collide with a fresh one and there is nothing to resume from.
    println!(
        "state      {} · {} run(s) resumed{}{}",
        store.describe(),
        restored.len(),
        if swept > 0 { format!(" · {swept} expired") } else { String::new() },
        if broken > 0 { format!(" · {broken} unreplayable") } else { String::new() },
    );
    let sessions: Arc<Mutex<HashMap<String, Session>>> = Arc::new(Mutex::new(restored));

    // What this bay may spend, resumed from the store so a deploy does not reset the month.
    let mut meter = meter::Meter::open(&store);
    println!("meter      {}", meter.describe());

    // Runs opened and runs finished, resumed from the store. Deliberately not a count of
    // people: there is no signup here, so there is nothing that is a person to count. See
    // `usage::LIMITS`, which travels with every reply the endpoint gives.
    let mut usage = usage::Usage::open(&store);
    println!("usage      {}", usage.describe());

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
    println!("fuel       {}", fuel.describe());
    // Signed halves waiting on the browser, keyed by player. Never persisted: a blockhash goes
    // stale in about a minute, so a pending transaction that outlives the process is worthless.
    let pendings: Arc<Mutex<HashMap<String, PendingWork>>> = Arc::new(Mutex::new(HashMap::new()));

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
    println!("Vitals — play at http://{bound}");

    // One slow local model, and /api/say holds a worker for as long as it takes. Without a
    // ceiling a single caller can occupy the GPU indefinitely.
    let mut said: Vec<Instant> = Vec::new();
    const SAY_PER_MIN: usize = 20;

    // `mut` for exactly one route: reading a request body needs the request mutably, and the
    // match below borrows it for the whole of its scrutinee. See `/api/review`.
    for mut req in server.incoming_requests() {
        let url = req.url().to_string();
        let path = url.split('?').next().unwrap_or("/").to_string();

        // The apex answers with the front door — the landing, and the two documents about the
        // company — and anything deeper moves permanently to the game origin. 301 on purpose:
        // the split is a recorded decision, not a phase. Everything the apex serves carries its
        // own short cache life so a proxy caching the apex never holds anything of the game's.
        if host_of(&req) == APEX {
            let resp = match apex_target(&url) {
                None => html(&front_door(&path)).with_header(
                    Header::from_bytes(&b"Cache-Control"[..], &b"public, max-age=300"[..]).unwrap(),
                ),
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
                let _ = req.respond(html(LANDING));
                continue;
            }
            (Method::Get, "/play") => {
                // The page is served by the same process that holds the token, so handing it over
                // does not widen anything: reaching the page and reaching the API are one boundary.
                let page = match &token {
                    Some(tk) => PAGE.replace("__VITALS_TOKEN__", tk),
                    None => PAGE.replace("__VITALS_TOKEN__", ""),
                };
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
                if let meter::Verdict::SlowDown { retry_secs } =
                    meter.allow_free(&format!("new:{}", client_addr(&req)), &store)
                {
                    let _ = req.respond(json_code(serde_json::json!({
                        "error": "too many new runs from this address — give it a minute",
                        "retry_in": retry_secs,
                    }), 429));
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
                        usage.started(&ep, s.owner.as_deref(), &store);
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
                        count_finish(&mut usage, s, was_over, &store);
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
                        count_finish(&mut usage, s, was_over, &store);
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
                        count_finish(&mut usage, s, was_over, &store);
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
                    Some(s) => match rubric_path(&s.ep) {
                        None => json(serde_json::json!({ "case": s.ep, "items": [] })),
                        Some(p) => {
                            let sheet = std::fs::read_to_string(&p)
                                .map_err(|e| e.to_string())
                                .and_then(|rj| vitals_osce::sheet_for_run(&s.sce_json, &s.tape, &rj));
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
                    },
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
                // Snapshot what the model needs, then release the lock: a local 26B reply takes
                // seconds and the tick loop must not block behind it.
                //
                // `ep` comes out with the rest of it, because *which patient is in this bed* is a
                // fact about the session. It was not read at all before: one persona was loaded
                // at boot and every case in the season borrowed it, so asking OSCE-A's
                // seventy-one-year-old man anything got an answer from a nineteen-year-old woman
                // about her shrimp allergy — in her name, on her allergy, at her age.
                let (hist, status, spo2, ep) = {
                    let mut map = sessions.lock().unwrap();
                    let Some(s) = map.get_mut(&id).filter(|s| s.answers_to(caller.as_deref())) else {
                        let _ = req.respond(no_such_session());
                        continue;
                    };
                    // The question goes on the tape. The answer never will.
                    s.tape.push(Step::asked(&q));
                    (s.said.clone(), format!("{:?}", s.state.status), s.state.vitals.spo2, s.ep.clone())
                };
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
                let want = lang::language(param(&url, "lang").as_deref());
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
                let t = tree.lock().unwrap();
                let mut v = usage.view();
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
            (Method::Get, "/privacy") => html(&PRIVACY.replace(BUILD_STAMP, BUILD)),
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
            (Method::Get, "/review") => html(&REVIEW.replace(BUILD_STAMP, BUILD)),
            (Method::Get, "/api/chain") => {
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
                t.leaves.push(rec.leaf());
                let tree_id = t.tree_id;
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
                                          index, score: rec.score(),
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

#[cfg(test)]
mod tests {
    use super::*;

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
        for p in ["/", "/play", "/api/new", "/api/step", "/api/finish", "/api/kit", "/api/tape", "/api/chain",
                  "/api/meter", "/api/fuel", "/api/stars", "/api/lang", "/api/usage", "/donate",
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
        let season = PAGE
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
                "{ep} advertises {:.0} minutes and cannot be completed inside them: the win                  lands at {:.1}",
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
        let season = PAGE
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
        for (i, _) in PAGE.match_indices("· M").chain(PAGE.match_indices("· F")) {
            let rest = &PAGE[i..];
            let field = &rest[..rest.find(ends).unwrap_or(rest.len())];
            assert_eq!(
                field.matches('·').count(),
                1,
                "a patient descriptor carries a third field — {field:?} — and everything past the \
                 age is a fact the candidate is supposed to have to ask the patient for"
            );
        }
        // And no body weight, in any of them or anywhere else.
        for (i, _) in PAGE.match_indices("kg") {
            let head = PAGE[..i].trim_end_matches(' ');
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
            !PAGE.contains("&exam=1"),
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

}
