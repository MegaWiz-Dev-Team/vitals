import re

with open("script.html") as f:
    text = f.read()

# Clean misplaced breakdowns
text = text.replace('<!-- 03 -->\n<section class="beat">\n\n  <div class="ui-breakdown">', '<!-- 03 -->\n<section class="beat">')
text = text.replace('<!-- 05 -->\n<section class="beat">\n\n  <div class="ui-breakdown">', '<!-- 05 -->\n<section class="beat">')

# 1. Beat 03: Insert UI Screen Breakdown right after slide-thumb
beat3_ui = '''
  <div class="ui-breakdown" style="break-inside:avoid;">
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
text = re.sub(r'(<div class="slug"><span class="n">03</span>.*?<div class="slide-thumb">.*?</div>)', r'\1\n' + beat3_ui, text, count=1, flags=re.DOTALL)

# 2. Beat 05: Insert UI Screen Breakdown right after slide-thumb
beat5_ui = '''
  <div class="ui-breakdown" style="break-inside:avoid;">
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
text = re.sub(r'(<div class="slug"><span class="n">05</span>.*?<div class="slide-thumb">.*?</div>)', r'\1\n' + beat5_ui, text, count=1, flags=re.DOTALL)

# 3. Beat 08: Insert UI Breakdown for Verifier & Market
beat8_ui = '''
  <div class="ui-breakdown" style="break-inside:avoid;">
    <span class="h">🔍 คำอธิบายกลไกตรวจสอบและตลาด (Screen Capture Breakdown)</span>
    <div class="ui-grid">
      <div class="ui-item">
        <span class="k">1. The One Who Relies On It (Residency Program)</span>
        <p>• โรงพยาบาลปลายทางรัน Verifier เองได้ เพื่อ re-run ประวัติคนไข้และตรวจฝีมือจริงก่อนรับแพทย์เข้าทำงาน</p>
      </div>
      <div class="ui-item warn">
        <span class="k">2. You Cannot Buy a Leaf (Anti-Pay-to-Win)</span>
        <p>• เงินซื้อการปลดล็อกด่านหรือสปอนเซอร์ได้ แต่ไม่มีวันใช้เงินซื้อคะแนน/วุฒิบัตรบนบล็อกเชนได้</p>
      </div>
    </div>
  </div>
'''
text = re.sub(r'(<div class="slug"><span class="n">08</span>.*?<div class="slide-thumb">.*?</div>)', r'\1\n' + beat8_ui, text, count=1, flags=re.DOTALL)

with open("script.html", "w") as f:
    f.write(text)

print("Nesting and breakdowns updated cleanly.")
