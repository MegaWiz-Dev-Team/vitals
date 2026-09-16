//! The physiology archetypes — the small library of deterministic state machines a case is
//! compiled under — and the rule that picks one, or refuses.
//!
//! An archetype is a *shape* of deterioration (a pressure that falls, a saturation that falls, a
//! consciousness that fades) with the treatment roles that bend it. It is parameterised from the
//! case — the starting vitals, which roles the management plan names, which orders the case
//! forbids — and never the other way round: a case whose words fit no shape, or whose vitals at
//! presentation do not match the shape its words suggest, is **refused with a reason**. The
//! archetype library grows by adding a shape, not by stretching one over a patient it does not
//! describe.

use crate::embla::{Case, Vitals0};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Archetype {
    /// Distributive shock from infection: pressure falls, fluids + antibiotics (+ pressor, +
    /// source control where the plan names it) turn it.
    SepticShock,
    /// Blood is leaving: pressure falls fast, volume and blood and stopping the bleeding turn it.
    HaemorrhagicShock,
    /// The pump is failing: pressure and saturation fall together; inotrope and pressor turn it,
    /// a fluid bolus makes it worse.
    CardiogenicShock,
    /// The muscles of breathing are failing with a clear chest: saturation falls until the
    /// airway is taken over; the specific antidote stops the progression.
    NeuromuscularRespiratoryFailure,
    /// Consciousness fading with a low sugar: GCS falls, the airway follows; dextrose and the
    /// specific therapy turn it.
    CnsDepressionHypoglycaemia,
    /// A child in compensated shock: the pulse pressure narrows before the systolic falls;
    /// measured fluid turns it and too much fluid harms.
    PaediatricCompensatedShock,
    /// Lungs or airway failing with a beating heart: saturation falls; oxygen plus the specific
    /// therapy the plan names turn it.
    HypoxicRespiratoryFailure,
}

pub const ALL: [Archetype; 7] = [
    Archetype::PaediatricCompensatedShock,
    Archetype::CardiogenicShock,
    Archetype::HaemorrhagicShock,
    Archetype::SepticShock,
    Archetype::NeuromuscularRespiratoryFailure,
    Archetype::CnsDepressionHypoglycaemia,
    Archetype::HypoxicRespiratoryFailure,
];

/// What a treatment role does to the shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// One of the therapies that, together, turn the trajectory. Every critical role the plan
    /// names must be done before the patient starts to recover.
    Critical,
    /// Helps, buys time, or is simply right — never required for the turn.
    Supportive,
    /// Hurts. Recorded as harm, and pushes the shape the wrong way.
    Harmful,
    /// Must happen before hands-on care (isolation before examination). A trigger records harm
    /// when it is skipped.
    Gate,
}

/// A one-off nudge to the vitals when the role is applied. Small on purpose: the *turn* comes
/// from the state change once every critical role is in, not from one order's delta.
#[derive(Debug, Clone, Copy)]
pub struct Nudge {
    pub var: &'static str,
    pub delta: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct Role {
    /// Becomes the intervention id `tx_<id>` and the rubric needle.
    pub id: &'static str,
    pub label: &'static str,
    /// Found in a plan step ⇒ the role exists in this case; also the free-text matcher keywords.
    pub kw: &'static [&'static str],
    pub kind: Kind,
    /// The line the engine emits. Neutral: no name, no pronoun that assumes a person.
    pub beat: &'static str,
    /// The harm sentence, for `Kind::Harmful` — the `no_harm` needle is a substring of it.
    pub harm: Option<&'static str>,
    pub nudges: &'static [Nudge],
    /// Equipment the order leaves on the patient, with the setting the chart shows.
    pub equipment: Option<(&'static str, f64)>,
}

const fn role(
    id: &'static str,
    label: &'static str,
    kw: &'static [&'static str],
    kind: Kind,
    beat: &'static str,
    nudges: &'static [Nudge],
) -> Role {
    Role { id, label, kw, kind, beat, harm: None, nudges, equipment: None }
}

const fn harm(
    id: &'static str,
    label: &'static str,
    kw: &'static [&'static str],
    text: &'static str,
    nudges: &'static [Nudge],
) -> Role {
    Role { id, label, kw, kind: Kind::Harmful, beat: text, harm: Some(text), nudges, equipment: None }
}

const fn n(var: &'static str, delta: f64) -> Nudge {
    Nudge { var, delta }
}

// ── roles every archetype knows ─────────────────────────────────────────────────────

pub const OXYGEN: Role = Role {
    id: "oxygen",
    label: "Oxygen",
    kw: &["oxygen", "high-flow", "high flow", "non-rebreather", "nasal cannula", "face mask", "o2 mask", "ออกซิเจน"],
    kind: Kind::Supportive,
    beat: "oxygen on — the reservoir bag fills and the trace steadies a little",
    harm: None,
    nudges: &[n("spo2", 3.0)],
    equipment: Some(("o2", 10.0)),
};

pub const COMMON: &[Role] = &[
    OXYGEN,
    role("admit", "Admit to a monitored bed",
        &["admit", "icu", "hdu", "intensive care", "high-dependency", "high dependency", "monitored bed", "ccu", "step-down", "รับไว้", "รับนอน", "แอดมิท"],
        Kind::Supportive, "admitted to a monitored bed — the team is told what to watch for", &[]),
    role("monitor", "Monitoring plan",
        &["monitoring", "monitor ", "reassess", "serial ", "ติดตาม", "เฝ้าระวัง", "ประเมินซ้ำ", "strict fluid balance"],
        Kind::Supportive, "continuous monitoring set — the numbers are watched, not assumed", &[]),
    role("explain", "Explain and obtain consent",
        &["explain", "communicat", "consent", "counsel", "อธิบาย", "ให้ความรู้", "แจ้งญาติ"],
        Kind::Supportive, "the situation is explained plainly and consent is taken", &[]),
    role("notify", "Notify public health",
        &["notif", "surveillance", "public-health", "public health", "reportable", "report the case", "แจ้งโรค", "รายงานโรค"],
        Kind::Supportive, "the case is notified — the health authority is told today", &[]),
    role("analgesia", "Analgesia",
        &["analgesia", "morphine", "fentanyl", "paracetamol", "pain relief", "ยาแก้ปวด"],
        Kind::Supportive, "analgesia given — pain is treated, not used as a sign", &[]),
    role("antiemetic", "Antiemetic",
        &["ondansetron", "antiemetic", "metoclopramide", "ยาแก้อาเจียน"],
        Kind::Supportive, "antiemetic in — the vomiting settles", &[]),
    role("catheter", "Urinary catheter and hourly output",
        &["urinary catheter", "catheter", "urine output", "สายสวน"],
        Kind::Supportive, "catheter in — urine output is measured by the hour", &[]),
    role("nil_by_mouth", "Nothing by mouth",
        &["nothing by mouth", "nil by mouth", "npo", "nasogastric", "ng tube", "งดน้ำงดอาหาร"],
        Kind::Supportive, "nothing by mouth — a nasogastric tube drains the stomach", &[]),
    role("position", "Position the patient",
        &["recovery position", "sit up", "sit her up", "sit him up", "upright", "head up", "legs up", "จัดท่า"],
        Kind::Supportive, "repositioned — the airway and the breathing are easier", &[]),
];

/// Orders that hurt only when the case says not to give them. Included when a plan step or a
/// red flag names one inside a negated sentence.
pub const NEGATED_HARMS: &[Role] = &[
    harm("nsaid", "Non-steroidal anti-inflammatory",
        &["nsaid", "ibuprofen", "diclofenac", "mefenamic", "ketorolac", "naproxen"],
        "a non-steroidal given where bleeding or the kidney forbids it",
        &[n("sbp", -3.0)]),
    harm("steroids", "Corticosteroid",
        &["corticosteroid", "steroid", "dexamethasone", "hydrocortisone", "mannitol", "สเตียรอยด์"],
        "a corticosteroid given where the evidence says it harms rather than helps",
        &[n("gcs", -1.0)]),
    harm("im_injection", "Intramuscular injection",
        &["intramuscular", "im injection"],
        "an intramuscular injection into a bleeding patient — a haematoma and an exposed needle",
        &[n("sbp", -2.0)]),
    harm("fluoroquinolone", "Fluoroquinolone",
        &["ciprofloxacin", "fluoroquinolone", "levofloxacin"],
        "a fluoroquinolone where resistance is near-universal — the hour is lost to a drug that will not work",
        &[]),
    harm("oral_only", "Oral-only therapy",
        &["oral-only", "oral only", "oral antimalarial"],
        "oral therapy alone for a patient who is vomiting and obtunded — the drug never arrives",
        &[]),
    harm("prophylactic_anticonvulsant", "Prophylactic phenobarbital",
        &["prophylactic phenobarbital", "phenobarbital"],
        "prophylactic phenobarbital — it raised mortality in the trial",
        &[n("rr", -3.0)]),
    harm("wound_interference", "Cutting or sucking the bite",
        &["incision", "cryotherapy", "electric shock", "suction the bite", "ice"],
        "the bite was cut and sucked — nothing gained, a wound made",
        &[]),
    harm("hypotonic_fluid", "Hypotonic or sugar-containing bolus",
        &["hypotonic", "dextrose-containing", "d5w", "5% dextrose", "half-strength", "0.45%"],
        "a hypotonic or sugar-containing bolus for shock — the volume leaves the vessels",
        &[n("sbp", -3.0)]),
];

// ── shared role definitions, picked into archetypes below ────────────────────────────

const FLUIDS_KW: &[&str] = &["crystalloid", "ringer", "saline", "fluid", "nss", "ml/kg", "สารน้ำ", "ให้น้ำเกลือ"];
const ANTIBIOTIC_KW: &[&str] = &["antibiotic", "antimicrobial", "ceftriaxone", "meropenem", "piperacillin", "tazobactam", "cefotaxime", "ceftazidime", "vancomycin", "metronidazole", "ampicillin", "gentamicin", "amikacin", "azithromycin", "ยาปฏิชีวนะ"];
const PRESSOR_KW: &[&str] = &["noradrenaline", "norepinephrine", "vasopressor", "adrenaline infusion", "epinephrine infusion", "dopamine", "vasopressin", "ยากระตุ้นความดัน"];
const TRANSFUSION_KW: &[&str] = &["transfus", "packed red", "fresh frozen plasma", "ffp", "platelets", "blood products", "whole blood", "ให้เลือด"];
const SPECIFIC_KW: &[&str] = &["artesunate", "artemether", "quinine", "ribavirin", "antivenom", "anti-snake", "asv", "benznidazole", "nifurtimox", "antitoxin", "aciclovir", "acyclovir", "oseltamivir", "ivig", "immunoglobulin", "plasma exchange", "plasmapheresis", "praziquantel"];
const AIRWAY_KW: &[&str] = &["intubat", "endotracheal", "secure the airway", "airway", "bag-valve", "ventilat", "ใส่ท่อช่วยหายใจ", "ทางเดินหายใจ"];
const DEXTROSE_KW: &[&str] = &["dextrose", "d50", "d10", "50% glucose", "glucose 50", "treat hypoglyc", "hypoglycaemia at once", "correct hypoglyc", "กลูโคส"];
const SOURCE_KW: &[&str] = &["laparotomy", "surgical", "surgery", "source control", "drainage", "debridement", "ercp", "percutaneous", "ผ่าตัด", "ศัลย", "ระบายหนอง"];
const SEDATION_KW: &[&str] = &["sedat", "midazolam", "diazepam", "lorazepam", "benzodiazepine"];
const BETA_BLOCKER_KW: &[&str] = &["beta-blocker", "beta blocker", "metoprolol", "propranolol", "bisoprolol", "carvedilol", "verapamil", "diltiazem"];

const FLUIDS: Role = role("fluids", "Crystalloid bolus, reassessed", FLUIDS_KW, Kind::Critical,
    "crystalloid running — perfusion reassessed after each bolus", &[n("sbp", 8.0)]);
const FLUIDS_CAUTIOUS: Role = role("fluids", "Cautious crystalloid, titrated", FLUIDS_KW, Kind::Supportive,
    "a measured crystalloid bolus — the lungs are checked before the next", &[n("sbp", 5.0)]);
const ANTIBIOTICS: Role = role("antibiotics", "Broad-spectrum antibiotics", ANTIBIOTIC_KW, Kind::Critical,
    "broad-spectrum antibiotic in — after the cultures, inside the hour", &[]);
const ANTIBIOTICS_SUPPORT: Role = role("antibiotics", "Empirical antibiotics", ANTIBIOTIC_KW, Kind::Supportive,
    "empirical antibiotic in — cover while the cultures cook", &[]);
const CULTURES: Role = role("cultures", "Blood cultures before antibiotics", &["blood culture", "cultures", "เพาะเชื้อ"], Kind::Supportive,
    "two sets of blood cultures drawn before the first dose", &[]);
const PRESSOR: Role = role("vasopressor", "Vasopressor", PRESSOR_KW, Kind::Critical,
    "noradrenaline titrated through a dedicated line — the pressure answers", &[n("sbp", 12.0)]);
const PRESSOR_SUPPORT: Role = role("vasopressor", "Vasopressor", PRESSOR_KW, Kind::Supportive,
    "noradrenaline titrated — the pressure is held while the cause is treated", &[n("sbp", 10.0)]);
const SOURCE_CONTROL: Role = role("source_control", "Source control", SOURCE_KW, Kind::Critical,
    "the surgical team is at the bedside — source control is being arranged", &[]);
const TRANSFUSION: Role = role("transfusion", "Transfusion", TRANSFUSION_KW, Kind::Critical,
    "blood products up — what was lost is being replaced", &[n("sbp", 8.0)]);
const TRANSFUSION_SUPPORT: Role = role("transfusion", "Transfusion", TRANSFUSION_KW, Kind::Supportive,
    "blood products up, slowly, with the chest checked for overload", &[n("sbp", 4.0)]);
const SPECIFIC: Role = role("specific_therapy", "Specific therapy for the cause", SPECIFIC_KW, Kind::Critical,
    "the specific therapy is in — the cause is treated, not only the numbers", &[]);
const AIRWAY: Role = Role {
    id: "airway", label: "Secure the airway", kw: AIRWAY_KW, kind: Kind::Critical,
    beat: "the airway is secured and the breathing is taken over — the saturation climbs",
    harm: None, nudges: &[n("spo2", 6.0)], equipment: Some(("ett", 0.0)),
};
const AIRWAY_SUPPORT: Role = Role {
    id: "airway", label: "Airway protection", kw: AIRWAY_KW, kind: Kind::Supportive,
    beat: "the airway is protected — suction, positioning, and a plan to intubate if it slips",
    harm: None, nudges: &[n("spo2", 4.0)], equipment: Some(("ett", 0.0)),
};
const DEXTROSE: Role = role("dextrose", "Correct the hypoglycaemia", DEXTROSE_KW, Kind::Critical,
    "dextrose through the line — the sugar is corrected and will be rechecked", &[n("gcs", 3.0)]);
const DEXTROSE_SUPPORT: Role = role("dextrose", "Correct the hypoglycaemia", DEXTROSE_KW, Kind::Supportive,
    "dextrose through the line — the sugar is corrected and will be rechecked", &[n("gcs", 1.0)]);
const ELECTROLYTES: Role = role("electrolytes", "Correct the electrolytes", &["potassium", "calcium gluconate", "magnesium", "electrolyte", "hyperkalaemia", "hypokalaemia", "hypocalc", "insulin 10 units"], Kind::Supportive,
    "electrolytes corrected under ECG monitoring", &[]);
const ISOLATE: Role = role("isolate", "Isolate and protect staff", &["isolat", "ppe", "personal protective", "precaution", "แยกผู้ป่วย"], Kind::Gate,
    "isolation room, full protective equipment, a contact log started", &[]);
const HAEMOSTASIS: Role = role("haemostasis", "Stop the bleeding", &["tranexamic", "endoscop", "surgical", "surgery", "laparotomy", "uterotonic", "oxytocin", "uterine massage", "balloon", "pressure dressing", "embolis", "ligat", "ผ่าตัด", "ห้ามเลือด", "ส่องกล้อง"], Kind::Critical,
    "the bleeding point is being dealt with — pressure, drugs, or the theatre", &[n("sbp", 4.0)]);
const PPI: Role = role("ppi", "Proton-pump inhibitor", &["pantoprazole", "omeprazole", "proton pump", "ppi"], Kind::Supportive,
    "proton-pump inhibitor in", &[]);
const INOTROPE: Role = role("inotrope", "Inotrope", &["dobutamine", "inotrope", "inotropic", "milrinone", "levosimendan"], Kind::Critical,
    "dobutamine running — the heart is asked for a little more", &[n("sbp", 8.0), n("spo2", 1.0)]);
const DIURETIC: Role = role("diuretic", "Diuretic for congestion", &["furosemide", "diuretic", "ยาขับปัสสาวะ"], Kind::Supportive,
    "furosemide in — the lungs begin to dry", &[n("spo2", 3.0)]);
const DIURETIC_CRITICAL: Role = role("diuretic", "Diuretic for congestion", &["furosemide", "diuretic", "ยาขับปัสสาวะ"], Kind::Critical,
    "furosemide in — the lungs begin to dry", &[n("spo2", 4.0)]);
const NIV: Role = Role {
    id: "niv", label: "Non-invasive ventilation", kw: &["cpap", "niv", "non-invasive", "bipap"], kind: Kind::Supportive,
    beat: "the mask seals and the pressure supports each breath", harm: None,
    nudges: &[n("spo2", 4.0)], equipment: Some(("niv", 10.0)),
};
const PACING: Role = role("pacing", "Pads on, pacing ready", &["pacing", "pads", "atropine"], Kind::Supportive,
    "pads on, pacing ready — a block will not be a surprise", &[]);
const REPERFUSION: Role = role("reperfusion", "Reperfusion", &["pci", "angiography", "cath lab", "thrombolysis", "fibrinolysis", "reperfusion", "streptokinase", "alteplase", "tenecteplase", "coronary"], Kind::Critical,
    "the cath lab is activated — reperfusion is on its way", &[n("sbp", 6.0)]);
const ANTIPLATELET: Role = role("antiplatelet", "Antiplatelet", &["aspirin", "clopidogrel", "ticagrelor", "antiplatelet"], Kind::Supportive,
    "antiplatelet given", &[]);
const ANTIARRHYTHMIC: Role = role("antiarrhythmic", "Antiarrhythmic or cardioversion", &["amiodarone", "cardioversion", "adenosine"], Kind::Supportive,
    "the rhythm is treated", &[]);
const ANTICOAG: Role = role("anticoagulation", "Anticoagulation", &["heparin", "enoxaparin", "anticoag", "thromboprophylaxis", "ยาต้านการแข็งตัว"], Kind::Supportive,
    "anticoagulation started", &[]);
const ANTICOAG_CRITICAL: Role = role("anticoagulation", "Anticoagulation or lysis", &["heparin", "enoxaparin", "anticoag", "thromboly", "alteplase", "ยาต้านการแข็งตัว"], Kind::Critical,
    "anticoagulation started — the clot stops growing", &[n("spo2", 3.0)]);
const PERICARDIOCENTESIS: Role = role("pericardiocentesis", "Pericardiocentesis if tamponade", &["pericardiocentesis"], Kind::Supportive,
    "the effusion is watched with the needle ready", &[]);
const NEOSTIGMINE: Role = role("neostigmine", "Atropine-neostigmine trial", &["neostigmine", "anticholinesterase"], Kind::Supportive,
    "atropine then neostigmine — the eyelids are watched for twenty minutes", &[n("spo2", 1.0)]);
const ANAPHYLAXIS_READY: Role = role("adrenaline_ready", "Adrenaline drawn up at the bedside", &["adrenaline 1:1000", "drawn up", "antivenom reaction"], Kind::Supportive,
    "adrenaline drawn up beside the infusion", &[]);
const LIGATURE: Role = role("ligature", "Release the ligature safely", &["ligature", "tourniquet"], Kind::Supportive,
    "the ligature is released slowly, now that the antidote is running", &[]);
const TETANUS: Role = role("tetanus", "Tetanus toxoid", &["tetanus"], Kind::Supportive,
    "tetanus toxoid given", &[]);
const ANTICONVULSANT: Role = role("anticonvulsant", "Treat a seizure", &["lorazepam", "diazepam", "phenytoin", "levetiracetam", "anticonvulsant", "seizure treatment", "ยากันชัก"], Kind::Supportive,
    "a benzodiazepine is ready for the next seizure — the sugar was checked first", &[]);
const LUMBAR_PUNCTURE: Role = role("lumbar_puncture", "Lumbar puncture", &["lumbar puncture", "csf"], Kind::Supportive,
    "lumbar puncture done after the sugar was corrected — meningitis is being excluded", &[]);
const BRONCHODILATOR: Role = role("bronchodilator", "Bronchodilator", &["salbutamol", "nebul", "bronchodilator", "ipratropium", "saba", "ยาพ่น", "ขยายหลอดลม"], Kind::Critical,
    "nebuliser hissing — the wheeze loosens", &[n("spo2", 4.0), n("rr", -2.0)]);
const STEROIDS_SUPPORT: Role = role("steroids", "Systemic corticosteroid", &["prednisolone", "dexamethasone", "hydrocortisone", "corticosteroid", "steroid", "สเตียรอยด์"], Kind::Supportive,
    "steroid given — for the hours ahead, not this minute", &[]);
const MAGNESIUM: Role = role("magnesium", "Magnesium sulfate", &["magnesium"], Kind::Supportive,
    "magnesium infusing", &[n("spo2", 1.0)]);
const CHEST_DRAIN: Role = role("chest_drain", "Decompress the chest", &["chest drain", "thoracostomy", "needle decompression", "intercostal", "aspiration of", "ใส่สายระบาย"], Kind::Critical,
    "the drain goes in and the air hisses out — the lung re-expands", &[n("spo2", 8.0), n("rr", -4.0)]);
const NITRATE: Role = role("nitrate", "Nitrate", &["nitrate", "nitroglycerin", "gtn", "isosorbide"], Kind::Supportive,
    "nitrate given", &[n("spo2", 1.0)]);
const ADRENALINE_NEB: Role = role("adrenaline_nebulised", "Nebulised adrenaline", &["nebulised adrenaline", "nebulized adrenaline", "adrenaline nebul", "racemic"], Kind::Supportive,
    "nebulised adrenaline — the stridor softens for now", &[n("spo2", 3.0)]);
const IV_ACCESS: Role = role("iv_access", "Vascular access", &["intraosseous", "iv lines", "iv access", "two large-bore", "large-bore"], Kind::Supportive,
    "two lines in", &[]);

// intrinsic harms — present in the archetype whether or not the plan mentions them
const FLUID_BOLUS_HARM: Role = harm("fluid_bolus", "Fluid bolus", FLUIDS_KW,
    "a fluid bolus into a congested heart — the lungs fill", &[n("spo2", -4.0), n("sbp", -3.0)]);
const BETA_BLOCKER_HARM: Role = harm("beta_blocker", "Beta-blocker or rate-slowing drug", BETA_BLOCKER_KW,
    "a negative inotrope in shock — the output falls further", &[n("sbp", -8.0), n("hr", -10.0)]);
const SEDATION_HARM: Role = harm("sedation", "Sedative before the airway", SEDATION_KW,
    "a sedative before the airway was secured — the last of the breathing goes", &[n("spo2", -6.0), n("rr", -4.0)]);
const OVERLOAD_SENTINEL: Role = role("fluids", "Measured crystalloid, reassessed hourly", FLUIDS_KW, Kind::Critical,
    "crystalloid at the measured rate — pulse pressure, refill and haematocrit rechecked at the hour", &[n("sbp", 4.0), n("dbp", -2.0), n("fluid_load", 1.0)]);

impl Archetype {
    pub fn id(self) -> &'static str {
        match self {
            Archetype::SepticShock => "septic_shock",
            Archetype::HaemorrhagicShock => "haemorrhagic_shock",
            Archetype::CardiogenicShock => "cardiogenic_shock",
            Archetype::NeuromuscularRespiratoryFailure => "neuromuscular_respiratory_failure",
            Archetype::CnsDepressionHypoglycaemia => "cns_depression_hypoglycaemia",
            Archetype::PaediatricCompensatedShock => "paediatric_compensated_shock",
            Archetype::HypoxicRespiratoryFailure => "hypoxic_respiratory_failure",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Archetype::SepticShock => "distributive (septic) shock",
            Archetype::HaemorrhagicShock => "haemorrhagic shock",
            Archetype::CardiogenicShock => "cardiogenic shock",
            Archetype::NeuromuscularRespiratoryFailure => "neuromuscular respiratory failure",
            Archetype::CnsDepressionHypoglycaemia => "CNS depression with hypoglycaemia",
            Archetype::PaediatricCompensatedShock => "paediatric compensated shock",
            Archetype::HypoxicRespiratoryFailure => "hypoxic respiratory failure",
        }
    }

    pub fn from_id(id: &str) -> Option<Archetype> {
        ALL.into_iter().find(|a| a.id() == id)
    }

    /// The words that suggest this shape. Matched against the diagnosis first (worth ten), then
    /// the red flags, tags, specialty and title (worth one each).
    fn signals(self) -> &'static [&'static str] {
        match self {
            Archetype::SepticShock => &["septic shock", "sepsis", "septic", "peritonitis", "cholangitis", "urosepsis", "necrotising", "necrotizing", "toxic shock", "meningococc", "perforation", "perforated", "multi-organ failure", "ช็อกจากการติดเชื้อ"],
            Archetype::HaemorrhagicShock => &["haemorrhagic shock", "hemorrhagic shock", "haemorrhage", "hemorrhage", "bleeding", "blood loss", "ruptured", "rupture", "ectopic", "melaena", "melena", "hematemesis", "haematemesis", "variceal", "postpartum", "viral haemorrhagic fever", "vhf", "lassa", "ebola", "exsanguinat", "เลือดออก"],
            Archetype::CardiogenicShock => &["cardiogenic shock", "myocarditis", "pulmonary oedema", "pulmonary edema", "heart failure", "stemi", "myocardial infarction", "tamponade", "cardiomyopathy", "low output", "lvef"],
            Archetype::NeuromuscularRespiratoryFailure => &["neurotoxic", "envenom", "krait", "cobra", "guillain", "myasthenia", "botulism", "neuromuscular", "bulbar", "paralysis", "organophosphate", "ptosis", "periodic paralysis", "งูกัด"],
            Archetype::CnsDepressionHypoglycaemia => &["cerebral malaria", "coma", "impaired consciousness", "encephalopathy", "hypoglycaemia", "hypoglycemia", "status epilepticus", "meningitis", "encephalitis", "reduced consciousness", "unconscious", "altered mental status", "seizure", "ซึม", "หมดสติ"],
            Archetype::PaediatricCompensatedShock => &["dengue shock", "shock", "dehydration", "hypovolaemia", "hypovolemia", "plasma leak", "ช็อก"],
            Archetype::HypoxicRespiratoryFailure => &["asthma", "copd", "pneumonia", "pulmonary embolism", "pneumothorax", "bronchiolitis", "croup", "epiglottitis", "respiratory failure", "hypoxia", "hypoxaemia", "hypoxemia", "pulmonary oedema", "pulmonary edema", "ards", "bronchospasm", "airway obstruction", "stridor", "laryngospasm", "respiratory distress", "หอบ", "หายใจลำบาก"],
        }
    }

    /// Does the presentation actually look like this shape? Words alone never compile a case.
    fn gate(self, case: &Case, v0: &Vitals0) -> Result<(), String> {
        let shock = v0.sbp <= 95.0;
        match self {
            Archetype::SepticShock | Archetype::HaemorrhagicShock | Archetype::CardiogenicShock => {
                if shock { Ok(()) } else { Err(format!("systolic {:.0} at presentation is not shock", v0.sbp)) }
            }
            Archetype::PaediatricCompensatedShock => {
                let age = case.patient.age.unwrap_or(99);
                let child = age < 15 || case.meta.search_tags.iter().any(|t| {
                    let t = t.to_lowercase();
                    t == "child" || t.contains("paediatric") || t.contains("pediatric")
                });
                if !child {
                    return Err("not a child".into());
                }
                let pp = v0.sbp - v0.dbp;
                if pp <= 25.0 || v0.sbp <= 90.0 { Ok(()) } else { Err(format!("pulse pressure {pp:.0} and systolic {:.0} are not shock in a child", v0.sbp)) }
            }
            Archetype::NeuromuscularRespiratoryFailure | Archetype::HypoxicRespiratoryFailure => {
                if v0.spo2 <= 94.0 || v0.rr >= 24.0 { Ok(()) } else { Err(format!("saturation {:.0} and rate {:.0} at presentation are not respiratory failure", v0.spo2, v0.rr)) }
            }
            Archetype::CnsDepressionHypoglycaemia => {
                if v0.gcs <= 13 { Ok(()) } else { Err(format!("GCS {} at presentation is not CNS depression", v0.gcs)) }
            }
        }
    }

    /// Pick the archetype for a case, or say why none fits.
    ///
    /// Score every shape by its words — a hit in the diagnosis itself counts ten, a hit anywhere
    /// else counts one — then walk the candidates from the best down and take the first whose
    /// physiological gate the starting vitals pass. No words at all: refused, naming the
    /// diagnosis. Words but no gate: refused, naming what the words suggested and what the
    /// vitals said, so the reviewer sees the case was **not forced**.
    pub fn detect(case: &Case, v0: &Vitals0) -> Result<Archetype, String> {
        let dx = {
            let d = &case.hidden.correct_diagnosis;
            let mut s = d.display.to_lowercase();
            for a in &d.aliases {
                s.push(' ');
                s.push_str(&a.to_lowercase());
            }
            s.push(' ');
            s.push_str(&case.meta.title.to_lowercase());
            s
        };
        let rest = case.haystack();
        let mut scored: Vec<(u32, Archetype)> = ALL
            .into_iter()
            .map(|a| {
                let score = a.signals().iter().map(|k| if dx.contains(k) { 10 } else if rest.contains(k) { 1 } else { 0 }).sum::<u32>();
                (score, a)
            })
            .filter(|(s, _)| *s > 0)
            .collect();
        // Stable: ties keep ALL's order, which is the clinical priority (a child's shock before
        // an adult's, a failing pump before a leaking vessel before an infection).
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        if scored.is_empty() {
            return Err(format!(
                "no archetype fits: '{}' names no deterioration this library models (stable presentation, {} tier {})",
                case.hidden.correct_diagnosis.display,
                case.meta.care_setting.as_deref().unwrap_or("?"),
                case.meta.clinical_tier.map_or("?".to_string(), |t| t.to_string()),
            ));
        }
        let mut why = Vec::new();
        for (_, a) in &scored {
            match a.gate(case, v0) {
                Ok(()) => return Ok(*a),
                Err(e) => why.push(format!("{} — {e}", a.id())),
            }
        }
        Err(format!(
            "words suggest {} but the vitals at presentation do not ({}); not forced",
            scored[0].1.id(),
            why.join("; ")
        ))
    }

    /// The roles this shape knows, beyond [`COMMON`]. Order is the order interventions are
    /// listed in the scenario and the order the golden path gives them.
    pub fn roles(self) -> &'static [Role] {
        match self {
            Archetype::SepticShock => &[FLUIDS, ANTIBIOTICS, PRESSOR, SOURCE_CONTROL, CULTURES, TRANSFUSION_SUPPORT, ELECTROLYTES, DEXTROSE_SUPPORT, STEROIDS_SUPPORT, ANTICOAG, PPI, IV_ACCESS],
            Archetype::HaemorrhagicShock => &[ISOLATE, FLUIDS, TRANSFUSION, HAEMOSTASIS, SPECIFIC, PRESSOR_SUPPORT, ANTIBIOTICS_SUPPORT, CULTURES, ELECTROLYTES, PPI, IV_ACCESS],
            Archetype::CardiogenicShock => &[INOTROPE, PRESSOR, REPERFUSION, SPECIFIC, DIURETIC, NIV, PACING, ANTIPLATELET, ANTIARRHYTHMIC, ANTICOAG, PERICARDIOCENTESIS, ELECTROLYTES, AIRWAY_SUPPORT],
            Archetype::NeuromuscularRespiratoryFailure => &[AIRWAY, SPECIFIC, NEOSTIGMINE, ANAPHYLAXIS_READY, LIGATURE, TETANUS, ELECTROLYTES, IV_ACCESS],
            Archetype::CnsDepressionHypoglycaemia => &[DEXTROSE, SPECIFIC, AIRWAY_SUPPORT, ANTICONVULSANT, FLUIDS_CAUTIOUS, TRANSFUSION_SUPPORT, ANTIBIOTICS_SUPPORT, CULTURES, LUMBAR_PUNCTURE, ELECTROLYTES],
            Archetype::PaediatricCompensatedShock => &[OVERLOAD_SENTINEL, DEXTROSE_SUPPORT, TRANSFUSION_SUPPORT, ELECTROLYTES, IV_ACCESS, ANTIBIOTICS_SUPPORT, CULTURES],
            Archetype::HypoxicRespiratoryFailure => &[BRONCHODILATOR, CHEST_DRAIN, ANTICOAG_CRITICAL, ANTIBIOTICS, DIURETIC_CRITICAL, ADRENALINE_NEB, STEROIDS_SUPPORT, MAGNESIUM, NIV, NITRATE, AIRWAY_SUPPORT, CULTURES, IV_ACCESS],
        }
    }

    /// Orders that hurt in this shape whatever the plan says.
    pub fn intrinsic_harms(self) -> &'static [Role] {
        match self {
            Archetype::CardiogenicShock => &[FLUID_BOLUS_HARM, BETA_BLOCKER_HARM],
            Archetype::NeuromuscularRespiratoryFailure => &[SEDATION_HARM],
            Archetype::HypoxicRespiratoryFailure => &[SEDATION_HARM, BETA_BLOCKER_HARM],
            Archetype::CnsDepressionHypoglycaemia => &[SEDATION_HARM],
            Archetype::SepticShock | Archetype::HaemorrhagicShock | Archetype::PaediatricCompensatedShock => &[],
        }
    }

    /// In this shape, is oxygen one of the therapies that turn the trajectory?
    pub fn oxygen_is_critical(self) -> bool {
        matches!(self, Archetype::HypoxicRespiratoryFailure)
    }

    /// Sim minutes from the first second to death, untreated from the start. The ward runs one
    /// sim second per real second on shift and one sim minute per real hour between shifts, so
    /// these sit inside the range the sixteen converted cases already span (3 to 14 minutes).
    pub fn death_minutes(self) -> f64 {
        match self {
            Archetype::SepticShock => 12.0,
            Archetype::HaemorrhagicShock => 10.0,
            Archetype::CardiogenicShock => 11.0,
            Archetype::NeuromuscularRespiratoryFailure => 9.0,
            Archetype::CnsDepressionHypoglycaemia => 14.0,
            Archetype::PaediatricCompensatedShock => 12.0,
            Archetype::HypoxicRespiratoryFailure => 10.0,
        }
    }

    /// The bound the validator holds the untreated replay to. Twice the design time: a scenario
    /// that takes longer than that to kill an untreated patient is not the shape it claims.
    pub fn death_bound_sec(self) -> f64 {
        self.death_minutes() * 60.0 * 2.0
    }

    /// Sim seconds in the recovering state before the win is declared.
    pub fn recovery_sec(self) -> f64 {
        300.0
    }
}
