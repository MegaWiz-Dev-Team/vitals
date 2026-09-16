//! Everything the tick reaches outside itself, behind one trait.
//!
//! Five things, all of which the mini already has on the path: the token from Secret Manager
//! (`gcloud secrets versions access`), a face from mflux, a state from the Gemini image editor on
//! Vertex, webp from `cwebp`, and an upload with `gcloud storage cp --no-clobber`. A test hands
//! the tick a fake; `--dry-run` hands it nothing at all, because a dry run never gets this far.
//!
//! **The token.** Read at run time, held in memory, spelled out once as a header, never written
//! to disk and never logged — `Token`'s `Debug` is the word redacted. gcloud runs as the logged-in
//! user on the mini; nothing here carries a credential of its own.

use crate::door::Token;
use std::path::Path;
use std::process::Command;

pub trait Tools {
    /// `VITALS_TOKEN`, from the target project's Secret Manager.
    fn secret_token(&self, project: &str) -> Result<Token, String>;
    /// A base face, as a PNG at `out_png`.
    fn paint(&self, prompt: &str, seed: u64, out_png: &Path) -> Result<(), String>;
    /// One state, edited from the base, as PNG bytes.
    fn edit(&self, project: &str, model: &str, base: &[u8], mime: &str, prompt: &str) -> Result<Vec<u8>, String>;
    /// The photorealism gate: one yes-or-no question about an image, put to the text model on
    /// Vertex with the image inline. `true` is yes; the string is the model's one sentence why,
    /// for the log and for whoever reads a refusal.
    fn judge(&self, project: &str, model: &str, image: &[u8], mime: &str, question: &str) -> Result<(bool, String), String>;
    /// PNG bytes to webp bytes at this quality.
    fn webp(&self, png: &[u8], quality: u8) -> Result<Vec<u8>, String>;
    /// `local` to `gs://<bucket>/<object>`, never overwriting.
    fn upload(&self, local: &Path, object: &str) -> Result<(), String>;
    /// A public object, by url — how a base already in the bucket is fetched for editing.
    fn fetch(&self, url: &str) -> Result<Vec<u8>, String>;
}

/// The real tools, as subprocesses and one HTTPS call.
pub struct Shell {
    pub bucket: String,
}

fn run(cmd: &mut Command) -> Result<Vec<u8>, String> {
    let what = format!("{:?}", cmd.get_program());
    let out = cmd.output().map_err(|e| format!("{what}: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let tail: String = err.chars().rev().take(400).collect::<Vec<_>>().into_iter().rev().collect();
        return Err(format!("{what} exited {}: {}", out.status, tail.trim()));
    }
    Ok(out.stdout)
}

impl Shell {
    fn access_token() -> Result<String, String> {
        let out = run(Command::new("gcloud").args(["auth", "print-access-token"]))?;
        Ok(String::from_utf8_lossy(&out).trim().to_string())
    }
}

impl Tools for Shell {
    fn secret_token(&self, project: &str) -> Result<Token, String> {
        let out = run(Command::new("gcloud").args([
            "secrets", "versions", "access", "latest", "--secret", "vitals-token", "--project", project,
        ]))
        .map_err(|e| format!("the token could not be read from Secret Manager in {project}: {e}"))?;
        let s = String::from_utf8(out).map_err(|_| "the secret is not text".to_string())?;
        if s.trim().is_empty() {
            return Err("the secret is empty".into());
        }
        Ok(Token::new(s))
    }

    fn paint(&self, prompt: &str, seed: u64, out_png: &Path) -> Result<(), String> {
        run(Command::new("mflux-generate").args([
            "--model", "ostris/Flex.1-alpha", "--base-model", "dev", "--quantize", "4", "--steps", "20",
            "--width", "768", "--height", "768", "--seed", &seed.to_string(), "--prompt", prompt,
            "--output", &out_png.display().to_string(),
        ]))?;
        if !out_png.exists() {
            return Err(format!("mflux-generate exited 0 but wrote nothing at {}", out_png.display()));
        }
        Ok(())
    }

    fn edit(&self, project: &str, model: &str, base: &[u8], mime: &str, prompt: &str) -> Result<Vec<u8>, String> {
        use base64::Engine;
        let parts = vertex_generate(project, model, base, mime, prompt, true)?;
        for part in &parts {
            if let Some(data) = part.pointer("/inlineData/data").and_then(|d| d.as_str()) {
                return base64::engine::general_purpose::STANDARD
                    .decode(data)
                    .map_err(|e| format!("Vertex image is not base64: {e}"));
            }
        }
        Err("Vertex returned text and no image".into())
    }

    fn judge(&self, project: &str, model: &str, image: &[u8], mime: &str, question: &str) -> Result<(bool, String), String> {
        let parts = vertex_generate(project, model, image, mime, question, false)?;
        let text: String = parts.iter().filter_map(|p| p.get("text").and_then(|t| t.as_str())).collect::<Vec<_>>().join(" ");
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let word = text.trim_start_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
        let why = text.split_once(['.', ',']).map(|x| x.1).unwrap_or("").trim().to_string();
        if word.starts_with("yes") {
            Ok((true, why))
        } else if word.starts_with("no") {
            Ok((false, why))
        } else {
            Err(format!("the judge answered neither yes nor no: {}", text.chars().take(120).collect::<String>()))
        }
    }

    fn webp(&self, png: &[u8], quality: u8) -> Result<Vec<u8>, String> {
        let dir = std::env::temp_dir().join(format!("vitals-factory-{}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let src = dir.join("in.png");
        let dst = dir.join("out.webp");
        std::fs::write(&src, png).map_err(|e| e.to_string())?;
        run(Command::new("cwebp").args([
            "-quiet", "-q", &quality.to_string(), "-m", "6",
            &src.display().to_string(), "-o", &dst.display().to_string(),
        ]))?;
        let out = std::fs::read(&dst).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_dir_all(&dir);
        Ok(out)
    }

    fn upload(&self, local: &Path, object: &str) -> Result<(), String> {
        run(Command::new("gcloud").args([
            "storage", "cp", "--no-clobber", &local.display().to_string(), &format!("gs://{}/{object}", self.bucket),
        ]))
        .map(|_| ())
    }

    fn fetch(&self, url: &str) -> Result<Vec<u8>, String> {
        use std::io::Read;
        let mut buf = Vec::new();
        ureq::AgentBuilder::new()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .get(url)
            .call()
            .map_err(|e| format!("GET {url}: {e}"))?
            .into_reader()
            .read_to_end(&mut buf)
            .map_err(|e| format!("GET {url}: {e}"))?;
        Ok(buf)
    }
}

/// One `generateContent` call on Vertex's global endpoint with an image inline and a text part,
/// as `gcloud auth print-access-token`'s user. `want_image` asks for an image back (the editor);
/// without it the model answers in text (the judge). Returns the first candidate's parts.
fn vertex_generate(project: &str, model: &str, image: &[u8], mime: &str, text: &str, want_image: bool) -> Result<Vec<serde_json::Value>, String> {
    use base64::Engine;
    let token = Shell::access_token()?;
    let url = format!(
        "https://aiplatform.googleapis.com/v1/projects/{project}/locations/global/publishers/google/models/{model}:generateContent"
    );
    let generation = if want_image {
        serde_json::json!({"responseModalities": ["TEXT", "IMAGE"], "imageConfig": {"aspectRatio": "1:1"}})
    } else {
        serde_json::json!({"temperature": 0, "maxOutputTokens": 80})
    };
    let body = serde_json::json!({
        "contents": [{"role": "user", "parts": [
            {"inlineData": {"mimeType": mime, "data": base64::engine::general_purpose::STANDARD.encode(image)}},
            {"text": text}
        ]}],
        "generationConfig": generation
    });
    let resp = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .post(&url)
        .set("Authorization", &format!("Bearer {token}"))
        .set("Content-Type", "application/json")
        .send_string(&body.to_string());
    let v: serde_json::Value = match resp {
        Ok(r) => r.into_json().map_err(|e| format!("Vertex answered something that is not JSON: {e}"))?,
        Err(ureq::Error::Status(code, r)) => {
            let text = r.into_string().unwrap_or_default();
            return Err(format!("Vertex HTTP {code}: {}", text.chars().take(400).collect::<String>()));
        }
        Err(e) => return Err(format!("Vertex: {e}")),
    };
    v.pointer("/candidates/0/content/parts")
        .and_then(|p| p.as_array())
        .cloned()
        .ok_or_else(|| format!("Vertex returned no candidate: {}", v.to_string().chars().take(300).collect::<String>()))
}

/// sha256 of some bytes, as the bucket names them.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}
