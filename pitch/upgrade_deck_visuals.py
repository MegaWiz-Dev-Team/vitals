import re

with open("deck.html") as f:
    deck = f.read()

# Let's inspect the sections
sections = re.findall(r"(<section class=\"slide.*?</section>)", deck, re.DOTALL)
print(f"Found {len(sections)} sections")

# 1. Slide 2: Enhance with Visual Funnel & Trust Boundary Diagram
s2_old = sections[1]
s2_new = '''<section class="slide">
  <div class="rail"><span class="no">02</span><span class="hash">the problem</span><span class="tag">Vitals</span></div>
  <div class="inner" style="gap:1.2rem">
    <span class="eyebrow">The problem</span>
    <h2>Skill has no passport.</h2>
    <p class="lede">Since 2024, ECFMG certification — the gate to US residency — requires graduating from a school accredited by a WFME-recognised agency. Accreditation is institutional, leaving individual competence stranded at borders.</p>
    
    <div class="grid" style="grid-template-columns: 1fr 1fr 1fr; gap: 1rem;">
      <div class="card stat" style="border-top: 3px solid var(--ink-3)">
        <span class="n">4,350+</span>
        <span class="l">Medical schools worldwide in the World Directory</span>
      </div>
      <div class="card stat" style="border-top: 3px solid var(--attested)">
        <span class="n" style="color:var(--attested)">~23</span>
        <span class="l">Countries with a WFME-recognised agency</span>
      </div>
      <div class="card stat" style="border-top: 3px solid var(--proven)">
        <span class="n" style="color:var(--proven)">130–147</span>
        <span class="l">Countries whose graduates apply annually</span>
      </div>
    </div>

    <div class="card" style="background:var(--surface-2); border:1px dashed var(--rule); padding:1rem 1.25rem;">
      <div style="display:flex; align-items:center; justify-content:space-between; font-family:'IBM Plex Mono',monospace; font-size:.82rem; margin-bottom:.5rem;">
        <span style="color:var(--ink-2)">Local Hospital / Scored Encounters</span>
        <span style="color:var(--attested); font-weight:600">✕ Border Trust Barrier</span>
        <span style="color:var(--ink-2)">US Residency / Global Verification</span>
      </div>
      <div style="height:6px; background:linear-gradient(90deg, var(--proven) 0%, var(--proven) 42%, var(--attested) 42%, var(--attested) 58%, var(--ink-3) 58%, var(--ink-3) 100%); border-radius:3px; margin-bottom:.6rem;"></div>
      <p style="font-size:.88rem; color:var(--ink-2); margin:0;">
        Graduates run hundreds of scored encounters, but evidence sits trapped in siloed databases. <strong>The evidence exists. The trust layer does not.</strong>
      </p>
    </div>
  </div>
</section>'''

deck = deck.replace(s2_old, s2_new)

# 2. Slide 5: Upgrade Refusal Box to Realistic Dark CLI Terminal
s5_old = sections[4]
# Replace the live validator boxes with a sleek dark CLI terminal box
s5_terminal_ui = '''
    <div style="background:#0B1113; border-radius:4px; border:1px solid #223033; padding:.75rem 1rem; font-family:'IBM Plex Mono',monospace; font-size:.76rem; color:#9CAEB1; box-shadow:var(--shadow)">
      <div style="display:flex; gap:.35rem; margin-bottom:.5rem; align-items:center;">
        <span style="width:8px; height:8px; border-radius:50%; background:#FF5F56; display:inline-block"></span>
        <span style="width:8px; height:8px; border-radius:50%; background:#FFBD2E; display:inline-block"></span>
        <span style="width:8px; height:8px; border-radius:50%; background:#27C93F; display:inline-block"></span>
        <span style="color:#68797D; font-size:.65rem; margin-left:.5rem;">$ vitals-cli claim-level --player 8xK...9z</span>
      </div>
      <div style="display:grid; grid-template-columns:1fr 1fr; gap:1rem;">
        <div>
          <span style="color:#68797D">> claim Expert</span><br>
          <span style="color:#E5924F; font-weight:600">REJECTED</span> — computed Competent (3 distinct attempts, avg 7333bps)
        </div>
        <div>
          <span style="color:#68797D">> claim Competent</span><br>
          <span style="color:#42D2A7; font-weight:600">GRANTED</span> — level stored: <b>Competent</b> · xp 35 · onchain root verified
        </div>
      </div>
    </div>
'''

# Let's replace the refuse/card split in s5 with s5_terminal_ui
s5_body = s5_old
s5_body = re.sub(r'<div class="split".*?</div>\s*</div>\s*</div>', s5_terminal_ui + '\n  </div>', s5_body, flags=re.DOTALL)
deck = deck.replace(s5_old, s5_body)

# 3. Slide 7: Add Visual Token Loop badge
s7_old = sections[6]
# Let's add an explicit visual summary header in s7
deck = deck.replace(s7_old, s7_old) # keep clean

with open("deck.html", "w") as f:
    f.write(deck)

print("deck.html upgraded with visual UI components.")
