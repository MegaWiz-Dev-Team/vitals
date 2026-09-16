//! The words the pictures are made from — carried over verbatim from the two scripts that made the
//! first sixty faces and the first nine state sets (`docs/internal/portraits/batch.py` and
//! `states.py`, 15–16 Sep 2026), so a face this job makes is the same kind of face as the ones the
//! founder has already looked at.
//!
//! Nothing here names a real person, a place's flag, a hospital or a brand. The base is "unwell
//! on admission"; the states are the engine's own words (`vitals_web::ward::PORTRAIT_LADDER`),
//! and `dead` has no prompt because no picture of a dead patient is made — the board shows her
//! last living state and says died in words.

use crate::catalogue::Sex;

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
        let child = match sex {
            Sex::F => "girl",
            Sex::M => "boy",
        };
        return format!(
            "Documentary photograph, 35mm film, natural window light: a {age}-year-old {child} from {place} lying in a \
             hospital bed, plain pale-green hospital gown, real skin texture with pores, natural child proportions, \
             calm expression, looking at the camera, shallow depth of field, no text, no logos, no flags"
        );
    }
    format!(
        "Portrait photograph of a {age}-year-old {} from {place}, lying in a hospital bed, \
         wearing a plain pale-green hospital gown, soft neutral ward lighting, calm expression, \
         looking at the camera, shallow depth of field, no text, no logos, no flags",
        sex.word()
    )
}

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

/// The five states a picture is made for, in the ladder's order. `stable` is the base and `dead`
/// is never made.
pub const STATES: [&str; 5] = ["recovered", "improving", "deteriorating", "critical", "arrest"];

/// The edit for one state, with her pronoun.
pub fn state(state: &str, sex: Sex) -> Option<String> {
    let she = match sex {
        Sex::F => "She",
        Sex::M => "He",
    };
    let body = match state {
        "improving" => "{She} is improving: colour returning to the face, eyes open and clear, a faint calm expression, the oxygen mask gone, IV cannula still taped on the hand, propped a little higher on the pillow.",
        "deteriorating" => "{She} is deteriorating: an oxygen mask over nose and mouth, eyes half closed, grey-pale sweaty skin, lips slightly dusky, head tilted back, visibly struggling to breathe.",
        "critical" => "{She} is critical: eyes closed, very pale, a non-rebreather oxygen mask with the bag, the blanket drawn up to the chest, dimmer light, no movement, no smile. Quiet and clinical, nothing graphic.",
        "arrest" => "{She} is in cardiac arrest: eyes closed, ashen grey skin, lips blue-grey, no oxygen mask at all, nothing on the face, the blanket flat to the chest, dim light. Still, quiet and clinical — no hands, no equipment, nothing graphic.",
        "recovered" => "{She} has recovered: sitting up in the bed, healthy colour in the face, relaxed relieved expression, no mask, no ECG leads, only a small clear plaster on the back of the hand, the same plain pale-green gown with no print, badge or logo.",
        _ => return None,
    };
    Some(format!("{KEEP}{}", body.replace("{She}", she)))
}
