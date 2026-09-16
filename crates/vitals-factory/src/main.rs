//! `vitals-factory`: one tick, then exit. launchd runs it every ten minutes.
//!
//! Configuration is the environment, because that is what a plist carries:
//!
//! | variable                 | default                                  | meaning                                  |
//! |--------------------------|------------------------------------------|------------------------------------------|
//! | `WARD`                   | *(required)*                             | the ward's origin, `https://…run.app`    |
//! | `QUEUE_DEPTH`            | 20                                       | keep at least this many packs waiting    |
//! | `BASES_PER_TICK`         | 2                                        | faces made with mflux per tick (≈ 3 min each on the mini, measured 16 Sep) |
//! | `VITALS_REPO`            | the checkout this binary was built from  | scenarios, persona files, pool, endemic  |
//! | `VITALS_WORLD_DIR`       | `~/.vitals/world`                        | the manifest, the ledger, the faces      |
//! | `VITALS_GCP_PROJECT`     | *(required)*                             | the ward's project, where its `vitals-door-token` secret lives (`vitals-academy-dev` for staging, `vitals-academy` for production) |
//! | `VITALS_VERTEX_PROJECT`  | `vitals-academy`                         | the image editor's project               |
//! | `VITALS_PORTRAIT_BUCKET` | `vitals-world-portraits`                 | where faces are published                |
//! | `VITALS_IMAGE_MODEL`     | `gemini-2.5-flash-image`                 | the state editor                         |
//! | `VITALS_JUDGE_MODEL`     | `gemini-2.5-flash`                       | the model that judges each face (11/11 on the 16 Sep calibration; gemini-3.1-flash-lite was 10/11 and is the fallback when 2.5 retires) |
//! | `FACTORY_SEED`           | the clock                                | the draw; set it to repeat a run         |
//!
//! `--dry-run` reads the ward and prints what a tick would do, fetching no secret, sending no
//! request and writing no file. Exit status is 1 when the tick logged an error, so launchd's log
//! and the exit code agree.

use std::path::PathBuf;
use vitals_factory::door::Http;
use vitals_factory::tick::{backfill_variants, default_repo, remake_face, tick, Config};
use vitals_factory::tools::Shell;

const USAGE: &str = "usage: vitals-factory [--once] [--dry-run] | --face KEY@AGE | --variants

One tick of the patient factory: read WARD's /api/ward, top its queue up to QUEUE_DEPTH, complete
one patient's faces, exit. Configuration is the environment (see the crate doc); --dry-run reads
and plans and touches nothing. --face KOR-0@8 remakes one face through the photorealism gate,
records it, and prints its url; the ward is not touched. --variants makes the 256 px sibling of
every portrait on file that has none, uploads and records them; the next tick carries them to the
ward.";

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty()).unwrap_or_else(|| default.to_string())
}

fn env_num(name: &str, default: u64) -> Result<u64, String> {
    match std::env::var(name) {
        Ok(v) if !v.trim().is_empty() => v.trim().parse().map_err(|_| format!("{name} must be a whole number, not {v:?}")),
        _ => Ok(default),
    }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn stamp() -> String {
    // Local wall time is what a person reading the log beside the launchd interval wants; UTC
    // seconds are what a machine wants. Both, cheaply, without a time-zone dependency.
    format!("[{}]", now())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut dry_run = false;
    let mut face: Option<String> = None;
    let mut variants = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dry-run" => dry_run = true,
            "--variants" => variants = true,
            "--face" => match it.next() {
                Some(spec) => face = Some(spec.clone()),
                None => {
                    eprintln!("--face needs KEY@AGE\n{USAGE}");
                    std::process::exit(2);
                }
            },
            // One tick is the only mode there is; the word is accepted so a launchd line and a
            // hand-typed line read the same.
            "--once" => {}
            "--help" | "-h" => {
                println!("{USAGE}");
                return;
            }
            other => {
                eprintln!("unknown argument {other}\n{USAGE}");
                std::process::exit(2);
            }
        }
    }
    let cfg = match config(dry_run) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("vitals-factory: {e}");
            std::process::exit(2);
        }
    };
    let tools = Shell { bucket: cfg.bucket.clone() };
    if variants {
        let report = backfill_variants(&cfg, &tools);
        let t = stamp();
        for line in &report.lines {
            println!("{t} {line}");
        }
        for e in &report.errors {
            eprintln!("{t} ERROR {e}");
        }
        if !report.errors.is_empty() {
            std::process::exit(1);
        }
        return;
    }
    if let Some(spec) = face {
        match remake_face(&cfg, &tools, &spec) {
            Ok((url, report)) => {
                let t = stamp();
                for line in &report.lines {
                    println!("{t} {line}");
                }
                println!("{t} {url}");
            }
            Err(boxed) => {
                let (e, report) = *boxed;
                let t = stamp();
                for line in &report.lines {
                    println!("{t} {line}");
                }
                eprintln!("{t} ERROR {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    let door = Http::new(&cfg.ward);
    let report = tick(&cfg, &door, &tools);
    let t = stamp();
    for line in &report.lines {
        println!("{t} {line}");
    }
    for e in &report.errors {
        eprintln!("{t} ERROR {e}");
    }
    println!(
        "{t} tick done: {} queued, {} duplicates, {} state(s) refused by the judge, {} refused by the door, depth {}, {} faces made of {} painted, {} states made, {} error(s)",
        report.queued,
        report.duplicates,
        report.rejected,
        report.door_rejected,
        report.depth.map_or("?".to_string(), |d| d.to_string()),
        report.faces_made,
        report.faces_tried,
        report.states_made,
        report.errors.len()
    );
    if !report.errors.is_empty() {
        std::process::exit(1);
    }
}

fn config(dry_run: bool) -> Result<Config, String> {
    let ward = std::env::var("WARD").ok().filter(|v| !v.trim().is_empty()).ok_or_else(|| {
        "WARD is not set — the ward's origin, e.g. https://vitals-world-jak4wea54q-as.a.run.app (dev) — and this job pushes nowhere it was not told to".to_string()
    })?;
    if !ward.starts_with("http://") && !ward.starts_with("https://") {
        return Err(format!("WARD must be an origin starting with https://, not {ward:?}"));
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let world_dir = std::env::var_os("VITALS_WORLD_DIR").map(PathBuf::from).unwrap_or_else(|| home.join(".vitals/world"));
    let seed = env_num("FACTORY_SEED", now())?;
    Ok(Config {
        ward: ward.trim_end_matches('/').to_string(),
        queue_depth: env_num("QUEUE_DEPTH", 20)? as usize,
        bases_per_tick: env_num("BASES_PER_TICK", 2)? as usize,
        repo: default_repo(),
        world_dir,
        secret_project: std::env::var("VITALS_GCP_PROJECT").ok().filter(|v| !v.trim().is_empty()).ok_or_else(|| {
            "VITALS_GCP_PROJECT is not set — the ward's own project, where its vitals-door-token secret lives \
             (vitals-academy-dev for staging, vitals-academy for production); a token from the wrong project is a door \
             that says unauthorised"
                .to_string()
        })?,
        vertex_project: env_or("VITALS_VERTEX_PROJECT", "vitals-academy"),
        bucket: env_or("VITALS_PORTRAIT_BUCKET", "vitals-world-portraits"),
        model: env_or("VITALS_IMAGE_MODEL", "gemini-2.5-flash-image"),
        judge_model: env_or("VITALS_JUDGE_MODEL", "gemini-2.5-flash"),
        dry_run,
        seed,
        now: now(),
    })
}
