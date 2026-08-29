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
//! # Wiring
//!
//! Nothing below is urgent. `static/review.html` also hands the reviewer their answers as JSON to
//! send back by hand, and `scripts/file-review.py` files that JSON under the same key this module
//! would have written — so reviews can arrive before this is wired, and the two routes cannot
//! produce two records of the same review.
//!
//! When it is wired: `pub mod review;` in `lib.rs`, `const REVIEW: &str =
//! include_str!("../static/review.html");` in `main.rs`, then
//!
//! ```text
//! (Method::Get,  "/review")     => html(REVIEW),
//! (Method::Post, "/api/review") => ... Submission::from_json(&body, now)?.save(&store)
//! ```
//!
//! `/api/review` is the first route in this server that reads a request body — every other one
//! takes query parameters — and it is public on purpose: a reviewer should not need an account to
//! tell us what is wrong. Both routes belong in the no-token list in `main.rs`'s test.

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
    /// key and the second write would erase the first. Hashing the content also makes a resend
    /// idempotent — a reviewer who taps Send twice on a flaky connection gets one record, not two.
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

    pub fn save(&self, store: &Store) -> std::io::Result<()> {
        store.put(KIND, &self.id, self)
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
}
