//! **A patient waiting for the door has a page, and nothing on it to do to her.**
//!
//! Founder's ruling, 18 ก.ย.: in preview the board publishes who is in the queue, and each of them
//! is a link. What opens is not a bed — she is not admitted, there is no chain account, no head, no
//! lease and no tape — so the page is the part of a chart that exists yet: her face, her name, her
//! age, where she is from, the case she was built for and the level it is written at, and one
//! sentence saying what she is waiting for.
//!
//! What must not be on it: a way to take a shift, a clock, a monitor, or anything from her case
//! beyond its title. A page that offered any of those would be offering a patient nobody may treat.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

struct Server {
    child: Child,
    port: u16,
    _state: std::path::PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self._state);
    }
}

impl Server {
    fn preview() -> Server {
        let state = std::env::temp_dir().join(format!("vitals-waiting-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&state);
        let mut child = Command::new(env!("CARGO_BIN_EXE_vitals-web"))
            .env("VITALS_WEB_BIND", "127.0.0.1:0")
            .env("VITALS_STATE_DIR", &state)
            .env("VITALS_WORLD", "1")
            .env("VITALS_WARD_DOOR", "preview")
            .env("VITALS_DOOR_TOKEN", "the-door-token")
            .env_remove("VITALS_PROGRAM_ID")
            .stdout(Stdio::piped())
            .spawn()
            .expect("start vitals-web");
        let out = child.stdout.take().expect("stdout");
        let mut me = Server { child, port: 0, _state: state };
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if let Some(a) = line.split("http://").nth(1) {
                me.port = a.trim().rsplit(':').next().and_then(|p| p.parse().ok()).unwrap_or(0);
                break;
            }
        }
        assert!(me.port > 0, "server never said what port it took");
        me
    }

    fn post(&self, path: &str, body: &Value) -> (u16, Value) {
        let url = format!("http://127.0.0.1:{}{path}", self.port);
        match ureq::post(&url)
            .set("Authorization", "Bearer the-door-token")
            .set("Content-Type", "application/json")
            .send_string(&body.to_string())
        {
            Ok(r) => (r.status(), r.into_json().unwrap_or(Value::Null)),
            Err(ureq::Error::Status(c, r)) => (c, r.into_json().unwrap_or(Value::Null)),
            Err(e) => panic!("{url}: {e}"),
        }
    }

    fn get_json(&self, path: &str) -> Value {
        let url = format!("http://127.0.0.1:{}{path}", self.port);
        match ureq::get(&url).call() {
            Ok(r) => r.into_json().unwrap_or(Value::Null),
            Err(ureq::Error::Status(_, r)) => r.into_json().unwrap_or(Value::Null),
            Err(e) => panic!("{url}: {e}"),
        }
    }

    fn get_text(&self, path: &str) -> (u16, String) {
        let url = format!("http://127.0.0.1:{}{path}", self.port);
        match ureq::get(&url).call() {
            Ok(r) => (r.status(), r.into_string().unwrap_or_default()),
            Err(ureq::Error::Status(c, r)) => (c, r.into_string().unwrap_or_default()),
            Err(e) => panic!("{url}: {e}"),
        }
    }
}

/// One waiting patient, queued through the factory's own door.
fn a_pack() -> Value {
    json!({
        // No case named: the ward picks one when it admits her, and the door refuses a pack that
        // names a case this ward does not hold. What the page is about is the person.
        "case": "",
        "difficulty": "intern",
        "persona": { "name": "Nusrat Jahan", "age": 64, "sex": "f", "country": "BGD" },
        "portrait": {},
        // Not claimed endemic: the ward picks the case for a pack that names none, and which case
        // it picks is what would make that claim true or false — the door says so.
        "endemic": false
    })
}

#[test]
fn a_waiting_patient_has_a_page_with_nothing_to_do_on_it() {
    let s = Server::preview();
    let (code, answer) = s.post("/api/ward/queue", &json!({ "packs": [a_pack()] }));
    assert_eq!(code, 200, "{answer}");
    assert_eq!(answer["queued"], 1, "{answer}");

    // The board says who is waiting, and the pack id is the address of her page.
    let board = s.get_json("/api/ward");
    let who = &board["queue"]["waiting_patients"][0];
    let pack = who["pack"].as_str().expect("a pack id on the row").to_string();
    assert_eq!(who["name"], "Nusrat Jahan");

    let (code, page) = s.get_text(&format!("/ward/waiting/{pack}"));
    assert_eq!(code, 200);
    for said in ["Nusrat Jahan", "64", "Bangladesh", "intern",
                 "waiting for the ward to open", "← the globe"] {
        assert!(page.contains(said), "the page does not say {said:?}");
    }
    for never in ["take a shift", "hand over", "id=\"cmd\"", "id=\"mini\"", "id=\"clock\""] {
        assert!(!page.contains(never),
                "a patient nobody may treat has no {never:?} on her page");
    }

    // A pack id nobody queued is a sentence, not a page about nobody.
    let (code, page) = s.get_text("/ward/waiting/not-a-pack");
    assert_eq!(code, 404);
    assert!(page.contains("Nobody here") || page.contains("no patient"),
            "an address with nobody behind it says so: {page}");
}
