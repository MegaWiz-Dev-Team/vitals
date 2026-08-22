with open("script.html") as f:
    s = f.read()

# Update header tag line and runbar
s = s.replace("Twelve slides, spoken", "Nine slides, spoken")
s = s.replace("<span>12 slides</span>", "<span>9 slides</span>")

# Slide 03: merge 03 and 04
# Beat 03 replace
old_beat_3_4 = '''<!-- 03 -->
<section class="beat">
  <div class="slug"><span class="n">03</span><h2>It already runs</h2><span class="clock">0:49 – 1:33 · 44s</span></div>
  <div class="onscreen"><span class="lbl">On screen</span><span>Slide 03 — three tapes, three results, then the three hashes</span></div>
  <div class="say">
    <p>This isn't a mockup. Three people played that case.</p>
    <p>The first gives adrenaline, oxygen, lays her flat, keeps her for observation. She lives.</p>
    <p>The second gives the adrenaline, then stands her up. In anaphylaxis the ventricle is empty — standing someone up can kill them. She survives, but the harm is on the record.</p>
    <p>Look at the hashes. <em>Same outcome. Completely different leaf.</em> We didn't record whether she lived. We recorded how you got there.</p>
    <p>The third gives antihistamines and waits.</p>
    <p class="beatpause">— pause —</p>
    <p>She dies. Nobody scripted that. There's no losing branch in a story tree — the physiology killed her, the way it would have.</p>
  </div>
  <div class="th"><span class="k">ไทย · สิ่งที่กำลังสื่อ</span><p><b>สิ่งที่ทำ:</b> พิสูจน์ว่ามันรันจริง ด้วยคนเล่นสามคนบนเคสเดียวกัน</p><p><b>หัวใจอยู่ที่คนที่สอง</b> — ให้ adrenaline ถูก แต่พยุงลุก ในภาวะแพ้รุนแรงหัวใจแทบไม่มีเลือดค้าง การลุกยืนฆ่าคนได้ เธอรอด แต่ harm ติดอยู่ในบันทึก และ <b>hash ต่างกัน ทั้งที่ผลลัพธ์เหมือนกัน</b></p><p>หยุดหลังคำว่า “stands her up” — หมอในห้องจะมีปฏิกิริยา และปฏิกิริยานั้นขายความสมจริงได้ดีกว่าคำอธิบายใดๆ</p></div>
  <div class="note"><span class="lbl">Delivery</span><p>This is the longest beat and it earns it. Pause after "stands her up" — a clinician in the room will react, and that reaction sells the fidelity better than you can.</p></div>
</section>

<!-- PLAY -->
<section class="beat">
  <div class="slug"><span class="n">04</span><h2>How it plays</h2><span class="clock">1:33 – 2:26 · 53s</span></div>
  <div class="onscreen"><span class="lbl">On screen</span><span>Slide 04 — three stills from the game, each with its mechanic mocked up over it</span></div>
  <div class="say">
    <p>So what is it like to play?</p>
    <p>Not a score. <em>The replay is the trophy</em> — a watchable ninety seconds a stranger can re-derive. Your profile isn't a rank, it's a shelf of runs.</p>
    <p>And because the tape replays exactly, you can race someone else's ghost. Theirs gets adrenaline at thirty-eight seconds, yours at one-twelve, and <em>you watch the two patients diverge while you're still deciding.</em></p>
    <p>Harm goes on your record too — which keeps the fastest run and the cleanest run from being the same run.</p>
    <p class="beatpause">— pause —</p>
    <p>One line has to hold. The fun and the credential share data and <em>never share incentives</em> — you rank on the 3R board, a verifier stakes $VIGIL. Grinding your favourite episode buys a better time, never a better standing.</p>
  </div>
  <div class="th"><span class="k">ไทย · สิ่งที่กำลังสื่อ</span><p><b>สิ่งที่ทำ:</b> ตอบว่าเล่นแล้วสนุกยังไง โดยยอมรับตรงๆ ก่อนว่า XP กับ level ไม่ใช่ความสนุก</p><p><b>Ghost racing</b> คือกลไกที่ determinism ให้มาฟรี — เอา run ของคนอื่นมาเล่นคู่กัน แล้ว<b>เห็นคนไข้สองคนแยกทางกันตอนที่ยังตัดสินใจไม่เสร็จ</b></p><p><b>ต้องพูด:</b> ทุก overlay บนสไลด์นี้เป็น mockup — พูดเบาๆ ครั้งเดียว คนที่จับได้เองจะเลิกเชื่อสไลด์ที่เป็นของจริงไปด้วย</p></div>
  <div class="note"><span class="lbl">Delivery</span><p>Every overlay on this slide is labelled MOCKUP and you should say so once, lightly — "none of this is built yet, the stills are." A judge who spots an unlabelled mockup stops believing the slides that were real.</p></div>
</section>'''

new_beat_3 = '''<!-- 03 -->
<section class="beat">
  <div class="slug"><span class="n">03</span><h2>How it plays & Live output</h2><span class="clock">0:49 – 2:05 · 76s</span></div>
  <div class="onscreen"><span class="lbl">On screen</span><span>Slide 03 — three gameplay cards on top, three deterministic execution leaves below</span></div>
  <div class="say">
    <p>This isn't a mockup. Three people played that case.</p>
    <p>The first gives adrenaline, oxygen, lays her flat, admits her. She lives.</p>
    <p>The second gives adrenaline, but stands her up. Standing someone up in anaphylaxis can kill them — the ventricle is empty. She survives, but the harm is on the record.</p>
    <p>Look at the leaves: <em>same outcome, completely different hash.</em> The replay is the trophy, not the score.</p>
    <p>Because the tape replays exactly, you can race someone else's ghost. Theirs gets adrenaline at thirty-eight seconds, yours at one-twelve, and <em>you watch the two patients diverge while you're still deciding.</em></p>
    <p class="beatpause">— pause —</p>
    <p>The third gives antihistamines and she dies. Nobody scripted that. The deterministic physiology killed her, the way it would have in real life.</p>
  </div>
  <div class="th"><span class="k">ไทย · สิ่งที่กำลังสื่อ</span><p><b>สิ่งที่ทำ:</b> รวมทั้งความสนุกของการเล่น (Ghost racing, Failure is content) และการพิสูจน์ผลลัพธ์จริงบน On-chain leaf</p><p><b>จุดเด่น:</b> ชี้ให้เห็นว่า <b>Hash ต่างกันแม้ผลลัพธ์จะรอดเหมือนกัน</b> เพราะบันทึกเส้นทางการตัดสินใจจริงทั้งหมด</p></div>
  <div class="note"><span class="lbl">Delivery</span><p>Point to the ghost racing card, then drop down to the three output rows. Pause after "stands her up" — the clinical realism sells itself.</p></div>
</section>'''

s = s.replace(old_beat_3_4, new_beat_3)

# Slide 04: merge 05 and 06
old_beat_5_6 = '''<!-- CASE -->
<section class="beat">
  <div class="slug"><span class="n">05</span><h2>The machine under the case</h2><span class="clock">2:26 – 3:26 · 59s</span></div>
  <div class="onscreen"><span class="lbl">On screen</span><span>Slide 05 — the vitals on the left, the automaton on the right, the three cards underneath</span></div>
  <div class="say">
    <p>So what were they actually deciding?</p>
    <p>She has minutes. One drug decides it — adrenaline, intramuscular. Oxygen, fluids, <em>lay her flat</em>. Then admit her, because anaphylaxis relapses hours later.</p>
    <p>On the right, the same thing as a machine. Adrenaline moves her from acute to recovered; let the airway close and she arrests. Then the relapse fires — <em>admitted, she holds; discharged, she dies at home.</em></p>
    <p class="beatpause">— pause —</p>
    <p>Now the part that matters. <em>An AI agent plays the patient</em> — frightened, not volunteering that she left her auto-injector at home. It decides what you <em>learn</em>.</p>
    <p>The automaton decides what <em>happens</em> — and only the automaton is in the hash.</p>
    <p>That split is why there can be a token at all. The model makes it worth playing; <em>the automaton makes it worth proving</em> — it's exact, so two honest verifiers cannot disagree.</p>
  </div>
  <div class="th"><span class="k">ไทย · สิ่งที่กำลังสื่อ</span><p><b>สิ่งที่ทำ:</b> เอาเคสที่คนเพิ่งดูมาอธิบายเครื่องจักรข้างใต้ ไล่จากการรักษาจริง → automaton → ที่อยู่ของ AI agent → เหตุผลที่มี token ได้</p><p><b>ประโยคแกน:</b> <b>agent ตัดสินว่าคุณ<i>รู้</i>อะไร · automaton ตัดสินว่า<i>เกิด</i>อะไร</b> และมีแค่ automaton ที่อยู่ใน hash</p><p>ลากมือตามไดอะแกรมไปด้วย รูปทรงของมันคือตัวข้อโต้แย้ง</p></div>
  <div class="note"><span class="lbl">Delivery</span><p>This is the beat that earns the tokenomics slide four minutes later. If a judge follows the sentence "the agent decides what you learn, the automaton decides what happens", the staking argument lands as a consequence instead of arriving as a separate pitch.</p></div>
</section>

<!-- 04 -->
<section class="beat">
  <div class="slug"><span class="n">06</span><h2>The mechanism</h2><span class="clock">3:26 – 3:58 · 33s</span></div>
  <div class="onscreen"><span class="lbl">On screen</span><span>Slide 06 — commit · play · reduce · anchor, then the leaf formula</span></div>
  <div class="say">
    <p>Every run reduces to one hash. You commit before you play, so the chain knows how many attempts you <em>started</em> — grinding for a lucky run is visible.</p>
    <p>The tape is what you did and when. We reduce it to discrete facts and anchor one leaf. <em>The trajectory is simulated. The outcome is proven.</em></p>
    <p><em>No language model anywhere in that path.</em> Chess engines have verified replays for decades; we point it at a patient who's dying on a clock.</p>
  </div>
  <div class="th"><span class="k">ไทย · สิ่งที่กำลังสื่อ</span><p><b>สิ่งที่ทำ:</b> อธิบายว่า run กลายเป็น hash เดียวได้ยังไง — commit ก่อนเล่น เล่นกับ automaton ย่อเหลือข้อเท็จจริงที่ไม่ต่อเนื่อง แล้ว anchor</p><p><b>ประโยคที่แยกเราจากทีมอื่นทั้งรอบ:</b> “no language model anywhere in that path” — พูดเรียบๆ เหมือนอ่านสเปก ไม่ใช่เหมือนโม้</p></div>
  <div class="note"><span class="lbl">Delivery</span><p>"No language model in that path" is the line that separates you from every other AI-plus-crypto submission this round. Say it flat, like a spec, not like a boast.</p></div>
</section>'''

new_beat_4 = '''<!-- 04 -->
<section class="beat">
  <div class="slug"><span class="n">04</span><h2>The automaton & reduction</h2><span class="clock">2:05 – 3:15 · 70s</span></div>
  <div class="onscreen"><span class="lbl">On screen</span><span>Slide 04 — state machine on left, 4-step reduction pipeline on right</span></div>
  <div class="say">
    <p>So what was the machine under the case?</p>
    <p>On the left, the clinical state machine. Adrenaline moves her from acute to recovered; let the airway close and she arrests. Biphasic relapse hours later: admitted, she holds; discharged, she dies.</p>
    <p>Now the split that matters: <em>An AI agent plays the patient's dialogue. It decides what you learn.</em></p>
    <p><em>The deterministic automaton decides what happens</em> — vitals, harm, terminal outcome. And only the automaton enters the hash.</p>
    <p class="beatpause">— pause —</p>
    <p>Four discrete steps: commit before play, run the automaton, reduce to facts, anchor one leaf. <em>No language model anywhere in that proof path.</em></p>
    <p>The model makes it worth playing; the automaton makes it worth proving.</p>
  </div>
  <div class="th"><span class="k">ไทย · สิ่งที่กำลังสื่อ</span><p><b>สิ่งที่ทำ:</b> แยกขาดระหว่าง AI (สร้างความสมจริง) กับ Automaton (ตรวจข้อเท็จจริง) ชี้ชัดว่า<b>ไม่มี LLM อยู่ใน Proof path</b></p></div>
  <div class="note"><span class="lbl">Delivery</span><p>Emphasize: "Agent decides what you learn; automaton decides what happens." That sentence justifies the blockchain verification model.</p></div>
</section>'''

s = s.replace(old_beat_5_6, new_beat_4)

# Slide 05: merge 07 and 08
old_beat_7_8 = '''<!-- 05 -->
<section class="beat">
  <div class="slug"><span class="n">07</span><h2>The refusal</h2><span class="clock">3:58 – 4:29 · 31s</span></div>
  <div class="onscreen"><span class="lbl">On screen</span><span>Slide 07 — live terminal preferred: claim Proficient → REJECTED, claim Competent → GRANTED</span></div>
  <div class="say">
    <p>Here's my favourite part. Watch the program refuse me.</p>
    <p>I claim Proficient. It recomputes the level with the same arithmetic the game uses and says no. <em>You're Competent. Three distinct cases.</em></p>
    <p>Proficient needs five. That threshold isn't a demo constant — it's been in the shipping competency model for a year, and it's now refusing me on chain.</p>
    <p>I claim Competent. Granted. No issuer key, no oracle, no authority anyone can lean on or subpoena.</p>
  </div>
  <div class="th"><span class="k">ไทย · สิ่งที่กำลังสื่อ</span><p><b>สิ่งที่ทำ:</b> ให้ดูโปรแกรมปฏิเสธคนพูดเอง claim Expert แล้วมันคำนวณใหม่แล้วบอกว่าไม่</p><p><b>รันสดถ้าทำได้</b> — protocol ที่ปฏิเสธผู้สร้างของมันเองคือของที่ขายดีที่สุดใน deck นี้ ถ้าเป็นภาพนิ่งค่าจะเหลือราวหนึ่งในสิบ และถ้าเป็นภาพนิ่งต้องบอกว่าเป็นภาพนิ่ง</p></div>
  <div class="note"><span class="lbl">Delivery</span><p>Run it live if you possibly can. A protocol refusing its own author is the single most persuasive thing in this deck, and a screenshot of it is worth about a tenth as much.</p></div>
</section>

<!-- 06 -->
<section class="beat">
  <div class="slug"><span class="n">08</span><h2>Architecture</h2><span class="clock">4:29 – 5:05 · 35s</span></div>
  <div class="onscreen"><span class="lbl">On screen</span><span>Slide 08 — the diagram. Off chain above the line, on chain below, one hash crossing it.</span></div>
  <div class="say">
    <p>Above the line: two inputs — the tape and the scenario — reduced to facts, hashed to one leaf. <em>No model on that path.</em></p>
    <p>Below the line, the leaf goes into a tree. To claim anything you hand the program the leaf and its path, and it recomputes the level itself.</p>
    <p>Notice what never crosses. <em>The chain never sees a run</em> — not the transcript, not the vitals, not the patient. Only a hash that has to prove against a root the chain built itself.</p>
  </div>
  <div class="th"><span class="k">ไทย · สิ่งที่กำลังสื่อ</span><p><b>สิ่งที่ทำ:</b> วางระบบทั้งหมดบนจอเดียว เหนือเส้นคือ off-chain ใต้เส้นคือ on-chain มี hash เดียวข้ามเส้น</p><p><b>สิ่งที่ต้องชี้ให้เห็น:</b> <b>chain ไม่เคยเห็น run</b> — ไม่เห็นบทสนทนา ไม่เห็นสัญญาณชีพ ไม่เห็นคนไข้ เห็นแค่ hash ที่ต้องพิสูจน์กับ root ที่มันสร้างเอง</p></div>
  <div class="note"><span class="lbl">Delivery</span><p>Trace the diagram with your hand as you say it — left to right on top, down the right edge, right to left underneath. The shape is the argument: everything expensive and private stays above the line, and one hash crosses.</p></div>
</section>'''

new_beat_5 = '''<!-- 05 -->
<section class="beat">
  <div class="slug"><span class="n">05</span><h2>Zero-trust architecture & refusal</h2><span class="clock">3:15 – 4:15 · 60s</span></div>
  <div class="onscreen"><span class="lbl">On screen</span><span>Slide 05 — off-chain/on-chain diagram + live validator claim rejection</span></div>
  <div class="say">
    <p>Here is the full architecture: two inputs above the line, one hash crossing it. The chain never sees a run — only a leaf proving against a Merkle root it built itself.</p>
    <p>And watch the program refuse me live on a validator:</p>
    <p>I claim Proficient. The program recomputes the level against my anchored leaves and says no: <em>REJECTED — you're Competent from three distinct attempts.</em></p>
    <p>I claim Competent. GRANTED.</p>
    <p class="beatpause">— pause —</p>
    <p>No issuer key. No oracle. No authority anyone can lean on, bribe, or subpoena. Take the chain away and you're back to trusting one company's database.</p>
  </div>
  <div class="th"><span class="k">ไทย · สิ่งที่กำลังสื่อ</span><p><b>สิ่งที่ทำ:</b> อธิบายสถาปัตยกรรม Merkle proof และโชว์หลักฐานที่โปรแกรม Solana สั่ง Reject เคลมปลอมของ User อัตโนมัติ</p></div>
  <div class="note"><span class="lbl">Delivery</span><p>Trace the diagram from off-chain inputs down to the on-chain refusal. A protocol refusing its own creator is the most memorable moment.</p></div>
</section>'''

s = s.replace(old_beat_7_8, new_beat_5)

# Renumber remaining beats: 09 -> 06, 10 -> 07, 11 -> 08, 12 -> 09
s = s.replace('<div class="slug"><span class="n">09</span><h2>Why Solana</h2><span class="clock">5:05 – 5:27 · 22s</span></div>', '<div class="slug"><span class="n">06</span><h2>Why Solana</h2><span class="clock">4:15 – 4:45 · 30s</span></div>')
s = s.replace('<span>Slide 09 —', '<span>Slide 06 —')

s = s.replace('<div class="slug"><span class="n">10</span><h2>$VIGIL</h2><span class="clock">5:27 – 6:21 · 54s</span></div>', '<div class="slug"><span class="n">07</span><h2>$VIGIL · Verification market</h2><span class="clock">4:45 – 5:35 · 50s</span></div>')
s = s.replace('<span>Slide 10 —', '<span>Slide 07 —')

s = s.replace('<div class="slug"><span class="n">11</span><h2>Precedent, and who we are</h2><span class="clock">6:21 – 7:06 · 46s</span></div>', '<div class="slug"><span class="n">08</span><h2>Precedent & Builders</h2><span class="clock">5:35 – 6:20 · 45s</span></div>')
s = s.replace('<span>Slide 11 —', '<span>Slide 08 —')

s = s.replace('<div class="slug"><span class="n">12</span><h2>Close and ask</h2><span class="clock">7:06 – 7:35 · 29s</span></div>', '<div class="slug"><span class="n">09</span><h2>Close and ask</h2><span class="clock">6:20 – 7:00 · 40s</span></div>')
s = s.replace('<span>Slide 12 —', '<span>Slide 09 —')

with open("script.html", "w") as f:
    f.write(s)

print("script.html updated to 9 beats.")
