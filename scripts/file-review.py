#!/usr/bin/env python3
"""File a reviewer's answers that came back by hand.

The review form can post itself to the server, and it can also hand the reviewer their answers
as JSON to send back over LINE or email — which is what happens while `/api/review` does not
exist yet, and what will keep happening whenever a reviewer is somewhere the server is not.

Both paths must land in the same place under the same key, or the same review filed twice by two
routes becomes two records. The key here is derived exactly as `review.rs` derives it: seconds,
then six bytes of a SHA-256 over the content. Re-filing the same answers overwrites one record
rather than adding a second.

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
ANSWER_LIMITS = {"id": 64, "asked": 400, "said": 4000}
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
        if not said.strip():
            continue  # an unanswered question is not an answer
        answers.append({
            "id": clamp(a.get("id"), ANSWER_LIMITS["id"]),
            "asked": clamp(a.get("asked"), ANSWER_LIMITS["asked"]),
            "said": said,
        })

    notes = clamp(raw.get("notes"), LIMITS["notes"])
    if not answers and not notes.strip():
        raise ValueError("empty: nothing was answered")

    name = clamp(raw.get("name"), LIMITS["name"])
    joined = "\x1e".join(f"{a['id']}={a['said']}" for a in answers)
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

    who = rec["name"] or ("ไม่ระบุชื่อ" if rec["anonymous"] else "(ไม่ลงชื่อ)")
    print(f"{rec['role']} · {who} · ตอบ {len(rec['answers'])} ข้อ"
          f"{' · มีบันทึกเพิ่ม' if rec['notes'] else ''}")
    print(f"  → {args.root}/{KIND}/{rec['id']}.json")

    if args.dry_run:
        return

    out = pathlib.Path(args.root) / KIND
    out.mkdir(parents=True, exist_ok=True)
    path = out / f"{rec['id']}.json"
    if path.exists():
        print("  (มีอยู่แล้ว — เนื้อหาเดียวกัน เขียนทับเป็นฉบับเดียว)")
    # Write beside then rename: a half-written record is worse than a missing one.
    tmp = path.with_suffix(".json.tmp")
    tmp.write_text(json.dumps(rec, ensure_ascii=False, indent=2) + "\n", "utf-8")
    tmp.replace(path)


if __name__ == "__main__":
    main()
