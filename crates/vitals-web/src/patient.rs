//! The patient, played by a local model.
//!
//! Everything here sits **outside the proof path** by construction. The tape records the question
//! you asked; it never records the answer, because the answer comes from a model and a model
//! cannot be re-derived. The model is what makes her worth talking to. The automaton is what makes
//! the run worth proving. Those are different jobs and they are kept in different places.
//!
//! Inference goes through the local Heimdall gateway — no cloud provider, no clinical text leaving
//! the machine.

use serde_json::{json, Value};

pub struct Patient {
    url: String,
    key: String,
    model: String,
    persona: Value,
}

impl Patient {
    /// `None` when the gateway is unreachable or unconfigured. The app then plays without a
    /// voice and says so, rather than blocking on a service the demo does not require.
    pub fn connect(story_path: &std::path::Path) -> Option<Patient> {
        let url = std::env::var("HEIMDALL_API_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8080/v1".into());
        let key = std::env::var("HEIMDALL_API_KEY").ok()?;
        let model = std::env::var("HEIMDALL_CHAT_MODEL")
            .unwrap_or_else(|_| "mlx-community/gemma-4-26b-a4b-it-4bit".into());
        let persona: Value = serde_json::from_str(&std::fs::read_to_string(story_path).ok()?).ok()?;
        Some(Patient { url, key, model, persona })
    }

    pub fn name(&self) -> String {
        self.persona["patient"]["name"].as_str().unwrap_or("the patient").to_string()
    }

    /// Build the character brief from the authored story, not from a prompt written here.
    ///
    /// The dialogue nodes are the truth of what she knows — including what she will only say if
    /// asked directly. Handing the model the authored lines keeps it anchored to the case instead
    /// of inventing a different allergy, which it does within one turn if you let it.
    fn system(&self, status: &str, spo2: f64) -> String {
        let p = &self.persona["patient"];
        let mut s = format!(
            "You are {}, {} years old, in an emergency department right now. \
             You are frightened and short of breath. Speak ONLY as her, in first person, in \
             English, in one or two short sentences. Broken, breathless phrasing. Never narrate, \
             never describe yourself from outside, never mention being an AI, never give medical \
             advice or diagnose yourself.\n\n",
            p["name"].as_str().unwrap_or("Ing"),
            p["age"].as_i64().unwrap_or(19)
        );
        s.push_str("What is true about you, and what you say if asked:\n");
        if let Some(d) = self.persona["dialogue"].as_array() {
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
            self.persona["fallback"].as_str().unwrap_or("I can't really talk any more.")
        ));
        s
    }

    /// Ask her something. `history` is the conversation so far as (role, content) pairs.
    pub fn say(
        &self,
        question: &str,
        history: &[(String, String)],
        status: &str,
        spo2: f64,
    ) -> Result<String, String> {
        let mut messages = vec![json!({"role":"system","content": self.system(status, spo2)})];
        // Only the last few turns — she is not having a long conversation, she is struggling.
        for (role, content) in history.iter().rev().take(8).rev() {
            messages.push(json!({"role": role, "content": content}));
        }
        messages.push(json!({"role":"user","content": question}));

        let body = json!({
            "model": self.model,
            "max_tokens": 90,
            "temperature": 0.7,
            "messages": messages
        });

        let resp = ureq::post(&format!("{}/chat/completions", self.url.trim_end_matches('/')))
            .set("Authorization", &format!("Bearer {}", self.key))
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
