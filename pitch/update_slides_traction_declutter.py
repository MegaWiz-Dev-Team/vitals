import re

# 1. Update deck.html
with open("deck.html") as f:
    deck = f.read()

# Update Slide 9 cards with precise 2-month traction & clear team role split
old_s9_grid = re.search(r'<div class="grid" style="grid-template-columns: repeat\(4, 1fr\); gap:\.55rem;">.*?</div>\s*</div>\s*</section>', deck, re.DOTALL)

new_s9_grid = '''<div class="grid" style="grid-template-columns: repeat(4, 1fr); gap:.65rem; margin-top:.4rem;">
      <div class="card">
        <span class="k">Why medicine first</span>
        <p>Highest stakes & regulation. Solve trust here and it generalises downward to every other skill credential.</p>
      </div>
      <div class="card">
        <span class="k">Who pays</span>
        <p>Institutions on standard invoices, sponsors funding scholarships, and scenario market take-rates. Players pay nothing.</p>
      </div>
      <div class="card" style="border-top:2px solid var(--proven); background:var(--surface-2)">
        <span class="k" style="color:var(--proven)">Head start (2 Mos Live)</span>
        <p><b>30 Medical Institutions (>75% of Thailand)</b> · <b>290+ Clinicians (68% active)</b> · 433 completed runs · <b>1st Place @ NECTEC 2026</b>.</p>
      </div>
      <div class="card" style="border-top:2px solid var(--proven); background:var(--surface-2)">
        <span class="k" style="color:var(--proven)">Team & Roles</span>
        <p><b>Medical Sim Lead (Creator of Embla)</b> × <b>Solana Core Rust Builder</b>. Live at <b>embla.megawiz.co.th</b>.</p>
      </div>
    </div>
  </div>
</section>'''

if old_s9_grid:
    deck = deck.replace(old_s9_grid.group(0), new_s9_grid)
    print("Updated Slide 9 grid.")

# Also declutter text in Slide 7 (Never Tokenized)
old_never = '''  <div class="card" style="border-top:3px solid var(--attested)">
    <span class="k">Never tokenized</span>
    <ul style="margin:.4rem 0 0; padding-left:1.1rem; font-size:.84rem; color:var(--ink-2); line-height:1.5;">
      <li><b>Credentials, badges, skill trees.</b> Token-2022 NonTransferable, no exceptions — a tradeable "Expert in Cardiology" is credential fraud with extra steps.</li>
      <li><b>Access to play.</b> No token gates a student out of practice, ever. Players never hold $VIGIL and never see it — the arena has its own name, <b>3R</b>, so the two layers do not even share a word.</li>
    </ul>
  </div>'''

# Declutter Slide 7 Never Tokenized if found
# Let's inspect s7
deck = re.sub(
    r'<div class="card" style="border-top:3px solid var\(--attested\)">.*?</div>\s*</div>\s*<div class="foot">',
    '''<div class="card" style="border-top:3px solid var(--attested)">
    <span class="k" style="color:var(--attested)">Never tokenized</span>
    <ul style="margin:.35rem 0 0; padding-left:1.1rem; font-size:.82rem; color:var(--ink-2); line-height:1.45;">
      <li><b>Zero NFT Diplomas:</b> Credentials are strictly Token-2022 NonTransferable.</li>
      <li><b>Zero Pay-to-Win:</b> Practice is 100% free. Standing cannot be bought.</li>
    </ul>
  </div>
  </div>
  <div class="foot">''',
    deck,
    flags=re.DOTALL
)

with open("deck.html", "w") as f:
    f.write(deck)

# 2. Update script.html
with open("script.html") as f:
    script = f.read()

# Update Beat 09 say and say-th
old_b9_say = re.search(r'<div class="slug"><span class="n">09</span>.*?<div class="say">(.*?)</div>\s*<div class="say-th">(.*?)</div>', script, re.DOTALL)

new_b9_say_content = '''    <p>The Grand Champion of Frontier this year was CrowdBrain — train in simulation, qualify through QA, route the best operators to real robots. That's this, for clinicians.</p>
    <p>And we didn't start four weeks ago — in just <em>two months of live production</em>, the underlying simulator won <em>1st Place at NECTEC AI for Thai 2026</em> and has organically reached <em>thirty medical institutions across over 75% of Thailand</em>, with over <em>two hundred and ninety clinicians</em> completing hundreds of clinical runs.</p>
    <p>Most teams spent this hackathon building the thing that produces the signal. We already had it. We spent it making the signal checkable on Solana.</p>
    <p>And episode six? A console called Skald drafts the series and every storyboard, checked automatically. <em>Skald verifies the content. Solana verifies the play.</em></p>'''

new_b9_say_th_content = '''      <span class="lbl">🇹🇭 คำแปลบทพูดภาษาไทย</span>
      <p>ผู้ชนะเลิศ Grand Champion ของงาน Frontier ปีนี้คือ CrowdBrain — ฝึกฝนในเครื่องจำลอง, คัดกรองด้วย QA, และส่งต่อคนเก่งไปคุมหุ่นยนต์จริง — Vitals ก็คือสิ่งนี้ แต่เป็นสำหรับแพทย์</p>
      <p>และเราไม่ได้เพิ่งเริ่มเมื่อ 4 สัปดาห์ก่อน — ในเวลาเพียง <b>2 เดือนที่เปิดใช้งานจริง</b> ตัว Simulator เบื้องหลังชนะเลิศรางวัลที่ 1 จาก NECTEC AI for Thai 2026 และเติบโตแบบ Organic จนครอบคลุม <b>30 สถาบันการแพทย์ (คิดเป็นกว่า 75% ของโรงเรียนแพทย์ทั้งประเทศ)</b> โดยมีบุคลากรทางการแพทย์กว่า <b>290 คน</b> เข้ามาฝึกรักษาจบไปแล้วกว่า 433 ครั้ง (ดูได้ที่ embla.megawiz.co.th)</p>
      <p>ทีมส่วนใหญ่ใช้เวลาในแฮกกาธอนสร้างตัวผลิตข้อมูล แต่เรามีมันอยู่แล้ว เราจึงใช้เวลานี้สร้างระบบตรวจสอบข้อมูลให้โปร่งใสระดับสากลบน Solana</p>
      <p>และเครื่องมือสร้างเคสของเรา (Skald) ช่วยร่างเคสและสตอรี่บอร์ด โดยมีทีมแพทย์คอยตรวจสอบความถูกต้องก่อนทุกขั้นตอน — ยึดหลักการเดียวกันคือ สิ่งที่ต้องถูกต้อง จะต้องถูกตรวจเช็กโดยอัตโนมัติ</p>'''

if old_b9_say:
    script = script.replace(old_b9_say.group(1), '\n' + new_b9_say_content + '\n  ')
    script = script.replace(old_b9_say.group(2), '\n' + new_b9_say_th_content + '\n    ')
    print("Updated Beat 09 in script.html.")

with open("script.html", "w") as f:
    f.write(script)

print("Updates complete. Ready to compile.")
