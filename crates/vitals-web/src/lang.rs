//! The language the patient speaks — and nothing else.
//!
//! A learner in Bangkok practises on a patient who answers in Thai; one in Jakarta will practise
//! on a patient who answers in Bahasa Indonesia. That is not decoration. The skill this product
//! measures is *understanding what the patient tells you*, and a Thai foundation doctor who
//! rehearses that skill in English has rehearsed a different skill from the one the ward will
//! ask for. So the patient's language is a real feature, and it is chosen by the learner.
//!
//! # The one rule
//!
//! **Language is presentation. It never becomes evidence.**
//!
//! A case's identity on chain is the sha256 of its `demo/**` file (`sce_hash`), bound into the
//! commitment and carried in the leaf. Translating a scenario file would mint a *different case*
//! — the stars, the marks and the cross-cohort statistics of the English original would no
//! longer be comparable with it, and "anybody can re-verify this run" would quietly become
//! "anybody can re-verify this run, if they hold the same translation". So:
//!
//!   - nothing in this module is read by [`vitals_replay`], and nothing here is an input to a
//!     leaf, a tape, a rubric or an `sce_hash`;
//!   - no file under `demo/` is touched to add a language;
//!   - two learners who take the same actions on the same case reach the *same leaf*, whichever
//!     language their screen was in.
//!
//! This is deliberately the same shape as `FILMS` in `main.rs`: a table hanging off keys the
//! engine already produces, consulted on the way to the screen and nowhere else.
//!
//! # What is *not* translated, on purpose
//!
//! The chart, the mark sheet, the drug names, the investigations and the debrief stay in
//! professional English. That is not laziness — it is how the job is done. A Thai doctor takes
//! the history in Thai and writes the chart in English, and a rubric that pays for "named the
//! diagnosis" has to mean one string in one language or the score stops meaning anything. The
//! patient's voice is local; the record is professional. See `docs/internal/LANGUAGE_LAYER.md`.

use serde_json::{json, Value};

/// How to tell, cheaply, whether a reply actually came back in this language.
///
/// Script only. It cannot separate English from Bahasa Indonesia (one alphabet, and a two-word
/// breathless sentence carries no grammar to go on), so [`Script::Latin`] declines to guess
/// rather than accusing a correct answer of being the wrong language — a false alarm on a good
/// reply is worse than a missed one, because the learner is told their patient misbehaved when
/// she did not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    /// No usable signal. Always accepted.
    Latin,
    /// Thai, U+0E00–U+0E7F.
    Thai,
    /// Kana or Han — for the day 日本語 lands.
    Japanese,
}

/// One language the bay can be played in.
pub struct Language {
    /// The tag the page and the API speak. BCP-47 primary subtag.
    pub id: &'static str,
    /// What the button says — endonym, because a Thai learner looks for "ไทย", not "Thai".
    pub native: &'static str,
    /// What the patient's brief calls this language. It is read by a model, so it is the English
    /// name: "answer in Thai" is an instruction every model understands, "answer in ไทย" is not.
    pub speaks: &'static str,
    /// How a reply in this language can be recognised. See [`Script`].
    pub script: Script,
}

/// Every language on offer. **The first entry is the default and the language the case files are
/// authored in** — everything else is a translation of it.
///
/// Adding one is an edit to this table plus rows in the tables below. Nothing else in the build
/// knows how many languages there are: the page builds its picker from `/api/lang`, the patient's
/// brief reads `speaks`, and every table is keyed by language id. Bahasa Indonesia (promised to
/// Superteam Indonesia as the M2 "language layer") and 日本語 land the same way.
pub const LANGUAGES: &[Language] = &[
    Language { id: "en", native: "English", speaks: "English", script: Script::Latin },
    Language { id: "th", native: "ไทย", speaks: "Thai", script: Script::Thai },
];

/// The language a request asked for, falling back to the default.
///
/// Never fails and never 400s. A language tag is a preference arriving from a browser; an
/// unknown one means somebody's `localStorage` is older than this build, and the answer to that
/// is the case in English, not an error page in the middle of a resuscitation.
pub fn language(id: Option<&str>) -> &'static Language {
    let want = id.unwrap_or_default();
    LANGUAGES.iter().find(|l| l.id == want).unwrap_or(&LANGUAGES[0])
}

/// The language the case files themselves are written in.
pub fn default_language() -> &'static Language {
    &LANGUAGES[0]
}

/// A string with a translation per language. Absent language ⇒ the original shows.
struct Line {
    /// The key. For a beat this is the *canonical rendered beat* — exactly the string
    /// `vitals_sce::render_beat` produces and the leaf hashes — so the table hangs off what the
    /// engine already says rather than off a second identifier somebody has to keep in sync.
    key: &'static str,
    tr: &'static [(&'static str, &'static str)],
}

impl Line {
    fn get(&self, lang: &Language) -> Option<&'static str> {
        self.tr.iter().find(|(l, _)| *l == lang.id).map(|(_, t)| *t)
    }
}

/// ── the beats ────────────────────────────────────────────────────────────────
///
/// Keyed by the canonical beat string. Two kinds live here and they behave differently:
///
///   - **the engine's own beats** (`status:*`, `terminal:*`) are produced by the runtime for
///     every case, so one row serves the whole season;
///   - **a case's scripted beats** (`threshold:…`, `harm:…`) are strings authored inside
///     `demo/**` or `conformance/**`. They are quoted here verbatim as the key. That is the
///     whole trick: the scenario file is never edited, and a beat with no row shows exactly the
///     English the case author wrote.
///
/// **Populated for EP1 only, on purpose.** This round proves the pipe end to end on one case;
/// every other case falls through to its original text, which is a legitimate state and not a
/// gap to be papered over. See `docs/internal/LANGUAGE_LAYER.md` for how to fill in a station.
const BEATS: &[Line] = &[
    // The runtime's own vocabulary. Deliberately ungendered in Thai — the English lines on the
    // page say "she" for every case in the season, including the two whose patient is a man.
    Line { key: "status:Stable", tr: &[("th", "อาการคงที่")] },
    Line { key: "status:Deteriorating", tr: &[("th", "อาการแย่ลง")] },
    Line { key: "status:Critical", tr: &[("th", "อาการวิกฤต")] },
    Line { key: "status:Improving", tr: &[("th", "อาการเริ่มดีขึ้น")] },
    Line { key: "status:Recovered", tr: &[("th", "อาการกลับมาปกติ")] },
    Line { key: "status:Arrest", tr: &[("th", "หัวใจหยุดเต้น")] },
    Line { key: "status:Dead", tr: &[("th", "ผู้ป่วยเสียชีวิต")] },
    Line { key: "terminal:WinDischarge", tr: &[("th", "ผู้ป่วยกลับบ้านได้")] },
    Line { key: "terminal:WinIcu", tr: &[("th", "ผู้ป่วยรอด — ย้ายเข้า ICU")] },
    Line { key: "terminal:DeathArrest", tr: &[("th", "ผู้ป่วยเสียชีวิต")] },
    Line { key: "terminal:DeathBiphasic", tr: &[("th", "ผู้ป่วยเสียชีวิตที่บ้านจากการแพ้ระลอกสอง")] },
    // EP1 — conformance/sce-anaphylaxis-ep1.json. Its whole scripted surface: one threshold beat
    // and the two harms. The patient's *words* in EP1 do not come through here at all — she is
    // played by a model, and the model is told which language to speak in (`patient.rs`).
    Line { key: "threshold:biphasic", tr: &[("th", "อาการแพ้กลับมาอีกระลอก")] },
    Line {
        key: "harm:arrhythmia (IV push 1:1000)",
        tr: &[("th", "หัวใจเต้นผิดจังหวะ — จากการฉีดอะดรีนาลีน 1:1000 เข้าหลอดเลือดดำ")],
    },
    Line {
        key: "harm:stand/walk collapse",
        tr: &[("th", "ให้ผู้ป่วยความดันต่ำลุกยืน — ผู้ป่วยทรุดลง")],
    },
];

/// The display line for one beat, or `None` to show the original.
///
/// `None` for the default language by construction: the page already holds the English wording
/// and there is no reason to send it back over the wire on every tick.
pub fn beat(lang: &Language, key: &str) -> Option<&'static str> {
    if lang.id == default_language().id {
        return None;
    }
    BEATS.iter().find(|l| l.key == key).and_then(|l| l.get(lang))
}

/// ── what the learner asks the patient ────────────────────────────────────────
///
/// The `ask` chips, and only those. Every other chip — the drugs, the labs, the procedures, the
/// differential — stays in professional English, because that is the language the order is
/// written in and the language the rubric pays for. Asking is the one act performed *at* the
/// patient, so asking is the one row of buttons that changes language with her.
///
/// **Keyed by the English phrase, which is what the chip still fires.** The label is a coat over
/// the button; `data-x` and the intervention id underneath it are untouched, so a Thai learner
/// and an English one who press the same chip write byte-identical tapes and earn the same
/// marks. That is the property that makes this safe.
const ASKS: &[Line] = &[
    Line { key: "what happened?", tr: &[("th", "เกิดอะไรขึ้น")] },
    Line { key: "any allergies?", tr: &[("th", "มีประวัติแพ้อะไรไหม")] },
    Line { key: "can you breathe?", tr: &[("th", "หายใจไหวไหม")] },
    Line { key: "when did it start?", tr: &[("th", "เริ่มมีอาการตอนไหน")] },
    Line { key: "do you have an epipen?", tr: &[("th", "มีปากกาฉีดอะดรีนาลีนติดตัวไหม")] },
    Line { key: "any medicines?", tr: &[("th", "กินยาอะไรประจำไหม")] },
    Line { key: "where is the pain?", tr: &[("th", "เจ็บตรงไหน")] },
    Line { key: "does it go anywhere?", tr: &[("th", "เจ็บร้าวไปที่อื่นไหม")] },
    Line { key: "how long?", tr: &[("th", "เป็นมานานแค่ไหน")] },
    Line { key: "do you smoke?", tr: &[("th", "สูบบุหรี่ไหม")] },
    Line { key: "how long has he been like this?", tr: &[("th", "น้องเป็นแบบนี้มานานแค่ไหน")] },
    Line { key: "is he drinking?", tr: &[("th", "น้องยังดื่มน้ำได้ไหม")] },
    Line { key: "any fever?", tr: &[("th", "มีไข้ไหม")] },
    Line { key: "immunisations?", tr: &[("th", "วัคซีนครบตามนัดไหม")] },
    Line { key: "any long flights?", tr: &[("th", "เพิ่งนั่งเครื่องบินนาน ๆ ไหม")] },
    Line { key: "are you on the pill?", tr: &[("th", "กินยาคุมกำเนิดอยู่ไหม")] },
    Line { key: "any leg swelling?", tr: &[("th", "ขาบวมไหม")] },
    Line { key: "what did you eat before this?", tr: &[("th", "ก่อนหน้านี้กินอะไรมา")] },
    Line { key: "can you breathe all right?", tr: &[("th", "หายใจสะดวกดีไหม")] },
    Line { key: "what did you eat today?", tr: &[("th", "วันนี้กินอะไรมาบ้าง")] },
    Line { key: "tell me about the diarrhoea", tr: &[("th", "เล่าเรื่องท้องเสียให้ฟังหน่อย")] },
    Line { key: "did you faint — even for a moment?", tr: &[("th", "มีวูบหรือหมดสติไหม แม้แค่แป๊บเดียว")] },
    Line { key: "any risk factors — smoking, sugar, pressure?", tr: &[("th", "มีปัจจัยเสี่ยงไหม บุหรี่ เบาหวาน ความดัน")] },
    Line { key: "where is the pain — what makes it better?", tr: &[("th", "เจ็บตรงไหน ท่าไหนแล้วดีขึ้น")] },
    Line { key: "does breathing change it?", tr: &[("th", "หายใจแล้วเจ็บเปลี่ยนไปไหม")] },
    Line { key: "any fever or a cold lately?", tr: &[("th", "ช่วงนี้มีไข้หรือเป็นหวัดไหม")] },
    Line { key: "when did the bark start?", tr: &[("th", "เริ่มไอเสียงก้องตอนไหน")] },
    Line { key: "is she drinking?", tr: &[("th", "น้องยังดื่มน้ำได้ไหม")] },
    Line { key: "has she had this before?", tr: &[("th", "น้องเคยเป็นแบบนี้มาก่อนไหม")] },
    Line { key: "are her shots up to date?", tr: &[("th", "วัคซีนของน้องครบตามนัดไหม")] },
    Line { key: "when is it worse?", tr: &[("th", "ช่วงไหนที่อาการแย่ลง")] },
    Line { key: "how often does this happen?", tr: &[("th", "เป็นแบบนี้บ่อยแค่ไหน")] },
    Line { key: "can you finish a sentence?", tr: &[("th", "พูดจบประโยคได้ไหม")] },
    Line { key: "tell me about the cough", tr: &[("th", "เล่าเรื่องอาการไอให้ฟังหน่อย")] },
    Line { key: "any illnesses? do you smoke?", tr: &[("th", "มีโรคประจำตัวไหม สูบบุหรี่ไหม")] },
    Line { key: "what pills do you take every day?", tr: &[("th", "กินยาอะไรประจำทุกวันบ้าง")] },
    Line { key: "how much blood — what colour?", tr: &[("th", "เลือดออกมากแค่ไหน สีอะไร")] },
    Line { key: "any dizziness standing up?", tr: &[("th", "ลุกยืนแล้วเวียนหัวไหม")] },
    Line { key: "what were you doing when it started?", tr: &[("th", "ตอนเริ่มมีอาการกำลังทำอะไรอยู่")] },
    Line { key: "any illnesses — any tablets?", tr: &[("th", "มีโรคประจำตัวไหม กินยาอะไรอยู่ไหม")] },
    Line { key: "how are your legs?", tr: &[("th", "ขาเป็นอย่างไรบ้าง")] },
    Line { key: "how much does she weigh?", tr: &[("th", "น้องหนักกี่กิโลกรัม")] },
    Line { key: "what did she eat?", tr: &[("th", "น้องกินอะไรไป")] },
    Line { key: "any known allergies?", tr: &[("th", "มีประวัติแพ้อะไรไหม")] },
    Line { key: "ask the niece what happened", tr: &[("th", "ถามหลานสาวว่าเกิดอะไรขึ้น")] },
    Line { key: "where does it hurt?", tr: &[("th", "เจ็บตรงไหน")] },
    Line { key: "can you feel your legs?", tr: &[("th", "รู้สึกที่ขาไหม")] },
];

/// ── typed orders that are not in English ─────────────────────────────────────
///
/// A learner who reads Thai buttons will type Thai into the order box, and the scenarios'
/// keyword lists are written by case authors in English with a handful of Thai words scattered
/// through them. This closes the gap **without touching a case file**: a Thai phrase is mapped to
/// the canonical English order, and the scenario's own matcher then decides what that order is.
///
/// Two properties make this safe, and both are tested:
///
///   1. It is consulted **only after** the scenario's own matcher has already declined. It can
///      therefore add recognition and can never override, re-route or shadow what a case author
///      wrote.
///   2. It resolves to an *order phrase*, not to an intervention id. Each station keeps deciding
///      for itself what "give adrenaline" means on its own patient — including deciding that it
///      means nothing, which is what a station with no such intervention already does.
///
/// The learner's own words still go on the tape verbatim, beside the id they resolved to. The
/// tape is the record of what was typed; the id is what replay re-runs. That is the arrangement
/// `Step::Act` was built for — "what lets an order arrive in a language no keyword list covers".
const ORDERS: &[(&str, &str)] = &[
    // Longest match wins, so the IV-push harm route cannot be swallowed by the IM one — but the
    // table is written specific-first anyway, because a reader should not have to know that.
    ("อะดรีนาลีนเข้าเส้น", "adrenaline iv push"),
    ("อะดรีนาลีนเข้าหลอดเลือด", "adrenaline iv push"),
    ("อะดรีนาลีน", "adrenaline im"),
    ("อะดรีนาลิน", "adrenaline im"),
    ("เอพิเนฟริน", "adrenaline im"),
    ("ออกซิเจน", "oxygen"),
    ("น้ำเกลือ", "normal saline bolus"),
    ("สารน้ำ", "normal saline bolus"),
    ("เปิดเส้น", "iv access"),
    ("นอนราบ", "lay her flat, legs up"),
    ("ยกขาสูง", "lay her flat, legs up"),
    ("ใส่ท่อช่วยหายใจ", "intubate, secure the airway"),
    ("ฟังปอด", "listen to the chest"),
    ("ฟังเสียงหัวใจ", "listen to the heart"),
    ("ดูผิวหนัง", "look at the skin"),
    ("เอกซเรย์ปอด", "chest x-ray"),
    ("เอกซเรย์ทรวงอก", "chest x-ray"),
    ("คลื่นไฟฟ้าหัวใจ", "12-lead ecg"),
    ("อีเคจี", "12-lead ecg"),
    ("แอสไพริน", "aspirin 300 chewed"),
    ("เดกซาเมทาโซน", "dexamethasone syrup"),
    ("ซัลบูทามอล", "salbutamol neb"),
    ("เฮพาริน", "heparin"),
    ("ให้เลือด", "transfuse packed cells"),
    ("ยาปฏิชีวนะ", "broad-spectrum antibiotics now"),
    ("ช็อกไฟฟ้า", "defibrillate"),
    ("รับไว้ในโรงพยาบาล", "admit"),
    ("ให้กลับบ้าน", "discharge home"),
];

/// The canonical English order a non-English phrase names, if this layer knows one.
///
/// Containment rather than equality, matching what the scenario matcher itself does — a learner
/// types "ให้ออกซิเจน 10 ลิตร", not a dictionary headword. The **longest** key that appears wins,
/// so a more specific phrase is never eaten by a prefix of itself.
pub fn canonical_order(text: &str) -> Option<&'static str> {
    let t = vitals_sce::text::canon(text).to_lowercase();
    let mut best: Option<(usize, &'static str)> = None;
    let mut consider = |k: &'static str, v: &'static str| {
        if t.contains(&vitals_sce::text::canon(k).to_lowercase())
            && best.is_none_or(|(n, _)| k.len() > n)
        {
            best = Some((k.len(), v));
        }
    };
    for (k, v) in ORDERS {
        consider(k, v);
    }
    // Typing back what a chip says is an order too — the button's own label, in whatever
    // language it was wearing when the learner read it.
    for line in ASKS {
        for (_, label) in line.tr {
            consider(label, line.key);
        }
    }
    best.map(|(_, v)| v)
}

/// Does this reply look like it came back in the language that was asked for?
///
/// A soft check, and only ever used to add a note beside an answer that is shown anyway. The
/// patient is played by a model; a model that answers in the wrong language is a disappointment,
/// not an error, and swallowing her only answer would be the worse failure of the two.
pub fn reply_is_in(lang: &Language, reply: &str) -> bool {
    match lang.script {
        Script::Latin => true,
        Script::Thai => reply.chars().any(|c| ('\u{0E00}'..='\u{0E7F}').contains(&c)),
        Script::Japanese => reply.chars().any(|c| {
            ('\u{3040}'..='\u{30FF}').contains(&c) || ('\u{4E00}'..='\u{9FFF}').contains(&c)
        }),
    }
}

/// ── the page's own furniture ─────────────────────────────────────────────────
///
/// The few strings the page says *in the patient's voice or about her*, plus the three seals an
/// exam draws over words it will not say yet: the two input placeholders, the stand-in for a harm
/// line, the two stand-ins for a beat the station is holding back until the bell, and the note
/// beside an answer that came back in the wrong language.
///
/// The seals are here rather than left to the page's English fallback for the reason the harm
/// seal already was: a seal is the one thing on screen during an exam where falling back to
/// English is not a cosmetic loss. A candidate who has switched the whole bedside into Thai and
/// then reads one line in English has been told, by the language change alone, that this line is
/// not the case talking — which is a tell about what was sealed. All three translate together or
/// the seal is its own signal.
///
/// Everything else on screen — the chart, the mark sheet, the monitor, the debrief, the buttons —
/// is deliberately absent. Translating the whole interface is a different project with a
/// different reviewer (a clinician who marks in that language), and shipping half of it would
/// leave a screen that is neither one language nor the other.
const UI: &[Line] = &[
    Line { key: "ask_placeholder", tr: &[("th", "ถามคนไข้ได้เลย…")] },
    Line { key: "order_placeholder", tr: &[("th", "หรือพิมพ์คำสั่งเอง…")] },
    // `harm_sealed` used to sit here — "⚠ harm recorded", drawn over a harm line while the
    // clock ran. The line it was drawn over is gone: a sealed run carries no harm beat and no
    // harm row, because a marker landing the instant a candidate acts is the verdict itself
    // whatever words are on it. A seal with nothing left to seal is a row nothing reads, and the
    // test below is the reason it goes rather than lingers.
    // The two beat seals. Deliberately flat: "declined" and "noted" are facts about the
    // record, and any warmer wording would begin explaining — which is the whole of what is
    // being held back. They are the page's BEAT_DECLINED / BEAT_NOTED, which read them off
    // PACK.ui and fell back to English because these two rows did not exist yet.
    Line { key: "beat_declined", tr: &[("th", "คำสั่งนี้ถูกปฏิเสธ")] },
    Line { key: "beat_noted", tr: &[("th", "บันทึกไว้ในระเบียนแล้ว")] },
    Line { key: "off_language", tr: &[("th", "— คนไข้ตอบกลับมาเป็นภาษาอื่น")] },
    Line { key: "picker_label", tr: &[("th", "ภาษาที่คนไข้พูด")] },
    // ── ending the attempt ───────────────────────────────────────────────────
    // The one control in the bay that cannot be taken back, and the three lines around it: what
    // it does, what the second press does, and what the feed says the moment it fires. Worded
    // to say that the case goes on rather than stops, because that is the whole difference
    // between this and an abandon — and a candidate who thinks they are freezing the patient
    // will use it as if they were.
    Line { key: "end_note", tr: &[("th", "จบการสอบสถานีนี้ เคสจะเดินต่อจากจุดนี้แล้วคิดคะแนน")] },
    Line { key: "end_confirm", tr: &[("th", "กดอีกครั้งเพื่อจบ")] },
    Line { key: "end_warn", tr: &[("th", "เคสจะเดินต่อจากจุดที่คุณหยุด — ย้อนกลับไม่ได้")] },
    Line { key: "time_called", tr: &[("th", "หมดเวลา — จบสถานี")] },
];

/// ── the kit ──────────────────────────────────────────────────────────────────
///
/// The device tray's own words: what each item is called, the line underneath that says what it
/// actually is, and the question the two irreversible ones ask before they happen.
///
/// This is the [`ASKS`] argument, not the chart's. Attaching oxygen is an act performed *at* the
/// patient, at her bedside, by hand — the same category as asking her a question — and a learner
/// who reaches for the flowmeter in Thai should read the flowmeter in Thai. What it is **not** is
/// a second rulebook: exactly as with the chips, a translated label *relabels and does not
/// re-fire*. The device id, the phrase `kit_phrase` mints, the intervention it matches and the
/// text that lands on the tape are all untouched, so a Thai learner and an English one who attach
/// the same device at the same setting write byte-identical tapes.
///
/// Keys are `<device id>.<field>`, and the device ids are the engine's own — `o2`, `iv`, `ett`,
/// `supine`, `defib`. A row with no translation shows the English the page already holds.
///
/// The Thai is Embla's, lifted verbatim from its `device_catalogue()`. Those words have been read
/// by real candidates in a real faculty; a fresh translation of a clinical label is a new thing to
/// get wrong, and "ทำครั้งเดียว ไม่ใช่อุปกรณ์" is a distinction a bedside actually has to make.
///
/// Deliberately absent: the tray's short name (`device` in the page's `KIT`) and the setting's
/// unit. Those sit in the chart's column, beside the readings, and the chart is English — see
/// `docs/internal/LANGUAGE_LAYER.md`.
///
/// Nothing here is case-specific. The catalogue is the same five items in every station, so a
/// label can never hint at what *this* patient needs — which is the property exam mode depends on.
const KIT: &[Line] = &[
    Line { key: "o2.label", tr: &[("th", "ออกซิเจน")] },
    // Reads as teaching, and is the reason the presets are 2/4/6/10/15 rather than a slider:
    // the numbers are the devices a hand actually reaches for.
    Line { key: "o2.detail", tr: &[("th", "cannula 2–4 · mask 6–8 · NRB 10–15")] },
    Line { key: "iv.label", tr: &[("th", "เปิดเส้น + สารน้ำ")] },
    // The fluid keeps its label: "0.9% NaCl" is what is printed on the bag on the trolley.
    Line { key: "iv.detail", tr: &[("th", "0.9% NaCl")] },
    Line { key: "ett.label", tr: &[("th", "ใส่ท่อช่วยหายใจ")] },
    Line { key: "ett.detail", tr: &[("th", "ETT 7.0 · cuffed")] },
    Line { key: "ett.confirm", tr: &[("th", "ใส่ท่อช่วยหายใจตอนนี้?")] },
    Line { key: "supine.label", tr: &[("th", "จัดท่านอนราบ ยกขา")] },
    Line { key: "supine.detail", tr: &[("th", "ทำครั้งเดียว ไม่ใช่อุปกรณ์")] },
    // Untranslated on purpose, and Embla makes the same call: the verb a Thai resus team shouts
    // is the English one.
    Line { key: "defib.label", tr: &[("th", "Defibrillate")] },
    Line { key: "defib.detail", tr: &[("th", "shock ได้เฉพาะ VF / pulseless VT")] },
    Line { key: "defib.confirm", tr: &[("th", "ปล่อยกระแสไฟฟ้า?")] },
];

/// What `/api/lang` hands the page.
///
/// `languages` is always the whole list, so the picker is built from the server's table and no
/// count of languages is hard-coded in the page. The rest is the pack for one language, and it is
/// **empty for the default** — the page already holds the English wording, and an empty pack is
/// what "show the original" looks like on the wire.
///
/// The kit table is safe to hand over whole for the reason a beat is not: the catalogue is the
/// same five devices in every station, so it says nothing about the case on screen.
///
/// Deliberately *not* in here: any case's scripted beat. Those arrive per-run through the view,
/// one line at a time, as the run earns them. A pack containing every beat of every case would be
/// an answer key served to anybody who opened devtools during an exam — the harm text alone names
/// the drug, the disease and the deadline. Exam integrity is why this endpoint is thin.
pub fn pack(lang: &Language) -> Value {
    let list: Vec<Value> = LANGUAGES
        .iter()
        .map(|l| json!({ "id": l.id, "native": l.native }))
        .collect();
    let table = |t: &'static [Line]| -> Value {
        let mut m = serde_json::Map::new();
        for line in t {
            if let Some(v) = line.get(lang) {
                m.insert(line.key.to_string(), Value::String(v.to_string()));
            }
        }
        Value::Object(m)
    };
    json!({
        "lang": lang.id,
        "languages": list,
        "asks": table(ASKS),
        "ui": table(UI),
        "kit": table(KIT),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nothing may hard-code two languages. Every table is keyed by language id and every
    /// language in the table gets a pack — which is the whole of what "add Bahasa Indonesia" has
    /// to touch. A third row landing in [`LANGUAGES`] must not need a code change anywhere else.
    #[test]
    fn the_table_is_the_only_place_that_knows_how_many_languages_there_are() {
        assert!(LANGUAGES.len() >= 2, "the picker needs something to pick between");
        for l in LANGUAGES {
            let p = pack(l);
            assert_eq!(p["lang"], l.id);
            assert_eq!(
                p["languages"].as_array().map(Vec::len),
                Some(LANGUAGES.len()),
                "{}: the picker must see every language, not just its own",
                l.id
            );
            assert!(!l.native.is_empty() && !l.speaks.is_empty(), "{} is unlabelled", l.id);
        }
        // Ids are what localStorage and the API agree on; two of the same would make the
        // preference ambiguous.
        for (i, a) in LANGUAGES.iter().enumerate() {
            assert!(
                !LANGUAGES[..i].iter().any(|b| b.id == a.id),
                "{} appears twice",
                a.id
            );
        }
    }

    /// The default language is the language the case files are written in, so it has nothing to
    /// translate: an empty pack and no beat lines. This is what keeps the English build byte-for-
    /// byte what it was before the language layer existed.
    #[test]
    fn the_default_language_adds_nothing_to_the_wire() {
        let en = default_language();
        let p = pack(en);
        assert!(p["asks"].as_object().is_some_and(serde_json::Map::is_empty));
        assert!(p["ui"].as_object().is_some_and(serde_json::Map::is_empty));
        assert!(p["kit"].as_object().is_some_and(serde_json::Map::is_empty));
        for l in BEATS {
            assert_eq!(beat(en, l.key), None, "{} was translated into the original", l.key);
        }
    }

    /// A device label may never invent a device. The keys are `<id>.<field>` and the ids are the
    /// engine's — if one drifts, the page silently falls back to English and nobody notices until
    /// a Thai learner sees one item in the wrong language.
    ///
    /// `kit_phrase` in `main.rs` is the other half of this pair: these five are exactly the
    /// devices it knows how to mint a phrase for.
    #[test]
    fn every_kit_label_names_a_device_the_engine_has() {
        const DEVICES: &[&str] = &["o2", "iv", "ett", "supine", "defib"];
        const FIELDS: &[&str] = &["label", "detail", "confirm"];
        for line in KIT {
            let (id, field) = line.key.split_once('.').unwrap_or_else(|| {
                panic!("{} is not <device>.<field>", line.key)
            });
            assert!(DEVICES.contains(&id), "{id} is not a device the engine has");
            assert!(FIELDS.contains(&field), "{field} is not a label the tray draws");
        }
        // Every device the tray can open needs a name and a line under it, or switching language
        // leaves the sheet half translated.
        for d in DEVICES {
            for f in ["label", "detail"] {
                let key = format!("{d}.{f}");
                assert!(KIT.iter().any(|l| l.key == key), "{key} is missing");
            }
        }
    }

    /// The catalogue is the same in every station, so no label can name the case. A row that
    /// mentioned a diagnosis or a drug would be an answer key drawn on the tray in exam mode,
    /// where even the harm line is sealed.
    #[test]
    fn no_device_label_gives_a_case_away() {
        const TELLS: &[&str] = &[
            "anaphylax", "แพ้", "asthma", "หอบ", "sepsis", "pneumonia", "croup",
            "embolism", "infarct", "stemi", "adrenaline", "steroid",
        ];
        for line in KIT {
            for (_, text) in line.tr {
                let t = text.to_lowercase();
                for tell in TELLS {
                    assert!(!t.contains(tell), "{}: {text:?} names the case", line.key);
                }
            }
        }
    }

    /// An unknown or absent tag is a preference this build does not have, not an error. It plays
    /// in the language the case was written in.
    #[test]
    fn an_unknown_language_falls_back_rather_than_failing() {
        for bad in [None, Some(""), Some("kl"), Some("th-TH"), Some("../../etc/passwd")] {
            assert_eq!(language(bad).id, default_language().id, "{bad:?} did not fall back");
        }
        assert_eq!(language(Some("th")).id, "th");
    }

    /// A beat with no row shows the original. That is the design, not a hole: every case except
    /// EP1 is in exactly this state today, and none of them may break because of it.
    #[test]
    fn a_beat_with_no_translation_asks_for_the_original() {
        let th = language(Some("th"));
        assert!(beat(th, "threshold:biphasic").is_some(), "EP1's own beat is the worked example");
        for untranslated in [
            "threshold:steeple sign on the neck film",
            "harm:no steroid by the seventh minute",
            "threshold:— \"Shrimp. Since I was young.\"",
            "",
        ] {
            assert_eq!(beat(th, untranslated), None, "{untranslated} claimed a translation");
        }
    }

    /// The order alias resolves to an English *order*, never to an intervention id — the station
    /// keeps deciding what that order does on its own patient.
    #[test]
    fn a_thai_order_becomes_the_english_order_and_the_longest_match_wins() {
        assert_eq!(canonical_order("ให้ออกซิเจน 10 ลิตร"), Some("oxygen"));
        assert_eq!(canonical_order("ฉีดอะดรีนาลีนเข้ากล้าม"), Some("adrenaline im"));
        // The specific phrase must not be eaten by the prefix it contains.
        assert_eq!(canonical_order("อะดรีนาลีนเข้าเส้น 1:1000"), Some("adrenaline iv push"));
        // Typing what the button said is an order too.
        assert_eq!(canonical_order("มีประวัติแพ้อะไรไหม"), Some("any allergies?"));
        // English is not this table's business — the scenario matcher already had it.
        assert_eq!(canonical_order("adrenaline im"), None);
        assert_eq!(canonical_order(""), None);
        assert_eq!(canonical_order("ยาหอม"), None);
    }

    /// Every alias points at a phrase the English chips already fire, so nothing here invents an
    /// order no case was written to understand. Ask aliases point back at their own key.
    #[test]
    fn every_alias_names_an_order_a_case_could_recognise() {
        for (k, v) in ORDERS {
            assert!(!k.is_empty() && !v.is_empty(), "{k} → {v} is half a row");
            assert!(v.is_ascii(), "{v} is not the canonical English order");
        }
        // The page is the authority on what an ask chip says, and this table only puts a coat on
        // one. A key that no chip fires is a translation nobody will ever see; a chip phrase
        // renamed in the page and not here silently reverts to English. Both are caught by
        // reading the page itself rather than by keeping a second list in a comment.
        let page = include_str!("../static/index.html");
        for line in ASKS {
            assert!(
                page.contains(&format!("'{}'", line.key)),
                "{} is translated and no chip in the page fires it",
                line.key
            );
            for (l, label) in line.tr {
                assert!(LANGUAGES.iter().any(|x| x.id == *l), "{} translates into no language", line.key);
                assert!(!label.is_empty(), "{} has an empty {l} label", line.key);
            }
        }
    }

    /// Every row in [`UI`] is a string the page actually asks for, and every string the page
    /// asks for has a row.
    ///
    /// Both halves fail silently, which is why they are pinned rather than reviewed: a key
    /// nobody reads is a translation that never appears, and a `PACK.ui.x` with no row shows the
    /// English fallback for ever. That second one is exactly how `beat_declined` and
    /// `beat_noted` shipped — the page was wired for them and this table had never heard of
    /// them, so the Thai bedside sealed a beat in English and looked completely normal doing it.
    #[test]
    fn every_ui_string_is_one_the_page_asks_for_and_every_one_it_asks_for_is_here() {
        // One row this table carries that nothing on screen reads: the language picker is two
        // bare <select>s with no caption, in the lightbox and the game bar, so `picker_label` is
        // a string translated for a label that was never drawn. Named here rather than deleted —
        // deleting it is a decision about the picker's design, not about this table — so that it
        // is a recorded hole and not a silent one, and so a *second* dead row cannot hide behind
        // the first.
        const UNWIRED: &[&str] = &["picker_label"];
        let page = include_str!("../static/index.html");
        for line in UI {
            assert!(
                page.contains(&format!("PACK.ui.{}", line.key))
                    || UNWIRED.contains(&line.key),
                "{} is translated and the page never reads it",
                line.key
            );
            for (l, text) in line.tr {
                assert!(LANGUAGES.iter().any(|x| x.id == *l), "{}: no such language {l}", line.key);
                assert!(!text.is_empty(), "{} has an empty {l} line", line.key);
            }
        }
        // The other direction, off the page itself, so a new `PACK.ui.` call site cannot be
        // added without a row to answer it.
        let mut asked: Vec<&str> = Vec::new();
        for (i, _) in page.match_indices("PACK.ui.") {
            let rest = &page[i + "PACK.ui.".len()..];
            let n = rest.find(|c: char| !c.is_ascii_alphanumeric() && c != '_').unwrap_or(0);
            if n > 0 && !asked.contains(&&rest[..n]) {
                asked.push(&rest[..n]);
            }
        }
        assert!(!asked.is_empty(), "the page stopped reading the pack at all");
        for key in UNWIRED {
            assert!(
                !asked.contains(key),
                "{key} is wired up now — take it out of UNWIRED so the guard is whole again"
            );
        }
        for key in asked {
            assert!(
                UI.iter().any(|l| l.key == key),
                "the page reads PACK.ui.{key} and no row answers it — it is English for ever"
            );
        }
    }

    /// The script check accepts a good answer and only flags one that is plainly in the wrong
    /// alphabet — and it never flags a language whose script it cannot read.
    #[test]
    fn the_wrong_language_check_only_fires_when_it_can_actually_tell() {
        let th = language(Some("th"));
        assert!(reply_is_in(th, "หายใจไม่ออกเลยค่ะ"));
        assert!(!reply_is_in(th, "I can't breathe."));
        // Latin has no signal, so it accepts everything rather than guessing.
        let en = default_language();
        assert!(reply_is_in(en, "I can't breathe."));
        assert!(reply_is_in(en, "หายใจไม่ออกเลยค่ะ"));
    }

    /// A beat key is a *rendered beat*, not a free-form label. Anything else silently never
    /// matches, which is the one failure mode of this table that would be invisible on screen.
    #[test]
    fn every_beat_key_is_shaped_like_something_the_engine_actually_says() {
        const KINDS: &[&str] = &["status:", "threshold:", "harm:", "terminal:"];
        for l in BEATS {
            assert!(
                KINDS.iter().any(|k| l.key.starts_with(k)),
                "{} is not a rendered beat — render_beat can never produce it",
                l.key
            );
            for (lg, text) in l.tr {
                assert!(LANGUAGES.iter().any(|x| x.id == *lg), "{}: no such language {lg}", l.key);
                assert!(!text.is_empty(), "{} has an empty {lg} line", l.key);
            }
        }
    }
}
