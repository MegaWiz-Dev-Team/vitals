#!/usr/bin/env python3
"""File a reviewer's answers that came back by hand.

The review form can post itself to the server, and it can also hand the reviewer their answers
as JSON to send back over LINE or email — which is what happens while `/api/review` does not
exist yet, and what will keep happening whenever a reviewer is somewhere the server is not.

Both paths must land in the same place under the same key, or the same review filed twice by two
routes becomes two records. The key here is derived exactly as `review.rs` derives it: seconds,
then six bytes of a SHA-256 over the content.

Re-filing the same answers replaces one record rather than adding a second — and, like
`Submission::file` in `review.rs`, that is decided by reading what is already on disk and not by
the key alone. The key cannot do it on its own: file the same answers a minute later and the
timestamp half of the key has moved, so the write lands beside the first record instead of on it.
A match keeps the key and the `at` of the record already there.

    scripts/file-review.py mook.json                  # into ./state (the default disk store)
    scripts/file-review.py --root /data mook.json
    pbpaste | scripts/file-review.py -                # straight from the clipboard
    scripts/file-review.py --dry-run mook.json        # show what would be written

Firestore deployments are not written here on purpose: filing into a live production store from
a laptop is not something a helper script should make easy.
"""

import argparse
import hashlib
import json
import pathlib
import sys
import time

KIND = "review"
ROLES = ("student", "physician")

# Same clamps as review.rs, and counted in characters for the same reason: a byte clamp cuts a
# Thai character in half and the record comes back as mojibake.
LIMITS = {"name": 120, "contact": 200, "notes": 8000, "revision": 64}
# `asked` holds the whole item as it was shown — the four lines the review documents put in front
# of every ruling, not a one-line question — so its clamp is `review::ASKED_MAX`, not the 400 it
# was when the form asked one-line questions.
ANSWER_LIMITS = {"id": 64, "asked": 4000, "chose": 32, "chose_label": 300, "said": 4000}
MAX_ANSWERS = 80


def clamp(v, n):
    return (v if isinstance(v, str) else "")[:n]


def key(at, content):
    d = hashlib.sha256(content.encode("utf-8")).digest()
    return f"{at:010d}-{d[:6].hex()}"


def build(raw, at):
    """The Rust `Submission::from_json`, in Python. Refuses the same things it refuses."""
    role = raw.get("role")
    if role not in ROLES:
        raise ValueError(f"role: expected one of {ROLES}, got {role!r}")

    answers = []
    for a in (raw.get("answers") or [])[:MAX_ANSWERS]:
        said = clamp(a.get("said"), ANSWER_LIMITS["said"])
        chose = clamp(a.get("chose"), ANSWER_LIMITS["chose"])
        # An unanswered question is still not an answer — but picking an option is an answer even
        # with the box empty, and it is the one both documents ask for most often: "what you are
        # doing now is correct". Dropping it here would make agreement read as silence.
        if not said.strip() and not chose:
            continue
        answers.append({
            "id": clamp(a.get("id"), ANSWER_LIMITS["id"]),
            "asked": clamp(a.get("asked"), ANSWER_LIMITS["asked"]),
            "chose": chose,
            "chose_label": clamp(a.get("chose_label"), ANSWER_LIMITS["chose_label"]),
            "said": said,
        })

    notes = clamp(raw.get("notes"), LIMITS["notes"])
    if not answers and not notes.strip():
        raise ValueError("empty: nothing was answered")

    name = clamp(raw.get("name"), LIMITS["name"])
    # Mirrors `Submission::key` exactly, chosen option included: two reviewers who pick opposite
    # branches of one ruling and write nothing would otherwise hash the same and overwrite.
    joined = "\x1e".join(f"{a['id']}={a['chose']}\x1d{a['said']}" for a in answers)
    return {
        "id": key(at, f"{name}\x1f{joined}\x1f{notes}"),
        "at": at,
        "role": role,
        "name": name,
        "contact": clamp(raw.get("contact"), LIMITS["contact"]),
        "anonymous": bool(raw.get("anonymous")),
        "answers": answers,
        "notes": notes,
        "revision": clamp(raw.get("revision"), LIMITS["revision"]),
    }


def says_the_same_as(a, b):
    """The same answers from the same person — what makes two submissions one review.

    Field by field, not by the six bytes of hash in the key: the hash finds a candidate cheaply,
    and acting on a collision would overwrite a different reviewer's answers with this one's.
    """
    if (a.get("role"), a.get("name"), a.get("notes")) != (b.get("role"), b.get("name"), b.get("notes")):
        return False
    x, y = a.get("answers") or [], b.get("answers") or []
    return len(x) == len(y) and all(
        (p.get("id"), p.get("said"), p.get("chose")) == (q.get("id"), q.get("said"), q.get("chose"))
        for p, q in zip(x, y)
    )


def already_filed(out, rec):
    """The record on disk that is this same review, if there is one.

    Only keys sharing the content half are opened — the rest cannot be this review, whatever
    second they arrived in.
    """
    suffix = "-" + rec["id"].split("-", 1)[1]
    for path in sorted(out.glob("*" + suffix + ".json")):
        try:
            prev = json.loads(path.read_text("utf-8"))
        except (OSError, json.JSONDecodeError):
            continue  # a record this script cannot read is not one it may overwrite
        if says_the_same_as(prev, rec):
            return path, prev
    return None, None


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("file", help="JSON from the review form, or - for stdin")
    ap.add_argument("--root", default="state", help="disk store root (default: state)")
    ap.add_argument("--at", type=int, help="unix seconds to file it under (default: now)")
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    text = sys.stdin.read() if args.file == "-" else pathlib.Path(args.file).read_text("utf-8")
    try:
        raw = json.loads(text)
    except json.JSONDecodeError as e:
        # The usual cause is a chat app that wrapped or trimmed the paste, so say where it broke.
        sys.exit(f"not valid JSON at line {e.lineno} col {e.colno}: {e.msg}")

    try:
        rec = build(raw, args.at if args.at is not None else int(time.time()))
    except ValueError as e:
        sys.exit(str(e))

    out = pathlib.Path(args.root) / KIND
    seen, prev = already_filed(out, rec) if out.is_dir() else (None, None)
    if seen is not None:
        # Keep the key and the arrival time the record already has: the review came in when it
        # came in, and the id in the file has to go on matching the name of the file.
        rec["id"], rec["at"] = prev["id"], prev["at"]

    who = rec["name"] or ("ไม่ระบุชื่อ" if rec["anonymous"] else "(ไม่ลงชื่อ)")
    print(f"{rec['role']} · {who} · ตอบ {len(rec['answers'])} ข้อ"
          f"{' · มีบันทึกเพิ่ม' if rec['notes'] else ''}")
    print(f"  → {args.root}/{KIND}/{rec['id']}.json")
    if seen is not None:
        print("  (รีวิวเดียวกันนี้ยื่นมาแล้ว — เขียนทับฉบับเดิม ไม่เพิ่มระเบียนใหม่)")

    if args.dry_run:
        return

    out.mkdir(parents=True, exist_ok=True)
    path = out / f"{rec['id']}.json"
    # Write beside then rename: a half-written record is worse than a missing one.
    tmp = path.with_suffix(".json.tmp")
    tmp.write_text(json.dumps(rec, ensure_ascii=False, indent=2) + "\n", "utf-8")
    tmp.replace(path)


if __name__ == "__main__":
    main()
