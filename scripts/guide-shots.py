#!/usr/bin/env python3
"""Retake the guide's pictures (crates/vitals-web/static/world/start/img) on staging, in one shift.

    scripts/guide-shots.py <staging bedside url> <out dir> [WxH]

Staging only: it takes the shift, gives one order, hands over and opens the receipt — a real shift
on staging's chain, which is what staging is for. Every press is a pointer press at the button's
own box after scrollIntoView, with the element under the pointer checked; the browser attaches to
the page target and terminates at the end.

  02-bedside-before   the bedside with the first-visit cover shut ("Just look first"), one green button
  03-after-take       after the strip's Treat: the order line open, the clock running
  04-first-order      "oxygen" typed and sent
  05-treating         about half a minute later — the monitor after the order
  06-handing-over     the strip while the hand-over anchors
  07-handed-over      handed over: the chain one shift longer
  08-receipt          the receipt page
The globe (01) is taken from production by GET with scripts/board-look.py's screenshot, not here.
"""
import base64, json, os, shutil, subprocess, sys, time, urllib.request

URL = sys.argv[1] if len(sys.argv) > 1 else ""
OUT = sys.argv[2] if len(sys.argv) > 2 else ""
W, H = [int(x) for x in (sys.argv[3] if len(sys.argv) > 3 else "1600x1000").split("x")]
if "367117259093" not in URL or "/ward/" not in URL or not OUT:
    sys.exit("usage: scripts/guide-shots.py <staging bedside url> <out dir> [WxH] — staging only")
os.makedirs(OUT, exist_ok=True)
PORT = 9840; PROFILE = os.path.join(os.environ.get("TMPDIR", "/tmp"), "chrome-guide-shots")
shutil.rmtree(PROFILE, ignore_errors=True)
CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
ch = subprocess.Popen([CHROME, "--headless=new", f"--remote-debugging-port={PORT}", f"--user-data-dir={PROFILE}",
                       "--remote-allow-origins=*", "--no-first-run", "--no-default-browser-check", "about:blank"],
                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
log = {"url": URL, "at": time.strftime("%Y-%m-%d %H:%M:%S %Z"), "shots": {}}
try:
    for _ in range(40):
        try: tabs = json.load(urllib.request.urlopen(f"http://127.0.0.1:{PORT}/json", timeout=2)); break
        except Exception: time.sleep(0.5)
    import websocket  # page target, never the first listed (that is the omnibox popup)
    ws = websocket.create_connection([x for x in tabs if x.get("type") == "page"][0]["webSocketDebuggerUrl"], timeout=120); seq = [0]
    def cdp(m, **p):
        seq[0] += 1; ws.send(json.dumps({"id": seq[0], "method": m, "params": p}))
        while True:
            r = json.loads(ws.recv())
            if r.get("id") == seq[0]: return r.get("result", {})
    def js(e):
        r = cdp("Runtime.evaluate", expression=e, returnByValue=True)
        return r.get("result", {}).get("value") if "exceptionDetails" not in r else None
    def press(sel):
        box = js(f"""(()=>{{const e=document.querySelector({json.dumps(sel)}); if(!e) return null; e.scrollIntoView({{block:'center'}});
            const r=e.getBoundingClientRect(); const x=r.x+r.width/2, y=r.y+r.height/2; const at=document.elementFromPoint(x,y);
            return [x,y,!!(at&&(at===e||e.contains(at))), e.disabled===true]}})()""")
        if not box or not box[2] or box[3]: return False
        for t in ("mouseMoved", "mousePressed", "mouseReleased"):
            cdp("Input.dispatchMouseEvent", type=t, x=box[0], y=box[1], button="left", clickCount=1)
        return True
    def shot(name, focus=None):
        # The page is captured as the viewport shows it; `focus` scrolls one element to the middle
        # first, so a picture of the monitor is a picture of the monitor and not of the header.
        if focus: js(f"(()=>{{const e=document.querySelector({json.dumps(focus)}); if(e) e.scrollIntoView({{block:'center'}}); return !!e}})()")
        else: js("window.scrollTo(0,0)")
        time.sleep(0.6)
        png = cdp("Page.captureScreenshot", format="png")["data"]
        path = os.path.join(OUT, name + ".png"); open(path, "wb").write(base64.b64decode(png))
        log["shots"][name] = {"strip": js("(document.querySelector('#wardsay')||{}).innerText||null"), "file": path}
    def strip(): return js("(document.querySelector('#wardsay')||{}).innerText||null")
    cdp("Page.enable"); cdp("Emulation.setFocusEmulationEnabled", enabled=True)
    cdp("Emulation.setDeviceMetricsOverride", width=W, height=H, deviceScaleFactor=1, mobile=False)
    cdp("Page.navigate", url=URL)
    for _ in range(20):
        time.sleep(1.5)
        if js("document.readyState==='complete' && !!document.querySelector('#wardsay')"): break
    time.sleep(2)
    press("#fv-look"); time.sleep(1.5)
    shot("02-bedside-before")
    log["take_pressed"] = press("#wardtake")
    for _ in range(30):
        time.sleep(3)
        if js("(()=>{const c=document.querySelector('#cmd'); return !!c && !c.disabled})()"): break
    time.sleep(2); shot("03-after-take")
    # "oxygen" is an order, and the box interprets what is typed by the mode that is on: in the
    # default "ask" mode it becomes a question ("ASKED oxygen" on the receipt). The guide's second
    # step is an order, so the drugs mode is pressed first, the way the picture should show it.
    log["drugs_mode_pressed"] = press('#modes button[data-m="drugs"]'); time.sleep(0.8)
    log["mode_on"] = js("(()=>{const b=document.querySelector('#modes button.on'); return b? b.dataset.m : null})()")
    doc = cdp("DOM.getDocument", depth=1); node = cdp("DOM.querySelector", nodeId=doc["root"]["nodeId"], selector="#cmd").get("nodeId")
    log["cmd_before"] = js("(()=>{const c=document.querySelector('#cmd'); return c? {disabled:c.disabled, value:c.value, focused:document.activeElement===c} : null})()")
    if node:
        cdp("DOM.focus", nodeId=node); cdp("Input.insertText", text="oxygen")
        log["cmd_after_type"] = js("(()=>{const c=document.querySelector('#cmd'); return c? {value:c.value, focused:document.activeElement===c} : null})()")
        cdp("Input.dispatchKeyEvent", type="keyDown", key="Enter", code="Enter", windowsVirtualKeyCode=13, nativeVirtualKeyCode=13, text="\r", unmodifiedText="\r")
        cdp("Input.dispatchKeyEvent", type="keyUp", key="Enter", code="Enter", windowsVirtualKeyCode=13, nativeVirtualKeyCode=13)
    time.sleep(4); log["didwork_after_enter"] = js("typeof DIDWORK!=='undefined' ? DIDWORK : null"); log["cmd_after_enter"] = js("(document.querySelector('#cmd')||{}).value"); shot("04-first-order")
    time.sleep(30); shot("05-treating", focus="#m-spo2")
    log["monitor_after"] = js("(()=>{const q=s=>(document.querySelector(s)||{}).innerText; return {hr:q('#m-hr'), spo2:q('#m-spo2'), bp:q('#m-bp'), rr:q('#m-rr')}})()")
    log["endrun_before"] = js("(()=>{const b=document.querySelector('#endrun'); return b? {disabled:b.disabled, text:b.innerText, display:getComputedStyle(b).display} : null})()")
    # The guide says it: hand over is pressed twice — the first press arms the button (it reads
    # "press again to record" for six seconds), the second sends the shift to the chain.
    log["handover_armed"] = press("#endrun"); time.sleep(1.2)
    log["endrun_armed_text"] = js("(document.querySelector('#endrun')||{}).innerText")
    shot("06-handing-over", focus="#endrun")
    log["handover_sent"] = press("#endrun")
    handed = None
    for _ in range(30):
        time.sleep(2)
        s = strip() or ""
        if "handed over" in s.lower(): handed = s; break
    log["strip_after_handover"] = handed or strip()
    time.sleep(2); shot("07-handed-over")
    href = js("(()=>{const a=[...document.querySelectorAll('a')].find(a=>/receipt|\\/shift\\//i.test(a.href+' '+a.innerText)); return a? a.href : null})()")
    log["receipt_href"] = href
    if href:
        cdp("Page.navigate", url=href); time.sleep(12); shot("08-receipt")
    print(json.dumps(log, ensure_ascii=False)); ws.close()
finally:
    ch.terminate()
    try: ch.wait(timeout=10)
    except Exception: ch.kill()
    subprocess.run(["pkill", "-9", "-f", f"user-data-dir={PROFILE}"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    shutil.rmtree(PROFILE, ignore_errors=True)
    left = subprocess.run(["pgrep", "-f", f"user-data-dir={PROFILE}"], capture_output=True, text=True).stdout.split()
    print(f"browser closed; {len(left)} process(es) left", file=sys.stderr)
