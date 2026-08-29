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
    /// The question text as it was shown. Storing it costs a few bytes and means an answer is
    /// still readable after the form is rewritten.
    pub asked: String,
    pub said: String,
}

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
                let said: String =
                    a.get("said").and_then(|x| x.as_str()).unwrap_or("").chars().take(4000).collect();
                if said.trim().is_empty() {
                    continue; // an unanswered question is not an answer
                }
                answers.push(Answer {
                    id: a.get("id").and_then(|x| x.as_str()).unwrap_or("").chars().take(64).collect(),
                    asked: a.get("asked").and_then(|x| x.as_str()).unwrap_or("").chars().take(400).collect(),
                    said,
                });
            }
        }
        let notes = take("notes", 8000);
        if answers.is_empty() && notes.trim().is_empty() {
            return Err("empty");
        }
        let name = take("name", 120);
        Ok(Submission {
            id: Submission::key(
                at,
                &format!(
                    "{name}\u{1f}{}\u{1f}{notes}",
                    answers.iter().map(|a| format!("{}={}", a.id, a.said)).collect::<Vec<_>>().join("\u{1e}")
                ),
            ),
            at,
            role,
            name,
            contact: take("contact", 200),
            anonymous: v.get("anonymous").and_then(|x| x.as_bool()).unwrap_or(false),
            answers,
            notes,
            revision: take("revision", 64),
        })
    }

    /// The half of the key that is a hash of the content, without the second it arrived in.
    fn fingerprint(&self) -> &str {
        self.id.split_once('-').map(|(_, h)| h).unwrap_or("")
    }

    /// The same answers from the same person — what makes two submissions one review.
    ///
    /// Compared field by field, not by the six bytes of hash in the key. The hash is enough to
    /// *find* a candidate cheaply and not enough to act on one: acting on a collision would mean
    /// overwriting a different reviewer's answers with this reviewer's, which is the one outcome
    /// this whole module is arranged to prevent.
    fn says_the_same_as(&self, other: &Submission) -> bool {
        self.role == other.role
            && self.name == other.name
            && self.notes == other.notes
            && self.answers.len() == other.answers.len()
            && self
                .answers
                .iter()
                .zip(&other.answers)
                .all(|(a, b)| a.id == b.id && a.said == b.said)
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
