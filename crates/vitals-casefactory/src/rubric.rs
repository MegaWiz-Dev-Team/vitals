//! The mark sheet, derived: what the plan says to do becomes `action`, what a sentence pins to
//! the clock becomes `action_by`, what the case forbids becomes `no_harm`, the win is one
//! `outcome`, and everything the case defines but does not pay for is cleared for
//! `no_unindicated`. Forty points, split by the case's own rubric dimension weights.

use crate::archetype::Archetype;
use crate::embla::Case;
use crate::interventions::Built;
use crate::plan::{Mapped, PlanStep};
use crate::scenario::Sim;
use serde_json::{json, Value};

/// A bucket of items that shares one dimension's weight.
struct Bucket {
    weight: f64,
    items: Vec<Value>,
}

/// Largest-remainder split of `total` into integer shares proportional to `weights`, every
/// share at least `min` where the weight is positive. Deterministic: ties go to the earlier index.
fn split(total: u32, weights: &[f64], min: u32) -> Vec<u32> {
    let n = weights.len();
    if n == 0 {
        return vec![];
    }
    let sum: f64 = weights.iter().sum();
    if sum <= 0.0 {
        return vec![0; n];
    }
    let mut shares: Vec<u32> = weights.iter().map(|w| if *w > 0.0 { min } else { 0 }).collect();
    let reserved: u32 = shares.iter().sum();
    let pool = total.saturating_sub(reserved);
    let exact: Vec<f64> = weights.iter().map(|w| w / sum * pool as f64).collect();
    let mut floors: Vec<u32> = exact.iter().map(|e| e.floor() as u32).collect();
    let mut left = pool - floors.iter().sum::<u32>();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| {
        let ra = exact[a] - exact[a].floor();
        let rb = exact[b] - exact[b].floor();
        rb.partial_cmp(&ra).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(&b))
    });
    for i in order {
        if left == 0 {
            break;
        }
        if weights[i] > 0.0 {
            floors[i] += 1;
            left -= 1;
        }
    }
    for i in 0..n {
        shares[i] += floors[i];
    }
    shares
}

/// The case's own checklist under one rubric dimension, trimmed, blanks dropped.
fn dimension_criteria<'a>(case: &'a Case, key: &str) -> impl Iterator<Item = &'a String> {
    let key = key.to_string();
    case.hidden.rubric.dimensions.iter().filter(move |d| d.key == key).flat_map(|d| d.criteria.iter()).filter(|c| !c.trim().is_empty())
}

fn weight(case: &Case, key: &str, default: f64) -> f64 {
    case.hidden.rubric.dimensions.iter().find(|d| d.key == key).map(|d| d.weight).unwrap_or(default)
}

/// The mark sheet, and the case's management checklist with what each criterion was paid on.
pub struct Derived {
    pub rubric: Value,
    pub criteria: Vec<PlanStep>,
}

pub fn derive(case: &Case, a: Archetype, mapped: &Mapped, built: &Built, sim: &Sim, source_sha: &str, review: Option<&crate::source::Review>) -> Derived {
    // ── history: the lines worth digging for ─────────────────────────────────────
    let mut history: Vec<Value> = Vec::new();
    for pass in ["on_direct_ask", "on_ask"] {
        for ask in built.asks.iter().filter(|k| k.present && k.reveal == pass) {
            if history.len() >= 3 {
                break;
            }
            let finding = built.voice.get(&ask.id).map(|v| v.finding.clone()).unwrap_or_default();
            history.push(json!({ "label": format!("Asked about: {finding}"), "type": "action", "needle": ask.id }));
        }
    }

    // ── examination: the systems the case's own criteria name, else any examination ──
    let mut exam: Vec<Value> = Vec::new();
    let criteria: Vec<String> = case
        .hidden
        .rubric
        .dimensions
        .iter()
        .filter(|d| d.key == "examination")
        .flat_map(|d| d.criteria.iter().map(|c| c.to_lowercase()))
        .collect();
    for (id, display, system) in &built.exams {
        if exam.len() >= 3 {
            break;
        }
        let words: Vec<String> = crate::text::keywords(display).into_iter().chain(crate::text::keywords(system)).filter(|w| !w.contains(' ')).collect();
        if criteria.iter().any(|c| words.iter().any(|w| c.contains(w.as_str()))) {
            exam.push(json!({ "label": format!("Examined: {display}"), "type": "action", "needle": id }));
        }
    }
    if exam.is_empty() && !built.exams.is_empty() {
        exam.push(json!({ "label": "Examined the patient", "type": "action_any", "any_of": built.exams.iter().map(|(id, _, _)| id.clone()).collect::<Vec<_>>() }));
    }

    // ── investigations: the first four of the expected workup ────────────────────
    // An investigation the plan also names as a treatment step (cultures before antibiotics)
    // is credited by either id, so the learner who says "blood cultures" and lands the
    // treatment-side intervention is not marked as having skipped the workup.
    let workup_n = case.hidden.expected_workup.len().min(built.ix.len());
    let ix: Vec<Value> = built.ix.iter().take(workup_n).take(4)
        .map(|(id, display)| {
            let low = crate::interventions::short_display(display).to_lowercase();
            let twins: Vec<String> = mapped.present.iter()
                .filter(|p| p.role.kind != crate::archetype::Kind::Harmful && p.role.kw.iter().any(|k| crate::text::contains_kw(&low, k)))
                .map(crate::plan::Present::tx_id)
                .collect();
            if twins.is_empty() {
                json!({ "label": format!("Ordered: {display}"), "type": "action", "needle": id })
            } else {
                let mut any_of = vec![id.clone()];
                any_of.extend(twins);
                json!({ "label": format!("Ordered: {display}"), "type": "action_any", "any_of": any_of })
            }
        })
        .collect();

    // ── diagnosis ────────────────────────────────────────────────────────────────
    let dx = vec![json!({ "label": "Named the diagnosis", "type": "action", "needle": built.dx_id })];

    // ── management: every critical and gate role, timed where a sentence timed it ─
    let mut tx: Vec<Value> = Vec::new();
    for p in mapped.present.iter().filter(|p| matches!(p.role.kind, crate::archetype::Kind::Critical | crate::archetype::Kind::Gate) || (p.role.kind == crate::archetype::Kind::Rescue && sim.paid_rescue.contains(&p.role.id))) {
        let id = p.tx_id();
        match sim.late.iter().find(|(rid, _)| rid == p.role.id) {
            Some((_, by)) => tx.push(json!({ "label": format!("{} within {}:{:02}", p.role.label, (*by as u32) / 60, (*by as u32) % 60), "type": "action_by", "needle": id, "by_sec": by })),
            None => tx.push(json!({ "label": p.role.label, "type": "action", "needle": id })),
        }
    }
    // ── the case's own management checklist: paid, never required ──────────────
    // A `management_safety` criterion names an order the reviewer wants marked even though it
    // does not turn the trajectory — the empirical antibiotic beside the antimalarial, the
    // vitamin A in measles, the tranexamic acid beside the uterotonics. The first present,
    // non-harmful role its words name is paid as an `action` under the criterion's own label;
    // a role the turn already pays for is not paid twice; a criterion that names no order is
    // listed with nothing under it, so the reviewer sees it was not placed.
    let mut criteria: Vec<PlanStep> = Vec::new();
    for c in dimension_criteria(case, "management_safety") {
        // every present order the criterion's words name; the first not already paid is the one
        // it pays, so "release the tourniquet after the airway" pays the release, not the airway
        let hits: Vec<&crate::plan::Present> = mapped
            .present
            .iter()
            .filter(|p| p.role.kind != crate::archetype::Kind::Harmful)
            .filter(|p| crate::plan::positive_hit(&p.role, c))
            .collect();
        let unpaid = hits.iter().find(|p| !tx.iter().any(|it| it["needle"] == p.tx_id()));
        let mut placed: Vec<String> = Vec::new();
        match (unpaid, hits.first()) {
            (Some(p), _) => {
                let id = p.tx_id();
                tx.push(json!({ "label": c.trim(), "type": "action", "needle": id }));
                placed.push(id);
            }
            (None, Some(p)) => placed.push(p.tx_id()),
            (None, None) => {}
        }
        criteria.push(PlanStep { step: c.clone(), interventions: placed });
    }
    let outcome_label = if sim.wins.len() > 1 {
        "The patient survives — home, or intensive care where the rhythm needed a shock"
    } else {
        match sim.win { "win_icu" => "The patient survives to intensive care", _ => "The patient survives to discharge" }
    };
    let outcome = vec![json!({ "label": outcome_label, "type": "outcome", "any_of": sim.wins })];

    // ── red flags: the harms the case defines, and the ones the clock defines ─────
    // The bucket holds as many items as it has points, first come first kept — so the case's
    // own `red_flag_recognition` checklist goes first: a criterion that names a harm the pack
    // can fire (by the forbidden order's own words, or by the order a trigger guards) puts
    // that harm at the head of the list, in the checklist's order. Everything else follows in
    // the order the archetype lists it.
    let mut no_harm: Vec<Value> = Vec::new();
    let mut named: Vec<String> = Vec::new();
    let harm_item = |p: &crate::plan::Present| p.role.harm.map(|h| json!({ "label": format!("Did not give: {}", p.role.label.to_lowercase()), "type": "no_harm", "needle": h }));
    let trigger_item = |text: &str| json!({ "label": format!("Avoided: {}", text.split(" — ").next().unwrap_or(text)), "type": "no_harm", "needle": text });
    // the longest keyword the criterion contains names the harm it means: "releasing the
    // tourniquet before the airway" is about the tourniquet, not the airway's clock
    let kw_len = |role: &crate::archetype::Role, c: &str| role.kw.iter().filter(|k| crate::text::contains_kw(&c.to_lowercase(), k)).map(|k| k.len()).max();
    for c in dimension_criteria(case, "red_flag_recognition") {
        let mut candidates: Vec<(usize, String, Value)> = Vec::new();
        for p in mapped.harmful() {
            if let (Some(len), Some(it)) = (kw_len(&p.role, c), harm_item(p)) {
                candidates.push((len, p.role.harm.unwrap_or_default().to_string(), it));
            }
        }
        for (id, text) in &sim.trigger_harms {
            let role_id = id.split_once('_').map(|(_, r)| r).unwrap_or(id);
            if let Some(len) = mapped.get(role_id).and_then(|p| kw_len(&p.role, c)) {
                candidates.push((len, text.clone(), trigger_item(text)));
            }
        }
        if let Some((_, needle, it)) = candidates.into_iter().max_by_key(|(len, _, _)| *len) {
            if !named.contains(&needle) {
                named.push(needle);
                no_harm.push(it);
            }
        }
    }
    for p in mapped.harmful() {
        if let Some(it) = harm_item(p) {
            if !named.contains(&p.role.harm.unwrap_or_default().to_string()) {
                no_harm.push(it);
            }
        }
    }
    // The clock's harms come after the case's own, and at most two of them: the timed
    // `action_by` items already score timeliness, so these are the reminder, not the mark.
    let mut late_items = 0;
    for (id, text) in &sim.trigger_harms {
        if named.contains(text) {
            continue;
        }
        if id.starts_with("late_") {
            late_items += 1;
            if late_items > 2 {
                continue;
            }
        }
        no_harm.push(trigger_item(text));
    }
    no_harm.truncate(6);

    // ── points ───────────────────────────────────────────────────────────────────
    // Forty, split across the buckets by the case's own dimension weights first (largest
    // remainder), then across a bucket's items evenly. A bucket with more items than points
    // keeps its first items — they are listed in priority order — and drops the rest, so every
    // item on the sheet is worth at least one whole point. `communication` is a judged
    // dimension with no deterministic evidence and is not a bucket; its weight is simply absent
    // from the sum, which is how it is redistributed.
    let buckets = [
        Bucket { weight: weight(case, "history_completeness", 15.0), items: history },
        Bucket { weight: weight(case, "examination", 10.0), items: exam },
        Bucket { weight: weight(case, "investigation_choice", 15.0), items: ix },
        Bucket { weight: weight(case, "diagnostic_accuracy", 25.0), items: dx },
        Bucket { weight: weight(case, "management_safety", 15.0) * 0.7, items: tx },
        Bucket { weight: weight(case, "management_safety", 15.0) * 0.3, items: outcome },
        Bucket { weight: weight(case, "red_flag_recognition", 10.0), items: no_harm },
    ];
    let weights: Vec<f64> = buckets.iter().map(|b| if b.items.is_empty() { 0.0 } else { b.weight }).collect();
    let total: u32 = 40;
    let bucket_points = split(total, &weights, 1);

    let mut items: Vec<Value> = Vec::new();
    for (i, b) in buckets.into_iter().enumerate() {
        if b.items.is_empty() || bucket_points[i] == 0 {
            continue;
        }
        let keep = (bucket_points[i] as usize).min(b.items.len());
        let per: Vec<f64> = vec![1.0; keep];
        let shares = split(bucket_points[i], &per, 1);
        for (j, mut it) in b.items.into_iter().take(keep).enumerate() {
            it["points"] = json!(shares[j]);
            items.push(it);
        }
    }

    // ── the deduction: everything defined and unpaid is cleared, so the sheet charges only
    //    for orders the case does not define at all
    let credited: Vec<String> = items.iter().filter_map(|it| it.get("needle").and_then(|n| n.as_str()).map(str::to_string))
        .chain(items.iter().filter(|it| it["type"] == "action_any").flat_map(|it| it["any_of"].as_array().cloned().unwrap_or_default().into_iter().filter_map(|v| v.as_str().map(str::to_string))))
        .collect();
    let allow: Vec<String> = built
        .interventions
        .iter()
        .filter_map(|iv| iv["id"].as_str().map(str::to_string))
        .filter(|id| !id.starts_with("ask_") && !id.starts_with("exam_"))
        .filter(|id| !credited.iter().any(|c| id.contains(c.as_str())))
        .collect();
    items.push(json!({ "label": "Ordered nothing this patient did not need", "type": "no_unindicated", "per_item": 2, "max_penalty": 6, "allow": allow }));

    // The shift's bar is the ward's one number — 70 %, the canonical star bar every rubric in
    // the system is pinned to — and never the case's own OSCE pass mark, which grades a
    // different exam (the clinical advisor's ruling 3.1, 20 Sep 2026).
    let pass_bps = vitals_progress::STAR_PASS_BPS;
    let osce_mark = case.hidden.rubric.pass_mark.map(|p| format!("{p:.0} %")).unwrap_or_else(|| "unset".into());
    // "provisional … Not clinically reviewed." until the case's own review block says otherwise;
    // then the sheet opens with the ruling and closes by naming the reviewer and the date.
    let (opening, closing) = match review.filter(|r| r.reviewed()) {
        Some(r) => (r.sentence(), format!("Clinically {}.", r.sentence())),
        None => ("provisional".to_string(), "Not clinically reviewed.".to_string()),
    };
    let rubric = json!({
        "case": case.meta.id,
        "pass_bps": pass_bps,
        "status": format!(
            "{opening} — compiled by vitals-casefactory from embla-cases {} v{} (sha256:{}) under the {} archetype: management_plan→action/action_by, red_flags→no_harm, correct_diagnosis→action, expected_workup→action, outcome from the replay. Judged dimensions dropped (communication). Points scaled to 40 from the case's rubric dimension weights. Pass mark {} % — the ward's, not the case's OSCE mark of {}. {closing}",
            case.meta.id,
            case.meta.version.as_deref().unwrap_or("?"),
            &source_sha[..source_sha.len().min(16)],
            a.id(),
            pass_bps / 100,
            osce_mark
        ),
        "items": items,
    });
    Derived { rubric, criteria }
}
