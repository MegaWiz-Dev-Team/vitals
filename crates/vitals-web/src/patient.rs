//! The patient, played by a local model.
//!
//! Everything here sits **outside the proof path** by construction. The tape records the question
//! you asked; it never records the answer, because the answer comes from a model and a model
//! cannot be re-derived. The model is what makes her worth talking to. The automaton is what makes
//! the run worth proving. Those are different jobs and they are kept in different places.
//!
//! Inference prefers the local Heimdall gateway. The public demo runs on a cloud model — a
//! **recorded exception** (decided 2026-08-24) to the standing rule of *Heimdall-only, no cloud
//! LLM*: this patient is synthetic, the dialogue carries no PHI, and playing her is not clinical
//! care. It is an exception, not an oversight — the rule still holds everywhere a real patient's
//! data could appear, which is why the local gateway stays preferred whenever it is reachable.

use crate::lang::Language;
use serde_json::{json, Value};

/// Which model is speaking for her.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// The local model on the Heimdall gateway. Preferred: nothing leaves the machine.
    Local,
    /// The cloud model, standing in when the local one cannot be reached.
    Cloud,
}

impl Backend {
    /// Pick the voice for an encounter, given which backends answered a reachability check.
    ///
    /// Local always wins when it can serve — the cloud is an understudy, not a load-balanced
    /// peer. `None` means neither answered, and the app plays without a voice and says so rather
    /// than pretending one exists. Decided once, here, and held for the whole encounter.
    pub fn choose(local_ok: bool, cloud_ok: bool) -> Option<Backend> {
        if local_ok {
            Some(Backend::Local)
        } else if cloud_ok {
            Some(Backend::Cloud)
        } else {
            None
        }
    }
}

/// One model endpoint the patient can speak through.
struct Wire {
    url: String,
    model: String,
    /// A static bearer (the local gateway key). `None` means mint one per request from the
    /// metadata server — how Cloud Run reaches Vertex with no key stored anywhere, the same
    /// pattern `store.rs` uses for Firestore.
    key: Option<String>,
}

pub struct Patient {
    /// The endpoint chosen for this encounter, and which kind it is.
    wire: Wire,
    backend: Backend,
    persona: Value,
}

impl Patient {
    /// `None` when the gateway is unreachable or unconfigured. The app then plays without a
    /// voice and says so, rather than blocking on a service the demo does not require.
    pub fn connect(story_path: &std::path::Path) -> Option<Patient> {
        let persona: Value = serde_json::from_str(&std::fs::read_to_string(story_path).ok()?).ok()?;

        // The local gateway: preferred, requires its key. Absent key ⇒ no local voice.
        let local = std::env::var("HEIMDALL_API_KEY").ok().map(|key| Wire {
            url: std::env::var("HEIMDALL_API_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8080/v1".into()),
            model: std::env::var("HEIMDALL_CHAT_MODEL")
                .unwrap_or_else(|_| "mlx-community/gemma-4-26b-a4b-it-4bit".into()),
            key: Some(key),
        });

        // The cloud stand-in: Vertex OpenAI-compat, keyless via the metadata server. Configured
        // by VITALS_VERTEX_URL (the openapi base) so it is off unless deliberately turned on.
        let cloud = std::env::var("VITALS_VERTEX_URL").ok().map(|url| Wire {
            url,
            model: std::env::var("VITALS_VERTEX_MODEL")
                .unwrap_or_else(|_| "google/gemini-3.1-flash-lite".into()),
            key: None,
        });

        // One reachability check each, now, so the choice is made once and held. A per-turn
        // decision could swap her voice mid-examination.
        let local_ok = local.as_ref().is_some_and(reachable);
        let cloud_ok = cloud.as_ref().is_some_and(reachable);
        let backend = Backend::choose(local_ok, cloud_ok)?;
        let wire = match backend {
            Backend::Local => local?,
            Backend::Cloud => cloud?,
        };
        Some(Patient { wire, backend, persona })
    }

    /// Which model is speaking this encounter — for the status line and the debrief.
    pub fn backend(&self) -> Backend {
        self.backend
    }

    pub fn name(&self) -> String {
        self.persona["patient"]["name"].as_str().unwrap_or("the patient").to_string()
    }

    /// Build the character brief from the authored story, not from a prompt written here.
    ///
    /// The dialogue nodes are the truth of what she knows — including what she will only say if
    /// asked directly. Handing the model the authored lines keeps it anchored to the case instead
    /// of inventing a different allergy, which it does within one turn if you let it.
    fn system(&self, status: &str, spo2: f64, lang: &Language) -> String {
        brief(&self.persona, status, spo2, lang)
    }

    /// Ask her something. `history` is the conversation so far as (role, content) pairs.
    ///
    /// `retry_hint` is the reveal gate's word on what a previous attempt gave away, carried
    /// verbatim into her brief for a regenerate. Opaque here on purpose: the gate owns what a
    /// hint says and the patient only promises to hear it — that separation is the contract
    /// between the two, and it is what lets the gate evolve without this file knowing.
    ///
    /// `lang` is the language she answers in. It is a *presentation* argument and nothing more:
    /// the case notes handed to the model are the English the author wrote, the model is asked to
    /// carry them across without changing a fact, and none of it — question, answer or language —
    /// is an input to a leaf. The question goes on the tape exactly as the learner typed it,
    /// which is what it did before this argument existed.
    pub fn say(
        &self,
        question: &str,
        history: &[(String, String)],
        status: &str,
        spo2: f64,
        retry_hint: Option<&str>,
        lang: &Language,
    ) -> Result<String, String> {
        let mut system = self.system(status, spo2, lang);
        if let Some(h) = retry_hint {
            system.push_str("\n\n");
            system.push_str(h);
        }
        let mut messages = vec![json!({"role":"system","content": system})];
        // Only the last few turns — she is not having a long conversation, she is struggling.
        for (role, content) in history.iter().rev().take(8).rev() {
            messages.push(json!({"role": role, "content": content}));
        }
        messages.push(json!({"role":"user","content": question}));

        let body = json!({
            "model": self.wire.model,
            "max_tokens": 90,
            "temperature": 0.7,
            "messages": messages
        });

        let bearer = self.wire.bearer()?;
        let resp = ureq::post(&format!("{}/chat/completions", self.wire.url.trim_end_matches('/')))
            .set("Authorization", &format!("Bearer {bearer}"))
            .set("Content-Type", "application/json")
            .timeout(std::time::Duration::from_secs(90))
            .send_json(body)
            .map_err(|e| format!("gateway: {e}"))?;

        let v: Value = resp.into_json().map_err(|e| format!("bad reply: {e}"))?;
        v["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "the gateway returned nothing".to_string())
    }
}

/// `(object, subject)` pronouns for a persona's `sex`, or a neutral pair when the case does not
/// say.
///
/// Unknown is answered with "this patient" / "they" rather than a guess. A brief that asserts the
/// wrong sex is worse than one that declines to assert any: the model is being handed the case as
/// fact, and everything it invents from a wrong fact is wrong downstream.
fn pronouns(sex: Option<&str>) -> (&'static str, &'static str) {
    match sex.map(str::trim).unwrap_or_default().to_ascii_uppercase().as_str() {
        "M" | "MALE" => ("him", "he"),
        "F" | "FEMALE" => ("her", "she"),
        _ => ("this patient", "they"),
    }
}

/// The character brief, built from the authored story and the language the learner chose.
///
/// A free function rather than a method so it can be read — and tested — without a gateway: the
/// one thing that must be true of a language layer is that the *right instruction leaves the
/// building*, and that has to be checkable on a laptop with no model running. `tests/patient.rs`
/// asserts exactly that against `demo/ep1-en.json`.
///
/// # Why the case notes stay in English
///
/// The notes below are the case, and the case is a file whose sha256 is its identity on chain.
/// Translating the file would mint a different case. So the model is handed the English the
/// author wrote and asked to *carry it across* — the facts are fixed, the language is not. That
/// is also the honest division of labour: paraphrasing a fixed set of facts into a patient's own
/// words is what this model is for, and it is doing it in English already.
pub fn brief(persona: &Value, status: &str, spo2: f64, lang: &Language) -> String {
    let p = &persona["patient"];
    // Read from the case, not written in here. The season is more than half men — a 71-year-old
    // with a swelling face, a 62-year-old vomiting blood, a five-year-old who will not lie down —
    // and a brief that calls every one of them "her" is a prompt telling the model something the
    // case says is false. `sex` is a field the persona has always carried.
    let (obj, subj) = pronouns(p["sex"].as_str());
    let mut s = format!(
        "You are {}, {} years old, in an emergency department right now. \
         You are frightened and short of breath. Speak ONLY as {}, in first person, in \
         {}, in one or two short sentences. Broken, breathless phrasing. Never narrate, \
         never describe yourself from outside, never mention being an AI, never give medical \
         advice or diagnose yourself.\n\n",
        p["name"].as_str().unwrap_or("Ing"),
        p["age"].as_i64().unwrap_or(19),
        obj,
        lang.speaks,
    );
    s.push_str("What is true about you, and what you say if asked:\n");
    if let Some(d) = persona["dialogue"].as_array() {
        for node in d {
            let reveal = node["reveal"].as_str().unwrap_or("on_ask");
            let line = node["patient"].as_str().unwrap_or("");
            let id = node["id"].as_str().unwrap_or("");
            s.push_str(&format!("- {id} ({reveal}): \"{line}\"\n"));
        }
    }
    s.push_str(&format!(
        "\nUse those as the truth. Paraphrase them naturally; do not invent a different \
         allergy, a different timeline, or symptoms not listed. Anything marked \
         on_direct_ask you volunteer only when asked about that exact thing.\n\
         \nRight now you are {status} and your oxygen saturation is {spo2:.0} percent. \
         The worse that is, the shorter and more broken your sentences get. If you are \
         critical or arrested you can barely speak at all.\n\
         If asked something you would not know, say you don't know.\n\
         Fallback if you cannot answer: \"{}\"",
        persona["fallback"].as_str().unwrap_or("I can't really talk any more.")
    ));
    // Only when she is not speaking the language the notes are written in. The English brief is
    // left exactly as it was, so nothing about the default encounter changed when this landed.
    if lang.id != crate::lang::default_language().id {
        s.push_str(&format!(
            "\n\nLANGUAGE. The notes above are the medical record and are written in English. \
             That is the language of the chart, not of this patient. You speak {0}. Every reply \
             you give is in {0} and only in {0} — including your fallback line, and including \
             when the doctor speaks to you in English. \
             Carry the notes across; do not change them. The same allergy, the same food, the \
             same timing, the same symptoms, the same numbers, the same things you will only say \
             if you are asked. Do not add a detail that is not in the notes and do not leave one \
             out because it is awkward to say in {0}. \
             Speak the way this patient would speak — a frightened patient, not a doctor. Where \
             {0} has an everyday word for something, use the everyday word; where the only word \
             {1} would have is the borrowed medical one, use that.",
            lang.speaks, subj
        ));
    }
    s
}

/// A cheap check that a model endpoint will answer before an encounter commits to it.
///
/// One short request with a tiny budget. It is allowed to be wrong in the safe direction: a
/// local model that passes here and dies mid-encounter just means one muted turn, not a swapped
/// voice — the backend is already fixed for the session by then.
fn reachable(w: &Wire) -> bool {
    let Ok(bearer) = w.bearer() else { return false };
    ureq::post(&format!("{}/chat/completions", w.url.trim_end_matches('/')))
        .set("Authorization", &format!("Bearer {bearer}"))
        .set("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(6))
        .send_json(json!({
            "model": w.model, "max_tokens": 1,
            "messages": [{"role": "user", "content": "ok"}]
        }))
        .is_ok()
}

impl Wire {
    /// The bearer token for this endpoint: the static key if it has one, else a short-lived
    /// token from the Cloud Run metadata server — the keyless path Vertex uses, the same one
    /// `store.rs` uses for Firestore.
    fn bearer(&self) -> Result<String, String> {
        if let Some(k) = &self.key {
            return Ok(k.clone());
        }
        if let Ok(t) = std::env::var("GOOGLE_ACCESS_TOKEN") {
            return Ok(t);
        }
        let resp = ureq::get(
            "http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token",
        )
        .set("Metadata-Flavor", "Google")
        .timeout(std::time::Duration::from_secs(5))
        .call()
        .map_err(|e| format!("metadata token: {e}"))?;
        let v: Value = resp.into_json().map_err(|e| format!("metadata token: {e}"))?;
        v["access_token"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "metadata token: no access_token".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The brief must not tell the model something the case denies. Seven of the season's
    /// seventeen patients are men, and "Speak ONLY as her" over a 71-year-old man is a false
    /// fact handed to a model that will build on it for the rest of the encounter.
    #[test]
    fn the_brief_speaks_as_the_patient_the_case_describes() {
        let man = json!({ "patient": { "name": "Somchai", "age": 71, "sex": "M" } });
        let s = brief(&man, "Deteriorating", 91.0, crate::lang::default_language());
        assert!(s.contains("Speak ONLY as him,"), "the man is still spoken of as a woman");
        assert!(!s.contains("as her,"), "\"her\" survived somewhere in the brief");

        let woman = json!({ "patient": { "name": "Ing", "age": 19, "sex": "F" } });
        let s = brief(&woman, "Deteriorating", 91.0, crate::lang::default_language());
        assert!(s.contains("Speak ONLY as her,"), "EP1's brief changed");
    }

    /// A case that does not say declines to assert, rather than guessing and being wrong half
    /// the time. `they` is not a claim about anybody.
    #[test]
    fn a_case_that_names_no_sex_is_not_given_one() {
        assert_eq!(pronouns(None), ("this patient", "they"));
        assert_eq!(pronouns(Some("")), ("this patient", "they"));
        assert_eq!(pronouns(Some("?")), ("this patient", "they"));
        // The field is authored by hand across seventeen files, so it is read forgivingly.
        for m in ["M", "m", " male ", "Male"] {
            assert_eq!(pronouns(Some(m)), ("him", "he"), "{m} was not read as a man");
        }
        for f in ["F", "f", " female ", "Female"] {
            assert_eq!(pronouns(Some(f)), ("her", "she"), "{f} was not read as a woman");
        }
    }
}
