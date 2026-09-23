#!/usr/bin/env bash
# Prove that scripts/push-case.sh asks the board before it sends, and refuses when the answer is no.
#
# On 23 Sep 2026 a pack went through production's door over a case the chain carried a shift
# against, and that shift stopped verifying. The check run beforehand asked the wrong question.
# This harness makes the board say "a shift is anchored on this case" and asserts nothing is
# sent; makes the catalogue say "same version" and asserts nothing is sent; and lets a clean
# pack through and asserts the door's answer is printed.
#
# gcloud and curl are stubs; nothing here touches a network, a project or a ward.
#
#   scripts/push-case-test.sh
set -uo pipefail
cd "$(dirname "$0")/.."

TARGET="$PWD/scripts/push-case.sh"
# Under the repo rather than the system temp dir: a sandboxed shell on the mini hung every stub
# named curl that lived under /var/folders, and the same files ran fine from here.
mkdir -p target
WORK="$(mktemp -d "$PWD/target/push-case.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/bin"

cat > "$WORK/bin/gcloud" <<'STUB'
#!/usr/bin/env bash
set -u
case "${CLOUDSDK_ACTIVE_CONFIG_NAME:-}" in
  vitals-ops)     ACCT="${STUB_ACCOUNT_PROD-vitals-ops@vitals-academy.iam.gserviceaccount.com}" ;;
  vitals-ops-dev) ACCT="${STUB_ACCOUNT_DEV-vitals-ops@vitals-academy-dev.iam.gserviceaccount.com}" ;;
  *)              ACCT="someone@example.com" ;;
esac
case "$1 $2" in
  "config get-value") printf '%s\n' "$ACCT"; exit 0 ;;
  "auth print-access-token") echo ya29.stub; exit 0 ;;
  "secrets versions") printf 'stub-door-token\n'; exit 0 ;;
esac
echo "stub gcloud: unhandled [$*]" >&2
exit 64
STUB

# The board carries one patient on ddx-example-1-en; whether a shift is on chain against him is
# the case's to say. The catalogue holds the case at the version the case says.
cat > "$WORK/bin/curl" <<'STUB'
#!/usr/bin/env bash
url="${@: -1}"
posted=0
for a in "$@"; do [ "$a" = "-X" ] && posted=1; done
case "$url" in
  */api/ward/cases)
    printf '{"cases":[{"case_id":"ddx-example-1-en","version":"%s","provisional":true}],"status":"stub"}\n' "${STUB_CATALOGUE_VERSION-0.1.1}"; exit 0 ;;
  */api/ward/case)
    printf '%s\n' "$STUB_POSTED_TO" >> "${STUB_SENT:-/dev/null}"
    printf '{"stored":"ddx-example-1-en","provisional":true,"version":"0.1.2","status":"stub"}\n200'; exit 0 ;;
  */api/ward)
    [ "${STUB_WARD_DOWN:-0}" = 1 ] && exit 7
    if [ "${STUB_ON_SHIFT:-0}" = 1 ]; then
      printf '{"patients":[{"patient_id":1790146977,"name":"Hodan Warsame","case":"ddx-example-1-en","state":"on_ward","closed_slot":null,"shifts":0,"on_shift_since":"2026-09-23T08:40:00Z"}]}\n'
    elif [ "${STUB_SHIFT_ON_CHAIN:-0}" = 1 ]; then
      printf '{"patients":[{"patient_id":1790037060,"name":"Yahya Al-Shami","case":"ddx-example-1-en","state":"died","closed_slot":502381820,"shifts":1}]}\n'
    else
      printf '{"patients":[{"patient_id":1790146977,"name":"Hodan Warsame","case":"ddx-example-1-en","state":"on_ward","closed_slot":null,"shifts":0}]}\n'
    fi
    exit 0 ;;
esac
echo "stub curl: unhandled $url" >&2; exit 64
STUB
chmod +x "$WORK/bin/gcloud" "$WORK/bin/curl"

PASS=0; FAIL=0; CASE_N=0

# run <name> <expectation> <needle> <target> <pack version> -- <env assignments...>
run() {
  local name="$1" expect="$2" needle="$3" target="$4" version="$5"; shift 5
  [ "${1-}" = "--" ] && shift
  CASE_N=$((CASE_N + 1))
  local sandbox="$WORK/case$CASE_N"; mkdir -p "$sandbox"
  # A compiler stamp when the case names one (STUB_STAMP=<commit>); none otherwise, like every
  # pack on production before 23 Sep 2026.
  local stamp=""
  for a in "$@"; do case "$a" in STUB_STAMP=*) stamp="${a#*=}" ;; esac; done
  if [ -n "$stamp" ]; then
    printf '{"case_id":"ddx-example-1-en","version":"%s","compiler":{"name":"vitals-casefactory","version":"0.9.4","commit":"%s"},"sce":{}}\n' "$version" "$stamp" > "$sandbox/pack.json"
  else
    printf '{"case_id":"ddx-example-1-en","version":"%s","sce":{}}\n' "$version" > "$sandbox/pack.json"
  fi
  local out rc
  out="$(env -i PATH="$WORK/bin:/usr/bin:/bin" HOME="$sandbox" TMPDIR="$sandbox" STUB_SENT="$sandbox/sent" STUB_POSTED_TO=door \
      "$@" bash "$TARGET" "$target" "$sandbox/pack.json" 2>&1 </dev/null)"
  rc=$?
  local sent=0; [ -f "$sandbox/sent" ] && sent="$(wc -l < "$sandbox/sent" | tr -d ' ')"
  local verdict=ok
  case "$expect" in
    refuses) [ "$rc" -ne 0 ] || verdict="exited 0 when it should have refused"
             [ "$sent" -eq 0 ] || verdict="sent $sent pack(s) through the door when it should have refused" ;;
    sends)   [ "$rc" -eq 0 ] || verdict="exited $rc when it should have sent"
             [ "$sent" -eq 1 ] || verdict="sent $sent pack(s), expected exactly 1" ;;
  esac
  if [ "$verdict" = ok ] && ! printf '%s' "$out" | grep -qF -- "$needle"; then
    verdict="never said \"$needle\""
  fi
  if [ "$verdict" = ok ]; then
    printf '  \033[32mpass\033[0m  %s \033[2m(exit %d, sent %d)\033[0m\n' "$name" "$rc" "$sent"; PASS=$((PASS + 1))
  else
    printf '  \033[31mFAIL\033[0m  %s — %s\n' "$name" "$verdict"
    printf '%s\n' "$out" | sed 's/^/        | /'; FAIL=$((FAIL + 1))
  fi
}

echo "── push-case refuses when ──"
run "the target is not staging or production" refuses "refusing" demo 0.1.2
run "a person is the active account" refuses "a person" production 0.1.2 -- STUB_ACCOUNT_PROD=someone@example.com
run "the ward cannot be read — the question cannot be asked" refuses "cannot be asked" production 0.1.2 -- STUB_WARD_DOWN=1
run "the chain carries a shift against the case (a ward closure counts)" refuses "the chain carries 1 shift(s) against ddx-example-1-en (1790037060 Yahya Al-Shami)" production 0.1.2 -- STUB_SHIFT_ON_CHAIN=1
run "the pack's version equals the one the door holds and it carries no compiler stamp" refuses "already holds ddx-example-1-en at v0.1.1" production 0.1.1
run "the same version with a -dirty stamp — two different packs could carry it" refuses "already holds ddx-example-1-en at v0.1.1" production 0.1.1 -- STUB_STAMP=0c7c5403135d-dirty
run "the same version with an unknown stamp" refuses "already holds ddx-example-1-en at v0.1.1" production 0.1.1 -- STUB_STAMP=unknown
run "a shift on chain refuses even a bumped version" refuses "the chain carries" staging 0.1.2 -- STUB_SHIFT_ON_CHAIN=1
run "a shift is in progress on the case — it will anchor" refuses "the chain carries 1 shift(s) against ddx-example-1-en (1790146977 Hodan Warsame)" production 0.1.2 -- STUB_ON_SHIFT=1

echo "── push-case sends when ──"
run "no shift is on chain against the case and the version is bumped" sends "stored: ddx-example-1-en v0.1.2 provisional" production 0.1.2
run "the case is new to the door (no version held)" sends "stored: ddx-example-1-en" staging 0.1.1 -- STUB_CATALOGUE_VERSION=
run "the pack's version differs from the held one" sends "1 sent, 0 refused" staging 0.1.3
# The door decides on the bytes and explains with (meta.version, compiler.commit): a recompile
# by a newer compiler under the author's unchanged version is a move, and the door is the one
# that holds the other side's stamp — the script sends and lets it answer.
run "the same version with a clean compiler stamp is sent — the door holds the other stamp and decides" sends "stored: ddx-example-1-en" production 0.1.1 -- STUB_STAMP=ccd73727b039

echo
printf '%d passed, %d failed\n' "$PASS" "$FAIL"
[ "$FAIL" -eq 0 ]
