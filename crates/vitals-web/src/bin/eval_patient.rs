//! Collect the patient's real replies to ordinary learner questions, for the leak eval.
//!
//! The reveal gate is not wired into the served route yet (`say(…, None, …)` in the bay), so what
//! the learner gets today is the prompt alone. This asks the same `Patient` the bay uses — same
//! brief, same model, empty history, a stable patient — every question in `EVAL_QUESTIONS` for
//! every persona with gated facts, and writes one JSON line per reply to `EVAL_OUT`. Judging is
//! done elsewhere (docs/internal/EVAL_REVEAL_GATE_PARAPHRASE_*.md): this binary only collects.
//!
//! Run: `VITALS_VERTEX_URL=… GOOGLE_ACCESS_TOKEN=$(gcloud auth print-access-token) \`
//!      `EVAL_QUESTIONS=questions.txt EVAL_OUT=replies.jsonl cargo run -p vitals-web --bin eval_patient`
use std::io::Write;

fn main() {
    let questions: Vec<String> = std::env::var("EVAL_QUESTIONS")
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| {
            s.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    let out_path = std::env::var("EVAL_OUT").unwrap_or_else(|_| "replies.jsonl".into());
    if questions.is_empty() {
        eprintln!("no questions: set EVAL_QUESTIONS to a file with one question per line");
        std::process::exit(2);
    }
    let Some(patient) = vitals_web::patient::Patient::connect() else {
        eprintln!("no model configured — set VITALS_VERTEX_URL + GOOGLE_ACCESS_TOKEN, or HEIMDALL_API_KEY");
        std::process::exit(2);
    };
    let repeats: usize = std::env::var("EVAL_REPEATS").ok().and_then(|s| s.parse().ok()).unwrap_or(1);
    let mut stories = vec![std::path::PathBuf::from("demo/ep1-en.json")];
    let mut personas: Vec<_> = std::fs::read_dir("demo/personas")
        .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect())
        .unwrap_or_default();
    personas.sort();
    stories.extend(personas);

    let mut out = std::fs::OpenOptions::new().create(true).append(true).open(&out_path).expect("open EVAL_OUT");
    for story in &stories {
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(story).unwrap_or_default()).unwrap_or_default();
        let gated = v["dialogue"].as_array().is_some_and(|a| a.iter().any(|n| n["reveal"] == "on_direct_ask"));
        if !gated {
            continue;
        }
        let id = v["id"].as_str().unwrap_or("").to_string();
        for q in &questions {
            for r in 0..repeats {
                let reply = patient
                    .say(&v, q, &[], "stable", 98.0, None, vitals_web::lang::default_language())
                    .unwrap_or_else(|e| format!("<error: {e}>"));
                let line = serde_json::json!({"persona": id, "question": q, "repeat": r, "reply": reply});
                writeln!(out, "{line}").ok();
                eprintln!("  {id:<22} {}", q.chars().take(40).collect::<String>());
            }
        }
    }
}
