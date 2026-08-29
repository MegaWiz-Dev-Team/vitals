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
///
/// # A region is not a different language
///
/// The tag is matched on its **primary subtag, case-insensitively**, so `th`, `th-TH`, `th_TH`
/// and `TH` all reach Thai. This used to be an exact match on the whole string, and the three
/// places a tag actually comes from are not the picker: a hand-typed URL, a browser handing over
/// its `navigator.language` (which is `th-TH` on a Thai phone, never bare `th`), and a
/// `localStorage` value written by an older build. Every one of those put a Thai speaker in
/// front of an English patient with a full Thai pack sitting right here, silently — the failure
/// looked like a patient who would not speak Thai rather than like a tag nobody parsed.
///
/// The underscore is in there because `th_TH` is what a POSIX locale looks like, and a tag
/// copied out of one reaches this function often enough to be worth a character.
///
/// What has *not* changed is the answer for a tag this build genuinely has no pack for: `kl`
/// falls back to the language the cases are authored in, and [`pack`] hands back no table rather
/// than a table of blanks. Matching wider only ever moves a tag from "no pack" to a pack that
/// exists; it never invents one.
///
/// Note the returned language's `id` is the canonical one (`th`, not `th-TH`), and `/api/lang`
/// echoes it as `lang` — which the page adopts — so a stale `localStorage` heals on first load
/// rather than staying a variant for ever.
pub fn language(id: Option<&str>) -> &'static Language {
    // Primary subtag only: everything from the first separator on is region, script or variant,
    // and none of those change which pack answers.
    let want = id.unwrap_or_default().split(['-', '_']).next().unwrap_or_default();
    LANGUAGES
        .iter()
        .find(|l| l.id.eq_ignore_ascii_case(want))
        .unwrap_or(&LANGUAGES[0])
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
/// **Whole, and kept whole by a test.** Every scripted beat of every case this server can play
/// has a Thai row, because the hole this table used to have was not a cosmetic one: a candidate
/// who had switched the bedside into Thai pressed an `ask` chip — the fastest control in the bay
/// — and the patient answered in English. `every_scripted_beat_of_every_case_has_a_thai_line`
/// reads `demo/**` and `conformance/**` off disk and fails if a case gains a beat this table has
/// never heard of, so the next station cannot ship half-translated.
///
/// # What the Thai is, and is not
///
/// Most of these lines are two things at once — words the patient says, in quotes, and a
/// sentence of what the examiner would see — and they stay two things. The quoted half is lay
/// Thai in the register of *that* person: a blunt 71-year-old man, an anxious grandmother, a
/// mother answering for a three-year-old, a schoolteacher who is not the parent. The unquoted
/// half is the examiner's observation and is written in the register a Thai clinician writes
/// notes in, which is Thai carrying English clinical terms — `SpO2`, `ECG`, `PEFR`, `Westley`,
/// `CURB-65`, `tamponade`, and every drug name. Translating those is what makes a translated
/// note read as machine output; a Thai ward does not say them in Thai either.
///
/// **A translation may not be clearer than its original.** Several of these beats are diagnostic
/// *because* of what the patient does not say — the hesitation in "…a seafood buffet … only a
/// little", the blackout that Somsri calls sitting down, the pain Tan can only describe by the
/// chair it prefers. A tidier Thai sentence hands the candidate a diagnosis the English
/// withholds, which is an exam-integrity defect and not a translation improvement. Where the
/// vagueness looked deliberate it was kept, hedge for hedge and ellipsis for ellipsis.
///
/// Arabic numerals throughout, including inside Thai sentences: `3`, never `๓`.
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

    // ── OSCE-A · Somchai, 71, blunt and impatient — anaphylaxis after a seafood buffet ──
    Line {
        key: "threshold:— \"Shrimp. Since I was young. The doctor told me to stay off seafood.\"",
        tr: &[("th", "— \"กุ้ง เป็นมาตั้งแต่เด็ก หมอสั่งห้ามกินอาหารทะเล\"")],
    },
    Line {
        key: "threshold:— \"Tight. I can hear myself whistle.\" A wheeze you can hear from the door.",
        tr: &[("th", "— \"แน่น ๆ ได้ยินเสียงหวีดของตัวเอง\" เสียงวี้ดที่ได้ยินตั้งแต่หน้าประตู")],
    },
    Line {
        key: "threshold:— \"…a seafood buffet, with the old crowd. Only a little.\" It started thirty minutes after.",
        tr: &[("th", "— \"…บุฟเฟต์ซีฟู้ด ไปกับเพื่อนเก่า กินไปนิดเดียวเอง\" อาการเริ่มหลังจากนั้น 30 นาที")],
    },
    Line {
        key: "threshold:wheals over both arms, the neck, the face — forehead and right cheek swelling",
        tr: &[("th", "ผื่นลมพิษขึ้นทั้งสองแขน ลำคอ และใบหน้า — หน้าผากและแก้มขวาบวม")],
    },
    Line {
        key: "threshold:expiratory wheeze, both sides",
        tr: &[("th", "เสียงวี้ดตอนหายใจออก ทั้งสองข้าง")],
    },
    Line {
        key: "threshold:adrenaline 0.5 mg im, outer thigh — no hesitation",
        tr: &[("th", "adrenaline 0.5 mg IM ที่ต้นขาด้านนอก — ไม่ลังเล")],
    },
    Line {
        key: "harm:iv push adrenaline — arrhythmia on a beating heart",
        tr: &[("th", "ดัน adrenaline เข้าหลอดเลือดดำ — หัวใจที่ยังเต้นอยู่เสียจังหวะ")],
    },
    Line {
        key: "threshold:antihistamine for the itch — it will not hold the pressure",
        tr: &[("th", "ให้ antihistamine แก้คัน — แต่มันดึงความดันไว้ไม่ได้")],
    },
    Line {
        key: "threshold:steroid on board — for the late phase, not for now",
        tr: &[("th", "ให้ steroid ไปแล้ว — เผื่อระลอกหลัง ไม่ใช่สำหรับตอนนี้")],
    },
    Line {
        key: "threshold:tryptase 28.4 (ref < 11.4) — the mast cells have spoken",
        tr: &[("th", "tryptase 28.4 (อ้างอิง < 11.4) — mast cell ได้พูดแล้ว")],
    },
    Line {
        key: "threshold:sinus tachycardia 118 — no ischaemia",
        tr: &[("th", "sinus tachycardia 118 — ไม่มี ischaemia")],
    },
    Line {
        key: "threshold:wbc 11.2, eosinophils up — nothing surgical",
        tr: &[("th", "WBC 11.2, eosinophil สูงขึ้น — ไม่ใช่เรื่องที่ต้องผ่าตัด")],
    },
    Line {
        key: "threshold:clear fields, normal heart shadow",
        tr: &[("th", "ปอดโล่งทั้งสองข้าง เงาหัวใจปกติ")],
    },
    Line {
        key: "threshold:anaphylaxis — named for what it is",
        tr: &[("th", "anaphylaxis — เรียกชื่อได้ตรงตามที่มันเป็น")],
    },
    Line {
        key: "threshold:kept under the nurses' eyes — the second wave is real",
        tr: &[("th", "รับไว้ให้พยาบาลเฝ้าดู — ระลอกสองเกิดขึ้นได้จริง")],
    },
    Line {
        key: "harm:discharged during the observation window — biphasic reactions return to an empty bed",
        tr: &[("th", "ให้กลับบ้านทั้งที่ยังอยู่ในช่วงเฝ้าสังเกตอาการ — biphasic reaction กลับมาตอนที่เตียงว่างแล้ว")],
    },
    Line {
        key: "harm:adrenaline delayed — anaphylaxis is treated the moment it is named",
        tr: &[("th", "ให้ adrenaline ช้าเกินไป — anaphylaxis ต้องรักษาทันทีที่เรียกชื่อมันได้")],
    },

    // ── OSCE-A2 · Somsri, 68, anxious and talkative — anaphylaxis wearing a gut story ──
    Line {
        key: "threshold:— \"Peanuts. Badly, since I was a girl — it is written in my old hospital book.\" She scratches as she says it.",
        tr: &[("th", "— \"ถั่วลิสงค่ะ แพ้หนักด้วย เป็นมาตั้งแต่เด็ก — จดไว้ในสมุดโรงพยาบาลเล่มเก่าแล้ว\" เธอเกาไปพูดไป")],
    },
    Line {
        key: "threshold:— \"Somtam from my daughter's stall. There could have been ground peanut on top — it all started so fast after.\"",
        tr: &[("th", "— \"ส้มตำจากร้านของลูกสาวค่ะ ข้างบนอาจจะมีถั่วป่นโรยอยู่ — พอกินแล้วอาการมาเร็วมาก\"")],
    },
    Line {
        key: "threshold:— \"Three times in an hour, and the cramps twist like a wrung cloth.\" The gut is a shock organ too.",
        tr: &[("th", "— \"ชั่วโมงเดียวถ่ายไป 3 ครั้ง ปวดบิดเหมือนโดนบิดผ้า\" ลำไส้ก็เป็นอวัยวะที่บอกภาวะช็อกได้เหมือนกัน")],
    },
    Line {
        key: "threshold:— \"…I sat down hard for a moment. On the floor. Don't tell my daughter.\" A blackout she calls sitting down.",
        tr: &[("th", "— \"…แค่ทรุดนั่งลงแป๊บเดียวเองค่ะ ลงไปกับพื้น อย่าบอกลูกสาวนะ\" การหมดสติที่เธอเรียกว่าการนั่งลง")],
    },
    Line {
        key: "threshold:pink wheals broader than a thumbnail, everywhere — and the face swelling: forehead, both cheeks, the nose",
        tr: &[("th", "ผื่นลมพิษสีชมพู ใหญ่กว่าเล็บหัวแม่มือ ขึ้นทั่วตัว — และหน้าบวม: หน้าผาก แก้มทั้งสองข้าง จมูก")],
    },
    Line {
        key: "threshold:expiratory wheeze both sides, and she is working for it",
        tr: &[("th", "เสียงวี้ดตอนหายใจออกทั้งสองข้าง และเธอต้องออกแรงหายใจ")],
    },
    Line {
        key: "threshold:adrenaline 0.5 mg im, outer thigh — the itch was a systemic illness all along",
        tr: &[("th", "adrenaline 0.5 mg IM ที่ต้นขาด้านนอก — อาการคันนั้นเป็นโรคทั้งระบบมาตั้งแต่ต้น")],
    },
    Line {
        key: "threshold:a litre of warmed saline wide open — the shock gets volume, not just adrenaline",
        tr: &[("th", "saline อุ่น 1 ลิตร เปิดเต็มที่ — ภาวะช็อกต้องการสารน้ำ ไม่ใช่แค่ adrenaline")],
    },
    Line {
        key: "threshold:glucose 7.2 on a diabetic chart — the sugar is not the story tonight",
        tr: &[("th", "glucose 7.2 ในชาร์ตของคนไข้เบาหวาน — คืนนี้น้ำตาลไม่ใช่ประเด็น")],
    },
    Line {
        key: "threshold:tryptase 41.6 (ref < 11.4) — the mast cells shouting",
        tr: &[("th", "tryptase 41.6 (อ้างอิง < 11.4) — mast cell ตะโกนออกมา")],
    },
    Line {
        key: "threshold:sinus tachycardia 124 — no ischaemia behind the collapse",
        tr: &[("th", "sinus tachycardia 124 — ไม่มี ischaemia อยู่เบื้องหลังการทรุด")],
    },
    Line {
        key: "threshold:antihistamine for the itch — it will not hold a falling pressure",
        tr: &[("th", "ให้ antihistamine แก้คัน — แต่มันดึงความดันที่กำลังตกไว้ไม่ได้")],
    },
    Line {
        key: "threshold:anaphylaxis — skin plus gut plus a pressure of 86: two systems and a collapse",
        tr: &[("th", "anaphylaxis — ผิวหนัง บวกทางเดินอาหาร บวกความดัน 86: สองระบบและการทรุด")],
    },
    Line {
        key: "threshold:food poisoning? — poisoning does not swell a face or drop a pressure in forty minutes. count the systems.",
        tr: &[("th", "อาหารเป็นพิษ? — อาหารเป็นพิษไม่ทำให้หน้าบวมหรือความดันตกภายใน 40 นาที ลองนับระบบที่เกี่ยวข้องดูใหม่")],
    },
    Line {
        key: "threshold:admitted under the nurses' eyes — the second wave finds an empty corridor",
        tr: &[("th", "รับไว้ให้พยาบาลเฝ้าดู — ระลอกสองมาเจอแต่ทางเดินที่ว่างเปล่า")],
    },
    Line {
        key: "harm:antihistamine first while the pressure fell — the itch was never the emergency",
        tr: &[("th", "ให้ antihistamine ก่อนทั้งที่ความดันกำลังตก — อาการคันไม่เคยเป็นภาวะฉุกเฉิน")],
    },

    // ── OSCE-B · Somchai Jaidee, 25, frightened — STEMI ──
    Line {
        key: "threshold:— \"Like something sitting on my chest. It shoots down both arms, up to my throat.\" Six out of ten, sudden.",
        tr: &[("th", "— \"เหมือนมีอะไรมานั่งทับหน้าอกครับ มันแล่นลงแขนทั้งสองข้าง แล้วขึ้นมาถึงคอ\" 6 เต็ม 10 เกิดขึ้นทันทีทันใด")],
    },
    Line {
        key: "threshold:— \"I smoke a lot. Sugar, cholesterol, pressure — all of it. My father's heart gave out.\"",
        tr: &[("th", "— \"ผมสูบบุหรี่จัดครับ เบาหวาน ไขมัน ความดัน — มีหมดเลย พ่อผมก็เสียเพราะโรคหัวใจ\"")],
    },
    Line {
        key: "threshold:normal s1 s2 — a clean exam clears nothing here",
        tr: &[("th", "เสียงหัวใจ S1 S2 ปกติ — ตรวจร่างกายที่ปกติไม่ได้ตัดอะไรออกในเคสนี้")],
    },
    Line {
        key: "threshold:fast, regular, equal both sides",
        tr: &[("th", "ชีพจรเร็ว สม่ำเสมอ เท่ากันทั้งสองข้าง")],
    },
    Line {
        key: "threshold:st elevation on the screen — the ecg tells the truth",
        tr: &[("th", "ST elevation ขึ้นบนจอ — ECG บอกความจริง")],
    },
    Line {
        key: "threshold:troponin drawn — treat the ecg, not the lab clock",
        tr: &[("th", "เจาะ troponin แล้ว — รักษาตาม ECG ไม่ใช่รอนาฬิกาของแล็บ")],
    },
    Line {
        key: "threshold:unremarkable film — nothing else to blame",
        tr: &[("th", "ฟิล์มไม่พบความผิดปกติ — ไม่มีอย่างอื่นให้โทษ")],
    },
    Line {
        key: "threshold:300 mg chewed — cheap minutes of muscle",
        tr: &[("th", "เคี้ยว 300 mg — ซื้อกล้ามเนื้อหัวใจได้อีกหลายนาที ในราคาถูก")],
    },
    Line { key: "threshold:line in", tr: &[("th", "เปิดเส้นแล้ว")] },
    Line {
        key: "threshold:cath lab activated — door-to-balloon clock running",
        tr: &[("th", "เรียก cath lab แล้ว — นาฬิกา door-to-balloon เริ่มเดิน")],
    },
    Line {
        key: "threshold:the lab wants an ecg before it spins up",
        tr: &[("th", "cath lab ขอดู ECG ก่อนถึงจะเริ่มเตรียมทีม")],
    },
    Line {
        key: "threshold:lytic running — reperfusion, the second-best way",
        tr: &[("th", "ยาละลายลิ่มเลือดกำลังหยด — reperfusion ด้วยทางที่ดีเป็นอันดับสอง")],
    },
    Line {
        key: "threshold:you cannot lyse what you have not proven",
        tr: &[("th", "ยังพิสูจน์ไม่ได้ ก็ให้ยาละลายลิ่มเลือดไม่ได้")],
    },
    Line {
        key: "threshold:acute st-elevation mi — named",
        tr: &[("th", "acute ST-elevation MI — เรียกชื่อได้แล้ว")],
    },
    Line {
        key: "harm:discharged an evolving infarct — the arm pain was never anxiety",
        tr: &[("th", "ให้กลับบ้านทั้งที่กล้ามเนื้อหัวใจกำลังตาย — อาการปวดแขนนั้นไม่เคยเป็นความวิตกกังวล")],
    },
    Line { key: "threshold:admitted to coronary care", tr: &[("th", "รับไว้ในหอผู้ป่วยโรคหัวใจ")] },
    Line {
        key: "harm:ecg delayed beyond ten minutes — the infarct ran unseen",
        tr: &[("th", "ทำ ECG ช้ากว่า 10 นาที — กล้ามเนื้อหัวใจตายไปโดยไม่มีใครเห็น")],
    },

    // ── OSCE-B2 · Tan, 14, polite and careful — pericarditis ──
    Line {
        key: "threshold:— \"Sharp, like a blade, right here. Worse flat on my back — better when I sit up and lean over my knees.\" A pain with a favourite chair.",
        tr: &[("th", "— \"เจ็บแหลม ๆ เหมือนโดนใบมีดบาด ตรงนี้ครับ นอนหงายแล้วเจ็บกว่า — พอลุกนั่งโน้มตัวมาที่เข่าแล้วดีขึ้น\" ความเจ็บที่มีท่านั่งโปรดของมันเอง")],
    },
    Line {
        key: "threshold:— \"Breathing in deep makes it stab. Small breaths are safer.\" Pleuritic, positional — nothing an artery does.",
        tr: &[("th", "— \"หายใจเข้าลึก ๆ แล้วมันแทงครับ หายใจสั้น ๆ ปลอดภัยกว่า\" เจ็บแบบ pleuritic และเปลี่ยนตามท่า — ไม่ใช่สิ่งที่หลอดเลือดหัวใจทำ")],
    },
    Line {
        key: "threshold:— \"I had a cold last week. I've felt hot since yesterday.\" 38.2 on the chart of a fourteen-year-old.",
        tr: &[("th", "— \"อาทิตย์ที่แล้วเป็นหวัดครับ ตั้งแต่เมื่อวานรู้สึกตัวร้อน\" 38.2 ในชาร์ตของเด็กอายุ 14 ปี")],
    },
    Line {
        key: "threshold:a scratchy rub over the left sternal edge, loudest leaning forward — leather creaking on leather, in time with the beat",
        tr: &[("th", "ได้ยินเสียงเสียดสีหยาบ ๆ ที่ขอบกระดูกอกด้านซ้าย ดังที่สุดตอนโน้มตัวไปข้างหน้า — เหมือนหนังเสียดสีกับหนัง เข้าจังหวะกับการเต้นของหัวใจ")],
    },
    Line {
        key: "threshold:st elevation — but look at the shape and the spread: saddle-backed, in almost every lead, pr segments sagging. no artery owns all twelve.",
        tr: &[("th", "ST elevation — แต่ดูรูปร่างและการกระจายของมัน: ยกแบบ saddle-back เกือบทุก lead, PR segment ตกลง ไม่มีหลอดเลือดเส้นไหนเป็นเจ้าของทั้ง 12 lead")],
    },
    Line {
        key: "threshold:troponin barely above flat — a graze on the surface, not a dying wall",
        tr: &[("th", "troponin สูงกว่าเส้นปกติแค่นิดเดียว — เป็นรอยถลอกที่ผิว ไม่ใช่ผนังหัวใจที่กำลังตาย")],
    },
    Line {
        key: "threshold:a thin rim of fluid behind the heart, chambers filling well — no collapse, no tamponade tonight",
        tr: &[("th", "มีน้ำเป็นแนวบาง ๆ อยู่หลังหัวใจ ห้องหัวใจยังคลายรับเลือดได้ดี — ไม่มีห้องหัวใจยุบ ไม่มี tamponade คืนนี้")],
    },
    Line {
        key: "threshold:normal heart shadow, clear fields — nothing else to blame",
        tr: &[("th", "เงาหัวใจปกติ ปอดโล่ง — ไม่มีอย่างอื่นให้โทษ")],
    },
    Line {
        key: "threshold:ibuprofen with food, round the clock — the treatment is against the fire, not against a clot",
        tr: &[("th", "ibuprofen พร้อมอาหาร ให้ตรงเวลาตลอดวัน — รักษาที่ไฟการอักเสบ ไม่ใช่ที่ลิ่มเลือด")],
    },
    Line {
        key: "threshold:colchicine on board — fewer encores of this admission",
        tr: &[("th", "ให้ colchicine ด้วย — ลดโอกาสที่จะต้องกลับมานอนโรงพยาบาลซ้ำ")],
    },
    Line {
        key: "threshold:dark blood off the sac in a syringe — the pressure eases; the lesson costs a scar",
        tr: &[("th", "ดูดเลือดสีคล้ำออกจากถุงหุ้มหัวใจได้เต็มกระบอกฉีดยา — ความดันในถุงลดลง แต่บทเรียนนี้แลกมาด้วยแผลเป็น")],
    },
    Line {
        key: "threshold:there is nothing to drain — a thin rim, no tamponade; keep the needle",
        tr: &[("th", "ไม่มีอะไรให้ระบาย — น้ำเป็นแนวบาง ๆ ไม่มี tamponade เก็บเข็มไว้ก่อน")],
    },
    Line {
        key: "harm:aspirin loaded into a febrile fourteen-year-old — the chest-pain reflex, plus reye's syndrome on the table",
        tr: &[("th", "ให้ aspirin ขนาดสูงกับเด็ก 14 ปีที่มีไข้ — รีเฟล็กซ์เจอเจ็บหน้าอกแล้วให้ aspirin แถมยังวาง Reye's syndrome ไว้บนโต๊ะ")],
    },
    Line {
        key: "harm:the lab spun up for a rub — diffuse elevation has no culprit artery to open",
        tr: &[("th", "เรียก cath lab มาเพื่อเสียงเสียดสีเยื่อหุ้มหัวใจ — ST ที่ยกแบบกระจายทั่ว ไม่มีหลอดเลือดต้นเหตุให้เปิด")],
    },
    Line {
        key: "harm:a lytic into an inflamed pericardium — the sac fills with blood",
        tr: &[("th", "ให้ยาละลายลิ่มเลือดกับเยื่อหุ้มหัวใจที่กำลังอักเสบ — ถุงหุ้มหัวใจเต็มไปด้วยเลือด")],
    },
    Line {
        key: "threshold:acute pericarditis — the fever, the rub, the chair, and twelve leads that agree",
        tr: &[("th", "acute pericarditis — ไข้ เสียงเสียดสี ท่านั่งโน้มตัว และ 12 lead ที่พูดตรงกัน")],
    },
    Line {
        key: "threshold:fourteen, febrile, a rub, and elevation in every lead — this is not a plumbing problem. look at the shape again.",
        tr: &[("th", "อายุ 14 มีไข้ มีเสียงเสียดสี และ ST ยกในทุก lead — นี่ไม่ใช่ปัญหาท่อตัน ลองดูรูปร่างของคลื่นอีกครั้ง")],
    },
    Line {
        key: "threshold:admitted for rest, the echo repeated tomorrow — sport can wait a month",
        tr: &[("th", "รับไว้ให้พัก พรุ่งนี้ทำ echo ซ้ำ — กีฬารอได้อีกเดือนหนึ่ง")],
    },

    // ── OSCE-B3 · Pim, 3 — her *mother* speaks — mild croup ──
    Line {
        key: "threshold:— \"A runny nose for two days, no fever — then tonight she woke up barking like a little seal.\" The classic second night.",
        tr: &[("th", "— \"น้ำมูกไหลมา 2 วันค่ะ ไม่มีไข้ — แล้วคืนนี้ตื่นขึ้นมาไอเสียงก้องเหมือนลูกแมวน้ำเลย\" คืนที่สองแบบคลาสสิก")],
    },
    Line {
        key: "threshold:— \"No fever. 37.2 at home and again at triage.\" Cool — the first door away from the dangerous mimic.",
        tr: &[("th", "— \"ไม่มีไข้ค่ะ วัดที่บ้าน 37.2 มาวัดที่จุดคัดกรองก็ 37.2\" ตัวไม่ร้อน — ประตูบานแรกที่พาออกห่างจากโรคเลียนแบบที่อันตราย")],
    },
    Line {
        key: "threshold:— \"She finished a whole bottle of milk in the waiting room.\" Swallowing happily — no drool, no tripod, no statue-child.",
        tr: &[("th", "— \"นั่งรออยู่หน้าห้อง กินนมหมดไปทั้งขวดเลยค่ะ\" กลืนได้สบาย — ไม่มีน้ำลายไหล ไม่ได้นั่งยันตัวไปข้างหน้า ไม่ได้นั่งนิ่งแข็งเหมือนรูปปั้น")],
    },
    Line {
        key: "threshold:watched from the doorway on her mother's lap: a soft stridor only when she fusses, mild tugging below the ribs, colour good, chatting between coughs — westley 2, mild",
        tr: &[("th", "เฝ้าดูจากหน้าประตูขณะนั่งบนตักแม่: มี stridor เบา ๆ เฉพาะตอนงอแง ชายโครงบุ๋มเล็กน้อย สีผิวดี พูดคุยได้ระหว่างไอ — Westley 2, ระดับน้อย")],
    },
    Line {
        key: "threshold:spo2 98 on air, the probe on a toe mid-cartoon — she barely looks up",
        tr: &[("th", "SpO2 98 ในอากาศห้อง หนีบ probe ไว้ที่นิ้วเท้าระหว่างดูการ์ตูน — แทบไม่เงยหน้าขึ้นมาเลย")],
    },
    Line {
        key: "threshold:clear entry both sides under the bark — the noise is made above the chest, in the subglottis",
        tr: &[("th", "ลมเข้าปอดดีทั้งสองข้างใต้เสียงไอก้องนั้น — เสียงเกิดเหนือทรวงอกขึ้นไป ที่ใต้กล่องเสียง")],
    },
    Line {
        key: "threshold:the steeple sign on the ap neck, a clean epiglottis on the lateral, chest clear — croup drawn in white",
        tr: &[("th", "เห็น steeple sign ในฟิล์มคอท่า AP, epiglottis ปกติในท่า lateral, ปอดโล่ง — croup ที่ถูกวาดออกมาเป็นสีขาว")],
    },
    Line {
        key: "threshold:dexamethasone 0.15 mg/kg in syrup, swallowed on the first try — the one drug this whole visit is about",
        tr: &[("th", "dexamethasone 0.15 mg/kg ในรูปน้ำเชื่อม กลืนได้ตั้งแต่ครั้งแรก — ยาตัวเดียวที่การมาโรงพยาบาลครั้งนี้เป็นเรื่องของมัน")],
    },
    Line {
        key: "threshold:nebulised adrenaline through a soft mask — mist for a subglottis that stopped waiting",
        tr: &[("th", "พ่น adrenaline ผ่านหน้ากากนุ่ม ๆ — ละอองยาสำหรับใต้กล่องเสียงที่ไม่รออีกต่อไป")],
    },
    Line {
        key: "threshold:she is mild and settled on the lap — hold the mist for the child who needs it, and watch instead",
        tr: &[("th", "อาการน้อยและนั่งนิ่งอยู่บนตัก — เก็บการพ่นยาไว้ให้เด็กที่จำเป็นจริง ๆ แล้วเฝ้าดูแทน")],
    },
    Line {
        key: "threshold:nothing touches her that does not need to — she stays on the lap, and the airway stays hers",
        tr: &[("th", "ไม่มีอะไรไปแตะต้องเธอโดยไม่จำเป็น — เธออยู่บนตักแม่ต่อไป และทางเดินหายใจก็ยังเป็นของเธอ")],
    },
    Line {
        key: "threshold:a quiet hour by the nurses' station, probe on, mum in the chair — the bark already softer as the syrup works",
        tr: &[("th", "หนึ่งชั่วโมงเงียบ ๆ ข้างเคาน์เตอร์พยาบาล คา probe ไว้ แม่นั่งอยู่บนเก้าอี้ — เสียงไอเบาลงแล้วขณะที่ยาน้ำเชื่อมออกฤทธิ์")],
    },
    Line {
        key: "threshold:the night-two speech: no steam over a kettle, come straight back if the stridor sits at rest or the ribs pull hard — mum says it back word for word",
        tr: &[("th", "คำแนะนำสำหรับคืนที่สอง: ห้ามรมไอน้ำจากกาต้มน้ำ ถ้ามี stridor ตอนอยู่เฉย ๆ หรือชายโครงบุ๋มแรง ให้กลับมาทันที — แม่ทวนกลับได้ทุกคำ")],
    },
    Line {
        key: "threshold:mild croup — named, graded, and treated like what it is",
        tr: &[("th", "croup ระดับน้อย — เรียกชื่อ จัดระดับ และรักษาตามที่มันเป็น")],
    },
    Line {
        key: "threshold:epiglottitis? — she is loud, cool, drinking milk and barking. the lateral film already answered this. look again.",
        tr: &[("th", "epiglottitis? — เธอส่งเสียงดัง ตัวไม่ร้อน กินนมได้ และไอเสียงก้อง ฟิล์มท่า lateral ตอบคำถามนี้ไปแล้ว ลองดูอีกครั้ง")],
    },
    Line {
        key: "harm:antibiotics for a virus — the steeple was never bacterial, and the bottle teaches the family the wrong lesson",
        tr: &[("th", "ให้ยาปฏิชีวนะกับโรคจากไวรัส — steeple sign ไม่เคยเกิดจากแบคทีเรีย และยาขวดนั้นสอนบทเรียนผิด ๆ ให้ครอบครัว")],
    },
    Line {
        key: "harm:home without the steroid or the hour of watching — night three is worse than night two",
        tr: &[("th", "ให้กลับบ้านโดยไม่ได้ steroid และไม่ได้เฝ้าดูอีกหนึ่งชั่วโมง — คืนที่สามหนักกว่าคืนที่สอง")],
    },
    Line {
        key: "harm:no steroid by the seventh minute — croup is a steroid disease, and the subglottis stopped waiting",
        tr: &[("th", "ผ่านไปถึงนาทีที่ 7 ยังไม่ได้ steroid — croup เป็นโรคที่รักษาด้วย steroid และใต้กล่องเสียงไม่รอแล้ว")],
    },
    Line {
        key: "threshold:the bark deepens — you can hear the stridor from the desk now, at rest",
        tr: &[("th", "เสียงไอทุ้มลง — ตอนนี้ได้ยิน stridor จากโต๊ะพยาบาล ทั้งที่เธออยู่เฉย ๆ")],
    },

    // ── OSCE-C · Fon, 6 — her *mother* speaks — croup that drools ──
    Line {
        key: "threshold:— \"Last year. The same bark, the same bad night. Her sister had it at this age too.\" The family knows this illness by name.",
        tr: &[("th", "— \"ปีที่แล้วก็เป็นค่ะ ไอเสียงเดียวกัน คืนแย่ ๆ แบบเดียวกัน พี่สาวเขาก็เป็นตอนอายุเท่านี้\" ครอบครัวนี้รู้จักโรคนี้ถึงชื่อ")],
    },
    Line {
        key: "threshold:— \"No fever. I checked twice — she runs cool.\" Afebrile: the first door away from epiglottitis.",
        tr: &[("th", "— \"ไม่มีไข้ค่ะ วัดแล้ววัดอีกสองรอบ — ตัวเย็นด้วยซ้ำ\" ไม่มีไข้: ประตูบานแรกที่พาออกห่างจาก epiglottitis")],
    },
    Line {
        key: "threshold:— \"Every shot, on schedule, since she was born.\" Hib among them — the odds move further still.",
        tr: &[("th", "— \"ฉีดครบทุกเข็มตามนัดตั้งแต่เกิดค่ะ\" มี Hib รวมอยู่ด้วย — โอกาสยิ่งเอนไปอีกทางหนึ่ง")],
    },
    Line {
        key: "threshold:— \"Nights. She sleeps, then sits up barking. By day she is almost herself.\" The croup pattern, word for word.",
        tr: &[("th", "— \"ตอนกลางคืนค่ะ หลับไปแล้วลุกขึ้นมานั่งไอเสียงก้อง พอกลางวันก็เกือบเป็นปกติ\" รูปแบบของ croup ตรงทุกคำ")],
    },
    Line {
        key: "threshold:watched from the doorway, on her mother's lap: no stridor at rest, no retractions, colour good, drooling but chatting between coughs — westley 1, mild",
        tr: &[("th", "เฝ้าดูจากหน้าประตูขณะนั่งบนตักแม่: ไม่มี stridor ตอนอยู่เฉย ๆ ไม่มีการดึงรั้งของกล้ามเนื้อช่วยหายใจ สีผิวดี น้ำลายไหลแต่ยังพูดคุยได้ระหว่างไอ — Westley 1, ระดับน้อย")],
    },
    Line {
        key: "threshold:spo2 99 on air, the probe clipped to a toe mid-story — she barely noticed",
        tr: &[("th", "SpO2 99 ในอากาศห้อง หนีบ probe ไว้ที่นิ้วเท้าระหว่างฟังนิทาน — เธอแทบไม่รู้ตัว")],
    },
    Line {
        key: "threshold:clear entry both sides — the bark lives higher up, transmitted from the subglottis",
        tr: &[("th", "ลมเข้าปอดดีทั้งสองข้าง — เสียงไอก้องอยู่สูงขึ้นไป ส่งต่อลงมาจากใต้กล่องเสียง")],
    },
    Line {
        key: "threshold:portable films with mum holding her still: the steeple sign on the ap neck, a normal epiglottis on the lateral, chest clear",
        tr: &[("th", "ถ่ายฟิล์มเคลื่อนที่โดยให้แม่ช่วยจับนิ่ง: เห็น steeple sign ในฟิล์มคอท่า AP, epiglottis ปกติในท่า lateral, ปอดโล่ง")],
    },
    Line {
        key: "threshold:dexamethasone 0.15 mg/kg in syrup, taken from her mother's hand — an hour from now the night gets quieter",
        tr: &[("th", "dexamethasone 0.15 mg/kg ในรูปน้ำเชื่อม กินจากมือแม่ — อีกหนึ่งชั่วโมงคืนนี้จะเงียบลง")],
    },
    Line {
        key: "threshold:nebulised adrenaline through a soft mask her mother holds — mist for the swollen subglottis",
        tr: &[("th", "พ่น adrenaline ผ่านหน้ากากนุ่ม ๆ ที่แม่เป็นคนถือ — ละอองยาสำหรับใต้กล่องเสียงที่บวม")],
    },
    Line {
        key: "threshold:nothing touches her that does not need to — she stays on the lap, and breathes the easier for it",
        tr: &[("th", "ไม่มีอะไรไปแตะต้องเธอโดยไม่จำเป็น — เธออยู่บนตักแม่ต่อไป และหายใจได้สบายขึ้นเพราะอย่างนั้น")],
    },
    Line {
        key: "threshold:two quiet hours by the nurses' station, saturation probe on, mum in the chair — the bark is already softer",
        tr: &[("th", "สองชั่วโมงเงียบ ๆ ข้างเคาน์เตอร์พยาบาล คา probe วัดออกซิเจนไว้ แม่นั่งอยู่บนเก้าอี้ — เสียงไอเบาลงแล้ว")],
    },
    Line {
        key: "threshold:croup — loud, barking, mild, and named for what it is",
        tr: &[("th", "croup — เสียงดัง ไอก้อง อาการน้อย และเรียกชื่อได้ตรงตามที่มันเป็น")],
    },
    Line {
        key: "threshold:epiglottitis? — she is vaccinated, afebrile, barking loudly and unafraid to cry. the drool misled you once. look again.",
        tr: &[("th", "epiglottitis? — เธอฉีดวัคซีนครบ ไม่มีไข้ ไอเสียงก้องดัง และร้องไห้ได้ไม่กลัว น้ำลายที่ไหลหลอกคุณไปแล้วหนึ่งครั้ง ลองดูอีกครั้ง")],
    },
    Line {
        key: "harm:the tongue depressor goes in — she screams, and the airway she was protecting clamps down",
        tr: &[("th", "ไม้กดลิ้นสอดเข้าไป — เธอกรีดร้อง และทางเดินหายใจที่เธอปกป้องไว้ก็หุบลง")],
    },
    Line {
        key: "harm:a needle hunt before the steroid — she fights, shrieking, and the stridor you did not have is here now",
        tr: &[("th", "ไล่แทงเข็มหาเส้นก่อนจะได้ steroid — เธอดิ้นและกรีดร้อง แล้ว stridor ที่เมื่อครู่ยังไม่มีก็มาแล้ว")],
    },
    Line {
        key: "harm:seven minutes and no steroid — dexamethasone is the whole visit",
        tr: &[("th", "ผ่านไป 7 นาทียังไม่ได้ steroid — dexamethasone คือทั้งหมดของการมาครั้งนี้")],
    },

    // ── OSCE-C2 · Wasana, 53 — acute asthma ──
    Line {
        key: "threshold:— \"Third time this year. I only have the blue inhaler — I puff it when it gets bad.\" A reliever with nothing behind it.",
        tr: &[("th", "— \"ปีนี้เป็นครั้งที่ 3 แล้วค่ะ มีแต่ยาพ่นสีฟ้า — พ่นตอนที่มันหนักขึ้น\" ยาบรรเทาที่ไม่มีอะไรหนุนอยู่ข้างหลัง")],
    },
    Line {
        key: "threshold:she answers in full sentences, pausing once for air — moderate, not yet severe, and worth writing down",
        tr: &[("th", "เธอตอบเป็นประโยคเต็ม หยุดพักหายใจครั้งเดียว — ระดับปานกลาง ยังไม่รุนแรง และควรบันทึกไว้")],
    },
    Line {
        key: "threshold:loud expiratory wheeze both sides, a long slow breath out — air that queues to leave",
        tr: &[("th", "เสียงวี้ดตอนหายใจออกดังทั้งสองข้าง หายใจออกยาวและช้า — ลมที่ต้องต่อคิวออกจากปอด")],
    },
    Line {
        key: "threshold:pefr 400 — 85% of predicted; the ladder worked, and the number proves it",
        tr: &[("th", "PEFR 400 — 85% ของค่าที่ควรเป็น บันไดขั้นต่าง ๆ ได้ผล และตัวเลขก็ยืนยัน")],
    },
    Line {
        key: "threshold:pefr 310 — better, not done; the ladder has more rungs",
        tr: &[("th", "PEFR 310 — ดีขึ้น แต่ยังไม่จบ บันไดยังมีขั้นต่อไป")],
    },
    Line {
        key: "threshold:pefr 240 — 73% of predicted, moderate obstruction; a number to come back to after every rung",
        tr: &[("th", "PEFR 240 — 73% ของค่าที่ควรเป็น อุดกั้นระดับปานกลาง เป็นตัวเลขที่ต้องกลับมาวัดซ้ำหลังทุกขั้นบันได")],
    },
    Line {
        key: "threshold:a brown preventer and a written action plan — the fourth attack gets cancelled, not survived",
        tr: &[("th", "ยาพ่นสีน้ำตาลสำหรับควบคุมโรค พร้อมแผนการดูแลตัวเองที่เขียนไว้ — ครั้งที่ 4 จะถูกยกเลิก ไม่ใช่แค่รอดมาได้")],
    },
    Line {
        key: "threshold:prednisolone 40 mg swallowed — the twelve-hour fire brigade behind the neb",
        tr: &[("th", "กลืน prednisolone 40 mg — หน่วยดับเพลิงที่จะมาถึงใน 12 ชั่วโมง หนุนอยู่หลังยาพ่น")],
    },
    Line {
        key: "threshold:back-to-back salbutamol — the chest opens another notch",
        tr: &[("th", "พ่น salbutamol ต่อเนื่องติด ๆ กัน — ปอดเปิดขึ้นอีกขั้น")],
    },
    Line {
        key: "threshold:first neb up and misting — she breathes it greedily",
        tr: &[("th", "ยาพ่นชุดแรกแขวนขึ้นและเริ่มเป็นละออง — เธอสูดเข้าไปอย่างกระหาย")],
    },
    Line {
        key: "threshold:ipratropium joins the second neb — the next rung of the ladder",
        tr: &[("th", "ipratropium เข้าร่วมกับยาพ่นชุดที่สอง — ขั้นบันไดถัดไป")],
    },
    Line {
        key: "threshold:ipratropium rides with salbutamol, not instead of it — start the ladder first",
        tr: &[("th", "ipratropium ให้ไปพร้อม salbutamol ไม่ใช่ให้แทนกัน — เริ่มจากขั้นแรกของบันไดก่อน")],
    },
    Line {
        key: "threshold:magnesium dripping — the severe-attack card, kept warm in case the ladder stalls",
        tr: &[("th", "magnesium กำลังหยด — ไพ่สำหรับการกำเริบรุนแรง อุ่นเครื่องไว้เผื่อบันไดไปต่อไม่ได้")],
    },
    Line {
        key: "threshold:clear film — no pneumothorax hiding behind the wheeze",
        tr: &[("th", "ฟิล์มปกติ — ไม่มี pneumothorax ซ่อนอยู่หลังเสียงวี้ด")],
    },
    Line {
        key: "threshold:acute asthma exacerbation, moderate by the numbers — named with its grade",
        tr: &[("th", "acute asthma exacerbation ระดับปานกลางตามตัวเลข — เรียกชื่อพร้อมระดับความรุนแรง")],
    },
    Line {
        key: "harm:a sedative on a tiring asthmatic — the drive to breathe was the only thing holding her",
        tr: &[("th", "ให้ยากล่อมประสาทกับคนไข้หืดที่กำลังหมดแรง — แรงขับให้หายใจคือสิ่งเดียวที่ยังพยุงเธอไว้")],
    },
    Line {
        key: "harm:discharged mid-attack on a reliever alone — the third visit becomes a fourth",
        tr: &[("th", "ให้กลับบ้านกลางการกำเริบ โดยมีแต่ยาบรรเทา — ครั้งที่ 3 กลายเป็นครั้งที่ 4")],
    },
    Line {
        key: "harm:the neb wore off with nothing behind it — the wheeze walks back in",
        tr: &[("th", "ฤทธิ์ยาพ่นหมดลงโดยไม่มีอะไรหนุนอยู่ข้างหลัง — เสียงวี้ดเดินกลับเข้ามาอีก")],
    },

    // ── OSCE-C3 · Waen, 25, apologetic — right lower lobe pneumonia ──
    Line {
        key: "threshold:— \"A week of coughing, and since yesterday it comes up rusty — and this stab when I breathe in.\" Rust is a lobe talking.",
        tr: &[("th", "— \"ไอมาอาทิตย์หนึ่งแล้วค่ะ ตั้งแต่เมื่อวานเสมหะออกมาสีสนิม — แล้วก็เจ็บแปลบเวลาหายใจเข้า\" สีสนิมคือเสียงของปอดกลีบหนึ่งที่กำลังพูด")],
    },
    Line {
        key: "threshold:— \"Healthy. No pills, I don't smoke.\" Twenty-five, with nothing on the chart — which is what makes the oximeter interesting.",
        tr: &[("th", "— \"แข็งแรงดีค่ะ ไม่ได้กินยาอะไร ไม่สูบบุหรี่\" อายุ 25 ไม่มีอะไรในประวัติ — ซึ่งเป็นสิ่งที่ทำให้ตัวเลขบน oximeter น่าสนใจขึ้นมา")],
    },
    Line {
        key: "threshold:quiet at the right base, dull to the tap, crackles above the dullness — a lobe full of the wrong thing",
        tr: &[("th", "เสียงหายใจเบาลงที่ฐานปอดขวา เคาะได้เสียงทึบ มี crackles อยู่เหนือบริเวณที่ทึบ — ปอดกลีบหนึ่งเต็มไปด้วยสิ่งที่ไม่ควรอยู่ในนั้น")],
    },
    Line {
        key: "threshold:wbc 15.4, neutrophils marching — the marrow agrees with the stethoscope",
        tr: &[("th", "WBC 15.4, neutrophil เดินแถวขึ้นมา — ไขกระดูกเห็นตรงกับหูฟัง")],
    },
    Line {
        key: "threshold:24 a minute, shallow on the right — she is splinting the stab",
        tr: &[("th", "หายใจ 24 ครั้งต่อนาที ตื้นทางด้านขวา — เธอกลั้นการขยายปอดไว้เพราะความเจ็บแปลบ")],
    },
    Line {
        key: "threshold:a wedge of white in the right lower lobe, air bronchograms threading through — consolidation, signed",
        tr: &[("th", "ฝ้าขาวเป็นรูปลิ่มที่ปอดกลีบล่างขวา มี air bronchogram พาดผ่าน — consolidation ที่เซ็นชื่อกำกับไว้")],
    },
    Line {
        key: "threshold:sputum potted and blood away before the first dose — the lab gets its chance to name it",
        tr: &[("th", "เก็บเสมหะใส่ขวดและส่งเลือดเพาะเชื้อก่อนยาโดสแรก — แล็บได้โอกาสเรียกชื่อเชื้อ")],
    },
    Line {
        key: "threshold:confusion none, urea pending, rr 24, bp fine, age 25 — curb-65 zero, maybe one. the score says street; the oximeter, at 93, gets a vote too.",
        tr: &[("th", "ไม่สับสน urea ยังรอผล RR 24 ความดันปกติ อายุ 25 — CURB-65 เท่ากับ 0 หรืออาจจะ 1 คะแนนบอกว่าให้กลับบ้าน แต่ oximeter ที่ 93 ก็มีสิทธิ์ออกเสียงเหมือนกัน")],
    },
    Line {
        key: "threshold:co-amoxiclav plus a macrolide, first dose in her arm — the wall clock approves",
        tr: &[("th", "co-amoxiclav ร่วมกับ macrolide โดสแรกเข้าแขนเธอแล้ว — นาฬิกาบนผนังเห็นชอบด้วย")],
    },
    Line {
        key: "threshold:paracetamol for the fever and the stab — she takes a deeper breath without the knife in it",
        tr: &[("th", "paracetamol สำหรับไข้และอาการเจ็บแปลบ — เธอหายใจได้ลึกขึ้นโดยไม่มีมีดแทงอยู่ข้างใน")],
    },
    Line {
        key: "threshold:a short-stay bed with the probe on — the score walked, the sats stayed",
        tr: &[("th", "เตียงพักระยะสั้นโดยคา probe ไว้ — คะแนนบอกให้กลับ แต่ค่าออกซิเจนขอให้อยู่")],
    },
    Line {
        key: "threshold:icu, for this? save the bed — she needs a ward, two litres of oxygen and the first dose",
        tr: &[("th", "ICU เพื่อเรื่องนี้หรือ? เก็บเตียงไว้เถอะ — เธอต้องการหอผู้ป่วยธรรมดา ออกซิเจน 2 ลิตร และยาโดสแรก")],
    },
    Line {
        key: "threshold:community-acquired pneumonia, right lower lobe — named with its severity beside it",
        tr: &[("th", "community-acquired pneumonia ที่ปอดกลีบล่างขวา — เรียกชื่อพร้อมระดับความรุนแรงกำกับไว้ข้าง ๆ")],
    },
    Line {
        key: "harm:sent home saturating 93 — the score never listened to the oximeter",
        tr: &[("th", "ให้กลับบ้านทั้งที่ค่าออกซิเจน 93 — คะแนนไม่เคยฟัง oximeter เลย")],
    },
    Line {
        key: "harm:the first dose slid past the hour — mortality climbs with the clock on untreated pneumonia",
        tr: &[("th", "ยาโดสแรกเลยหนึ่งชั่วโมงไปแล้ว — ในปอดอักเสบที่ยังไม่ได้รักษา อัตราตายไต่ขึ้นตามเข็มนาฬิกา")],
    },

    // ── OSCE-D · Somchai Jaiman, 62, short and embarrassed — upper GI bleed ──
    Line {
        key: "threshold:— \"Heart pills. The white one and the pink one, every morning since the stent.\" Aspirin and clopidogrel, on a bleeding stomach.",
        tr: &[("th", "— \"ยาโรคหัวใจครับ เม็ดขาวกับเม็ดชมพู กินทุกเช้าตั้งแต่ใส่ขดลวด\" aspirin กับ clopidogrel บนกระเพาะที่กำลังเลือดออก")],
    },
    Line {
        key: "threshold:— \"A full glass of it, red like a fresh cut. And this morning the toilet was black tar.\"",
        tr: &[("th", "— \"อาเจียนออกมาเต็มแก้วเลยครับ แดงเหมือนแผลสด ๆ แล้วเมื่อเช้าถ่ายออกมาดำเหมือนยางมะตอย\"")],
    },
    Line {
        key: "threshold:— \"The room tips over when I stand. I went down on the bathroom floor.\" The pressure is lying; the story is not.",
        tr: &[("th", "— \"พอลุกยืนแล้วห้องมันเอียงไปหมดครับ ล้มลงไปกับพื้นห้องน้ำเลย\" ความดันกำลังโกหก แต่เรื่องที่เขาเล่าไม่ได้โกหก")],
    },
    Line {
        key: "threshold:hands cold to the wrist, conjunctivae the colour of paper — he is compensating, and compensating is a countdown",
        tr: &[("th", "มือเย็นขึ้นมาถึงข้อมือ เยื่อบุตาซีดเหมือนกระดาษ — ร่างกายยังชดเชยอยู่ และการชดเชยคือการนับถอยหลัง")],
    },
    Line {
        key: "threshold:soft, tender over the epigastrium, no guarding — nothing to cut tonight, something to scope",
        tr: &[("th", "ท้องนิ่ม กดเจ็บบริเวณลิ้นปี่ ไม่มีเกร็งท้อง — คืนนี้ไม่มีอะไรต้องผ่า แต่มีอะไรต้องส่องกล้อง")],
    },
    Line {
        key: "threshold:melena on the glove — an upper source, declared",
        tr: &[("th", "มี melena ติดถุงมือ — ประกาศชัดว่าจุดเลือดออกอยู่ทางเดินอาหารส่วนบน")],
    },
    Line {
        key: "threshold:hb 8.5, haematocrit 26 — and he is still diluting; the number is yesterday's news",
        tr: &[("th", "Hb 8.5, hematocrit 26 — และเลือดยังเจือจางลงเรื่อย ๆ ตัวเลขนี้คือข่าวของเมื่อวาน")],
    },
    Line {
        key: "threshold:inr 1.1, platelets fine — the problem is the two tablets, not the cascade",
        tr: &[("th", "INR 1.1, เกล็ดเลือดปกติ — ปัญหาอยู่ที่ยา 2 เม็ดนั้น ไม่ใช่ที่กระบวนการแข็งตัวของเลือด")],
    },
    Line {
        key: "threshold:group and screen away, four units crossmatching — the fridge clock starts",
        tr: &[("th", "ส่งตรวจ group and screen จองเลือดจับคู่ไว้ 4 ยูนิต — นาฬิกาของตู้เย็นเริ่มเดิน")],
    },
    Line {
        key: "threshold:two grey cannulas, one in each antecubital fossa — the bore is the resuscitation",
        tr: &[("th", "เข็มสีเทา 2 เส้น ข้อพับแขนข้างละเส้น — ขนาดรูเข็มคือการกู้ชีพ")],
    },
    Line {
        key: "threshold:warmed crystalloid wide open through both barrels — the pressure answers",
        tr: &[("th", "สารน้ำอุ่นเปิดเต็มที่ทั้งสองเส้น — ความดันตอบสนอง")],
    },
    Line {
        key: "threshold:a litre crawling through one thin green line — bore first, then volume",
        tr: &[("th", "สารน้ำ 1 ลิตรคลานผ่านเข็มเขียวเส้นเล็กเส้นเดียว — เอาขนาดเข็มก่อน แล้วค่อยเอาปริมาณ")],
    },
    Line {
        key: "threshold:crossmatched cells up and running warm through the second line",
        tr: &[("th", "เลือดที่จับคู่แล้วแขวนขึ้นและไหลอุ่น ๆ ผ่านเส้นที่สอง")],
    },
    Line {
        key: "threshold:o negative from the emergency drawer — the crossmatch owes you two more units",
        tr: &[("th", "เลือดกรุ๊ป O negative จากลิ้นชักฉุกเฉิน — การจับคู่เลือดยังติดค้างคุณอีก 2 ยูนิต")],
    },
    Line {
        key: "threshold:pantoprazole, bolus then the infusion — give the clot a ph it can live with",
        tr: &[("th", "pantoprazole ให้ bolus แล้วต่อด้วย infusion — ให้ลิ่มเลือดได้ pH ที่มันอยู่ได้")],
    },
    Line {
        key: "threshold:aspirin and clopidogrel held tonight — cardiology can have their argument in the morning",
        tr: &[("th", "งด aspirin และ clopidogrel คืนนี้ — ให้อายุรกรรมหัวใจมาเถียงกันตอนเช้า")],
    },
    Line {
        key: "threshold:the scope finds a visible vessel on the lesser curve — clipped, injected, dry",
        tr: &[("th", "ส่องกล้องพบหลอดเลือดโผล่ที่กระเพาะด้านโค้งเล็ก — หนีบคลิป ฉีดยา แล้วแห้ง")],
    },
    Line {
        key: "threshold:gi, on the phone: 'not on an empty tank. lines, volume, blood — then i scope him.'",
        tr: &[("th", "หมอทางเดินอาหารตอบทางโทรศัพท์: \"ถังยังว่างอยู่แบบนี้ไม่ส่อง เปิดเส้น ให้สารน้ำ ให้เลือดก่อน — แล้วผมจะส่องให้\"")],
    },
    Line {
        key: "threshold:an upper gi bleed on double antiplatelets — named, with the clock still running",
        tr: &[("th", "เลือดออกทางเดินอาหารส่วนบนในคนไข้ที่กินยาต้านเกล็ดเลือด 2 ตัว — เรียกชื่อได้แล้ว โดยที่นาฬิกายังเดินอยู่")],
    },
    Line {
        key: "threshold:the old notes say stent — but the troponin will be quiet and the toilet was black. wrong organ, same aspirin.",
        tr: &[("th", "ประวัติเก่าบอกว่าเคยใส่ขดลวด — แต่ troponin จะเงียบ และอุจจาระเป็นสีดำ ผิดอวัยวะ แต่เป็น aspirin ตัวเดียวกัน")],
    },
    Line {
        key: "harm:aspirin on top of clopidogrel into a bleeding stomach — the chest-pain reflex, doubled",
        tr: &[("th", "ให้ aspirin ทับ clopidogrel เข้าไปในกระเพาะที่กำลังเลือดออก — รีเฟล็กซ์เจอเจ็บหน้าอกแล้วให้ aspirin คูณสอง")],
    },
    Line {
        key: "harm:four minutes of falling pressure on one thin green line — shock was called at the door, and the access never answered",
        tr: &[("th", "ความดันตกอยู่ 4 นาทีโดยมีแต่เข็มเขียวเส้นเล็กเส้นเดียว — เรียกภาวะช็อกไว้ตั้งแต่หน้าประตู แต่เส้นที่เปิดไว้ไม่เคยตอบรับ")],
    },
    Line {
        key: "threshold:he retches again — fresh red, a full bowl of it; the sheet is soaked",
        tr: &[("th", "เขาอาเจียนอีกครั้ง — เลือดสดสีแดง เต็มชามหนึ่ง ผ้าปูเตียงเปียกโชก")],
    },

    // ── OSCE-D2 · Somsri Jaidee, 55, direct — pulmonary embolism ──
    Line {
        key: "threshold:— \"It hit while I was hanging the washing — a stab with every breath, and I cannot get a full one in.\" Sudden, pleuritic, out of nowhere.",
        tr: &[("th", "— \"มันมาตอนกำลังตากผ้าอยู่ค่ะ — เจ็บแปลบทุกครั้งที่หายใจ แล้วก็หายใจไม่เต็มปอด\" เกิดขึ้นทันที เจ็บแบบ pleuritic มาแบบไม่มีที่มา")],
    },
    Line {
        key: "threshold:— \"Breast cancer, two years now. I take the hormone tablets every night.\" Malignancy and hormones — a clot's two best friends.",
        tr: &[("th", "— \"เป็นมะเร็งเต้านมมา 2 ปีแล้วค่ะ กินยาฮอร์โมนทุกคืน\" มะเร็งกับฮอร์โมน — เพื่อนสนิทสองคนของลิ่มเลือด")],
    },
    Line {
        key: "threshold:the right calf sits three centimetres fuller than the left, warm and tender — a source, wearing a sock",
        tr: &[("th", "น่องขวาใหญ่กว่าซ้าย 3 เซนติเมตร อุ่นและกดเจ็บ — ต้นตอที่ซ่อนอยู่ใต้ถุงเท้า")],
    },
    Line {
        key: "threshold:— \"My right calf has been tight for a week. I thought it was the standing.\" Nobody had asked.",
        tr: &[("th", "— \"น่องขวาตึงมาอาทิตย์หนึ่งแล้วค่ะ นึกว่าเพราะยืนนาน\" ยังไม่มีใครถามเรื่องนี้เลย")],
    },
    Line {
        key: "threshold:clear fields, both sides — a chest this clear has no business being this hypoxic",
        tr: &[("th", "ปอดโล่งทั้งสองข้าง — ปอดที่ฟังโล่งขนาดนี้ ไม่ควรมีออกซิเจนต่ำขนาดนี้")],
    },
    Line {
        key: "threshold:cancer, a hundred and fifteen a minute, a swollen calf, nothing else it could be — wells 7, high. the next test is the scan, not the dimer.",
        tr: &[("th", "มะเร็ง ชีพจร 115 ครั้งต่อนาที น่องบวม และไม่มีอย่างอื่นที่จะเป็นไปได้ — Wells 7, ความเสี่ยงสูง การตรวจถัดไปคือ scan ไม่ใช่ D-dimer")],
    },
    Line {
        key: "threshold:1200 — and it changes nothing: at high probability a dimer cannot rule out, it can only stall the scan",
        tr: &[("th", "1200 — และมันไม่เปลี่ยนอะไรเลย: เมื่อความน่าจะเป็นสูง D-dimer ตัดโรคออกไม่ได้ ทำได้แค่ถ่วงเวลาการทำ scan")],
    },
    Line {
        key: "threshold:sinus tachycardia; an s1q3t3 if you squint — supportive, never diagnostic",
        tr: &[("th", "sinus tachycardia; เห็น S1Q3T3 ถ้าเพ่งดู — เป็นข้อสนับสนุน ไม่เคยเป็นข้อวินิจฉัย")],
    },
    Line {
        key: "threshold:a clean film — the great mimic leaves no shadow",
        tr: &[("th", "ฟิล์มสะอาด — นักเลียนแบบตัวฉกาจไม่ทิ้งเงาเอาไว้")],
    },
    Line {
        key: "threshold:a filling defect sits fat in the right pulmonary artery — the masquerader, unmasked in one pass of the gantry",
        tr: &[("th", "เห็น filling defect ก้อนโตอยู่ในหลอดเลือดแดงปอดข้างขวา — นักปลอมตัวถูกถอดหน้ากากในการสแกนรอบเดียว")],
    },
    Line {
        key: "threshold:a careful 500 and no more — a strained right heart drowns in enthusiasm",
        tr: &[("th", "ให้สารน้ำอย่างระมัดระวังแค่ 500 ไม่มากกว่านี้ — หัวใจซีกขวาที่กำลังรับภาระหนักจะจมน้ำเพราะความกระตือรือร้น")],
    },
    Line {
        key: "threshold:low-molecular-weight heparin into the belly — the clot stops growing tonight",
        tr: &[("th", "ฉีด low-molecular-weight heparin เข้าหน้าท้อง — คืนนี้ลิ่มเลือดหยุดโต")],
    },
    Line {
        key: "threshold:the scan first — name the clot, then starve it. heparin wants a target on film.",
        tr: &[("th", "ทำ scan ก่อน — เรียกชื่อลิ่มเลือดให้ได้ แล้วค่อยตัดเสบียงมัน heparin อยากได้เป้าหมายที่เห็นบนฟิล์ม")],
    },
    Line {
        key: "harm:lytics for a patient still holding her pressure — an intracranial bleed put on the table for nothing",
        tr: &[("th", "ให้ยาละลายลิ่มเลือดกับคนไข้ที่ความดันยังอยู่ได้ — เอาเลือดออกในสมองมาวางบนโต๊ะโดยไม่ได้อะไรกลับมา")],
    },
    Line {
        key: "threshold:pulmonary embolism — the masquerader named while it still matters",
        tr: &[("th", "pulmonary embolism — นักปลอมตัวที่ถูกเรียกชื่อในตอนที่ยังทันการณ์")],
    },
    Line {
        key: "harm:reassured as anxiety at 88% — the mimic's favourite exit",
        tr: &[("th", "ปลอบว่าเป็นแค่ความวิตกกังวลทั้งที่ค่าออกซิเจน 88% — ทางออกที่นักเลียนแบบชอบที่สุด")],
    },
    Line {
        key: "threshold:up to the unit on the infusion — the calf gets its ultrasound tomorrow",
        tr: &[("th", "ย้ายขึ้นหอผู้ป่วยพร้อมยาที่กำลังหยด — พรุ่งนี้น่องจะได้ทำอัลตราซาวด์")],
    },
    Line {
        key: "threshold:she greys mid-sentence — the next breath is faster and shallower than the last",
        tr: &[("th", "เธอหน้าซีดเทาลงกลางประโยค — ลมหายใจถัดไปเร็วและตื้นกว่าครั้งก่อน")],
    },

    // ── OSCE-D3 · Beam, 6, 20 kg — an adult speaks for her — paediatric anaphylaxis ──
    Line {
        key: "threshold:adrenaline 0.2 mg im, outer thigh — 0.01 a kilo, drawn to the twenty she weighs",
        tr: &[("th", "adrenaline 0.2 mg IM ที่ต้นขาด้านนอก — 0.01 ต่อกิโลกรัม คำนวณจากน้ำหนัก 20 กิโลกรัมของเธอ")],
    },
    Line {
        key: "harm:half a milligram into a twenty-kilo child — an adult dose, two and a half times hers; the tachycardia that follows is yours",
        tr: &[("th", "ให้ครึ่งมิลลิกรัมกับเด็กหนัก 20 กิโลกรัม — ขนาดยาผู้ใหญ่ มากกว่าของเธอ 2.5 เท่า หัวใจที่เต้นเร็วตามมาคือฝีมือคุณ")],
    },
    Line {
        key: "harm:iv push adrenaline — arrhythmia on a small beating heart",
        tr: &[("th", "ดัน adrenaline เข้าหลอดเลือดดำ — หัวใจดวงเล็กที่ยังเต้นอยู่เสียจังหวะ")],
    },
    Line {
        key: "threshold:the nurse holds the ampoule still: \"how much, doctor? she is twenty kilos.\" the drug waits on a number.",
        tr: &[("th", "พยาบาลถือหลอดยาค้างไว้: \"เท่าไหร่คะคุณหมอ น้องหนัก 20 กิโลกรัม\" ยารออยู่ที่ตัวเลข")],
    },
    Line {
        key: "threshold:— \"Twenty kilos, on the school scales last week,\" her mother says, already half-crying. The number every dose hangs on.",
        tr: &[("th", "— \"20 กิโลกรัมค่ะ ชั่งที่ตาชั่งโรงเรียนเมื่ออาทิตย์ที่แล้ว\" แม่ของน้องบอก น้ำตาคลอแล้ว ตัวเลขที่ทุกขนาดยาแขวนอยู่กับมัน")],
    },
    Line {
        key: "threshold:— \"Prawn fritters at the school fair. She knows she mustn't — she is six, she forgot to ask.\" Thirty minutes ago.",
        tr: &[("th", "— \"กุ้งชุบแป้งทอดที่งานโรงเรียนค่ะ น้องรู้ว่าห้ามกิน — แต่เด็ก 6 ขวบ ลืมถามก่อน\" เมื่อ 30 นาทีที่แล้ว")],
    },
    Line {
        key: "threshold:— \"Shrimp, since she was two. It is written on a card in her school bag.\"",
        tr: &[("th", "— \"แพ้กุ้งค่ะ ตั้งแต่ 2 ขวบ เขียนไว้ในบัตรที่อยู่ในกระเป๋านักเรียนของน้อง\"")],
    },
    Line {
        key: "threshold:wheals climbing her neck and arms, lips swelling as you watch — she keeps licking them",
        tr: &[("th", "ผื่นลมพิษไต่ขึ้นคอและแขน ริมฝีปากบวมขึ้นต่อหน้าต่อตา — เธอเอาแต่เลียริมฝีปากตัวเอง")],
    },
    Line {
        key: "threshold:expiratory wheeze both sides, tugging under the little ribs",
        tr: &[("th", "เสียงวี้ดตอนหายใจออกทั้งสองข้าง ชายโครงเล็ก ๆ บุ๋มตามการหายใจ")],
    },
    Line {
        key: "threshold:saline 20 mils a kilo — four hundred, warmed, running through a small pink cannula",
        tr: &[("th", "saline 20 มิลลิลิตรต่อกิโลกรัม — 400 มิลลิลิตร อุ่นแล้ว ไหลผ่านเข็มสีชมพูเส้นเล็ก")],
    },
    Line {
        key: "threshold:antihistamine for the itch — after the adrenaline, never instead of it",
        tr: &[("th", "ให้ antihistamine แก้คัน — หลัง adrenaline เท่านั้น ไม่ใช่ให้แทนกัน")],
    },
    Line {
        key: "threshold:steroid on board — for the late phase",
        tr: &[("th", "ให้ steroid ไปแล้ว — เผื่อระลอกหลัง")],
    },
    Line {
        key: "threshold:anaphylaxis — the season's first disease, child-sized",
        tr: &[("th", "anaphylaxis — โรคแรกของซีซัน ในขนาดของเด็ก")],
    },
    Line {
        key: "threshold:admitted with mum on the bed beside her — the biphasic wave finds the nurses ready",
        tr: &[("th", "รับไว้โดยมีแม่นอนอยู่บนเตียงข้าง ๆ — ระลอกสองมาเจอพยาบาลที่พร้อมแล้ว")],
    },
    Line {
        key: "harm:discharged during the observation window — biphasic reactions come back to children asleep in cars",
        tr: &[("th", "ให้กลับบ้านทั้งที่ยังอยู่ในช่วงเฝ้าสังเกตอาการ — biphasic reaction กลับมาหาเด็กที่หลับอยู่ในรถ")],
    },
    Line {
        key: "harm:adrenaline delayed — a child's airway closes faster than the argument about the dose",
        tr: &[("th", "ให้ adrenaline ช้าเกินไป — ทางเดินหายใจของเด็กปิดเร็วกว่าการเถียงกันเรื่องขนาดยา")],
    },

    // ── OSCE-D4 · Pranom, 72, drifting — her *niece* speaks — septic shock ──
    Line {
        key: "threshold:— the niece: \"Fever and shaking since yesterday, hardly any water passed. Her kidney stones ached all last week.\" A story with an address on it.",
        tr: &[("th", "— หลานสาว: \"มีไข้และหนาวสั่นตั้งแต่เมื่อวานค่ะ ปัสสาวะแทบไม่ออกเลย อาทิตย์ที่แล้วปวดนิ่วในไตทั้งอาทิตย์\" เรื่องเล่าที่มีที่อยู่กำกับมาด้วย")],
    },
    Line {
        key: "threshold:mottled knees, arms cold to the elbow, refill four seconds — the periphery closed its doors an hour ago",
        tr: &[("th", "เข่าเป็นลายด่าง แขนเย็นขึ้นมาถึงข้อศอก capillary refill 4 วินาที — ปลายมือปลายเท้าปิดประตูไปตั้งแต่ชั่วโมงที่แล้ว")],
    },
    Line {
        key: "threshold:she flinches through the drowsiness at the right loin — cva tenderness, the pus signing its address",
        tr: &[("th", "เธอสะดุ้งทั้งที่ยังซึมอยู่ เมื่อกดที่บั้นเอวขวา — CVA tenderness หนองเซ็นชื่อบอกที่อยู่ของมัน")],
    },
    Line {
        key: "threshold:lactate 5.6 — the tissues running on debt; a number to clear, not just to file",
        tr: &[("th", "lactate 5.6 — เนื้อเยื่อกำลังเดินด้วยหนี้ เป็นตัวเลขที่ต้องทำให้ลดลง ไม่ใช่แค่บันทึกเก็บไว้")],
    },
    Line {
        key: "threshold:two sets off different arms, urine bottled behind them — the lab gets its evidence before the bombardment",
        tr: &[("th", "เจาะเพาะเชื้อ 2 ชุดจากแขนคนละข้าง ตามด้วยเก็บปัสสาวะใส่ขวด — แล็บได้หลักฐานก่อนการระดมยิง")],
    },
    Line {
        key: "threshold:urine like weak tea, packed with white cells and bacteria — the source named on a strip",
        tr: &[("th", "ปัสสาวะสีเหมือนน้ำชาอ่อน อัดแน่นด้วยเม็ดเลือดขาวและแบคทีเรีย — ต้นตอถูกเรียกชื่อบนแถบตรวจ")],
    },
    Line {
        key: "threshold:two grey cannulas, one in each fold — the bore is the resuscitation",
        tr: &[("th", "เข็มสีเทา 2 เส้น ข้อพับข้างละเส้น — ขนาดรูเข็มคือการกู้ชีพ")],
    },
    Line {
        key: "threshold:meropenem dosed to the kidneys, running in the first bag — the golden hour spent on what kills the killer",
        tr: &[("th", "meropenem ปรับขนาดตามการทำงานของไต หยดอยู่ในถุงแรก — ใช้ golden hour ไปกับสิ่งที่ฆ่าตัวฆ่า")],
    },
    Line {
        key: "harm:the antibiotics flew before a single culture — the enemy will never be named",
        tr: &[("th", "ยาปฏิชีวนะพุ่งเข้าไปก่อนจะเก็บเพาะเชื้อสักชุด — ศัตรูจะไม่มีวันถูกเรียกชื่อ")],
    },
    Line {
        key: "threshold:thirty mils a kilo of warmed crystalloid through both barrels — the pressure lifts its head",
        tr: &[("th", "สารน้ำอุ่น 30 มิลลิลิตรต่อกิโลกรัม ผ่านทั้งสองเส้น — ความดันเงยหัวขึ้น")],
    },
    Line {
        key: "threshold:noradrenaline climbing by the minute — the map holds at 65 and stays there",
        tr: &[("th", "ปรับ noradrenaline ขึ้นทีละนาที — MAP ขึ้นไปอยู่ที่ 65 แล้วอยู่ตรงนั้น")],
    },
    Line {
        key: "threshold:pressors into an empty tank — fill her first: vasoconstriction has nothing to squeeze",
        tr: &[("th", "ให้ยาบีบหลอดเลือดในถังที่ว่างเปล่า — เติมสารน้ำให้เธอก่อน: การบีบหลอดเลือดไม่มีอะไรให้บีบ")],
    },
    Line {
        key: "threshold:a catheter for the hourly truth — fifteen mils the first hour, a kidney whispering",
        tr: &[("th", "ใส่สายสวนปัสสาวะเพื่อดูความจริงรายชั่วโมง — ชั่วโมงแรกได้ 15 มิลลิลิตร ไตกำลังกระซิบ")],
    },
    Line {
        key: "threshold:ct shows a stone corking the right ureter — urology books the theatre tonight: the pus gets a door",
        tr: &[("th", "CT พบนิ่วอุดท่อไตข้างขวาเหมือนจุกขวด — ศัลยกรรมทางเดินปัสสาวะจองห้องผ่าตัดคืนนี้: หนองจะได้มีทางออก")],
    },
    Line {
        key: "threshold:the unit accepts her, pump and all",
        tr: &[("th", "ICU รับเธอไว้ พร้อมเครื่องให้ยาทั้งชุด")],
    },
    Line {
        key: "threshold:septic shock from an obstructed, infected kidney — named with its source and its clock",
        tr: &[("th", "septic shock จากไตที่ติดเชื้อและมีการอุดกั้น — เรียกชื่อพร้อมต้นตอและนาฬิกาของมัน")],
    },
    Line {
        key: "harm:half the resuscitation is speed — five minutes of shock on dry lines",
        tr: &[("th", "ครึ่งหนึ่งของการกู้ชีพคือความเร็ว — ปล่อยให้ช็อกอยู่ 5 นาทีโดยไม่มีสารน้ำไหลเข้าเส้น")],
    },
    Line {
        key: "harm:the golden hour closed with no antimicrobial on board — every hour of delay stacks the odds against her",
        tr: &[("th", "golden hour ปิดลงโดยที่ยังไม่ได้ยาต้านจุลชีพ — ทุกชั่วโมงที่ช้าไป โอกาสรอดของเธอยิ่งลดลง")],
    },
    Line {
        key: "harm:the tank is full and the pressure still sags — vasoplegia does not answer to volume",
        tr: &[("th", "ถังเต็มแล้วแต่ความดันยังตก — vasoplegia ไม่ตอบสนองต่อสารน้ำ")],
    },
    Line {
        key: "harm:a dammed kidney reseeds the blood — the pressure sags against a rising pump",
        tr: &[("th", "ไตที่ถูกกั้นไว้ปล่อยเชื้อกลับเข้ากระแสเลือดอีกรอบ — ความดันตกสวนทางกับยาที่ปรับขึ้นเรื่อย ๆ")],
    },

    // ── EP2-EP5 · the episode automatons. Their thresholds are cutscene keys rather than
    //    prose — the page's own SAY table is what makes a sentence of them, and these rows are
    //    that sentence in Thai. The harms are prose and reach the feed directly. Ungendered,
    //    like the runtime's own vocabulary above: one row serves whichever patient earns it.
    Line { key: "threshold:stemi_recognised", tr: &[("th", "ECG บอกความจริง")] },
    Line {
        key: "harm:nitrate in RV infarct — preload collapse",
        tr: &[("th", "ให้ nitrate ในภาวะ RV infarct — preload ทรุดลง")],
    },
    Line { key: "threshold:deteriorate", tr: &[("th", "อาการทรุดลง")] },
    Line { key: "threshold:reperfusion", tr: &[("th", "เลือดกลับไปเลี้ยงกล้ามเนื้อหัวใจ")] },
    Line { key: "threshold:rosc", tr: &[("th", "จังหวะการเต้นของหัวใจกลับมา")] },
    Line {
        key: "harm:unsynchronised shock to a perfusing rhythm",
        tr: &[("th", "ช็อกไฟฟ้าแบบไม่ sync ใส่จังหวะหัวใจที่ยังมีเลือดไปเลี้ยง")],
    },
    Line { key: "threshold:delay", tr: &[("th", "ล่าช้า")] },
    Line { key: "threshold:code_blue", tr: &[("th", "มอนิเตอร์ร้องเตือน — code blue")] },

    Line {
        key: "harm:throat examination provoked laryngospasm",
        tr: &[("th", "การตรวจในลำคอกระตุ้นให้กล่องเสียงหดเกร็ง")],
    },
    Line {
        key: "harm:cannulation distressed the child",
        tr: &[("th", "การแทงเส้นทำให้เด็กตื่นกลัวและร้องดิ้น")],
    },
    Line { key: "harm:separating the child from the mother", tr: &[("th", "พรากเด็กออกจากแม่")] },
    Line { key: "threshold:team_called", tr: &[("th", "เรียกทีมมาแล้ว")] },
    Line { key: "threshold:airway_secured", tr: &[("th", "ทางเดินหายใจปลอดภัยแล้ว")] },
    Line {
        key: "harm:uncontrolled intubation attempt",
        tr: &[("th", "พยายามใส่ท่อช่วยหายใจโดยไม่มีการควบคุม")],
    },
    Line { key: "threshold:going_quiet", tr: &[("th", "ผู้ป่วยเงียบลง — นั่นแย่กว่าเดิม")] },

    Line { key: "threshold:suspicion_raised", tr: &[("th", "เริ่มสงสัยแล้ว")] },
    Line { key: "threshold:diagnosis", tr: &[("th", "ได้การวินิจฉัยแล้ว")] },
    Line { key: "threshold:rescue", tr: &[("th", "ให้การรักษาแบบกู้ชีพ")] },
    Line {
        key: "harm:systemic thrombolysis without haemodynamic indication",
        tr: &[("th", "ให้ยาละลายลิ่มเลือดทั้งระบบโดยไม่มีข้อบ่งชี้ทางระบบไหลเวียนโลหิต")],
    },
    Line { key: "threshold:looks_normal", tr: &[("th", "ดูเหมือนปกติ")] },
    Line {
        key: "harm:discharged with an undiagnosed pulmonary embolism",
        tr: &[("th", "ให้กลับบ้านทั้งที่ยังไม่ได้วินิจฉัย pulmonary embolism")],
    },
    Line { key: "threshold:collapse", tr: &[("th", "ผู้ป่วยทรุดลง")] },

    Line { key: "threshold:triage_done", tr: &[("th", "คัดแยกผู้ป่วยเรียบร้อยแล้ว")] },
    Line { key: "threshold:haemostasis", tr: &[("th", "ห้ามเลือดได้แล้ว")] },
    Line {
        key: "harm:crystalloid resuscitation in haemorrhagic shock — dilutional coagulopathy",
        tr: &[("th", "กู้ชีพด้วยสารน้ำในภาวะช็อกจากการเสียเลือด — เลือดเจือจางจนแข็งตัวไม่ได้")],
    },
    Line { key: "threshold:to_theatre", tr: &[("th", "ส่งเข้าห้องผ่าตัด")] },
    Line {
        key: "harm:moved to theatre without controlling the bleeding",
        tr: &[("th", "ส่งเข้าห้องผ่าตัดโดยยังไม่ได้ห้ามเลือด")],
    },
    Line { key: "threshold:others_waiting", tr: &[("th", "ยังมีคนอื่นรออยู่")] },
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
    ///
    /// `kl` is the case worth naming: a real tag, correctly formed, for a language this build
    /// has no pack for. It is the *only* reason to fall back now that `th-TH` and `TH` resolve
    /// (see `a_thai_tag_reaches_thai_whatever_shape_it_arrives_in`) — widening the match moved
    /// the variants of a language we have onto its pack, and moved nothing else.
    ///
    /// **And it falls back to the original, never to a blank.** That is the half worth pinning:
    /// an unknown tag resolves to the default, `beat` returns `None` for the default by
    /// construction, and every table hands back an empty *pack* rather than a pack of empty
    /// strings. A bay whose seals and beats came back as `""` would look like a case with
    /// nothing to say rather than a case nobody has translated.
    #[test]
    fn an_unknown_language_falls_back_rather_than_failing() {
        for bad in [
            None,
            Some(""),
            Some("kl"),
            // A region on a language we still do not have is no more matchable than the bare
            // tag: widening the match must not turn an unknown language into a known one.
            Some("kl-GL"),
            Some("KL"),
            Some("-"),
            Some("../../etc/passwd"),
        ] {
            let l = language(bad);
            assert_eq!(l.id, default_language().id, "{bad:?} did not fall back");
            // Nothing on the wire, so the page keeps the English it already holds.
            let p = pack(l);
            for t in ["asks", "ui", "kit"] {
                assert!(
                    p[t].as_object().is_some_and(serde_json::Map::is_empty),
                    "{bad:?}: {t} came back as a table of blanks rather than as no table"
                );
            }
            for line in BEATS {
                assert_eq!(beat(l, line.key), None, "{bad:?} claimed a translation for {}", line.key);
            }
        }
        assert_eq!(language(Some("th")).id, "th");
    }

    /// A Thai speaker gets a Thai patient however the tag reached us.
    ///
    /// This is the bug this test exists for, and it was invisible from the inside: the picker
    /// only ever sends `th`, so every hand-run check passed while the three tags that arrive
    /// from *outside* the picker — a typed URL, `navigator.language` (`th-TH` on a Thai phone,
    /// never bare `th`), a POSIX-shaped `th_TH` — landed a Thai learner on an English patient
    /// with the whole Thai pack sitting one row away. Nothing failed; the patient just answered
    /// in English, which reads as a broken patient rather than an unparsed tag.
    ///
    /// Resolving is not enough on its own, so this checks the pack came with it: an id of `th`
    /// and an empty `asks` would be the same silence in a different place.
    #[test]
    fn a_thai_tag_reaches_thai_whatever_shape_it_arrives_in() {
        let th = language(Some("th"));
        for tag in ["th", "th-TH", "th_TH", "TH", "Th", "tH-th", "th-Thai-TH", "th-"] {
            let l = language(Some(tag));
            assert_eq!(l.id, "th", "{tag} did not reach Thai — an English patient for a Thai learner");
            // The id alone proves nothing; the pack is what the bedside actually speaks.
            let p = pack(l);
            assert_eq!(p["lang"], "th", "{tag}: the page would keep asking under the variant");
            for t in ["asks", "ui", "kit"] {
                assert!(
                    p[t].as_object().is_some_and(|m| !m.is_empty()),
                    "{tag}: resolved to Thai and came back with an empty {t}"
                );
            }
            // And the same beats, not merely a language that claims the same id.
            assert_eq!(beat(l, "threshold:biphasic"), beat(th, "threshold:biphasic"), "{tag}");
        }
        // The default is matched the same way, so an `en-GB` browser is not a fallback by luck.
        for tag in ["en", "en-GB", "EN", "en_US"] {
            assert_eq!(language(Some(tag)).id, "en", "{tag}");
        }
        // Region tolerance is not prefix matching. `tha` is a different subtag from `th` —
        // matching it would hand `thai-something` to Thai and, the day a `t` language lands,
        // hand it every tag beginning with `t`.
        assert_eq!(language(Some("tha")).id, default_language().id, "a longer subtag matched");
        assert_eq!(language(Some("t")).id, default_language().id, "a shorter subtag matched");
        assert_eq!(language(Some("thai")).id, default_language().id, "a longer subtag matched");
    }

    /// A beat with no row still shows the original rather than nothing.
    ///
    /// Every case this server plays is translated now, so the fallback has no case left to
    /// exercise — which is precisely why it is pinned here on invented strings instead. It is
    /// the path a *future* case takes between the day its file lands and the day its rows do,
    /// and `beat` returning `None` (show the English the author wrote) rather than `Some("")`
    /// is the difference between an untranslated line and a blank one.
    #[test]
    fn a_beat_with_no_translation_asks_for_the_original() {
        let th = language(Some("th"));
        assert!(beat(th, "threshold:biphasic").is_some(), "EP1's own beat is the worked example");
        // Deliberately not near-misses of real keys: a string that merely *looks* like a beat
        // this table holds would pass for the wrong reason the day somebody edits the real one.
        for untranslated in [
            "threshold:a case that has not been written yet",
            "harm:a harm no scenario declares",
            "status:Bewildered",
            "",
        ] {
            assert_eq!(beat(th, untranslated), None, "{untranslated} claimed a translation");
        }
        // Lookup is exact, and a prefix of a real key is not that key. Worth pinning: several
        // beats below share a long opening clause with another, and a `starts_with` here would
        // hand one station's line to a different station's patient.
        assert!(beat(th, "harm:no steroid by the seventh minute").is_none(), "a prefix matched");
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

    /// ── the half-translated station, made impossible ─────────────────────────
    ///
    /// Where this table's rows come from, checked against the files they came from. It reads
    /// every case on the shelf off disk with the runtime's own parser, asks it for every beat it
    /// could ever emit, and fails if one of them has no Thai.
    ///
    /// The bug it closes was found on production and it is worth stating plainly, because the
    /// shape of it is what makes a *test* the right answer rather than a careful edit. A
    /// candidate started OSCE-A in Thai. The page was Thai, the chips were Thai, the free-text
    /// answers came back Thai — and then they pressed `ask_breathing`, the fastest control in
    /// the bay, and the patient replied `— "Tight. I can hear myself whistle."` in English. The
    /// table had three rows against two hundred and forty-five beats, and nothing anywhere said
    /// so: an untranslated beat renders as its English original, which is a legitimate state for
    /// a case nobody has translated yet and completely indistinguishable, on screen, from a case
    /// somebody translated *most* of.
    ///
    /// So the invariant is all-or-nothing per shelf, not per line, and it is enforced from the
    /// scenario files rather than from a list kept beside them. A new station cannot ship half
    /// translated; a new *beat* inside an existing station cannot either.
    ///
    /// Deliberately not covered: `conformance/sce-archive`, which holds superseded versions of
    /// live cases for verifiers to replay old tapes against. Those are not on the shelf and
    /// `/api/new` cannot be asked for one.
    #[test]
    fn every_scripted_beat_of_every_case_has_a_thai_line() {
        let th = language(Some("th"));
        let (mut cases, mut beats) = (0usize, 0usize);
        for path in case_files() {
            let raw = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let sce = vitals_sce::Sce::from_json(&raw)
                .unwrap_or_else(|e| panic!("{} is not a scenario: {e}", path.display()));
            cases += 1;
            for key in scripted_beats(&sce) {
                beats += 1;
                assert!(
                    beat(th, &key).is_some(),
                    "{}: no Thai for {key:?}\n\
                     A Thai candidate reads this line in English, mid-station, with no sign that \
                     anything is wrong. Add a row to BEATS keyed by exactly this string.",
                    path.display()
                );
            }
        }
        // A guard that reads no files passes every time. These are floors, not counts: adding a
        // case raises them, and only *removing* one — a deliberate act — trips them.
        assert!(cases >= 17, "only {cases} case files found — the shelf moved and this guard went blind");
        assert!(beats >= 240, "only {beats} beats found — the parser stopped seeing effects");
    }

    /// Every case on the shelf, in the order `/api/new` can be asked for one.
    ///
    /// Read from the directories rather than from a list, so a station that lands tomorrow is
    /// covered by the guard above the day its file appears and not the day somebody remembers to
    /// name it here. `demo/ep1-en.json` is absent on purpose: it is EP1's *text* layer — the
    /// persona and the cutscene map — and carries no automaton at all.
    fn case_files() -> Vec<std::path::PathBuf> {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut v = Vec::new();
        for dir in ["demo/stations", "demo/scenarios"] {
            let d = root.join(dir);
            let entries = std::fs::read_dir(&d).unwrap_or_else(|e| panic!("{}: {e}", d.display()));
            for e in entries {
                let p = e.expect("a directory entry").path();
                if p.extension().is_some_and(|x| x == "json") {
                    v.push(p);
                }
            }
        }
        // EP1 plays from the conformance case itself — the file the anchored vectors were
        // written against — rather than from `demo/`. See `scenario_path` in `main.rs`.
        v.push(root.join("conformance/sce-anaphylaxis-ep1.json"));
        v.sort();
        v
    }

    /// Every beat one case can put on screen, spelled the way `render_beat` spells it.
    ///
    /// Typed off `vitals_sce::Sce` rather than walked over raw JSON, so this sees exactly what
    /// the runtime sees: an `Effect::Beat` is a `threshold:`, an `Effect::Harm` and an
    /// intervention's own `harm` are a `harm:`, and a beat buried in the arm of a branch counts
    /// like any other. A JSON walk would also have picked up EP1's `cutscenes.harm`, which is a
    /// clip name and not a beat at all.
    fn scripted_beats(sce: &vitals_sce::Sce) -> Vec<String> {
        use vitals_sce::schema::Effect;
        fn walk(es: &[Effect], out: &mut Vec<String>) {
            for e in es {
                match e {
                    Effect::Beat { beat } => out.push(format!("threshold:{beat}")),
                    Effect::Harm { harm } => out.push(format!("harm:{harm}")),
                    Effect::Branch { branch, els } => {
                        for a in branch {
                            walk(&a.then, out);
                        }
                        walk(els, out);
                    }
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        for i in &sce.interventions {
            if let Some(h) = &i.harm {
                out.push(format!("harm:{h}"));
            }
            walk(&i.effects, &mut out);
        }
        for s in &sce.states {
            for t in &s.transitions {
                walk(&t.doo, &mut out);
            }
        }
        for t in &sce.triggers {
            walk(&t.doo, &mut out);
        }
        out.sort();
        out.dedup();
        out
    }

    /// ── the English that stays, and the English that is a bug ────────────────
    ///
    /// A Thai row has to be Thai. The failure this catches is not a typo: it is a row that was
    /// filled in by copying the key across, which passes every other test in this file — the
    /// key exists, the language exists, the string is non-empty — and reads on screen exactly
    /// like the untranslated line it was supposed to replace.
    ///
    /// The rule is a **run length**, not a word list, because a word list of every clinical term
    /// a case may use is a second table to keep in sync and it would be wrong within a month.
    /// What is actually true of this material is simpler: the English a Thai clinician says out
    /// loud is a *term* — `SpO2`, `steeple sign`, `low-molecular-weight heparin`, `acute
    /// ST-elevation MI` — and the longest of them is three words. Four English words in a row,
    /// unbroken by a Thai one, is a sentence, and a sentence is the bug.
    ///
    /// Numbers are skipped rather than counted, so `adrenaline 0.5 mg IM` reads as the three
    /// words it is; an em dash or an ellipsis ends a run, because those are the beat's own
    /// structural joints and the observation after one is a fresh clause.
    #[test]
    fn a_thai_line_is_thai_with_clinical_terms_in_it_and_never_an_english_sentence() {
        const MOST_ENGLISH_WORDS_IN_A_ROW: usize = 3;
        let is_thai = |c: char| ('\u{0E00}'..='\u{0E7F}').contains(&c);
        for table in [BEATS, ASKS, UI, KIT] {
            for line in table {
                for (lg, text) in line.tr {
                    let l = language(Some(lg));
                    if l.script != Script::Thai {
                        continue;
                    }
                    // ๐-๙ never, even inside a Thai sentence: a dose, an age and a saturation
                    // are read off the same screen as the chart, and the chart is Arabic.
                    assert!(
                        !text.chars().any(|c| ('\u{0E50}'..='\u{0E59}').contains(&c)),
                        "{}: {text:?} uses Thai numerals",
                        line.key
                    );
                    // `defib.label` is one deliberate word of English on a Thai tray — the verb
                    // a Thai resus team shouts — so a *whole* row of English is allowed to be a
                    // term. Four in a row is not.
                    let mut run = 0usize;
                    for tok in text.split_whitespace() {
                        if tok.chars().any(is_thai) || tok.contains('—') || tok.contains('…') {
                            run = 0;
                        } else if tok.chars().any(|c| c.is_ascii_alphabetic()) {
                            run += 1;
                            assert!(
                                run <= MOST_ENGLISH_WORDS_IN_A_ROW,
                                "{}: {text:?} runs {run} English words together — that is a \
                                 sentence, not a clinical term",
                                line.key
                            );
                        }
                    }
                }
            }
        }
        // And a beat, specifically, is the patient talking: every one of its rows says something
        // in Thai. (The kit tray is exempt by the row above it — "Defibrillate" is the label.)
        for line in BEATS {
            for (lg, text) in line.tr {
                if language(Some(lg)).script == Script::Thai {
                    assert!(
                        text.chars().any(is_thai),
                        "{}: {text:?} is a Thai row with no Thai in it",
                        line.key
                    );
                }
            }
        }
    }
}
