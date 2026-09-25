#!/usr/bin/env bash
# Prove scripts/bytes-read.sh sums a paged integrity list correctly and stops honestly — without a
# ward, a project or a network. gcloud and curl are stubs.
#
#   scripts/bytes-read-test.sh
set -uo pipefail
cd "$(dirname "$0")/.."
TARGET="$PWD/scripts/bytes-read.sh"
mkdir -p target
WORK="$(mktemp -d "$PWD/target/bytes-read.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/bin"

cat > "$WORK/bin/gcloud" <<'STUB'
#!/usr/bin/env bash
case "$1 $2" in
  "secrets versions") [ "${STUB_NO_TOKEN:-0}" = 1 ] && exit 1; printf 'stub-door-token\n'; exit 0 ;;
esac
echo "stub gcloud: unhandled [$*]" >&2; exit 64
STUB

# Three pages: patients 1–3, then 4–6, then 7. The ward's totals are the sums; each page says its
# own counts only. STUB_FAIL_AT=<n> makes call n answer 503; STUB_ENDLESS=1 never completes.
cat > "$WORK/bin/curl" <<'STUB'
#!/usr/bin/env bash
url="${@: -1}"
n=$(( $(cat "$STUB_CALLS" 2>/dev/null || echo 0) + 1 )); echo "$n" > "$STUB_CALLS"
auth=0; for a in "$@"; do case "$a" in "authorization: Bearer stub-door-token") auth=1 ;; esac; done
[ "$auth" = 1 ] || { printf '{"error":"door"}\n401'; exit 0; }
echo "$url" >> "$STUB_URLS"
[ "${STUB_FAIL_AT:-0}" = "$n" ] && { printf 'ward busy\n503'; exit 0; }
after="${url##*after=}"; [ "$after" = "$url" ] && after=""
if [ "${STUB_ENDLESS:-0}" = 1 ]; then
  printf '{"complete":false,"next_after":%d,"read_from":%s,"shifts":1,"proved":1,"proved_as_it_stands":0,"ambiguous":0,"unrebuildable":0,"no_tape":0,"not_asked":0,"disagreements":0}\n200' "$n" "${after:-null}"; exit 0
fi
case "$after" in
  "")  printf '{"complete":false,"next_after":3,"read_from":null,"shifts":10,"proved":6,"proved_as_it_stands":1,"ambiguous":0,"unrebuildable":2,"no_tape":1,"not_asked":0,"disagreements":0}\n200' ;;
  3)   printf '{"complete":false,"next_after":6,"read_from":3,"shifts":8,"proved":3,"proved_as_it_stands":0,"ambiguous":1,"unrebuildable":1,"no_tape":3,"not_asked":0,"disagreements":1}\n200' ;;
  6)   printf '{"complete":true,"next_after":null,"read_from":6,"shifts":4,"proved":2,"proved_as_it_stands":0,"ambiguous":0,"unrebuildable":0,"no_tape":1,"not_asked":1,"disagreements":0}\n200' ;;
  *)   printf '{"error":"no such page"}\n400' ;;
esac
STUB
chmod +x "$WORK/bin/gcloud" "$WORK/bin/curl"

PASS=0; FAIL=0
ok()  { printf '  \033[32mpass\033[0m  %s\n' "$1"; PASS=$((PASS+1)); }
bad() { printf '  \033[31mFAIL\033[0m  %s\n' "$1"; FAIL=$((FAIL+1)); }
run() { # run <env...> -- <target>
  local envs=(); while [ "$1" != "--" ]; do envs+=("$1"); shift; done; shift
  rm -f "$WORK/calls" "$WORK/urls"
  env -i PATH="$WORK/bin:/usr/bin:/bin" HOME="$WORK" TMPDIR="$WORK" STUB_CALLS="$WORK/calls" STUB_URLS="$WORK/urls" "${envs[@]}" bash "$TARGET" "$@" 2>&1
}

echo "── bytes-read ──"
# 1. three pages are followed by next_after and summed; the totals are the ward's, and exit 0.
out="$(run -- staging)"; rc=$?
printf '%s' "$out" | grep -q 'shifts 22 · proved 11 · proved as it stands 1 · ambiguous 1 · unrebuildable 3 · no tape 5 · not asked 1 · disagreements 1' \
  && [ "$rc" -eq 0 ] && [ "$(cat "$WORK/calls")" = 3 ] && grep -q 'after=3$' "$WORK/urls" && grep -q 'after=6$' "$WORK/urls" \
  && printf '%s' "$out" | grep -q 'over 3 call(s)' && printf '%s' "$out" | grep -q 'no tape as a share of every shift on the list: 5/22 = 23 percent' \
  && ok "three pages followed by next_after, summed to the ward's totals, exit 0" || bad "paged sum wrong (rc=$rc, calls=$(cat "$WORK/calls" 2>/dev/null)): $out"

# 2. every call carries the door token; the token is never printed.
printf '%s' "$out" | grep -q 'stub-door-token' && bad "the token was printed" || ok "the token reaches the door and never the output"

# 3. a page that fails stops the read and says the totals are partial; exit non-zero.
out="$(run STUB_FAIL_AT=2 -- staging)"; rc=$?
printf '%s' "$out" | grep -q 'answered 503' && printf '%s' "$out" | grep -q 'partial' && [ "$rc" -ne 0 ] && printf '%s' "$out" | grep -q 'shifts 10 ·' \
  && ok "a failed page stops the read, the totals are called partial, exit $rc" || bad "a failed page was not handled (rc=$rc): $out"

# 4. a ward that never says complete is cut off at BYTES_MAX_CALLS; exit non-zero.
out="$(run STUB_ENDLESS=1 BYTES_MAX_CALLS=4 -- staging)"; rc=$?
[ "$(cat "$WORK/calls")" = 4 ] && printf '%s' "$out" | grep -q 'stopping: 4 calls' && [ "$rc" -ne 0 ] \
  && ok "an endless ward is cut off at the call cap (4 calls), exit $rc" || bad "the call cap did not hold (calls=$(cat "$WORK/calls" 2>/dev/null), rc=$rc): $out"

# 5. no token, nothing asked.
out="$(run STUB_NO_TOKEN=1 -- production)"; rc=$?
[ "$rc" -eq 1 ] && printf '%s' "$out" | grep -q 'token could not be read' && [ ! -e "$WORK/calls" ] \
  && ok "without the token nothing is asked (exit 1)" || bad "asked without a token (rc=$rc, calls=$(cat "$WORK/calls" 2>/dev/null)): $out"

# 6. the target is named or refused.
out="$(run -- prod)"; rc=$?
[ "$rc" -eq 1 ] && printf '%s' "$out" | grep -q "refusing: the target" && ok "an unknown target is refused" || bad "unknown target accepted (rc=$rc): $out"

# 7. production pins the ops configuration for the process — the stub sees it in its environment.
cat > "$WORK/bin/gcloud" <<'STUB'
#!/usr/bin/env bash
[ "${CLOUDSDK_ACTIVE_CONFIG_NAME:-}" = vitals-ops ] || { echo "config was ${CLOUDSDK_ACTIVE_CONFIG_NAME:-unset}" >&2; exit 1; }
printf 'stub-door-token\n'
STUB
out="$(run -- production)"; rc=$?
[ "$rc" -eq 0 ] && ok "production runs under configuration vitals-ops, pinned per process" || bad "production did not pin vitals-ops (rc=$rc): $out"

echo; printf '%d passed, %d failed\n' "$PASS" "$FAIL"; [ "$FAIL" -eq 0 ]
