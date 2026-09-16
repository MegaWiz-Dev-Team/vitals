//! `vitals-casefactory` — compile embla-cases into World packs.
//!
//! ```text
//! vitals-casefactory compile --cases <dir | dir@ref> --id <case_id> --out <dir>
//! vitals-casefactory compile --cases <dir | dir@ref> --all --out <dir>
//! ```
//!
//! A pack is written only when it passes every gate; a refused case leaves no file and is
//! named, with its reason, on stderr and — for `--all` — in `<out>/REPORT.md`. Exit status is
//! 1 when a single requested case was refused, 0 for a library run that wrote its report.

use std::path::PathBuf;
use vitals_casefactory::report::{render, Outcome};
use vitals_casefactory::source::Library;
use vitals_casefactory::{compile, Source};

const USAGE: &str = "usage:
  vitals-casefactory compile --cases <dir | dir@ref> --id <case_id> --out <dir>
  vitals-casefactory compile --cases <dir | dir@ref> --all --out <dir>

Compiles cases from the embla-cases library into World packs (<out>/<case_id>.pack.json).
A pack is written only if it passes every gate; --all also writes <out>/REPORT.md.";

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some("compile") {
        eprintln!("{USAGE}");
        std::process::exit(2);
    }
    let Some(cases) = arg(&args, "--cases") else { eprintln!("{USAGE}"); std::process::exit(2) };
    let Some(out) = arg(&args, "--out") else { eprintln!("{USAGE}"); std::process::exit(2) };
    let all = args.iter().any(|a| a == "--all");
    let id = arg(&args, "--id");
    if !all && id.is_none() {
        eprintln!("{USAGE}");
        std::process::exit(2);
    }

    let lib = match Library::open(&cases) {
        Ok(l) => l,
        Err(e) => { eprintln!("error: {e}"); std::process::exit(2) }
    };
    let out = PathBuf::from(out);
    if let Err(e) = std::fs::create_dir_all(&out) {
        eprintln!("error: cannot create {}: {e}", out.display());
        std::process::exit(2);
    }

    let ids: Vec<String> = if all {
        match lib.list() {
            Ok(v) => v,
            Err(e) => { eprintln!("error: {e}"); std::process::exit(2) }
        }
    } else {
        vec![id.unwrap_or_default()]
    };

    let git_ref = lib.ref_label();
    let season = lib.season_sources();
    let mut results: Vec<(String, Outcome)> = Vec::new();
    let mut refused_any = false;
    for id in ids {
        let outcome = if season.contains(&id) {
            Outcome::Refused(vitals_casefactory::Refusal {
                case_id: id.clone(),
                reason: vitals_casefactory::report::SEASON_SOURCE_REASON.to_string(),
            })
        } else {
            match lib.read(&id) {
            Err(e) => Outcome::Refused(vitals_casefactory::Refusal { case_id: id.clone(), reason: e }),
            Ok(json) => match compile(&json, Source::of("embla-cases", &git_ref, &json)) {
                Ok(pack) => {
                    let path = out.join(format!("{}.pack.json", pack.case_id));
                    match serde_json::to_string_pretty(&pack) {
                        Ok(text) => match std::fs::write(&path, text) {
                            Ok(()) => {
                                println!("compiled  {id}  {}  → {}", pack.archetype, path.display());
                                Outcome::Compiled(Box::new(pack))
                            }
                            Err(e) => Outcome::Refused(vitals_casefactory::Refusal { case_id: id.clone(), reason: format!("cannot write {}: {e}", path.display()) }),
                        },
                        Err(e) => Outcome::Refused(vitals_casefactory::Refusal { case_id: id.clone(), reason: format!("cannot serialise: {e}") }),
                    }
                }
                Err(r) => Outcome::Refused(r),
            },
        }
        };
        if let Outcome::Refused(r) = &outcome {
            refused_any = true;
            eprintln!("refused   {id}  — {}", r.reason);
        }
        results.push((id, outcome));
    }

    if all {
        let report = render(&results, &lib.dir.display().to_string(), &git_ref, lib.commit().as_deref());
        let path = out.join("REPORT.md");
        if let Err(e) = std::fs::write(&path, report) {
            eprintln!("error: cannot write {}: {e}", path.display());
            std::process::exit(2);
        }
        let compiled = results.iter().filter(|(_, o)| matches!(o, Outcome::Compiled(_))).count();
        println!("{} cases: compiled {compiled}, refused {} — {}", results.len(), results.len() - compiled, path.display());
    } else if refused_any {
        std::process::exit(1);
    }
}
