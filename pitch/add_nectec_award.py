import re

# 1. Update deck.html
with open("deck.html") as f:
    deck = f.read()

# Update Slide 9 cards
old_s9_cards = '''      <div class="card"><span class="k">Head start</span><p>A clinical simulator already in production — 424 authored scenarios, deterministic scoring, a hash-chained audit log, real students.</p></div>
      <div class="card"><span class="k">Team</span><p>Ex-Medical Sim Lead + Solana Core Rust Builder. 424 scenarios shipped.</p></div>'''

new_s9_cards = '''      <div class="card" style="border-top:2px solid var(--proven)"><span class="k">Head start & Traction</span><p><b>1st Place Grand Winner @ NECTEC AI for Thai 2026</b> — 424 authored scenarios in production, deterministic scoring, real medical students.</p></div>
      <div class="card" style="border-top:2px solid var(--proven)"><span class="k">Team & Pedigree</span><p>Ex-Medical Sim Lead + Solana Core Rust Builder. Live platform at <b>embla.megawiz.co.th</b>.</p></div>'''

deck = deck.replace(old_s9_cards, new_s9_cards)

# Update Slide 10: Add live proof badge bar in Working now or under footer
old_s10_working = '''        <div class="sub-h">Working now</div>
        <ul>
          <li><strong>Replay → leaf.</strong> EP1 runs against the shipped automaton; three tapes, three distinct leaves, reproducible across runs.</li>
          <li><strong>Program → refusal.</strong> Native Solana program live on a validator, recomputing levels and rejecting claims that don't survive its own arithmetic.</li>
          <li><strong>Playable</strong> — a browser app: talk to her on a local model, treat her on a real bedside monitor, and the run anchors when it ends.</li>
          <li><strong>Rust end to end</strong> — one scoring implementation compiled into the game, the verifier, and the program.</li>
        </ul>'''

new_s10_working = '''        <div class="sub-h">Working now (Live in Production)</div>
        <ul>
          <li><strong>Replay → leaf.</strong> EP1 runs against shipped automaton; three tapes, three distinct leaves, reproducible across runs.</li>
          <li><strong>Program → refusal.</strong> Native Solana program live on validator, recomputing levels and rejecting invalid claims.</li>
          <li><strong>Award-Winning Engine.</strong> 1st Place @ NECTEC AI for Thai 2026 (<code>team01.aiforthai.in.th</code> / <code>embla.megawiz.co.th</code>).</li>
          <li><strong>Rust end to end.</strong> One scoring implementation compiled into game, verifier, and onchain program.</li>
        </ul>'''

deck = deck.replace(old_s10_working, new_s10_working)

with open("deck.html", "w") as f:
    f.write(deck)

# 2. Update script.html
with open("script.html") as f:
    script = f.read()

# Update Beat 09 say & say-th
old_b9_say = '''    <p>And we didn't start four weeks ago — the simulator underneath is in production with real students, four hundred and twenty-four scenarios deep.</p>
    <p>Most teams spent this hackathon building the thing that produces the signal. We already had it. We spent it making the signal checkable.</p>'''

new_b9_say = '''    <p>And we didn't start four weeks ago — the simulator underneath won <em>1st Place at NECTEC AI for Thai 2026</em>, and is in live production with real students across four hundred and twenty-four scenarios.</p>
    <p>Most teams spent this hackathon building the thing that produces the signal. We already had it. We spent it making the signal checkable.</p>'''

script = script.replace(old_b9_say, new_b9_say)

old_b9_say_th = '''      <p>และเราไม่ได้เพิ่งเริ่มเมื่อ 4 สัปดาห์ก่อน — ตัว Simulator เบื้องหลังถูกใช้งานจริงกับนักศึกษาแพทย์แล้ว โดยมีเคสการรักษาลึกถึง 424 สถานการณ์</p>
      <p>ทีมส่วนใหญ่ใช้เวลาในแฮกกาธอนสร้างตัวผลิตข้อมูล แต่เรามีมันอยู่แล้ว เราจึงใช้เวลานี้สร้างระบบตรวจสอบข้อมูลให้โปร่งใสระดับสากล</p>'''

new_b9_say_th = '''      <p>และเราไม่ได้เพิ่งเริ่มเมื่อ 4 สัปดาห์ก่อน — ตัว Simulator เบื้องหลังชนะเลิศรางวัลที่ 1 จาก NECTEC AI for Thai 2026 และถูกใช้งานจริงกับนักศึกษาแพทย์แล้วกว่า 424 สถานการณ์ (ดูได้ที่ embla.megawiz.co.th)</p>
      <p>ทีมส่วนใหญ่ใช้เวลาในแฮกกาธอนสร้างตัวผลิตข้อมูล แต่เรามีมันอยู่แล้ว เราจึงใช้เวลานี้สร้างระบบตรวจสอบข้อมูลให้โปร่งใสระดับสากล</p>'''

script = script.replace(old_b9_say_th, new_b9_say_th)

with open("script.html", "w") as f:
    f.write(script)

print("NECTEC Award and Live URLs successfully added to deck.html and script.html.")
