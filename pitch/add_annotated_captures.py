import re

with open("script.html") as f:
    text = f.read()

# Add CSS for UI capture breakdown box
ui_box_css = '''
/* UI Capture Breakdown Box */
.ui-breakdown{
  background:var(--surface); border:1px solid var(--rule); border-radius:4px;
  padding:1rem 1.15rem; margin:.9rem 0; display:flex; flex-direction:column; gap:.6rem;
  box-shadow:var(--shadow); font-size:.88rem;
}
.ui-breakdown .h{font-family:"IBM Plex Mono",monospace; font-size:.7rem; letter-spacing:.14em; text-transform:uppercase; color:var(--proven); font-weight:600}
.ui-grid{display:grid; grid-template-columns:repeat(auto-fit, minmax(14rem, 1fr)); gap:.8rem; margin-top:.3rem}
.ui-item{background:var(--surface-2); border-radius:3px; padding:.6rem .8rem; border-left:3px solid var(--proven)}
.ui-item.warn{border-left-color:var(--attested)}
.ui-item .k{font-family:"IBM Plex Mono",monospace; font-size:.68rem; font-weight:600; color:var(--ink); display:block; margin-bottom:.2rem}
.ui-item p{font-size:.82rem; color:var(--ink-2); margin:0; line-height:1.45}
'''

text = text.replace('/* the spoken line', ui_box_css + '\n/* the spoken line')

# 1. Beat 03: Add UI Screen Breakdown for Gameplay Stills
beat3_ui = '''
  <div class="ui-breakdown">
    <span class="h">🔍 คำอธิบายองค์ประกอบหน้าจอ Gameplay (Screen Capture Breakdown)</span>
    <div class="ui-grid">
      <div class="ui-item">
        <span class="k">1. Ghost Racing & Bedside Monitor</span>
        <p>• <b>SpO₂ 18 (Alarm แดง):</b> มอนิเตอร์แสดงค่าออกซิเจนในเลือดตกวิกฤต<br>• <b>Ghost 0:38 vs You 1:12:</b> แถบ Ghost แสดงความเร็วเปรียบเทียบกับแพทย์คนอื่นแบบเรียลไทม์</p>
      </div>
      <div class="ui-item warn">
        <span class="k">2. Harm on Record (Standing Collapse)</span>
        <p>• <b>Harm: Stand/Walk Collapse:</b> บันทึกข้อผิดพลาดติดลง Replay ทันทีที่สั่งคนไข้ลุกยืน<br>• <b>50 saves · 12 harms:</b> แยกความเร็วในการรักษาออกจากความปลอดภัย</p>
      </div>
      <div class="ui-item warn">
        <span class="k">3. Death Card & Gold Path</span>
        <p>• <b>DeathArrest at 2:14:</b> เวลาที่คนไข้หัวใจหยุดเต้นจากการให้ยาผิด<br>• <b>Gold Path (0:40):</b> เฉลยแนวทางรักษามาตรฐานเพื่อการเรียนรู้ข้อผิดพลาด</p>
      </div>
    </div>
  </div>
'''

text = text.replace('<!-- 03 -->\n<section class="beat">', '<!-- 03 -->\n<section class="beat">\n' + beat3_ui)

# 2. Beat 05: Add UI Screen Breakdown for Live Validator CLI Terminal
beat5_ui = '''
  <div class="ui-breakdown">
    <span class="h">🔍 คำอธิบายหน้าต่าง Live Validator CLI (Screen Capture Breakdown)</span>
    <div class="ui-grid">
      <div class="ui-item warn">
        <span class="k">> claim Expert ➔ REJECTED</span>
        <p>• <b>ผลการตรวจ:</b> โปรแกรมบน Solana Validator คำนวณประวัติจริงแล้วสั่งปฏิเสธทันที เพราะเล่นผ่านเพียง 3 เคส (เกณฑ์ Expert ต้องผ่าน 5 เคสขึ้นไป)</p>
      </div>
      <div class="ui-item">
        <span class="k">> claim Competent ➔ GRANTED</span>
        <p>• <b>ผลการตรวจ:</b> บันทึกระดับ Competent ลง On-chain State ตามการคำนวณจริง ปราศจากการแทรกแซงจากแอดมินหรือผู้สร้างระบบ</p>
      </div>
    </div>
  </div>
'''

text = text.replace('<!-- 05 -->\n<section class="beat">', '<!-- 05 -->\n<section class="beat">\n' + beat5_ui)

with open("script.html", "w") as f:
    f.write(text)

print("script.html updated with screen capture breakdowns.")
