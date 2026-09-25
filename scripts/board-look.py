#!/usr/bin/env python3
"""What the ward's front page actually renders — read from a real browser, printed, closed.

    scripts/board-look.py <staging|production> [label]

Why. On 25 Sep 2026 three defects in one night were invisible to every unit test by construction
— two functions disagreeing about a patient's ending, a sentence the page asserted with no
source, and stored clocks outliving the code that wrote them — and each was obvious within
seconds of looking at a rendered card. So a deploy note carries what the page showed, before
and after the first pass on the new revision, the way it carries the image digest: the `.clock`
elements as rendered, the cap sentence if any, and the first beds in the order the page put them.

It opens one headless Chrome on its own profile, reads, and terminates it — then checks that
nothing of its own is left holding an /api/ward/stream slot, because an open tab is one of the
instance's concurrent requests for as long as it lives (six were found on 23 Sep, one five days
old). A read of production is a GET and one slot for about ten seconds; never leave it running.
"""
import base64, json, os, shutil, subprocess, sys, time, urllib.request

TARGET = sys.argv[1] if len(sys.argv) > 1 else ""
LABEL = sys.argv[2] if len(sys.argv) > 2 else "look"
HOSTS = {
    "staging": "https://vitals-world-367117259093.asia-southeast1.run.app",
    "production": "https://world.vitals.academy",
}
if TARGET not in HOSTS:
    sys.exit("usage: scripts/board-look.py <staging|production> [label]")
URL = os.environ.get("VITALS_WARD_URL", HOSTS[TARGET]) + "/"
PORT = 9760 + (abs(hash(LABEL)) % 30)
PROFILE = os.path.join(os.environ.get("TMPDIR", "/tmp"), f"chrome-board-look-{LABEL}")
CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
if not os.path.exists(CHROME):
    sys.exit("no Chrome at the usual path; nothing opened")

ch = subprocess.Popen([CHROME, "--headless=new", f"--remote-debugging-port={PORT}", f"--user-data-dir={PROFILE}",
                       "--remote-allow-origins=*", "--no-first-run", "--no-default-browser-check", "--window-size=1400,1000", "about:blank"],
                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
try:
    for _ in range(40):
        try:
            tabs = json.load(urllib.request.urlopen(f"http://127.0.0.1:{PORT}/json", timeout=2)); break
        except Exception:
            time.sleep(0.5)
    else:
        sys.exit("Chrome did not answer on its debugging port")
    import websocket  # page_target_note: /json lists browser_ui targets first; attach to type "page"  # pip: websocket-client — the same dependency the face checks use
    ws = websocket.create_connection([x for x in tabs if x.get("type") == "page"][0]["webSocketDebuggerUrl"], timeout=60)
    seq = [0]
    def cdp(method, **params):
        seq[0] += 1
        ws.send(json.dumps({"id": seq[0], "method": method, "params": params}))
        while True:
            m = json.loads(ws.recv())
            if m.get("id") == seq[0]:
                return m.get("result", {})
    def js(expr):
        r = cdp("Runtime.evaluate", expression=expr, returnByValue=True)
        if "exceptionDetails" in r:
            return {"page_script_failed": r["exceptionDetails"].get("exception", {}).get("description", "?")[:300]}
        return r.get("result", {}).get("value")
    cdp("Page.enable"); cdp("Page.navigate", url=URL); time.sleep(9)
    seen = js("""(() => {
      const t = e => String(e.innerText ?? e.textContent ?? '').replace(/\\s+/g, ' ').trim();
      const clocks = [...document.querySelectorAll('.clock')].map(t);
      // A bed is an <li> whose first line is `.who` — the name in <b>, then the bed — in the order
      // the page put them, which after 25 Sep 2026 is the ward's own clock.
      const beds = [...document.querySelectorAll('li .who')].slice(0, 8)
        .map(e => t(e.querySelector('b') || e).slice(0, 40));
      // Rendered text only: innerText of what is on screen, never a script's or a style's source.
      const cap = [...document.querySelectorAll('body *')]
        .filter(e => e.childElementCount === 0 && !/^(SCRIPT|STYLE|TEMPLATE|NOSCRIPT)$/.test(e.tagName))
        .map(e => String(e.innerText || '').replace(/\\s+/g, ' ').trim())
        .find(s => /five minutes in/i.test(s)) || null;
      return { clocks, beds, cap_sentence_shown: cap };
    })()""")
    rev = None
    try:
        rev = json.load(urllib.request.urlopen(URL + "api/ward", timeout=90)).get("revision")
    except Exception:
        pass
    print(json.dumps({"target": TARGET, "revision_answering": rev, "read_at": time.strftime("%Y-%m-%d %H:%M:%S %Z"), **(seen or {})}, ensure_ascii=False))
    ws.close()
finally:
    ch.terminate()
    try:
        ch.wait(timeout=10)
    except Exception:
        ch.kill()
    subprocess.run(["pkill", "-9", "-f", f"user-data-dir={PROFILE}"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    shutil.rmtree(PROFILE, ignore_errors=True)
    left = subprocess.run(["pgrep", "-f", f"user-data-dir={PROFILE}"], capture_output=True, text=True).stdout.split()
    print(f"browser closed; {len(left)} process(es) of this read left", file=sys.stderr)
    if left:
        sys.exit(2)
