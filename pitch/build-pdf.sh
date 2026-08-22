#!/usr/bin/env bash
# Render the pitch HTML to PDF. Headless Chrome, because it is the only renderer on this
# machine that honours the print stylesheets (@page size, break-after, CSS variables).
#
# The deck prints one slide per landscape page; the script prints as an A4 document.
# Both force the light palette in print — see the @media print block in each file.
set -euo pipefail

CHROME="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
[ -x "$CHROME" ] || { echo "Chrome not found at $CHROME — set CHROME=..." >&2; exit 1; }

cd "$(dirname "$0")"
here="$PWD"

for f in deck script; do
  # virtual-time-budget gives Google Fonts time to land; without it the PDF silently
  # falls back to Helvetica and the whole thing looks like a different project.
  "$CHROME" --headless=new --disable-gpu --no-sandbox \
    --run-all-compositor-stages-before-draw \
    --virtual-time-budget=12000 --no-pdf-header-footer \
    --print-to-pdf="$here/Vitals-$f.pdf" "file://$here/$f.html" 2>/dev/null
  pages=$(python3 - "$here/Vitals-$f.pdf" <<'PY'
import re,sys
print(len(re.findall(rb'/Type\s*/Page[^s]', open(sys.argv[1],'rb').read())))
PY
)
  echo "Vitals-$f.pdf — $pages pages"
done
