//! **A tape with no shock on it hashes exactly as it did before shocks existed.**
//!
//! `Step::Shock` is the fourth step to be added to a format that is already anchored on a public
//! chain. Each of the three before it — `Act`, `Ask`, `Set`/`Off` — was added under the same
//! bargain and wrote the same sentence into `leaf()`: a distinct one-character prefix, so a tape
//! that never uses the new step produces byte-for-byte the bytes it produced before the step
//! existed, and every leaf already anchored still verifies.
//!
//! That sentence has never been checked. This file checks it, and checks it against the only
//! thing that can settle it — the *actual bytes* every anchored run names.
//!
//! A run's identity on chain is `sce_hash`, and `conformance/sce-archive/` holds every version of
//! every case any anchored run could have been played against, named by its own digest and never
//! deleted. So the property is: one fixed tape that uses every step kind **except** a shock,
//! replayed against every file in that archive, produces exactly these leaves. Regenerate the
//! table only when you have decided, deliberately, that an anchored run is allowed to move — and
//! it never is.
//!
//! Measured across the change that added the step: all twenty-five versions archived before it
//! produced identical leaves after it.

use std::path::{Path, PathBuf};
use vitals_replay::{hex, leaf, replay, sce_hash, Step};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn archive() -> PathBuf {
    root().join("conformance/sce-archive")
}

/// One tape that uses every step kind there is, except a shock.
///
/// Deliberately not good medicine and deliberately not case-specific: it is a fixture for hashes,
/// and it has to mean the same thing against seventeen different patients. What matters is that
/// every `leaf()` arm except the new one is exercised — a tick, a raw order, a resolved order, a
/// question, a dial, and a device coming off — because a prefix collision would show up as one of
/// *those* leaves moving, not as the shock's.
fn every_step_but_a_shock() -> Vec<Step> {
    vec![
        Step::Tick(30.0),
        Step::did("adrenaline im"),
        Step::acted("give oxygen", "oxygen"),
        Step::asked("any allergies?"),
        Step::Set("o2".into(), 6.0),
        Step::Tick(90.0),
        Step::Off("o2".into()),
        Step::did("normal saline bolus"),
        Step::Tick(600.0),
        Step::Tick(600.0),
    ]
}

fn leaf_of(sce_json: &str, tape: &[Step]) -> String {
    let h = sce_hash(sce_json);
    let r = replay(sce_json, tape).expect("replay");
    hex(&leaf(&h, tape, &r))
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// `(sce_hash, leaf of `every_step_but_a_shock` against it)`, for every version in the archive.
///
/// Both halves are checkable by hand: the first is the file's own name and its sha256, and the
/// second is what `cargo test -p vitals-replay --test shock_tape -- --nocapture` prints.
const PINNED: &[(&str, &str)] = &[
    ("0a74511e605d02ec2ec5ff1d23705fea54b7d01b66db0997f7782a3096adeb7f",
     "6d29143bc1ea02a86af150e0871131a4185c7788588b8e8a84cc7957f4638142"),
    ("109bee5badd6e52955e4c8312f29be10c305f32f2ba777a445eff149d2747ea3",
     "9e96286353e141bae615d319f503ce73772c45e0dbe795d8492bff9d9b2a07d4"),
    ("11382d87ea21b9177787966f539c50d488c9e6544786442835d49fba466c9a7b",
     "71cc192d12829a716a86afe12dbf228ebbc84bbfa0fcc41f28ab3ee8d8bd8b2e"),
    ("145c0f6827c7f39ace39d8f5fd7bda33a92b996a7502367a81b03cd70a58e63d",
     "7eeeff48bf70673881b8be11ce0afb41ea90325b4bcda013e739d1a32d9bf23e"),
    ("242a0c9f770e22b87031fa0c2917346d47839ff133385b1251fa9d9df341bf28",
     "70172dde2bcb11f4aefa7c12a77eb9a2d653df5646ad37e682a9ee3595f50d7b"),
    ("30a48cb233a2a0b8bf8e811c5d74db0f097b315f4abfb6f5b5ae8f6fd1addfb8",
     "32db6828c01023dd144bd4f320d4f604688f8f3aae89af8aa0f6e200cb31c9d4"),
    ("36b6d1c22d41c681eb0edb565d58ff32e1f32b24d4d074ee7db2220d80b6be72",
     "787877c8a186a833d7e54bdd91b9a5c4aa1a8aa3c3be175adb6b2458dedc9f62"),
    ("3b54e3af94e2d225a06468dee1e005441f16a905fd9596244db04cb7891f8fc8",
     "98c5e2d6d770a7e6ab5b6e3eaf86a06e432692ac6059d77e417eb36c4f8416ee"),
    ("3c4359e677049054381cd9d41edab11af7e789a3703c0abc2306a226ed863b7e",
     "3a9a5e19c9d27333d5a01a2d32bf8fdf5f37c469864d25102d3f5ca7cacce478"),
    ("44f9c59774dc4b6eaa9c00b282665c78ca9e2b535e9ca8dd39fcb1b7b7987117",
     "4ab1bb5ac84490f837db42bbd312d01f1381c00f1ea6e4ce391fb197afc2a2ea"),
    ("4706a5c57e2f559b520652cdc9c9cdb227241ad62d689e911a381b6774ed7a5d",
     "0a11c08055d7528ae75dc267af2b708aaa84326c7be0e68f7adf3a286eca8847"),
    ("4d5c177971b36efe19b8e51d0c3bcc283e7b43da0fc9ac18c86d34481675271e",
     "921fca22dad0877add43a9f98116f4bf7cd0c19689f9bed73241673ebd674a5e"),
    ("4ee5521614895b474296fdcdc4e355009d23e6a5fcbff5d1bfdd86765d1e993d",
     "1f9a0477243774ab26a8a3cea1bfa0041886d0ceb2762f152ded89d5827b7170"),
    ("4f763103e02fa0480678a55a67bd0d1589a390235a24403c92e3f10b5747abbb",
     "4a08f78c599ffcbd902342111d975fc6f6b514e70754263e1c7c8ddc8cb38a50"),
    ("5c0e1270f68b9665be6ea7a90552129ee649712bfa77327c8fdb969adcaaceff",
     "487d5d50ac4dcc601ff56772b245a9aabdfb2b71c463f7b3a033c6aedf5cefb6"),
    ("627368e3beefd457f97597dbd81ad108b9864fb08f01f1baac32ca1671ff54d7",
     "65c647130dcc12412eb1b945a6d7c07765b5e66c749e1d691fe172cfb21684e5"),
    ("661d058cf7788ebcac31ffc9d5ed9962b25ee73a482f57df4f2a754d182c66bf",
     "fc9d231b8f3e8bb27649d3fbd6a57d27098b9d55dec0727d8243e78803aab726"),
    ("6e14f222b29b047d406be2e13841de60dddc12c2b7b9ee74bbc59a8357f27638",
     "afa75558c6bb0393529096a4c4ba0b76dc4b8a3d46f0bcc65f594f47b40b891e"),
    ("6f7e620fc6ea084c6bb30bd9eaabd0d6fac574bc15ac189620c3d5bc42f414cc",
     "004dd56f9a65eb8e9add368c1ceb4864f320b46f243986e6d61eeab14a267a13"),
    ("7e4998729c67f71e7d48e238347e5dc1f30d231ef8cd85aa16e070616aa63f68",
     "62b1e42b8e5f56d811b7b424e0f9e6506181d29311ed5329805cb90f33699867"),
    ("87fbc1290cb48f8ec78bb0d6b6efc80677ae6f9b9b26628fce4fb1b9c1b7d662",
     "02a3113803efe2e4d767af5066dd2f10f966b574b8d390a895532389a58d54b0"),
    ("8d59708b53440a9564f51c08380ed2ac84b291fd99659f49c0353d185ca33948",
     "0c08dc5dfb3a909a7ee24ba6e91df26a1618a361f56eb0837999fa6cd0552ba6"),
    ("90e52ac0e31d7ce60965ef1a2ef60302a695bf6baf0d5fa0be3431b0053cb642",
     "db3d0a4f04dc91a7b8ac57fda4ab4f5c7a084b36060a4112797f838316593fa1"),
    ("91efb325fc8e06e541d6837189d6a992562a53e97ae57d32cd6fd8d0c8b6c3f0",
     "bc72a408f3e92cef757fbd81e5cbd03ecd37ec712541b89eddfd68003808d595"),
    ("9433956764028dd157b71c5c0a1f06333ca107ece26b781f4b311811d2229f33",
     "18f7c8ee87eea0fed726a341141c2a1d3caf3c0625e091e2c9aa01d43e98ce46"),
    ("aa1d8be0a5c8a69dc969337dd9979fc2207426797019d1b25bd84f40cf901545",
     "d487db0a95f888a122017917edec0b18dc27a94958b18de917c754ef94202be1"),
    ("abce636f126ed9588d03c6d8ecc7306bd628a5802e06c0f1a18a4f3c60639f2a",
     "19d0d0212a77f81b6acd49f5cbad2b235b3ef2083cab227d75f0d98675063665"),
    ("ac52be1cda7ea6199664b25759217dcb8a04a7ac65adaeaca572ccf202828798",
     "9014bbf7dcff0b3dc0cc08319855ae87b817c41217b576540a11ace963d92635"),
    ("b61fbebc65f71eaead162fd377183044b4acf62a1a6f5e3f39658c900ed723e8",
     "a72ce1667f3aa22908bae0866b35f4a8d738619d13cdc6848312ebdbf7b43ec1"),
    ("b9bc24963c3ed5bf344c51bb1749155a1df4f10b59d6209c27ccb902686a5e68",
     "134ce4c0397b5f99b934f28c239c8d48c62df715638da59f3f61aab1f6f19280"),
    ("b9bfa9c57e40dcc5dfc342431b8ae9b7f2836649876f721d2d0f62fe70a577fb",
     "465740596cbe7769105e23ccf24ef04e566231f3c8aeeab7290eed4359716ea3"),
    ("c1788b378cf400595a94ca3e97eba47bfbc0e558d7ad07af71ff2c419d2334a8",
     "28f5906df265702b6900a8e9b74ac66d38e1ce33d6feece816257dd682253f98"),
    ("c3111d6cc242dd41c54e9cbf0f23751f5eac65262d99cc1aff3524a4afff5c67",
     "6255fb390a53e1debc03b62978efe0f4d7c5f3fa9b14f8cde2a5f9ec84e55b56"),
    ("d4e616827ba1d262821d94b15036bd59c3d4a35a00f716eb090cef1de74cf5d1",
     "3b552afefc3c11b09f8451a787e470d6d271a80820a1c944a5a1c1e37abfd0bf"),
    ("e39de235c0a2ddb92b954930944029e8b02ff5ef13eb16dcaf9271942ac1426d",
     "70e28fcd1f19d5ce94545ff46e6c5479c6e102e42aa75696aa8630bf460140cf"),
    ("ece6ed587279f3ae51dd6ccfbccb8ba0b47fb0ee8578e61eacd72e6702f122d7",
     "1d6840ccb14985bd691c07b590c09e1e30a6026e1b2989d1a5cca29713cfb009"),
    ("ee5cfc438a4c46c554d329824891383d046ebeb1320e58e4b33094ca53807b9b",
     "9682c823ef8eee27919b230e726e126cd05478bd3bcd5661dc7b3bc159e8fa78"),
    ("f9dbb7de0551815620d88e7b8ff66292e5fbdf565801c972e216ce329c380e45",
     "96f3f84345bdb78f4afc6745a575ba4fc557c1e894e5f4cec0eba54b920280ff"),
];

#[test]
fn every_scenario_an_anchored_run_can_name_still_hashes_to_the_same_leaf() {
    let dir = archive();
    let tape = every_step_but_a_shock();
    let mut moved = Vec::new();
    for (h, want) in PINNED {
        let f = dir.join(format!("{h}.json"));
        let json = read(&f);
        // The archive is named by digest, so this is also the check `VERIFICATION.md` §5 tells a
        // stranger to run — done here on every build rather than only when somebody asks.
        assert_eq!(&hex(&sce_hash(&json)), h, "{h}.json does not hash to its own name");
        let got = leaf_of(&json, &tape);
        println!("    (\"{h}\",\n     \"{got}\"),");
        if &got != want {
            moved.push(format!("  {h}\n    was {want}\n    now {got}"));
        }
    }
    assert!(
        moved.is_empty(),
        "{} archived scenario(s) now replay to a different leaf — every run anchored against \
         them has been orphaned:\n{}",
        moved.len(),
        moved.join("\n")
    );
}

/// …and the table covers the whole archive, so adding a version cannot quietly skip the check.
#[test]
fn nothing_in_the_archive_is_left_out_of_the_table() {
    let mut on_disk: Vec<String> = std::fs::read_dir(archive())
        .expect("the archive")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.len() == 69 && n.ends_with(".json"))
        .map(|n| n[..64].to_string())
        .collect();
    on_disk.sort();
    let mut pinned: Vec<String> = PINNED.iter().map(|(h, _)| (*h).to_string()).collect();
    pinned.sort();
    assert_eq!(on_disk, pinned, "the archive and the pin table have drifted apart");
}

/// The prefix has to be the *only* thing that separates a shock from every other step.
///
/// Asserted structurally rather than by reading `leaf()`: one tape per step kind, and seven
/// distinct leaves. A prefix reused for two kinds would collapse two of these onto each other,
/// which is exactly the failure that would let one run be presented as another.
#[test]
fn no_two_kinds_of_step_hash_the_same_way() {
    let json = read(&root().join("conformance/sce-anaphylaxis-ep1.json"));
    let kinds: Vec<(&str, Step)> = vec![
        ("tick", Step::Tick(1.0)),
        ("do", Step::Do("x".into())),
        ("act", Step::Act { text: "x".into(), id: String::new() }),
        ("ask", Step::Ask("x".into())),
        ("set", Step::Set("x".into(), 1.0)),
        ("off", Step::Off("x".into())),
        ("shock", Step::Shock(1.0)),
    ];
    let mut seen: Vec<(&str, String)> = Vec::new();
    for (name, step) in kinds {
        let l = leaf_of(&json, std::slice::from_ref(&step));
        if let Some((other, _)) = seen.iter().find(|(_, h)| h == &l) {
            panic!("`{name}` and `{other}` hash to the same leaf — a prefix is shared");
        }
        seen.push((name, l));
    }
    assert_eq!(seen.len(), 7);
}

/// A shock is on the leaf, and the joules are on it too.
///
/// The number the learner dialled is part of what happened: 200 J and 360 J into the same rhythm
/// are the same clinical act at two different energies, and a leaf that could not tell them apart
/// would be certifying a run nobody played.
#[test]
fn the_energy_reaches_the_leaf() {
    let json = read(&root().join("conformance/sce-anaphylaxis-ep1.json"));
    let base = every_step_but_a_shock();
    let with = |j: f64| {
        let mut t = base.clone();
        t.insert(1, Step::Shock(j));
        leaf_of(&json, &t)
    };
    let none = leaf_of(&json, &base);
    assert_ne!(with(200.0), none, "a shock left no trace on the leaf");
    assert_ne!(with(200.0), with(360.0), "two different energies produced one leaf");
    // …and adding one to the end is not the same as adding one at the front. The tape is ordered
    // and the leaf has to say so.
    let mut tail = base.clone();
    tail.push(Step::Shock(200.0));
    assert_ne!(with(200.0), leaf_of(&json, &tail), "the leaf lost the order of the tape");
}

/// Millijoules, like every other float on the tape.
///
/// `leaf()` quantises so that a float which round-trips differently through JSON cannot change a
/// hash. A dial with three decimal places is not a thing, so this is about serialisation rather
/// than about medicine — but a tape that is written to disk and read back has to hash the same
/// either way, and that is the property.
#[test]
fn the_energy_is_quantised_before_it_is_hashed() {
    let json = read(&root().join("conformance/sce-anaphylaxis-ep1.json"));
    let l = |j: f64| leaf_of(&json, &[Step::Shock(j)]);
    assert_eq!(l(200.0), l(200.000_04), "a rounding wobble moved the leaf");
    assert_ne!(l(200.0), l(200.002), "the dial lost a difference it should keep");
}

/// A saved tape and a replayed tape are the same tape.
///
/// The step goes to disk through serde and comes back through it, and a shock that serialised to
/// something the reader could not parse would lose a run — not corrupt it, lose it.
#[test]
fn a_shock_survives_the_round_trip_through_disk() {
    let tape = vec![Step::Tick(1.0), Step::Shock(360.0), Step::Tick(1.0)];
    let json = serde_json::to_string(&tape).expect("serialise");
    assert!(json.contains("shock"), "the step does not name itself on disk: {json}");
    let back: Vec<Step> = serde_json::from_str(&json).expect("deserialise");
    assert_eq!(tape, back);
}

