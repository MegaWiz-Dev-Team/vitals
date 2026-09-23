//! The words the pictures are made from — carried over verbatim from the two scripts that made the
//! first sixty faces and the first nine state sets (`docs/internal/portraits/batch.py` and
//! `states.py`, 15–16 Sep 2026), so a face this job makes is the same kind of face as the ones the
//! founder has already looked at.
//!
//! Nothing here names a real person, a place's flag, a hospital or a brand. The base is "unwell
//! on admission"; the states are the engine's own words (`vitals_web::ward::PORTRAIT_LADDER`),
//! and `dead` has no prompt because no picture of a dead patient is made — the board shows her
//! last living state and says died in words.

use crate::sex::Sex;

/// Below this age the painter needs a different opening (developer-16, 16 Sep: KOR-0 at 8 came
/// out as a doll).
pub const CHILD_UNDER: u16 = 16;

/// The base: her, in a bed, on the day she was admitted.
///
/// Two prompts, not one. The adult prompt is the batch's, verbatim, and made the sixty faces the
/// founder looked at. A child asked for the same way comes out as anime — "an 8-year-old girl from
/// South Korea" gave three anime pictures in a row on three seeds — and the brief's clause
/// ("photorealistic, natural child proportions, documentary style; not a drawing, not anime, not a
/// doll") did not change that: a diffusion model attends to the words "anime" and "doll" whether
/// or not a "not" stands before them. What worked, on two seeds, was a positive opening in a
/// photographer's own vocabulary and no negatives at all; both faces passed the gate on both judge
/// models (calibration 16 Sep 2026, scratch trials A and C).
pub fn base(age: u16, sex: Sex, place: &str) -> String {
    if age < CHILD_UNDER {
        return format!(
            "Documentary photograph, 35mm film, natural window light: {} from {place} lying in a hospital bed, plain \
             pale-green hospital gown, real skin texture with pores, natural proportions for her age, calm expression, \
             looking at the camera, shallow depth of field, no text, no logos, no flags",
            child_phrase(age, sex)
        );
    }
    format!(
        "Portrait photograph of a {age}-year-old {} from {place}, lying in a hospital bed, \
         wearing a plain pale-green hospital gown, soft neutral ward lighting, calm expression, \
         looking at the camera, shallow depth of field, no text, no logos, no flags",
        sex.word()
    )
}

/// Who the child is, in words the painter reads as an age. "An 8-year-old girl" comes out as a
/// toddler (judged 3–4 on every seed tried); "a schoolgirl aged 8" comes out as a schoolgirl
/// (judged 7; a schoolgirl aged 6 judged 6). But "a schoolboy aged 12" came out as a boy of about
/// five on all three seeds of the first real tick (17 Sep 2026, SSD-3 at 12: judged 5, 5, and the
/// third refused on style; the pictures are in work/refused) — the word pins the look at
/// primary-school age whatever the number after it says. So four brackets: little, school,
/// young adolescent for ten to twelve, teenage from thirteen. The age gate is what proves the
/// bracket, one face at a time.
pub fn child_phrase(age: u16, sex: Sex) -> String {
    let (girl, boy) = match age {
        0..=5 => ("a little girl", "a little boy"),
        6..=9 => ("a schoolgirl", "a schoolboy"),
        10..=12 => ("a young adolescent girl", "a young adolescent boy"),
        _ => ("a teenage girl", "a teenage boy"),
    };
    format!("{} aged {age}", if sex == Sex::F { girl } else { boy })
}

/// The second question the gate asks of a child's face, verbatim from the brief. The answer is
/// read as a number and held to the door's band for the drawn age.
pub const AGE: &str = "About how old does this child look? Answer with one number.";

/// The one question the gate asks of every face.
///
/// Not the brief's wording, and here is why. Asked "Is this a photorealistic photograph-style image
/// of one real-looking human patient with natural proportions — not a drawing, anime, doll, or 3D
/// render? Answer yes or no.", both judge models answered **No** to every face we have — the doll
/// and the six the founder had already looked at — because they read it as "is this a real
/// photograph?" and correctly recognised AI-generated skin. Told that the picture is generated on
/// purpose and asked what it shows and in what style, they say No to the doll, No to anime, No to
/// a picture with two children in it (the first remake of KOR-0@8 passed one before this clause
/// existed), and Yes to the singles. Calibrated 16 Sep 2026 on 11 faces: gemini-2.5-flash 11/11,
/// gemini-3.1-flash-lite 10/11 (one false refusal, KEN-1, for a garbled gown tag).
pub const PHOTOREAL: &str = "This picture is AI-generated on purpose; do not judge whether it is a real photo. Judge only \
what it shows and its STYLE: is it a photograph-style picture of exactly ONE person — one patient in the bed and nobody \
else — with natural human proportions, natural skin and natural eyes, rather than a drawing, anime, cartoon, doll or \
stylised 3D render? Answer yes or no, then one short sentence why.";

/// What every state edit begins with: the same person, the same room.
pub const KEEP: &str = "Edit this photo, keeping exactly the same person — same face, same hair, same skin, same age — \
and the same hospital bed, gown, lighting and framing. No text, no logos, no printed badge on the gown. ";

/// The five states made after admission, in the ladder's order. `stable` is made too, at pack
/// time, from the base (see [`STABLE`]); `dead` is never made.
pub const STATES: [&str; 5] = ["recovered", "improving", "deteriorating", "critical", "arrest"];

/// The key the painted face is filed under. Never sent: the ward's ladder does not know it, and
/// the founder's rule is that a patient's picture looks like a patient — so the face the painter
/// made (calm, looking at the camera, often smiling) is the reference the states are edited from
/// and nothing else.
pub const BASE: &str = "base";

/// The made stable: admitted and holding, not well. The smile is `recovered`'s.
pub const STABLE: &str = "stable";

/// The one observable thing each state shows — read by the editor and the judge alike.
///
/// The judge reads a sentence literally and the editor renders what it is told, so both read one
/// text: the edit prompt is [`KEEP`] + "She is now <feature>." and the gate's second question
/// carries the same words. One feature, no interpretive words ("critically ill", "struggling")
/// and no colour of skin or lips: the first run of the gate (16 Sep, Salma Gaber) refused four of
/// five states on props and adjectives the editor had not rendered, and the identity question
/// failed on arrest, where colour words change the face most. Stable's stays as it was. Recovered
/// is the only face that smiles (founder's rule, 16 Sep): improving is better, not well, and the
/// three worse states end in "not smiling" — the first patient through the gate kept a faint smile
/// in her critical and arrest faces because nothing had said otherwise.
pub fn feature(state: &str) -> Option<&'static str> {
    Some(match state {
        "stable" => "unwell and tired but comfortable, lying in a hospital bed, eyes open, not smiling",
        "improving" => "propped up a little on the pillow, eyes open and clear, no oxygen mask, a small plaster on the back of the hand, not smiling",
        "deteriorating" => "wearing an oxygen mask over the nose and mouth, eyes half closed, not smiling",
        "critical" => "eyes closed, an oxygen mask on, the blanket drawn up to the chest, not smiling",
        "arrest" => "eyes closed, no mask, lying completely still, the blanket flat to the chest, not smiling",
        "recovered" => "sitting up in the bed with no oxygen mask, looking well",
        _ => return None,
    })
}

/// What the judge is asked to see: the feature, verbatim.
pub fn sentence(state: &str) -> Option<&'static str> {
    feature(state)
}

/// The gate's first question, asked with the reference picture first and the made picture second.
pub const SAME_PERSON: &str = "Both pictures are AI-generated on purpose; do not judge whether they are real photos. The first is \
the reference. Is this the same person as the reference picture? Judge the shape of the face and the features, the hair \
and the age only — the state, the expression, the equipment, and the skin colour and tone may differ. Answer yes or no, \
then one short sentence why.";

/// The gate's second question, with the state's own sentence in it.
pub fn shows(state: &str) -> Option<String> {
    sentence(state).map(|s| format!(
        "This picture is AI-generated on purpose; do not judge whether it is a real photo. Does this picture show a patient \
         who is {s}? Answer yes or no, then one short sentence why."
    ))
}

/// The edit for one state, with her pronoun — the founder's wording for an adult, and for a child
/// the gentle wording below.
pub fn state(state: &str, sex: Sex) -> Option<String> {
    state_for(state, sex, false)
}

/// The edit for one state: [`KEEP`], then "She is now <feature>." — the same words the judge is
/// given. A child's states were once asked for in gentler words than an adult's (the image editor
/// refused the adult wording of "deteriorating" for a child as prohibited content, 16 Sep); the
/// features carry no colour of skin or lips for anyone now, so one text serves both, and `child`
/// is kept so the wording can part again without changing a caller.
pub fn state_for(state: &str, sex: Sex, child: bool) -> Option<String> {
    let _ = child;
    let she = match sex {
        Sex::F => "She",
        Sex::M => "He",
    };
    let f = feature(state)?;
    Some(format!("{KEEP}{she} is now {f}."))
}
