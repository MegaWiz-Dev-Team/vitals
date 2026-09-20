//! `REPORT.md` for a library run: what compiled under which archetype, what was refused and
//! why — grouped by reason, so the next archetype to write is the top of a list rather than a
//! guess.

use crate::Pack;
use crate::Refusal;
use std::collections::BTreeMap;

/// The refusal every season source carries, word for word, so the report can group on it.
pub const SEASON_SOURCE_REASON: &str = "season source: recorded in embla-cases deployments.jsonl as deployed to target vitals — World never carries the season's content";

pub enum Outcome {
    Compiled(Box<Pack>),
    Refused(Refusal),
}

/// Coarsen a refusal reason to the family it belongs to, for grouping.
pub fn reason_family(reason: &str) -> String {
    let r = reason;
    if r.starts_with("language: ") {
        let lang = r.trim_start_matches("language: ").split(' ').next().unwrap_or("?");
        format!("language: {lang} (no translation step yet)")
    } else if r.starts_with("season source") {
        "season source (the season's content, excluded by rule)".into()
    } else if r.starts_with("the prose still states") {
        "prose still states the patient's age or sex (placeholder pass missed it)".into()
    } else if r.starts_with("presenting vitals within normal") {
        "presenting vitals within normal (NEWS2 low — cut by the advisor's rule 3.7)".into()
    } else if r.starts_with("no archetype fits: ") && !r.contains("names no deterioration") {
        // a diagnosis the library knows it cannot model yet — keep the shape's name
        let why = r.trim_start_matches("no archetype fits: ");
        format!("no archetype yet: {}", why.split(" — ").next().unwrap_or(why))
    } else if r.starts_with("no archetype fits") {
        "no archetype fits (stable presentation)".into()
    } else if r.contains("not forced") {
        let suggested = r.split("words suggest ").nth(1).and_then(|s| s.split(' ').next()).unwrap_or("?");
        format!("words suggest {suggested} but the vitals do not (not forced)")
    } else if r.contains("names no therapy") {
        let a = r.split("the plan names no therapy the ").nth(1).and_then(|s| s.split(' ').next()).unwrap_or("?");
        format!("plan names no critical therapy for {a}")
    } else if r.contains("no blood pressure") || r.contains("no heart rate") || r.contains("no respiratory rate") || r.contains("is not a blood pressure") {
        "vital signs missing or unreadable".into()
    } else if r.starts_with("difficulty") {
        "difficulty not one the ward serves".into()
    } else if r.contains("does not parse") {
        "case.json does not parse".into()
    } else if r.contains("management path") {
        "management path does not win (archetype/plan mismatch)".into()
    } else if r.contains("untreated") {
        "untreated replay does not die in bound".into()
    } else if r.contains("leaks") {
        "pack leaks a name or a marker".into()
    } else if r.contains("rubric") {
        "rubric derivation failed".into()
    } else {
        r.split(':').next().unwrap_or(r).to_string()
    }
}

pub fn render(results: &[(String, Outcome)], library: &str, git_ref: &str, commit: Option<&str>) -> String {
    let compiled: Vec<(&String, &Pack)> = results.iter().filter_map(|(id, o)| match o { Outcome::Compiled(p) => Some((id, p.as_ref())), _ => None }).collect();
    let refused: Vec<(&String, &Refusal)> = results.iter().filter_map(|(id, o)| match o { Outcome::Refused(r) => Some((id, r)), _ => None }).collect();

    let mut s = String::new();
    s.push_str("# Case factory report\n\n");
    s.push_str(&format!("Library: `{library}` at `{git_ref}`{}\n\n", commit.map(|c| format!(" (commit `{c}`)")).unwrap_or_default()));
    s.push_str(&format!("**Totals:** {} cases — compiled {} · refused {}\n\n", results.len(), compiled.len(), refused.len()));

    // the persona placeholders: what was written, and whether anything got past the scan
    let age_ph: usize = compiled.iter().map(|(_, p)| p.placeholders.age).sum();
    let sex_ph: usize = compiled.iter().map(|(_, p)| p.placeholders.sex).sum();
    let prose_refused = refused.iter().filter(|(_, r)| r.reason.starts_with("the prose still states")).count();
    s.push_str(&format!(
        "**Persona placeholders:** `{{age}}` written {age_ph} times and sex placeholders {sex_ph} times across {} packs; {prose_refused} pack(s) refused because prose still stated the patient's age or sex.\n\n",
        compiled.len()
    ));

    // archetype coverage
    let mut by_arch: BTreeMap<&str, Vec<&Pack>> = BTreeMap::new();
    for (_, p) in &compiled {
        by_arch.entry(p.archetype.as_str()).or_default().push(p);
    }
    s.push_str("## Archetype coverage\n\n| archetype | compiled | endemic | untreated death (sim s, min–max) |\n|---|---|---|---|\n");
    for (a, packs) in &by_arch {
        let endemic = packs.iter().filter(|p| p.endemic).count();
        let deaths: Vec<f64> = packs.iter().map(|p| p.replay.untreated_death_sec).collect();
        let lo = deaths.iter().cloned().fold(f64::INFINITY, f64::min);
        let hi = deaths.iter().cloned().fold(0.0, f64::max);
        s.push_str(&format!("| {a} | {} | {endemic} | {lo:.0}–{hi:.0} |\n", packs.len()));
    }

    // the ACLS scenarios, in the order the advisor asked the first three to be taught (3.4):
    // VF/pVT, then PEA/asystole, then a bradycardia with a pulse — the tachycardias after.
    // This orders the report only: the ward draws a case by the patient's country, level and
    // the pack's version, and the patient factory names the case it built her for.
    let acls_rank = |p: &Pack| -> Option<(u8, &'static str)> {
        match p.sce["initial_state"].as_str().unwrap_or("") {
            "arrest_vf" => Some((1, "VF/pVT")),
            "arrest_pea" | "arrest_asystole" => Some((2, "PEA/asystole")),
            "brady_unstable" => Some((3, "bradycardia with a pulse")),
            s if s.starts_with("tachy_") => Some((4, "tachycardia with a pulse (SVT/AF)")),
            _ => None,
        }
    };
    let mut acls: Vec<(u8, &'static str, &String, &Pack)> = compiled.iter().filter_map(|(id, p)| acls_rank(p).map(|(r, l)| (r, l, *id, *p))).collect();
    if !acls.is_empty() {
        acls.sort_by(|a, b| a.0.cmp(&b.0).then(a.2.cmp(b.2)));
        s.push_str("\n## ACLS scenarios, in teaching order\n\n");
        s.push_str("The clinical advisor's order (3.4, 20 Sep 2026): VF/pVT first, PEA/asystole second, bradycardia with a pulse third; the tachycardias with a pulse follow. The rhythm is spoken in words on the chart. This orders the report — the ward draws by the patient's country, level and the pack's version, and the patient factory names her case.\n\n| # | scenario | case | entry | wins |\n|---|---|---|---|---|\n");
        for (rank, label, id, p) in &acls {
            let wins: Vec<&str> = p.sce["outcomes"].as_array().map(|o| o.iter().filter(|o| o["kind"] == "win").filter_map(|o| o["id"].as_str()).collect()).unwrap_or_default();
            s.push_str(&format!("| {rank} | {label} | `{id}` | {} | {} |\n", p.sce["initial_state"].as_str().unwrap_or("?"), wins.join(", ")));
        }
    }

    // refused by language: how many World-ready library cases are not in English
    let mut by_lang: BTreeMap<String, Vec<&String>> = BTreeMap::new();
    for (id, r) in &refused {
        if let Some(rest) = r.reason.strip_prefix("language: ") {
            let lang = rest.split(' ').next().unwrap_or("?").to_string();
            by_lang.entry(lang).or_default().push(id);
        }
    }
    let by_language_total: usize = by_lang.values().map(Vec::len).sum();
    s.push_str("\n## Refused by language (World compiles English cases only — no translation step yet)\n\n");
    if by_lang.is_empty() {
        s.push_str("None: every case in this library that reached the gate is in English.\n");
    } else {
        s.push_str(&format!("{by_language_total} case(s) refused before anything else was read.\n\n| language | cases |\n|---|---|\n"));
        for (lang, ids) in &by_lang {
            s.push_str(&format!("| {lang} | {} |\n", ids.len()));
        }
        for (lang, ids) in &by_lang {
            s.push_str(&format!("\n### {lang} — {}\n\n", ids.len()));
            for id in ids {
                s.push_str(&format!("- `{id}`\n"));
            }
        }
    }

    // the season's own, refused by rule
    let season: Vec<&String> = refused.iter().filter(|(_, r)| r.reason.starts_with("season source")).map(|(id, _)| *id).collect();
    s.push_str("\n## Season sources (refused by rule — World never carries the season's content)\n\n");
    if season.is_empty() {
        s.push_str("None recorded for this library (no `deployments.jsonl` entry with target `vitals`).\n");
    } else {
        for id in &season {
            s.push_str(&format!("- `{id}`\n"));
        }
    }

    // per case
    s.push_str("\n## Per case\n\n| case | result | archetype / reason |\n|---|---|---|\n");
    for (id, o) in results {
        match o {
            Outcome::Compiled(p) => s.push_str(&format!(
                "| `{id}` | compiled | {} — dies untreated at {:.0} s, wins `{}` at {:.0} s{} |\n",
                p.archetype, p.replay.untreated_death_sec, p.replay.win_outcome, p.replay.win_sec,
                if p.endemic { ", endemic" } else { "" }
            )),
            Outcome::Refused(r) => s.push_str(&format!("| `{id}` | refused | {} |\n", r.reason.replace('|', "/"))),
        }
    }

    // refused, grouped
    let mut groups: BTreeMap<String, Vec<&String>> = BTreeMap::new();
    for (id, r) in &refused {
        groups.entry(reason_family(&r.reason)).or_default().push(id);
    }
    let mut ordered: Vec<(&String, &Vec<&String>)> = groups.iter().collect();
    ordered.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(b.0)));
    s.push_str("\n## Refused, by reason\n\n");
    s.push_str("The archetype library grows from the top of this list.\n\n");
    for (family, ids) in ordered {
        s.push_str(&format!("### {family} — {}\n\n", ids.len()));
        for id in ids {
            s.push_str(&format!("- `{id}`\n"));
        }
        s.push('\n');
    }
    s
}
