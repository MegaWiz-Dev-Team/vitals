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

/// Below this age the painter needs telling that a child is a photograph too (developer-16,
/// 16 Sep: KOR-0 at 8 came out as a doll).
pub const CHILD_UNDER: u16 = 16;

/// What a child's prompt adds, verbatim from the brief.
pub const CHILD: &str = "photorealistic, natural child proportions, documentary style; not a drawing, not anime, not a doll";

/// The base: her, in a bed, on the day she was admitted.
pub fn base(age: u16, sex: Sex, place: &str) -> String {
    let child = if age < CHILD_UNDER { format!(", {CHILD}") } else { String::new() };
    format!(
        "Portrait photograph of a {age}-year-old {} from {place}, lying in a hospital bed, \
         wearing a plain pale-green hospital gown, soft neutral ward lighting, calm expression, \
         looking at the camera, shallow depth of field, no text, no logos, no flags{child}",
        sex.word()
    )
}

/// The one question the gate asks of every face.
///
/// Not the brief's wording, and here is why: asked "Is this a photorealistic photograph-style image
/// of one real-looking human patient with natural proportions — not a drawing, anime, doll, or 3D
/// render? Answer yes or no.", gemini-3.1-flash-lite answered **No** to every face we have — the
/// doll, and the six the founder had already looked at — because it read the question as "is this
/// a real photograph?" and correctly recognised AI-generated skin (16 Sep 2026, calibrated on
/// KOR-0@8, VNM-0@8, THA-0, IDN-1, MMR-1@16, PAK-0@57, PHL-0@69). Told that the picture is generated
/// on purpose and asked about STYLE only, it says No to the doll and Yes to the six. That is the
/// question the gate exists to ask.
pub const PHOTOREAL: &str = "This picture is AI-generated on purpose; do not judge whether it is a real photo. Judge its \
STYLE only: does it look like a photograph of a real person — natural human proportions, natural skin, natural eyes — \
rather than a drawing, anime, cartoon, doll, or stylised 3D render? Answer yes or no.";

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
