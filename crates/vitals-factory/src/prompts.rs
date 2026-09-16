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

/// The base: her, in a bed, on the day she was admitted.
pub fn base(age: u16, sex: Sex, place: &str) -> String {
    format!(
        "Portrait photograph of a {age}-year-old {} from {place}, lying in a hospital bed, \
         wearing a plain pale-green hospital gown, soft neutral ward lighting, calm expression, \
         looking at the camera, shallow depth of field, no text, no logos, no flags",
        sex.word()
    )
}

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
