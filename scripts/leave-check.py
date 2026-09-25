#!/usr/bin/env python3
"""Order 2, proven in a browser: a stranger who has given an order and switches away is told, on
the way back, that she still has the patient and nothing counts until she hands over.

    scripts/leave-check.py <staging bedside url>        (staging only — it takes a shift)

What it does, as a person would: opens the bedside, presses "Treat her" (a real pointer press at
the button's own box, never the handler), types one order into the order line and presses Enter,
then takes the page through hidden → visible the way a phone's app switch does — Chrome's own
page lifecycle (`Page.setWebLifecycleState` frozen → active), which fires `visibilitychange` —
and reads the strip `#wardsay`. It prints what the strip said and by which route the change was
fired. If the lifecycle route did not change `document.visibilityState`, it says so and falls
back to a synthetic event, marked as such: a synthetic event proves the listener, not the browser.

It hands the patient back at the end so no staging bed is left half-taken, closes the browser
and checks nothing of its own is left holding a stream. Never run against production: a take
there is a real shift on the real chain.

What it has proven so far (25 Sep 2026, staging 00098, six runs): a browser leaving the bedside
makes no call to /api/ward/anchor, and a hand-over with nothing on the tape anchors nothing.
What it has not: the take itself — `#fv-treat` only shuts the first-visit cover, the take is the
strip's `#wardtake`, and in those runs that button was found but not under the pointer, so no
order was ever given and the reminder on return is proven only by the synthetic route. The
trigger a phone fires is unverified by this script; that is a phone in a hand for thirty seconds.
"""
import json, os, shutil, subprocess, sys, time, urllib.request

URL = sys.argv[1] if len(sys.argv) > 1 else ""
if "367117259093" not in URL or "/ward/" not in URL:
    sys.exit("usage: scripts/leave-check.py <staging bedside url> — staging only, a bedside (/ward/<id>)")
LABEL = "leave"
PORT = 9790 + (abs(hash(URL)) % 9)
PROFILE = os.path.join(os.environ.get("TMPDIR", "/tmp"), f"chrome-leave-check-{PORT}")
CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
ORDER = os.environ.get("LEAVE_CHECK_ORDER", "oxygen")

ch = subprocess.Popen([CHROME, "--headless=new", f"--remote-debugging-port={PORT}", f"--user-data-dir={PROFILE}",
                       "--remote-allow-origins=*", "--no-first-run", "--no-default-browser-check",
                       "--window-size=1280,900", "about:blank"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
report = {"url": URL, "read_at": time.strftime("%Y-%m-%d %H:%M:%S %Z")}
try:
    for _ in range(40):
        try:
            tabs = json.load(urllib.request.urlopen(f"http://127.0.0.1:{PORT}/json", timeout=2)); break
        except Exception:
            time.sleep(0.5)
    else:
        sys.exit("Chrome did not answer on its debugging port")
    import websocket  # page_target_note: /json lists browser_ui targets first; attach to type "page"
    ws = websocket.create_connection([x for x in tabs if x.get("type") == "page"][0]["webSocketDebuggerUrl"], timeout=90)
    seq = [0]
    def cdp(method, **params):
        seq[0] += 1
        ws.send(json.dumps({"id": seq[0], "method": method, "params": params}))
        while True:
            m = json.loads(ws.recv())
            if m.get("id") == seq[0]:
                return m.get("result", {})
    def js(expr):
        r = cdp("Runtime.evaluate", expression=expr, returnByValue=True, awaitPromise=True)
        if "exceptionDetails" in r:
            return {"page_script_failed": r["exceptionDetails"].get("exception", {}).get("description", "?")[:300]}
        return r.get("result", {}).get("value")
    def press(selector):
        # scrolled into view first — a pointer press lands where the pointer is, and a button
        # below the fold is not there — and the element under the point is checked to be the
        # button itself (or inside it), so a cover or an overlay on top is reported, not clicked
        box = js(f"""(()=>{{const e=document.querySelector({json.dumps(selector)}); if(!e) return null;
            e.scrollIntoView({{block:'center'}}); const r=e.getBoundingClientRect();
            const x=r.x+r.width/2, y=r.y+r.height/2; const at=document.elementFromPoint(x,y);
            return [x, y, r.width, r.height, !!(at && (at===e || e.contains(at))), e.disabled===true,
                    at ? (at.id || at.className || at.tagName) : null]}})()""")
        report.setdefault("presses", []).append({"selector": selector, "found": bool(box), "hit": bool(box and box[4]), "disabled": bool(box and box[5]), "under_pointer": box[6] if box else None})
        if not box or box[2] == 0 or not box[4] or box[5]:
            return False
        x, y = box[0], box[1]
        cdp("Input.dispatchMouseEvent", type="mouseMoved", x=x, y=y)
        cdp("Input.dispatchMouseEvent", type="mousePressed", x=x, y=y, button="left", clickCount=1)
        cdp("Input.dispatchMouseEvent", type="mouseReleased", x=x, y=y, button="left", clickCount=1)
        return True
    def strip():
        return js("(()=>{const s=document.querySelector('#wardsay'); return s? s.innerText.replace(/\\s+/g,' ').trim() : null})()")

    cdp("Page.enable")
    # a headless window has no focus, so nothing can be typed into it until focus is emulated
    cdp("Emulation.setFocusEmulationEnabled", enabled=True)
    w, h = [int(x) for x in os.environ.get("LEAVE_CHECK_WIN", "412,915").split(",")]
    cdp("Emulation.setDeviceMetricsOverride", width=w, height=h, deviceScaleFactor=1, mobile=(w < 600))
    # load until the bedside has drawn: a cold instance or a long pass can hold the first paint
    for attempt in range(2):
        cdp("Page.navigate", url=URL)
        for _ in range(20):
            time.sleep(1.5)
            drawn = js("document.readyState==='complete' && !!document.querySelector('#fv-treat, #wardsay')")
            if drawn:
                break
        if drawn:
            break
    report["page_drawn"] = bool(drawn)
    report["name"] = js("(()=>{const b=document.querySelector('.who b, h1, .name'); return b? b.innerText.trim():null})()")
    # the first-visit cover's "Treat her" only shuts the cover; the take is the strip's own button
    # (`#wardtake` → takeShift: open, take, declare — three chain steps), so both are pressed
    report["cover_treat_pressed"] = press("#fv-treat"); time.sleep(3)
    report["take_pressed"] = press("#wardtake"); time.sleep(8)
    report["strip_after_take"] = strip()
    # one order, typed as a person types it
    # the order line: `#cmd` on the bay, or whichever text field is actually on screen — and say
    # what was there, so a failure to type is a readable fact rather than a false "no order"
    report["fields_on_screen"] = js("[...document.querySelectorAll('input,textarea')].filter(e=>getComputedStyle(e).display!=='none').map(e=>(e.id||e.name||'?')+':'+(e.type||e.tagName.toLowerCase()))")
    # the line may be disabled while the take is still being confirmed on the chain: say so, wait
    # for it, then focus through the browser's own DOM agent rather than the page's focus()
    # a take is a chain instruction and the line opens when the chain has it: up to ninety seconds
    waited = 0
    for _ in range(30):
        state = js("(()=>{const c=document.querySelector('#cmd'); return c? {disabled:c.disabled, readonly:c.readOnly, display:getComputedStyle(c).display} : null})()")
        if state and not state.get("disabled") and not state.get("readonly"):
            break
        time.sleep(3); waited += 3
    report["order_line_state"] = state
    report["order_line_waited_s"] = waited
    report["strip_when_line_opened"] = strip()
    report["shift_state"] = js("(()=>({pending: typeof WARDPENDING!=='undefined' ? !!WARDPENDING : null, shift: typeof WARDSHIFT!=='undefined' && WARDSHIFT ? {name: WARDSHIFT.name, index: WARDSHIFT.index} : null, strip: (document.querySelector('#wardsay')||{}).innerText||null}))()")
    focused = False
    doc = cdp("DOM.getDocument", depth=1)
    node = cdp("DOM.querySelector", nodeId=doc["root"]["nodeId"], selector="#cmd").get("nodeId")
    if node:
        cdp("DOM.focus", nodeId=node)
        focused = js("document.activeElement && document.activeElement.id==='cmd'")
    report["order_line_focused"] = focused
    report["visibility_before_leaving"] = js("document.visibilityState")
    if focused:
        cdp("Input.insertText", text=ORDER)
        cdp("Input.dispatchKeyEvent", type="keyDown", key="Enter", code="Enter", windowsVirtualKeyCode=13, nativeVirtualKeyCode=13)
        cdp("Input.dispatchKeyEvent", type="keyUp", key="Enter", code="Enter", windowsVirtualKeyCode=13, nativeVirtualKeyCode=13)
        time.sleep(4)
    report["order_given"] = js("typeof DIDWORK!=='undefined' ? DIDWORK : null")
    report["strip_before_leaving"] = strip()
    # the app switch: Chrome's own lifecycle, hidden then visible
    cdp("Page.setWebLifecycleState", state="frozen"); time.sleep(1.5)
    hidden_seen = js("document.visibilityState")
    cdp("Page.setWebLifecycleState", state="active"); time.sleep(2.5)
    report["visibility_via_lifecycle"] = {"while_frozen": hidden_seen, "after": js("document.visibilityState")}
    report["strip_on_return"] = strip()
    report["route"] = "browser lifecycle (frozen → active)"
    if not (report["strip_on_return"] or "").lower().startswith("you still have"):
        # the browser did not change visibility for this page: prove the listener with a synthetic
        # event, and say so — this route proves the page, not the phone
        js("""(()=>{Object.defineProperty(document,'visibilityState',{configurable:true,get:()=>'hidden'});
               document.dispatchEvent(new Event('visibilitychange'));
               Object.defineProperty(document,'visibilityState',{configurable:true,get:()=>'visible'});
               document.dispatchEvent(new Event('visibilitychange')); return true})()""")
        time.sleep(1.5)
        report["strip_on_return_synthetic"] = strip()
        report["route"] = "synthetic visibilitychange — proves the listener, not the browser"
    report["anchor_calls_while_leaving"] = js("performance.getEntriesByType('resource').filter(e=>/\\/api\\/ward\\/anchor/.test(e.name)).length")
    # hand her back so the bed is not left half-taken
    report["handed_back"] = press("#endrun")  # the strip's own "hand over" button (bay.js:4803)
    time.sleep(5)
    report["strip_at_end"] = strip()
    print(json.dumps(report, ensure_ascii=False))
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
