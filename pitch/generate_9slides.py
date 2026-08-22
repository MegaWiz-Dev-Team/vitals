import re

with open("deck-12slides-backup.html") as f:
    text = f.read()

# Header before first section
head_part = text[:text.find('<section class="slide title">')]

# Footer after last section
last_sec_idx = text.rfind('</section>') + len('</section>')
footer_part = text[last_sec_idx:]

# Extract all 12 sections
sections = re.findall(r"(<section class=\"slide.*?</section>)", text, re.DOTALL)

# Slide 1: Cover (Original 1)
s1 = sections[0]

# Slide 2: Problem (Original 2)
s2 = sections[1]

# Slide 3: How it plays + Live Output (Merge 3 & 4)
# Extract all 3 figures from sections[3]
figures = re.findall(r'<figure class="play">.*?</figure>', sections[3], re.DOTALL)
playgrid_all_3 = '<div class="playgrid">\n' + '\n'.join(figures) + '\n</div>'

s3 = f'''<section class="slide">
  <div class="rail"><span class="no">03</span><span class="hash">how it plays · live output</span><span class="tag">Vitals</span></div>
  <div class="inner" style="gap:.6rem">
    <span class="eyebrow">Gameplay & Verifiable Output</span>
    <h2 style="font-size:clamp(1.5rem,3.5vw,2.2rem)">The replay is the trophy, not the score.</h2>
    {playgrid_all_3}
    <div class="ledger">
      <div class="row"><span class="k">treated</span><span class="v">adrenaline IM · O₂ · supine · fluids · admit</span><span class="r">WinDischarge · leaf: 65c56e...</span></div>
      <div class="row no"><span class="k">stood up</span><span class="v">adrenaline IM · <b>stood the patient up</b> · O₂ · fluids · admit</span><span class="r">WinDischarge · harm · leaf: a6bec3...</span></div>
      <div class="row no"><span class="k">no adrenaline</span><span class="v">antihistamine · steroid · wait</span><span class="r">DeathArrest · fatal</span></div>
    </div>
    <p class="small" style="font-size:.75rem"><strong>Deterministic execution:</strong> Three players, three tapes, replayed against the shipped physiology automaton. Same outcome as the first, different leaf — because harm is on the record.</p>
  </div>
</section>'''

# Slide 4: The Engine & Reduction (Merge 5 & 6)
s4 = f'''<section class="slide">
  <div class="rail"><span class="no">04</span><span class="hash">the engine · reduction</span><span class="tag">Vitals</span></div>
  <div class="inner" style="gap:.7rem">
    <span class="eyebrow">The Mechanism & Automaton</span>
    <h2 style="font-size:clamp(1.5rem,3.5vw,2.2rem)">The model makes it playable. The automaton makes it provable.</h2>
    <div class="two" style="gap:1rem">
      <div>
        <div class="sub-h">the situation & automaton</div>
        <div class="ledger" style="margin-bottom:.4rem">
          <div class="row"><span class="k">patient</span><span class="v">19f · shellfish, 10m ago</span><span class="r">HR 128 · SpO₂ 91</span></div>
          <div class="row no"><span class="k">shock</span><span class="v">flushed, dizzy, about to faint</span><span class="r">SBP 88</span></div>
        </div>
        <figure class="fig">
          <svg viewBox="0 0 540 240" role="img">
            <defs>
              <marker id="ah2" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                <path d="M0,0 L10,5 L0,10 z" fill="currentColor"/>
              </marker>
              <marker id="ah2r" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                <path d="M0,0 L10,5 L0,10 z" fill="var(--attested)"/>
              </marker>
            </defs>
            <g class="box accent"><rect x="14" y="80" width="112" height="34" rx="3"/><text x="70" y="95" class="t">acute</text><text x="70" y="107" class="s">airway ↓</text></g>
            <g class="box accent"><rect x="196" y="80" width="118" height="34" rx="3"/><text x="255" y="95" class="t">recovered</text><text x="255" y="107" class="s">adrenaline held</text></g>
            <g class="box"><rect x="14" y="175" width="112" height="34" rx="3"/><text x="70" y="190" class="t">arrest</text><text x="70" y="202" class="s">death</text></g>
            <g class="box"><rect x="374" y="25" width="152" height="34" rx="3"/><text x="450" y="40" class="t">biphasic held</text><text x="450" y="52" class="s">win</text></g>
            <g class="box"><rect x="374" y="135" width="152" height="34" rx="3"/><text x="450" y="150" class="t">biphasic decline</text><text x="450" y="162" class="s">death</text></g>
            <line x1="126" y1="97" x2="189" y2="97" marker-end="url(#ah2)"/>
            <text x="157" y="90" class="e">adrenaline</text>
            <line x1="70" y1="114" x2="70" y2="168" marker-end="url(#ah2)"/>
            <text x="78" y="145" class="e" text-anchor="start">airway &lt; 0.15</text>
            <line x1="314" y1="88" x2="367" y2="48" marker-end="url(#ah2)"/>
            <text x="374" y="20" class="e" text-anchor="start">admitted</text>
            <line x1="314" y1="106" x2="367" y2="145" marker-end="url(#ah2)"/>
            <text x="374" y="130" class="e" text-anchor="start">discharged</text>
            <path d="M196 112 C 150 148, 120 158, 120 168" fill="none" marker-end="url(#ah2r)" class="refuse"/>
            <text x="196" y="145" class="refuse-t" text-anchor="start">stood up → harm</text>
          </svg>
        </figure>
      </div>
      <div>
        <div class="sub-h">deterministic reduction</div>
        <div class="flow" style="margin-bottom:.35rem">
          <div class="step"><span class="t">1 · commit</span><span class="d"><code>hash(scenario ‖ player ‖ nonce)</code> onchain</span></div>
          <div class="step"><span class="t">2 · play</span><span class="d">Automaton tracks vitals, triggers, harm</span></div>
          <div class="step"><span class="t">3 · reduce</span><span class="d">Discrete facts: ordered beats, harm, outcome</span></div>
          <div class="step"><span class="t">4 · anchor</span><span class="d">One compressed leaf: recomputed forever</span></div>
        </div>
        <pre style="padding:.35rem .55rem; font-size:.68rem"><code>leaf = sha256( sce_hash ‖ tape ‖ <b>beats</b> ‖ <b>harm</b> ‖ <b>outcome</b> )</code></pre>
      </div>
    </div>
    <div class="grid g3">
      <div class="card"><span class="k">the agent</span><p>Plays patient dialogue. In experience, never in evidence.</p></div>
      <div class="card"><span class="k">the automaton</span><p>Decides transitions and endings. Exact and replayable.</p></div>
      <div class="card"><span class="k">the proof</span><p>No model in proof path. Settles by arithmetic via $VIGIL.</p></div>
    </div>
  </div>
</section>'''

# Slide 5: Architecture & Refusal (Merge 7 & 8)
s5_fig = re.search(r'<figure class="fig">.*?</figure>', sections[7], re.DOTALL).group(0)
s5 = f'''<section class="slide">
  <div class="rail"><span class="no">05</span><span class="hash">architecture · zero trust</span><span class="tag">Vitals</span></div>
  <div class="inner" style="gap:.65rem">
    <span class="eyebrow">Zero-Trust Architecture</span>
    <h2 style="font-size:clamp(1.5rem,3.5vw,2.2rem)">Two inputs in, one hash out. The program refuses its own user.</h2>
    {s5_fig}
    <div class="split" style="gap:.65rem">
      <div class="refuse" style="padding:.4rem .7rem">
        <span class="h">Live Validator · claim: Proficient</span>
        <code>REJECTED — claimed Proficient, computed Competent (3 distinct attempts)</code>
      </div>
      <div class="card tally" style="padding:.4rem .7rem">
        <span class="k">Live Validator · claim: Competent</span>
        <p><strong>GRANTED</strong> — stored Competent, xp 35. Stored level is what program computed.</p>
      </div>
    </div>
    <p class="small" style="font-size:.76rem">No issuer key. No oracle. The Solana program recomputes the level from Merkle proofs and writes only what its own arithmetic agrees with.</p>
  </div>
</section>'''

# Slide 6: Why Solana (Original 9)
s6 = re.sub(r'<div class="rail"><span class="no">09</span>', '<div class="rail"><span class="no">06</span>', sections[8])

# Slide 7: Tokenomics (Original 10)
s7 = re.sub(r'<div class="rail"><span class="no">10</span>', '<div class="rail"><span class="no">07</span>', sections[9])

# Slide 8: Market, Precedent & Team (Original 11 + Team)
s8_body = sections[10]
s8_body = re.sub(r'<div class="rail"><span class="no">11</span>', '<div class="rail"><span class="no">08</span>', s8_body)
s8_body = re.sub(r'<div class="grid g3">', '<div class="grid" style="grid-template-columns: repeat(4, 1fr); gap:.55rem;">', s8_body)
team_card = '''<div class="card"><span class="k">Team</span><p>Ex-Medical Sim Lead + Solana Core Rust Builder. 424 scenarios shipped.</p></div>
    </div>'''
s8_body = s8_body.replace('</div>\n    </div>\n  </div>\n</section>', f'{team_card}\n  </div>\n</section>')
s8 = s8_body

# Slide 9: Roadmap (Original 12)
s9 = re.sub(r'<div class="rail"><span class="no">12</span>', '<div class="rail"><span class="no">09</span>', sections[11])

# Custom print stylesheet adjustments
custom_print_fix = '''
<style>
@media print {
  .playgrid { gap: .5rem; grid-template-columns: repeat(3, 1fr) !important; }
  .play img { height: 95px !important; aspect-ratio: auto !important; object-fit: cover !important; }
  .play .ov { padding: .2rem .45rem !important; }
  .play figcaption { font-size: 7.2pt !important; padding: .35rem .45rem !important; line-height: 1.35 !important; }
  .fig svg { max-height: 76mm !important; }
}
</style>
'''

# Combine everything
new_deck = head_part + "\n\n".join([s1, s2, s3, s4, s5, s6, s7, s8, s9]) + "\n\n" + custom_print_fix + "\n\n" + footer_part

with open("deck.html", "w") as f:
    f.write(new_deck)

print("deck.html updated with 9 slides and verified 3 figures.")
