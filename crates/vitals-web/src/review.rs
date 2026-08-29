//! Reviewer feedback — what a clinician or a student sends back after working through the brief.
//!
//! One document per submission. Reviewers are few and each writes once or twice, so there is
//! nothing here worth the races a shared append-only document would invite: two people finishing
//! in the same second must not be able to overwrite each other's answers, and separate keys make
//! that impossible rather than unlikely.
//!
//! Nothing here is a clinical record. It is opinion about the product, kept so the team can act
//! on it and so a reviewer never has to say the same thing twice.
//!
//! # The two ways in
//!
//! `/api/review` is the first route in this server that reads a request body — every other one
//! takes query parameters, and a reviewer's answers are roughly 72KB once percent-encoded, which
//! fits in no URL. It is public on purpose and has no caller identity at all: a reviewer should
//! not need an account to tell us what is wrong, `role` and `name` are self-declared attribution
//! rather than authentication, and `name` is optional so an uncomfortable answer can still be
//! sent. See the route in `main.rs` for why no existing identity mechanism was borrowed for it.
//!
//! The other way in stays open, because a reviewer is not always somewhere the server is:
//! `static/review.html` hands the answers back as JSON when it was not served by us, and
//! `scripts/file-review.py` files that JSON under the same key this module derives — byte for
//! byte, and with the same replace-in-place rule as [`Submission::file`] — so the same review
//! arriving by both routes cannot become two records.

use serde::{Deserialize, Serialize};

use crate::store::Store;

pub const KIND: &str = "review";

/// Who is answering. The two roles carry different weight and must not be conflated: a student's
/// answer is evidence about the learner experience, a physician's is the clinical sign-off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Student,
    Physician,
}

impl Role {
    /// The wire spelling, and the one that goes into [`Submission::identity`].
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Student => "student",
            Role::Physician => "physician",
        }
    }

    pub fn parse(s: &str) -> Option<Role> {
        match s {
            "student" => Some(Role::Student),
            "physician" => Some(Role::Physician),
            _ => None,
        }
    }
}

/// One answer, kept as the reviewer wrote it. The question id travels with it so a later edit to
/// the form cannot silently re-label an old answer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Answer {
    pub id: String,
    /// The item as it was shown — not the question line alone, but the four lines the review
    /// documents put in front of every ruling: what the system does now, why we think it may be
    /// wrong, what we would change it to, and the question.
    ///
    /// Storing all of it is what lets a reviewer work from the form on a phone with no document
    /// open beside them, and what lets the answer still be read a month later against the thing
    /// it was about. The clamp is [`ASKED_MAX`], sized for those four lines rather than for a
    /// one-line question.
    pub asked: String,
    /// Which of the item's named answers was picked, by id — `""` when the item offered none, or
    /// when the reviewer only wrote prose.
    ///
    /// The review documents keep asking for a branch: *(ก) or (ข)*, *asystole or PEA*, *confirm
    /// or change*. Reading that back out of prose means someone re-reading every answer to find
    /// out which way a ruling went, which is exactly the work these documents were written to
    /// avoid. So the branch is stored as a value.
    ///
    /// It is also what makes **agreement recordable**. Both documents say plainly that "what you
    /// are doing now is correct" is an answer worth as much as any other; with prose alone it
    /// arrives as silence, indistinguishable from an item the reviewer never reached, and we
    /// would go back and ask them a second time.
    #[serde(default)]
    pub chose: String,
    /// That option's wording as it was shown, kept for the same reason [`Answer::asked`] is: an
    /// id means nothing on its own, and a later edit to the form must not be able to re-label a
    /// choice somebody already made.
    #[serde(default)]
    pub chose_label: String,
    pub said: String,
}

impl Answer {
    /// Did the reviewer do anything here? Picking an option counts, on its own.
    fn answered(&self) -> bool {
        !self.said.trim().is_empty() || !self.chose.is_empty()
    }
}

/// How much of one item's shown text is kept.
///
/// Four lines of context, not a question. `osce-d3`'s allergy ruling quotes the teacher's script
/// and the station's contradicting line; หมวด 2 quotes a four-row table of heart rates. Four
/// hundred characters — the clamp this field shipped with — held about a third of the shortest of
/// them, and would have cut the rest away silently, leaving an answer whose question nobody could
/// reconstruct. `tests/page.rs` holds every item the form actually asks under this number, so a
/// question that outgrows it fails a gate instead of losing its context on the way to disk.
pub const ASKED_MAX: usize = 4000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Submission {
    pub id: String,
    /// Unix seconds, server-side. A client clock is not evidence of anything.
    pub at: u64,
    pub role: Role,
    /// Optional on purpose — a reviewer who wants to say something uncomfortable should be able
    /// to, and an unsigned answer is still worth reading.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub contact: String,
    /// Set by a physician who reviewed but does not want to be named in the rubric files.
    #[serde(default)]
    pub anonymous: bool,
    #[serde(default)]
    pub answers: Vec<Answer>,
    /// Anything that did not fit a question. Often the most useful field.
    #[serde(default)]
    pub notes: String,
    /// Which build they saw, so an answer can be read against the thing that produced it.
    #[serde(default)]
    pub revision: String,
}

impl Submission {
    /// A key that sorts by time and is derived from the content.
    ///
    /// Time alone is not enough: two reviewers finishing in the same second would land on the same
    /// key and the second write would erase the first. Hashing the content makes a resend
    /// *identifiable* — two keys sharing a suffix are the same answers twice — which is not the
    /// same as making it idempotent, and the difference is [`Submission::file`]'s job. The key
    /// alone cannot do it: a reviewer who taps Send again two seconds later is at a different
    /// second, so the key is different, so the write lands beside the first instead of on it.
    fn key(at: u64, content: &str) -> String {
        use sha2::{Digest, Sha256};
        let d = Sha256::digest(content.as_bytes());
        format!(
            "{at:010}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            d[0], d[1], d[2], d[3], d[4], d[5]
        )
    }

    /// Build from the posted JSON. Everything is clamped here rather than trusted: this endpoint
    /// is public, and a review form is exactly the shape of thing that attracts junk.
    pub fn from_json(v: &serde_json::Value, at: u64) -> Result<Submission, &'static str> {
        let role = v.get("role").and_then(|r| r.as_str()).and_then(Role::parse).ok_or("role")?;
        let take = |k: &str, max: usize| -> String {
            v.get(k)
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .chars()
                .take(max)
                .collect()
        };
        let mut answers = Vec::new();
        if let Some(arr) = v.get("answers").and_then(|a| a.as_array()) {
            for a in arr.iter().take(80) {
                let field = |k: &str, max: usize| -> String {
                    a.get(k).and_then(|x| x.as_str()).unwrap_or("").chars().take(max).collect()
                };
                let answer = Answer {
                    id: field("id", 64),
                    asked: field("asked", ASKED_MAX),
                    chose: field("chose", 32),
                    chose_label: field("chose_label", 300),
                    said: field("said", 4000),
                };
                // An unanswered question is still not an answer — but *picking an option* is an
                // answer even with the box left empty, and it is the most common one the two
                // review documents ask for: "what you are doing now is correct". Dropping it
                // here would put that back where it was, indistinguishable from silence.
                if !answer.answered() {
                    continue;
                }
                answers.push(answer);
            }
        }
        let notes = take("notes", 8000);
        if answers.is_empty() && notes.trim().is_empty() {
            return Err("empty");
        }
        let mut s = Submission {
            // Filled in below, from the finished record. The key cannot be assembled here from
            // loose locals without writing out the list of identifying fields a second time, and
            // a second list is exactly how `role` came to be in one of them and not the other.
            id: String::new(),
            at,
            role,
            name: take("name", 120),
            contact: take("contact", 200),
            anonymous: v.get("anonymous").and_then(|x| x.as_bool()).unwrap_or(false),
            answers,
            notes,
            revision: take("revision", 64),
        };
        s.id = Submission::key(at, &s.identity());
        Ok(s)
    }

    /// The half of the key that is a hash of the content, without the second it arrived in.
    fn fingerprint(&self) -> &str {
        self.id.split_once('-').map(|(_, h)| h).unwrap_or("")
    }

    /// **Everything that makes this review this review and not another one.**
    ///
    /// One definition, read in exactly two places: hashed into the key, and compared when a
    /// resend is matched against what is already filed. It is one function because it was two,
    /// and the two disagreed. The sameness test compared `role`; the key never hashed it. So a
    /// physician and a student who answered the same question the same way in the same second
    /// computed the *same key*, the sameness test correctly said they were different reviews —
    /// and the write went to the key anyway, replacing one with the other. Both were told 200.
    /// That was not a hypothetical: the first production smoke test did it, and the physician's
    /// record is gone.
    ///
    /// Fields are length-prefixed rather than separator-joined. A separator scheme is unambiguous
    /// only while nobody types the separator, and being unambiguous is this string's entire job.
    ///
    /// **What is deliberately absent, and why.** `contact`, `anonymous` and `revision` are not
    /// here, and that is the difference between *a different review* and *the same review, sent
    /// again with a correction*. A physician who resends after fixing a typo in their LINE id, or
    /// after ticking “do not name me”, has not written a second review — and because these are
    /// absent, [`Submission::file`] recognises the resend and updates the record in place instead
    /// of filing them twice. `Answer::asked` and `Answer::chose_label` are absent for a related
    /// reason: they are the *form's* words, not the reviewer's, and an edit to the form's wording
    /// must not turn one review into two.
    ///
    /// The limit this leaves, stated rather than hidden: two different people who leave the name
    /// blank and answer identically are one record here. Nothing on this endpoint can tell them
    /// apart — it has no caller identity by design — and merging is the safer of the two wrong
    /// answers, because the alternative duplicates every honest resend.
    fn identity(&self) -> String {
        let mut out = String::new();
        fn field(out: &mut String, s: &str) {
            out.push_str(&s.chars().count().to_string());
            out.push(':');
            out.push_str(s);
        }
        field(&mut out, self.role.as_str());
        field(&mut out, &self.name);
        field(&mut out, &self.notes);
        field(&mut out, &self.answers.len().to_string());
        for a in &self.answers {
            field(&mut out, &a.id);
            field(&mut out, &a.chose);
            field(&mut out, &a.said);
        }
        out
    }

    /// The same review from the same person, sent again.
    ///
    /// Exact string equality on [`Submission::identity`], not a hash comparison: the six bytes in
    /// the key are enough to *find* a candidate cheaply and never enough to act on one, because
    /// acting on a collision would overwrite a different reviewer's answers with this one's.
    fn says_the_same_as(&self, other: &Submission) -> bool {
        self.identity() == other.identity()
    }

    /// File this submission, recognising a resend of one already stored.
    ///
    /// The one way to write a review, because two ways is how a store ends up with two records of
    /// one thing. Tapping Send again on a bad connection is the ordinary case rather than the rare
    /// one — the button is at the end of an hour of typing, and the reviewer has no way to tell a
    /// slow reply from a lost one. Two records of one review is not a tidiness problem: these are
    /// read to decide what to change, and one physician read twice looks exactly like two
    /// physicians agreeing.
    ///
    /// A match is **replaced in place**, keeping the key and the arrival time it already had, so
    /// the record still says when the review first came in and the file still agrees with its own
    /// `id`. Replaced rather than skipped: a physician who resends after ticking *do not name me*
    /// changed something the key does not hash, and dropping that send would leave them named.
    ///
    /// Returns the record as it was actually stored — which is what the reply should quote, since
    /// on a resend it is not the record that was handed in.
    pub fn file(&self, store: &Store) -> std::io::Result<Submission> {
        let mut out = self.clone();
        // Keys first, records second. This runs on a public endpoint on every submission, and a
        // review can be a fifth of a megabyte; only the one key that could be a resend is read.
        let mine = format!("-{}", self.fingerprint());
        for key in store.keys(KIND).into_iter().filter(|k| k.ends_with(&mine)) {
            match store.get::<Submission>(KIND, &key) {
                Some(prev) if prev.says_the_same_as(self) => {
                    out.id = key;
                    out.at = prev.at;
                    break;
                }
                // A record this build cannot read is not a record it may overwrite.
                _ => continue,
            }
        }
        store.put(KIND, &out.id, &out)?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(extra: &str) -> serde_json::Value {
        serde_json::from_str(&format!(
            r#"{{"role":"student","name":"มุก","answers":[{{"id":"q1","asked":"เหมือน ER จริงไหม","said":"เหมือนมาก"}}]{extra}}}"#
        ))
        .unwrap()
    }

    #[test]
    fn keeps_what_was_asked_alongside_the_answer() {
        let s = Submission::from_json(&body(""), 1_700_000_000).unwrap();
        assert_eq!(s.answers[0].asked, "เหมือน ER จริงไหม");
        assert_eq!(s.answers[0].said, "เหมือนมาก");
        assert_eq!(s.role, Role::Student);
    }

    #[test]
    fn thai_survives_the_length_clamp() {
        // Clamping by chars, not bytes — a byte clamp cuts a Thai character in half and the
        // record comes back as mojibake, which is worse than a truncated one.
        let long = "ก".repeat(9000);
        let v: serde_json::Value = serde_json::from_str(&format!(
            r#"{{"role":"student","notes":"{long}"}}"#
        ))
        .unwrap();
        let s = Submission::from_json(&v, 1).unwrap();
        assert_eq!(s.notes.chars().count(), 8000);
        assert!(s.notes.starts_with('ก'));
    }

    #[test]
    fn blank_submission_is_refused() {
        let v: serde_json::Value = serde_json::from_str(r#"{"role":"physician"}"#).unwrap();
        assert_eq!(Submission::from_json(&v, 1).unwrap_err(), "empty");
    }

    #[test]
    fn unknown_role_is_refused() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"role":"dean","notes":"hello"}"#).unwrap();
        assert_eq!(Submission::from_json(&v, 1).unwrap_err(), "role");
    }

    /// **Agreeing is an answer, and it must not read as silence.**
    ///
    /// Both review documents say it in as many words — "ตอบว่า 'ที่ทำอยู่นั่นแหละถูกแล้ว' ก็เป็น
    /// คำตอบที่มีค่าเท่ากันครับ". With prose alone that answer arrives as an empty box, gets
    /// dropped as unanswered, and is indistinguishable from a ruling the physician never reached
    /// — so we would go back and ask him a second time for something he already told us.
    #[test]
    fn agreeing_with_the_current_behaviour_is_recorded() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"role":"physician","answers":[
                 {"id":"r-ep4","asked":"ep4 — pulmonary embolism","chose":"pea",
                  "chose_label":"ยืนยัน — PEA","said":""},
                 {"id":"r-ep5","asked":"ep5 — บาดเจ็บ","said":"  "}
               ]}"#,
        )
        .unwrap();
        let s = Submission::from_json(&v, 1).unwrap();
        assert_eq!(s.answers.len(), 1, "a picked option was dropped as if nothing was said");
        assert_eq!(s.answers[0].id, "r-ep4");
        assert_eq!(s.answers[0].chose, "pea");
        // The wording travels with the id for the same reason the question does: an id alone
        // cannot be read, and a later edit to the form must not re-label an old choice.
        assert_eq!(s.answers[0].chose_label, "ยืนยัน — PEA");
        assert!(s.answers[0].said.is_empty());
    }

    /// Two physicians ruling opposite ways on the same item, neither writing prose, must not
    /// land on one key — which is what happens if only the ids and the empty boxes are hashed.
    #[test]
    fn opposite_branches_of_one_ruling_are_two_records() {
        let one = |chose: &str| -> Submission {
            let v: serde_json::Value = serde_json::from_str(&format!(
                r#"{{"role":"physician","answers":[{{"id":"win-means","asked":"“ชนะ” แปลว่าอะไร","chose":"{chose}","said":""}}]}}"#
            ))
            .unwrap();
            Submission::from_json(&v, 500).unwrap()
        };
        assert_ne!(one("a").id, one("b").id, "two opposite rulings hashed to one key");
        assert_eq!(one("a").id, one("a").id);
    }

    /// The four lines the review documents put in front of every ruling have to survive. The
    /// clamp this field shipped with held four hundred characters, which is a question and not
    /// an item — and it cut silently, so the answer would have arrived without the thing it was
    /// an answer to.
    #[test]
    fn an_item_keeps_the_context_and_not_just_the_question() {
        let ctx = "ตอนนี้: ".to_string() + &"ก".repeat(1500) + "\nคำถาม: ควรเป็นเท่าไหร่ครับ";
        let v: serde_json::Value = serde_json::json!({
            "role": "physician",
            "answers": [{ "id": "gcs-cases", "asked": ctx, "said": "ลง 1 แต้มทุก 2 นาที" }],
        });
        let s = Submission::from_json(&v, 1).unwrap();
        assert_eq!(s.answers[0].asked, ctx, "the item's context was cut");
        assert!(ctx.chars().count() > 400, "this test has to exceed the old clamp to mean anything");
    }

    #[test]
    fn empty_answers_are_dropped_not_stored() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"role":"student","answers":[{"id":"q1","asked":"a","said":"  "},{"id":"q2","asked":"b","said":"ตอบ"}]}"#,
        )
        .unwrap();
        let s = Submission::from_json(&v, 1).unwrap();
        assert_eq!(s.answers.len(), 1);
        assert_eq!(s.answers[0].id, "q2");
    }

    #[test]
    fn two_submissions_in_the_same_second_do_not_collide() {
        let a = Submission::from_json(&body(r#","notes":"หนึ่ง""#), 42).unwrap();
        let b = Submission::from_json(&body(r#","notes":"สอง""#), 42).unwrap();
        assert!(a.id.starts_with("0000000042-"));
        assert_ne!(a.id, b.id, "different answers in the same second must not overwrite each other");
    }

    #[test]
    fn sending_the_same_thing_twice_writes_one_record() {
        // A reviewer on a bad connection taps Send again. That must not double-count them.
        let a = Submission::from_json(&body(r#","notes":"เหมือนกัน""#), 99).unwrap();
        let b = Submission::from_json(&body(r#","notes":"เหมือนกัน""#), 99).unwrap();
        assert_eq!(a.id, b.id);
    }

    /// A full submission with every field populated, for the decision table below.
    fn full() -> serde_json::Value {
        serde_json::json!({
            "role": "physician",
            "name": "นพ.ศิรวิทย์ ตันศิริ",
            "contact": "LINE: sirawit.t",
            "anonymous": false,
            "answers": [{
                "id": "r-ep2", "asked": "1.1 · ep2 — STEMI, จุดจบที่หัวใจหยุดเต้น",
                "chose": "asys", "chose_label": "ยืนยัน — asystole เป็นจุดจบร่วมของทั้งสองทาง",
                "said": "ตกลงตามนี้ครับ"
            }],
            "notes": "หมายเหตุเพิ่มเติม",
            "revision": "vitals 0.6.0"
        })
    }

    /// **Which fields make two submissions two reviews, and which make them one review twice.**
    ///
    /// The table exists because `role` was missing from it and nobody could see that it was. The
    /// sameness test compared `role`, the key did not hash it, and the first production smoke
    /// test — a physician and a student answering one question the same way in the same second —
    /// put both on one key and left one record. The physician's answers are gone and the server
    /// returned 200 to both of them.
    ///
    /// So the question is not "is `role` in the key now". It is "is there a third field with the
    /// same property", and the only way to answer that once is to write down the whole table and
    /// let a new field fail it. A field added to `Submission` and to `identity` fails the second
    /// half; a field added to `Submission` and *not* to `identity` fails the first.
    #[test]
    fn the_fields_that_make_two_reviews_two_are_exactly_these() {
        let base = Submission::from_json(&full(), 1_000).unwrap();

        // ── identifying: change it and this is a different review ───────────────────────────
        // Each of these is something a reviewer chose or wrote. Two submissions differing in any
        // one of them are two people, or one person who changed their answer — never a resend.
        let identifying: Vec<(&str, serde_json::Value)> = vec![
            ("role", serde_json::json!("student")),
            ("name", serde_json::json!("มุก")),
            ("notes", serde_json::json!("หมายเหตุคนละอัน")),
        ];
        for (field, val) in identifying {
            let mut v = full();
            v[field] = val;
            let other = Submission::from_json(&v, 1_000).unwrap();
            assert_ne!(other.id, base.id, "`{field}` does not reach the key — two reviewers who \
                differ only in it land on one record and the second erases the first");
            assert!(!base.says_the_same_as(&other), "`{field}` is not part of sameness");
        }
        // The same, inside an answer.
        for field in ["id", "chose", "said"] {
            let mut v = full();
            v["answers"][0][field] = serde_json::json!("something-else-entirely");
            let other = Submission::from_json(&v, 1_000).unwrap();
            assert_ne!(other.id, base.id, "answer `{field}` does not reach the key");
            assert!(!base.says_the_same_as(&other), "answer `{field}` is not part of sameness");
        }
        // And answering one more question is a different review, not a resend of this one.
        let mut v = full();
        v["answers"].as_array_mut().unwrap().push(serde_json::json!({
            "id": "r-ep4", "asked": "1.4", "chose": "pea", "said": ""
        }));
        let more = Submission::from_json(&v, 1_000).unwrap();
        assert_ne!(more.id, base.id, "an extra answer does not reach the key");

        // ── not identifying: change it and this is the same review, corrected ───────────────
        // These are either housekeeping the reviewer may fix on a resend, or the form's own
        // words rather than theirs. Hashing them would file an honest correction as a duplicate.
        let housekeeping: Vec<(&str, serde_json::Value)> = vec![
            ("contact", serde_json::json!("โทร 08x-xxx-xxxx")),
            ("anonymous", serde_json::json!(true)),
            ("revision", serde_json::json!("vitals 9.9.9")),
        ];
        for (field, val) in housekeeping {
            let mut v = full();
            v[field] = val;
            let other = Submission::from_json(&v, 1_000).unwrap();
            assert_eq!(other.id, base.id, "`{field}` reaches the key — a reviewer correcting it \
                on a resend would be filed twice instead of updated");
            assert!(base.says_the_same_as(&other), "`{field}` wrongly counts as a new review");
        }
        for field in ["asked", "chose_label"] {
            let mut v = full();
            v["answers"][0][field] = serde_json::json!("the form was reworded since");
            let other = Submission::from_json(&v, 1_000).unwrap();
            assert_eq!(other.id, base.id, "answer `{field}` reaches the key — rewording the form \
                would split one review into two");
            assert!(base.says_the_same_as(&other), "answer `{field}` wrongly counts as new");
        }
    }

    /// The production failure, reproduced at the smallest scale that shows it.
    ///
    /// A physician and a student, the same second, the same answer to the same question. Before
    /// `role` reached the key these were one document, and which of the two survived depended
    /// only on which arrived second.
    #[test]
    fn a_physician_and_a_student_who_answer_alike_are_two_records() {
        let store = tmp("roles");
        let one = |role: &str| {
            let mut v = full();
            v["role"] = serde_json::json!(role);
            Submission::from_json(&v, 1_787_991_448).unwrap().file(&store).unwrap()
        };
        let doc = one("physician");
        let stu = one("student");
        assert_ne!(doc.id, stu.id, "one key for two reviewers");
        assert_eq!(store.keys(KIND).len(), 2, "the second submission erased the first");

        // Both are still readable, and each still says who wrote it.
        let a: Submission = store.get(KIND, &doc.id).expect("the physician's record");
        let b: Submission = store.get(KIND, &stu.id).expect("the student's record");
        assert_eq!(a.role, Role::Physician);
        assert_eq!(b.role, Role::Student);
    }

    fn tmp(name: &str) -> Store {
        let p = std::env::temp_dir().join(format!("vitals-review-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        Store::open(p).expect("store")
    }

    /// The failure the key alone cannot prevent, and the reason [`Submission::file`] exists.
    ///
    /// A reviewer taps Send, sees nothing happen, and taps it again — eighteen seconds later, in
    /// the real measurement this test was written from. Identical answers, a different second, a
    /// different key, two records. The keys even said they were the same review: they shared the
    /// content hash. Nothing was reading them.
    #[test]
    fn a_resend_seconds_later_is_still_one_record() {
        let store = tmp("resend");
        let first = Submission::from_json(&body(r#","notes":"ส่งครั้งแรก""#), 1_787_988_636)
            .unwrap()
            .file(&store)
            .unwrap();
        let again = Submission::from_json(&body(r#","notes":"ส่งครั้งแรก""#), 1_787_988_654)
            .unwrap()
            .file(&store)
            .unwrap();

        assert_ne!(
            Submission::key(1_787_988_654, "x"),
            Submission::key(1_787_988_636, "x"),
            "the two sends must be at different seconds or this test proves nothing"
        );
        assert_eq!(again.id, first.id, "the resend was filed under a second key");
        assert_eq!(again.at, first.at, "the resend restamped when the review arrived");
        assert_eq!(store.keys(KIND).len(), 1, "one review, two records");
    }

    /// Replaced, not skipped. A physician who resends after ticking *do not name me* changed
    /// something the key does not hash, and a skipped write would leave them named in the file
    /// the rubric credits reviewers from.
    #[test]
    fn a_resend_carries_the_later_wishes_of_the_reviewer() {
        let store = tmp("anon");
        Submission::from_json(&body(r#","notes":"ตรวจแล้ว""#), 10).unwrap().file(&store).unwrap();
        let second = Submission::from_json(&body(r#","notes":"ตรวจแล้ว","anonymous":true"#), 20)
            .unwrap()
            .file(&store)
            .unwrap();
        assert_eq!(store.keys(KIND).len(), 1);
        let stored: Submission = store.get(KIND, &second.id).expect("the record");
        assert!(stored.anonymous, "the reviewer's second thoughts were dropped");
        assert_eq!(stored.at, 10, "the record forgot when the review arrived");
    }

    /// Two reviews that are not the same review stay two, however close together they land.
    #[test]
    fn a_different_answer_is_a_different_record() {
        let store = tmp("distinct");
        Submission::from_json(&body(r#","notes":"หนึ่ง""#), 42).unwrap().file(&store).unwrap();
        Submission::from_json(&body(r#","notes":"สอง""#), 42).unwrap().file(&store).unwrap();
        assert_eq!(store.keys(KIND).len(), 2, "a second review overwrote the first");
    }
}
