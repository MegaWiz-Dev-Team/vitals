//! `vitals-factory`: one tick, then exit. launchd runs it every ten minutes.
//!
//! Configuration is the environment, because that is what a plist carries:
//!
//! | variable                 | default                                  | meaning                                  |
//! |--------------------------|------------------------------------------|------------------------------------------|
//! | `WARD`                   | *(required)*                             | the ward's origin, `https://…run.app`    |
//! | `QUEUE_DEPTH`            | 20                                       | keep at least this many packs waiting    |
//! | `BASES_PER_TICK`         | 3                                        | faces made with mflux per tick           |
//! | `VITALS_REPO`            | the checkout this binary was built from  | scenarios, persona files, pool, endemic  |
//! | `VITALS_WORLD_DIR`       | `~/.vitals/world`                        | the manifest, the ledger, the faces      |
//! | `VITALS_GCP_PROJECT`     | `vitals-academy`                         | Secret Manager and Vertex                |
//! | `VITALS_PORTRAIT_BUCKET` | `vitals-world-portraits`                 | where faces are published                |
//! | `VITALS_IMAGE_MODEL`     | `gemini-2.5-flash-image`                 | the state editor                         |
//! | `FACTORY_SEED`           | the clock                                | the draw; set it to repeat a run         |
//!
//! `--dry-run` reads the ward and prints what a tick would do, fetching no secret, sending no
//! request and writing no file. Exit status is 1 when the tick logged an error, so launchd's log
//! and the exit code agree.

use std::path::PathBuf;
use vitals_factory::door::Http;
use vitals_factory::tick::{default_repo, tick, Config};
use vitals_factory::tools::Shell;

const USAGE: &str = "usage: vitals-factory [--dry-run]

One tick of the patient factory: read WARD's /api/ward, top its queue up to QUEUE_DEPTH, complete
one patient's faces, exit. Configuration is the environment (see the crate doc); --dry-run reads
and plans and touches nothing.";

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
    for a in &args {
        match a.as_str() {
            "--dry-run" => dry_run = true,
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
    let door = Http::new(&cfg.ward);
    let tools = Shell { bucket: cfg.bucket.clone() };
    let report = tick(&cfg, &door, &tools);
    let t = stamp();
    for line in &report.lines {
        println!("{t} {line}");
    }
    for e in &report.errors {
        eprintln!("{t} ERROR {e}");
    }
    println!(
        "{t} tick done: {} queued, {} duplicates, {} rejected, depth {}, {} faces made, {} states made, {} error(s)",
        report.queued,
        report.duplicates,
        report.rejected,
        report.depth.map_or("?".to_string(), |d| d.to_string()),
        report.faces_made,
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
        bases_per_tick: env_num("BASES_PER_TICK", 3)? as usize,
        repo: default_repo(),
        world_dir,
        project: env_or("VITALS_GCP_PROJECT", "vitals-academy"),
        bucket: env_or("VITALS_PORTRAIT_BUCKET", "vitals-world-portraits"),
        model: env_or("VITALS_IMAGE_MODEL", "gemini-2.5-flash-image"),
        dry_run,
        seed,
        now: now(),
    })
}
