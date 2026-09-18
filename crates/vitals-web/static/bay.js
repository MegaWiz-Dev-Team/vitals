/* The bay itself: the automaton's client, the monitor, the orders, the chart, the tape and
   the chain. Shared by the Eternal entry (index.html) and the ward's shift page
   (world/shift.html) — one engine on the server, one surface in the browser.
   Cut verbatim out of index.html on 16 ก.ย.; the token and the build stamp are injected by
   the server exactly as they were when this lived in the page. */

const $=s=>document.querySelector(s);
/* Bind a handler only if this page has the element. The bay is shared by the Eternal entry, which
   has a lobby and a shelf, and the ward's shift page, which has neither — and one handler bound to
   a missing element throws at parse time and takes the whole bay down with it. */
const onLobby=(sel,prop,fn)=>{const el=$(sel); if(el)el[prop]=fn; return el;};
/* Is this page the one with the season's front page on it? Everything that draws the shelf, the
   hero, the record ring or the meter asks first, and does nothing on a page that has none of them.
   One check at the top of each rather than a guard on every line inside it: the functions are
   whole-purpose, and half of one running on a page it was not written for is worse than none. */
const HASLOBBY=()=>!!$('#hero');
/* Which patient's shift this page is, when it is one: the bay is one bay, and /ward/<id> is the
   parameter (CWF_PLAN.md ruling 13). Declared up here rather than beside the rest of the ward's
   code at the foot of the script, because the card renderer reads it and a const in its temporal
   dead zone would throw the first time a station card is drawn. */
const WARD=(location.pathname.match(/^\/ward\/(\d+)\/?$/)||[])[1]||null;
/* The case this page is *reading*, when it is a review run: `/ward/review/<case id>`. Not a shift —
   nobody is in this bed, nothing is anchored and there is no head to take — but the same surface,
   because a reviewer has to read the case as a learner meets it. Declared beside `WARD` and for the
   same reason: the card renderer runs long before the ward's own code. */
const REVIEW=(location.pathname.match(/^\/ward\/review\/([a-z0-9-]+)\/?$/)||[])[1]||null;
/* The two pages this host serves, in the one thing they have in common: the case comes off the
   wire. Everything that used to ask "is this the ward" to answer "where does the case come from"
   asks this instead — a review run that asked `WARD` got the shelf's answer, which is EP1, which is
   the bug this whole surface was rebuilt to stop. What stays `WARD` is what is true only of a
   shift: a head to take, a bed, a chain. */
const WARDSURFACE=WARD||REVIEW;

/* The face the last view carried, or '' when there was none. Up here with `WARD` and for the same
   reason: `paintStill` is a long way above the rest of the ward's code and a `let` declared below
   its first call is a dead zone the page dies in, silently, with the markup looking perfectly
   fine. Read in `paintStill`, written in `paint` — the only two places a picture and a view meet. */
let WARDFACE='';
/* The case this shift is, as the payload described it — the card the page draws, the tray it
   draws, and the label each button in that tray wears. Declared up here with `WARD` and
   `WARDFACE` for the same reason they are: `ep()` reads the card and runs long before the ward's
   own code, and a `let` in its temporal dead zone takes the whole page down silently.
   Written in one place (`openShift`, off the payload) and nowhere else. */
let WARDCARD=null, WARDCHIPS=null, WARDLABEL={};
/* On a shift page the season does not exist. Not hidden after it renders — never rendered: a
   stranger who came from the globe to treat somebody must not land in the single-player product,
   and must find nothing here that leads into it. The class goes on before anything paints, and
   the lobby, its hero, its shelf, its stars and the bay's own way back to the shelf all go with
   it. `/` on this host is the globe; the way back is there. */
if(WARD||REVIEW){
  document.documentElement.classList.add('is-ward');
  if(REVIEW)document.documentElement.classList.add('is-review');
  addEventListener('DOMContentLoaded',()=>{
    document.querySelector('#lobby')?.classList.add('hide');
    document.querySelector('#game')?.classList.remove('hide');
    const n=document.querySelector('#ep-name'); if(n)n.textContent='the ward';
  },{once:true});
}
/* Escaping, up here with WARD because the renderers below use both and a const in its
   temporal dead zone throws the first time one of them draws. */
const esc=t=>String(t==null?'':t).replace(/[&<>"]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]));
/* Injected by the server when it is running with a token. Empty on a loopback demo. */
const TOKEN='__VITALS_TOKEN__';
const _fetch=window.fetch;
window.fetch=(u,o={})=>{
  if(TOKEN && typeof u==='string' && u.startsWith('/api/'))
    o={...o, headers:{...(o.headers||{}), Authorization:'Bearer '+TOKEN}};
  return _fetch(u,o);
};

/* ─── who you are ──────────────────────────────────────────────────────────────
   An Ed25519 key, generated here, kept here. The server pays the network fee and
   never sees this key — that is what makes the record on chain yours rather than
   the server's. Before this, the server signed as itself for everyone, so every
   player on a box shared one identity and therefore one level.

   No wallet to install and no SOL to buy, which is the whole point: someone who
   has never touched crypto should be able to finish a case and own the result. */
const B58='123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';
const b58=b=>{
  let d=[0];
  for(const x of b){ let c=x;
    for(let i=0;i<d.length;i++){ c+=d[i]<<8; d[i]=c%58; c=(c/58)|0; }
    while(c){ d.push(c%58); c=(c/58)|0; } }
  let out=''; for(const x of b){ if(x)break; out+='1'; }
  return out + d.reverse().map(i=>B58[i]).join('');
};
const hexOf=b=>[...b].map(x=>x.toString(16).padStart(2,'0')).join('');
const unhex=h=>new Uint8Array(h.match(/../g).map(x=>parseInt(x,16)));
const b64=b=>btoa(String.fromCharCode(...b));
const unb64=s=>new Uint8Array([...atob(s)].map(c=>c.charCodeAt(0)));

let ME=null;   /* {pub: base58, key: CryptoKey} — null when the browser cannot do Ed25519 */
/* Who the record belongs to, which is not the same as which machine is typing.
   Unset means "this machine is the person" — true for the browser you first played on. */
const acctOf=()=>localStorage.getItem('vitals.account')||(ME&&ME.pub)||'';
const setAcct=a=>a?localStorage.setItem('vitals.account',a):localStorage.removeItem('vitals.account');
async function identity(){
  if(ME)return ME;
  try{
    const stored=localStorage.getItem('vitals.key');
    let priv,pub;
    if(stored){
      const j=JSON.parse(stored);
      priv=await crypto.subtle.importKey('pkcs8',unb64(j.k),{name:'Ed25519'},true,['sign']);
      pub=unb64(j.p);
    }else{
      const kp=await crypto.subtle.generateKey({name:'Ed25519'},true,['sign','verify']);
      priv=kp.privateKey;
      pub=new Uint8Array(await crypto.subtle.exportKey('raw',kp.publicKey));
      const pk=new Uint8Array(await crypto.subtle.exportKey('pkcs8',priv));
      localStorage.setItem('vitals.key',JSON.stringify({k:b64(pk),p:b64(pub)}));
    }
    ME={pub:b58(pub),key:priv};
  }catch(e){
    /* Ed25519 in WebCrypto is not everywhere yet. Say so instead of quietly falling
       back to letting the server hold the key — that would be the exact thing this
       replaced, wearing the same UI. */
    ME=null;
  }
  return ME;
}
async function sign(hex){
  const me=await identity(); if(!me)throw new Error('this browser cannot sign');
  return hexOf(new Uint8Array(await crypto.subtle.sign({name:'Ed25519'},me.key,unhex(hex))));
}
/* prepare on the server → sign here → submit. The server cannot do the middle step. */
let runCommitted=false;
/* The two numbers the chain holds against this account: declared starts (the commitment
   account's count) and anchored proofs. Kept here so a commit can move the counter the moment
   it lands, without waiting for the next full chain read. */
let TALLY={started:null,anchored:0};
function showTally(){
  if(TALLY.started==null && !TALLY.anchored) return;
  $('#tally').textContent=`started ${TALLY.started??0} · anchored ${TALLY.anchored}`;
}
/* The mode is part of the declaration — bound into the commitment hash before play — so the
   ceremony says it out loud: the player sees the label they are binding. Practice until the
   OSCE stations open; entering one will pass exam=true. */
async function commitRun(exam){
  runCommitted=false;
  const r=await chainDo('/api/commit?id='+id+'&exam='+(exam?1:0));
  EXAMLIVE=!!(r&&r.committed&&r.exam);
  if(r&&r.committed){runCommitted=true;
    ev('note','—','committed — attempt #'+r.started+' on the record · '+(r.exam?'OSCE exam':'practice'));
    if(r.started!=null){TALLY.started=r.started; showTally();}}
  else if(r&&r.error&&!/no chain/.test(r.error)) ev('note','—','commit failed: '+r.error);
  return r;
}
async function chainDo(url){
  const me=await identity();
  if(!me)return {error:'this browser has no Ed25519 — try Chrome, Safari 17+ or Firefox 129+'};
  const r=await (await fetch(url+'&player='+me.pub+'&account='+acctOf())).json();
  if(r.error||!r.sign)return r;
  /* An anchor arrives as two messages — the record outgrew one packet — and the key signs both
     in the same breath. One message stays one signature. */
  const sig=await sign(r.sign);
  let q='/api/submit?player='+me.pub+'&sig='+sig;
  if(r.sign2)q+='&sig2='+await sign(r.sign2);
  return await (await fetch(q)).json();
}

/* ─── carrying the record between machines ────────────────────────────────────
   The key never moves. What moves is permission: an account lists the machines
   allowed to act for it, and a machine already on the list can add another.
   Reading a score needs neither — an account id is enough, which is what makes a
   level checkable from a machine you do not own. */
async function accountState(){
  const me=await identity(); if(!me)return null;
  return (await (await fetch(`/api/account?device=${me.pub}&account=${acctOf()}`)).json());
}
async function linkDevice(other){
  const me=await identity(); if(!me)return {error:'no key in this browser'};
  const r=await (await fetch(`/api/link?player=${me.pub}&account=${acctOf()}&device=${other}`)).json();
  if(r.error||!r.sign)return r;
  return await (await fetch('/api/submit?player='+me.pub+'&sig='+await sign(r.sign))).json();
}
const shortKey=k=>k?k.slice(0,6)+'…'+k.slice(-4):'—';
/* Every call that reaches into a case says who is asking. The server answers an owned case only
   to its owner — a session id used to be `s7`, and anybody who typed it could give your patient
   an order that lands on your tape for good. */
const asMe=()=>ME?'&player='+ME.pub:'';

/* ─── the season ───────────────────────────────────────────────────────────── */
const SEASON=[
 {id:'ep1',n:'EP1',sn:'S1:E1',t:'The Last Bite',tier:'student',art:'/img/stable.jpg',
  rt:'a 12-minute shift',mins:12,preview:'ep1_cold_open',
  who:'Ing · F 19', line:'breathless, urticaria all over, lips swelling',
  place:'Emergency department · night',
  d:'A nineteen-year-old, ten minutes after the wrong salad. Her throat is closing.'},
 /* The stations are exam-only interludes — same bay, same engine, but the card commits
    exam-ness before the first order, and the stars it earns open its set's episode door.
    Station Sets v2: the shelf face is a clinic card (the server's set table is the authority;
    these are the instant-paint copies).
    Copy rule for a station's `t`, `d`, `who` AND `line` alike: they are a stem, not a plan.
    They may say who walks in and what you can see from the door; they may not name anything the
    mark sheet marks — and they may not point at another case either.
    `line` is the strictest of the four, because it is the one that never leaves the screen: it
    is the "presents" row of the sheet, the label over the bed and the first sentence of the
    empty transcript, so whatever it says sits in front of the candidate for the whole station.
    Ten of the twelve used to answer their own mark sheet from that row, and between two and
    eight marks a station were payable off it without touching the patient.
    The test to apply to a candidate line is not "is it true" but "could I see it from the door
    without asking a question or laying a hand on the patient". A symptom a relative reports, a
    sound that needs a stethoscope, a weight that needs scales, a number that needs a machine
    and a history that needs a question all fail it. A's and C's lines already passed and are
    deliberately untouched.
    `t` fails the same test on seven stations and is only half fixable from here: the shelf card
    and the sheet's own headline take the server's set table when it has landed, and that table
    is in vitals-web/src/main.rs. What is fixed from here is the player bar, which prints `t`
    from this file and rides above the bay for the whole station. Six of these blurbs used to
    ("you saw this in EP1 — prove it", "the same disease in a different costume", "EP3 taught
    you the difference"), which hands a returning player the diagnosis on the card they click
    to be examined on it. A cross-reference is a hint with a story wrapped round it; the shelf
    is outside the exam room and the exam has already started by the time it is read.
    `t` used to be the case name — the diagnosis, spelled out, as the headline — and that string
    rides the player bar for the whole exam while the rubric is paying for naming the
    diagnosis. It is the stem now, and the answer is revealed after the outcome instead (REVEAL,
    below). Half of these blurbs used to read back demo/rubrics item for item — the order of the
    workup, the drug and the dose to draw off a weight the candidate is paid to ask for — which
    hands the candidate the answer on the card they click to be examined on it.
    The offending strings are described here and not quoted, which is the same rule one level up:
    this file is served to the candidate whole, so a comment that repeats the answer leaks it
    exactly as the card did, and view-source is no harder to read than the shelf.
    `spec` is the circuit band a real OSCE door wears (emergency · paediatrics), not the Eir
    organ specialty: an "eir-<organ system>" band printed over the bed answers this station's own
    trap before the candidate has touched the patient. */
 /* `line` is not decoration. `stemHtml` prints it as **presents**, which is the last thing a
    candidate reads before the clock starts, so it is the briefing — and these two had each
    other's, each one briefed on a presentation belonging to the other case.
    A straight swap does not fix it and was not enough: the lines also disagreed with their own
    patients about who they are. Each line below is written from its own `.sce.json`, says only
    what that scenario actually answers, and agrees with the title on its own card. */
 {id:'osce-a',n:'OSCE A',sn:'S1 · OSCE A',t:'Rash and facial swelling after a meal — M 71',tier:'student',station:true,
  spec:'emergency',rt:'an 8-minute station',mins:8,
  who:'Somchai · M 71', line:'urticaria head to toe, face swelling, a wheeze you can hear',
  place:'OSCE station · Nurse Mali examining',
  d:'Thirty minutes after a meal he will not talk about. Seventy-one, sweating, and the swelling has not stopped moving. Nurse Mali is marking.'},
 {id:'osce-a2',n:'OSCE A2',sn:'S1 · OSCE A2',t:'Belly cramps, loose stools, swollen face — F 68',tier:'student',station:true,
  spec:'emergency',rt:'an 8-minute station',mins:8,
  who:'Somsri · F 68', line:'cramping belly pain, loose stools, wheals and a swelling face',
  place:'OSCE station · Nurse Mali marking',
  d:'She arrives looking like food poisoning. Sixty-eight, and her face has changed shape since breakfast.'},
 /* Key art, from EP2 on: `art` is the 16:9 billboard crop and `art2` the 3:2 one a narrow
    screen gets instead (artOf() builds the <picture>). EP1 keeps stable.jpg — a real frame of
    its own patient, which no generated portrait improves on. The faces are canon; the rules
    for shooting more of them are in docs/internal/SEASON_ARC.md, "Canon ภาพตัวละคร". */
 {id:'ep2',n:'EP2',sn:'S1:E2',t:'Time Is Muscle',tier:'intern',rt:'a 12-minute shift',mins:12,
  art:'/img/ep2_prasit.jpg',art2:'/img/ep2_prasit_3x2.jpg',
  who:'Prasit · M 58', line:'crushing chest pain to the jaw, sweating',
  place:'Emergency department · evening',
  d:'Thirty years driving patients to this door — tonight Prasit arrives as one. The ECG tells the truth in ten minutes.'},
 {id:'osce-b',n:'OSCE B',sn:'S1 · OSCE B',t:'Chest pain — M 25',tier:'intern',station:true,
  spec:'emergency',rt:'a 10-minute station',mins:10,
  who:'Somchai Jaidee · M 25', line:'chest pain, nauseous, and certain he is dying',
  place:'OSCE station · Nurse Mali on the stopwatch',
  d:'Twenty-five, on the trolley and frightened of it. Nurse Mali is on the stopwatch and will not say what it is counting.'},
 {id:'osce-b2',n:'OSCE B2',sn:'S1 · OSCE B2',t:'Chest pain — M 14',tier:'intern',station:true,
  spec:'emergency',rt:'a 10-minute station',mins:10,
  who:'Tan · M 14', line:'chest pain, brought in by his father',
  place:'OSCE station · Nurse Mali marking',
  d:'Fourteen, on the trolley with chest pain. His father wants to know why a boy this age has chest pain at all.'},
 {id:'osce-b3',n:'OSCE B3',sn:'S1 · OSCE B3',t:'Barking cough — F 3',tier:'intern',station:true,
  spec:'paediatrics',rt:'a 10-minute station',mins:10,
  who:'Pim · F 3', line:'a barking cough, awake on her mother’s lap',
  place:'OSCE station · Nurse Mali marking',
  d:'A bark that woke the house, and a three-year-old awake on her mother’s lap. The rest of the visit is yours.'},
 {id:'ep3',n:'EP3',sn:'S1:E3',t:"Don't Make Him Cry",tier:'resident',rt:'a 14-minute shift',mins:14,
  art:'/img/ep3_khaopun.jpg',art2:'/img/ep3_khaopun_3x2.jpg',
  who:'Khaopun · M 5', line:'tripod, drooling, muffled voice, will not lie down',
  place:'Paediatric bay · evening',
  d:'Carried in by a grandfather you have met before. A five-year-old sitting very still — the stillness is the frightening part.'},
 {id:'osce-c',n:'OSCE C',sn:'S1 · OSCE C',t:'Barking cough and drooling, worse at night — F 6',tier:'resident',station:true,
  spec:'paediatrics',rt:'a 10-minute station',mins:10,
  who:'Fon · F 6', line:'barking cough, drooling on her mother’s shoulder, worse every night',
  place:'OSCE station · Nurse Mali marking, sixteen hours in',
  d:'Four nights of this, and her mother says tonight is the worst one. Six years old, awake in her mother’s arms, and out of patience with all of you.'},
 {id:'osce-c2',n:'OSCE C2',sn:'S1 · OSCE C2',t:'Wheeze and breathlessness — F 53',tier:'intern',station:true,
  spec:'emergency',rt:'a 10-minute station',mins:10,
  who:'Wasana · F 53', line:'breathless and wheezing, frightened',
  place:'OSCE station · Nurse Mali marking',
  d:'Fifty-three, wheezing, frightened, and out of her own answers.'},
 {id:'osce-c3',n:'OSCE C3',sn:'S1 · OSCE C3',t:'A week of cough — F 25',tier:'intern',station:true,
  spec:'emergency',rt:'a 10-minute station',mins:10,
  who:'Waen · F 25', line:'coughing, flushed, and breathing fast',
  place:'OSCE station · Nurse Mali marking',
  d:'Twenty-five, coughing, and she meant to ride the week out. Something last night finally walked her through the door.'},
 {id:'ep4',n:'EP4',sn:'S1:E4',t:'The Masquerader',tier:'resident',rt:'a 12-minute shift',mins:12,
  art:'/img/ep4_mali.jpg',art2:'/img/ep4_mali_3x2.jpg',
  who:'Mali · F 34', line:'breathless, pleuritic pain, looks anxious',
  place:'Emergency department · afternoon',
  d:'One of your own nurses, still in uniform. Everything looks normal — the oxygen is quietly low and the heart is too fast.'},
 {id:'osce-d',n:'OSCE D',sn:'S1 · OSCE D',t:'Vomited blood — M 62',tier:'intern',station:true,
  spec:'emergency',rt:'a 12-minute station',mins:12,
  who:'Somchai Jaiman · M 62', line:'vomited blood, still apologising for the mess',
  place:'OSCE station · Nurse Mali marking from a chair — doctor’s orders',
  d:'Sixty-two, on the trolley and apologetic about all of it. The room tipped over when he stood up this morning and put him down on his own bathroom floor.'},
 {id:'osce-d2',n:'OSCE D2',sn:'S1 · OSCE D2',t:'Sudden breathlessness, clear chest — F 55',tier:'resident',station:true,
  spec:'emergency',rt:'a 12-minute station',mins:12,
  who:'Somsri Jaidee · F 55', line:'breathless and frightened',
  place:'OSCE station · Nurse Mali marking',
  d:'Breathless since lunchtime and frightened by it. Fifty-five, and she walked into this bay on her own two feet. Nurse Mali is marking.'},
 {id:'osce-d3',n:'OSCE D3',sn:'S1 · OSCE D3',t:'Wheals, swollen lips and a wheeze — F 6',tier:'intern',station:true,
  spec:'paediatrics',rt:'a 10-minute station',mins:10,
  who:'Beam · F 6', line:'wheals up her neck and arms, lips swelling as you watch',
  place:'OSCE station · Nurse Mali marking',
  d:'Six years old on a trolley she is far too small for. Her teacher came in the ambulance and is doing all the talking.'},
 {id:'osce-d4',n:'OSCE D4',sn:'S1 · OSCE D4',t:'Fever, shaking, pressure of 80 — F 72',tier:'resident',station:true,
  spec:'emergency',rt:'a 14-minute station',mins:14,
  who:'Pranom · F 72', line:'drowsy and shaking, her niece answering for her',
  place:'OSCE station · Nurse Mali marking, the finale a week away',
  d:'Seventy-two, shaking under the blanket and too far away to give you much of the story herself. Her niece came in with her and has the rest of it.'},
 {id:'ep5',n:'EP5',sn:'S1:E5',t:'The Night the Stars Fell',tier:'resident',rt:'an 18-minute shift',mins:18,
  art:'/img/ep5_boonsong.jpg',art2:'/img/ep5_boonsong_3x2.jpg',
  who:'Boonsong · M 47', line:'the fireworks foreman — bleeding, chest not moving well, six more waiting',
  place:'Resus · mass casualty',
  d:'Everyone you have met, in one room, on the worst night of the year.'},
];
/* Which case the page is drawing, answered in one place.
   On the shelf an unknown id falls through to the first entry, which is right for a shelf: a
   stranger arriving with a stale link gets the season's opener rather than a blank screen. On a
   bed it is catastrophic, and it is what the founder was shown — a World-case patient wearing
   EP1's name, EP1's questions and EP1's pronouns, because a compiled case id is not in a table of
   the season's sixteen. `bay.js` ships to both hosts, so the table is here on the ward too; the
   rule is that a shift never asks it. The ward's card comes off the wire. */
function epOf(card, shelfId){ return card||SEASON.find(e=>e.id===shelfId)||SEASON[0]; }
const ep=()=>epOf(WARDSURFACE?WARDCARD:null, $('#ep').value);

/* An entry's art as a <picture>: a phone takes the 3:2 crop and never downloads the 16:9
   billboard, and an entry with only one file still works because <source> is optional.
   Everything below the fold is `loading="lazy"` — a shelf of five episodes plus twelve
   stations is a lot of megabytes to spend before the first card is even on screen. The
   billboard is the exception: it *is* the first screen, so it loads eagerly and says so,
   which is the difference between a hero that paints late and one that paints. */
function artOf(e,eager){
  if(!e||!e.art)return '';
  const src=e.art2?`<source media="(max-width:640px)" srcset="${e.art2}">`:'';
  const how=eager?'loading="eager" fetchpriority="high"':'loading="lazy"';
  return `<picture>${src}<img src="${e.art}" alt="" decoding="async" ${how}></picture>`;
}

/* ── B7 · "Previously on Vitals…" ─────────────────────────────────────────────
   Three frames, one line each, straight off the season arc (docs/internal/SEASON_ARC.md).
   EP2's recap wears EP1's real frames. The later ones now open on the key art of the
   episode they are remembering — the face the line names, and only that one: a still
   held three times in a row reads as a stuck projector, and the remaining lines (a cath
   lab, a radio channel, a factory running late) have no shot of their own, so those keep
   the dark memory-frame. A wrong face is still worse than no face. Phase 3 (narrative
   prose pass) owns this copy: polish the lines here and in STINGER, nowhere else. */
const RECAP={
 ep2:[['/img/critical.jpg','Ing, nineteen. The wrong salad, and her throat closing in minutes.'],
      ['/img/recovered.jpg','Adrenaline, first and fast. She walked out of this bay alive.'],
      ['/img/stable.jpg','\u201cOne day I\u2019m coming back here. To help.\u201d She meant it.']],
 ep3:[['/img/ep2_prasit.jpg','Prasit, 58. Thirty years driving the sick to this door — then the pain hit his jaw at the wheel.'],
      [null,'The ECG told the truth and the cath lab beat the clock. Time is muscle — you spent it well.'],
      [null,'He taped a radio channel to the triage desk: \u201cThe day you need a car — radio me.\u201d']],
 ep4:[[null,'Prasit, back through the doors at a run — his grandson in his arms. Khaopun, five years old, far too quiet.'],
      ['/img/ep3_khaopun.jpg','The boy would not cry, and the stillness was the emergency. You kept him calm; the airway held.'],
      [null,'His mother watched from the doorway in uniform — a nurse on this ward, sixteen hours into her own shift.']],
 ep5:[['/img/ep4_mali.jpg','Mali — Khaopun\u2019s mother. The nurse who held everyone else up, brought down by a clot grown on double shifts.'],
      [null,'The masquerader nearly took her. Heparin bought her back — but the system is stretched past its people.'],
      [null,'Tonight is the new year festival. The fireworks factory is running late.']],
};
/* ── B8 · stingers ── the black card after the outcome: one line that makes the next
   episode inevitable. Phase 3 polishes these together with RECAP above. */
const STINGER={
 ep1:'Ing goes home with a promise. Across town, a taxi driver rubs his chest and blames the traffic.',
 ep2:'\u201cThe day you need a car — radio me.\u201d He will be back sooner than that, carrying someone smaller.',
 ep3:'His mother finally exhales. Sixteen hours into her shift, her calf begins to ache.',
 ep4:'Mali is discharged in time for the new year. Nobody watches the fireworks factory after midnight.',
 ep5:'For the first time all night, the room is quiet. Everyone you ever saved is standing in it.',
};

/* ─── what you can do, by mode ─────────────────────────────────────────────── */
const MODES=[
 {id:'ask',  label:'ask',       icon:'💬'},
 {id:'exam', label:'examine',   icon:'🩺'},
 {id:'lab',  label:'lab',       icon:'🧪'},
 {id:'drug', label:'drugs',     icon:'💉'},
 {id:'proc', label:'procedure', icon:'🛠'},
 {id:'dx',   label:'diagnosis', icon:'🎯'},
];
const CHIPS={
 ep1:{ask:['what happened?','any allergies?','can you breathe?','when did it start?','do you have an epipen?','any medicines?'],
      exam:['look at the airway','listen to the chest','check the skin','check perfusion'],
      lab:['tryptase','blood gas','full blood count'],
      drug:['adrenaline im','oxygen','normal saline bolus','chlorpheniramine','hydrocortisone'],
      proc:['lay her flat','intubate, secure the airway','let her stand up'],
      dx:['anaphylaxis','admit for observation','discharge home']},
 ep2:{ask:['where is the pain?','does it go anywhere?','how long?','do you smoke?'],
      exam:['listen to the chest','check both arms','look at the neck veins'],
      lab:['ecg','troponin','chest x-ray'],
      drug:['aspirin','oxygen','heparin','nitrate'],
      proc:['activate the cath lab','defibrillate','thrombolysis'],
      dx:['anterior stemi','admit']},
 ep3:{ask:['how long has he been like this?','is he drinking?','any fever?','immunisations?'],
      exam:['look at him from the door','count the breathing','check the saturations'],
      lab:['blood gas','blood culture'],
      drug:['ceftriaxone','blow-by oxygen'],
      proc:['keep him calm','call ent and anaesthesia','secure the airway','look in the throat','iv access'],
      dx:['epiglottitis','admit to picu']},
 ep4:{ask:['when did it start?','any long flights?','are you on the pill?','any leg swelling?'],
      exam:['listen to the chest','check the calves','check the saturations'],
      lab:['d-dimer','ecg','chest x-ray','ctpa'],
      drug:['oxygen','heparin'],
      proc:['wells score','thrombolysis'],
      dx:['pulmonary embolism','admit','reassure and discharge']},
 /* Station chips. The asks are scripted interventions in the scenario — they route through the
    do path (fire() below), never /api/say: Step::Ask leaves no event on the tape, and an ask
    that leaves no event scores zero history marks. At a station, asking IS an action. */
 'osce-a':{ask:['any allergies?','what did you eat before this?','can you breathe all right?'],
      exam:['look at the skin','listen to the chest'],
      lab:['serum tryptase','12-lead ecg','full blood count','chest x-ray'],
      drug:['adrenaline im','oxygen mask','normal saline bolus','chlorpheniramine','hydrocortisone'],
      proc:['lay him flat, legs up','iv access'],
      dx:['anaphylaxis','vasovagal faint','acute urticaria only','septic shock','admit for observation','discharge home']},
 'osce-a2':{ask:['any allergies?','what did you eat today?','tell me about the diarrhoea','did you faint — even for a moment?'],
      exam:['look at the skin','listen to the chest'],
      lab:['serum tryptase','blood glucose','12-lead ecg'],
      drug:['adrenaline im','oxygen mask','normal saline bolus','chlorpheniramine','hydrocortisone'],
      proc:['lay her flat, legs up'],
      dx:['anaphylaxis','food poisoning','vasovagal syncope','acute gastritis','urinary sepsis','admit for observation','discharge home']},
 'osce-b':{ask:['where is the pain?','does it go anywhere?','any risk factors — smoking, sugar, pressure?'],
      exam:['listen to the heart','check the pulses'],
      lab:['12-lead ecg','troponin','chest x-ray'],
      drug:['aspirin 300 chewed','oxygen'],
      proc:['activate the cath lab','thrombolysis','iv access'],
      dx:['acute stemi','pericarditis','aortic dissection','oesophageal spasm','admit','reassure and discharge']},
 'osce-b2':{ask:['where is the pain — what makes it better?','does breathing change it?','any fever or a cold lately?'],
      exam:['listen to the heart — sit him forward'],
      lab:['12-lead ecg','troponin','echocardiogram','chest x-ray'],
      drug:['ibuprofen with food','colchicine','aspirin 300 chewed'],
      proc:['activate the cath lab','thrombolysis','pericardiocentesis'],
      dx:['pericarditis','acute stemi','myocarditis','pulmonary embolism','spontaneous pneumothorax','admit for observation']},
 'osce-b3':{ask:['when did the bark start?','any fever?','is she drinking?'],
      exam:['score her from the doorway','check the saturations','listen to the chest'],
      lab:['neck and chest films'],
      drug:['dexamethasone syrup','nebulised adrenaline','amoxicillin'],
      proc:['keep her on mum’s lap','watch her for an hour','give the safety-net advice'],
      dx:['croup','epiglottitis','bacterial tracheitis','bronchiolitis','inhaled foreign body','send her home']},
 'osce-c':{ask:['has she had this before?','any fever?','are her shots up to date?','when is it worse?'],
      exam:['score her from the doorway','check the saturations','listen to the chest'],
      lab:['neck and chest films'],
      drug:['dexamethasone syrup','nebulised adrenaline'],
      proc:['keep her on mum’s lap','watch her for two hours','look in the throat','a drip and bloods'],
      dx:['croup','epiglottitis','retropharyngeal abscess','bacterial tracheitis','inhaled foreign body']},
 'osce-c2':{ask:['how often does this happen?','can you finish a sentence?'],
      exam:['listen to the chest'],
      lab:['peak flow','chest x-ray'],
      drug:['salbutamol neb','ipratropium','prednisolone 40 mg','oxygen','inhaled steroid','diazepam'],
      proc:['reassess — peak flow again'],
      dx:['acute asthma exacerbation','pulmonary oedema','vocal cord dysfunction','pneumonia','discharge home']},
 'osce-c3':{ask:['tell me about the cough','any illnesses? do you smoke?'],
      exam:['listen to the chest','count the breathing'],
      lab:['chest x-ray','full blood count','sputum and blood cultures'],
      drug:['co-amoxiclav plus macrolide','oxygen','paracetamol'],
      proc:['curb-65','admit to a short-stay bed','send her home with tablets'],
      dx:['pneumonia','acute bronchitis','pulmonary embolism','pulmonary tuberculosis']},
 'osce-d':{ask:['what pills do you take every day?','how much blood — what colour?','any dizziness standing up?'],
      exam:['feel his hands, look at his eyes','press on the belly','rectal exam'],
      lab:['group and crossmatch four units','full blood count','coagulogram'],
      drug:['warmed crystalloid, wide open','transfuse packed cells','pantoprazole bolus and infusion','aspirin 300 chewed'],
      proc:['two large-bore lines','hold the aspirin and clopidogrel','call gi — endoscopy'],
      dx:['upper gi bleed','acute coronary syndrome','acute pancreatitis','perforated viscus','ruptured aortic aneurysm']},
 'osce-d2':{ask:['what were you doing when it started?','any illnesses — any tablets?','how are your legs?'],
      exam:['examine the calves','listen to the chest'],
      lab:['d-dimer','12-lead ecg','chest x-ray','ct pulmonary angiogram'],
      drug:['oxygen','low-molecular-weight heparin','thrombolysis'],
      proc:['wells score','500 ml of fluid'],
      dx:['pulmonary embolism','anxiety attack','pneumonia','pneumothorax','acute coronary syndrome','admit to the unit']},
 'osce-d3':{ask:['how much does she weigh?','what did she eat?','any known allergies?'],
      exam:['look at the skin','listen to the chest'],
      drug:['adrenaline 0.2 mg im','adrenaline 0.5 mg im','oxygen','saline 20 ml/kg','chlorpheniramine'],
      proc:['admit for observation'],
      dx:['anaphylaxis','acute asthma','vasovagal syncope','septic shock','send her home']},
 'osce-d4':{ask:['ask the niece what happened'],
      exam:['feel the skin — perfusion','press the right loin'],
      lab:['lactate','two sets of blood cultures','urinalysis'],
      drug:['broad-spectrum antibiotics','warmed crystalloid 30 ml/kg','noradrenaline','oxygen'],
      proc:['two large-bore lines','urinary catheter','call urology','icu bed'],
      dx:['septic shock','cardiogenic shock','hypovolaemic shock','diabetic ketoacidosis']},
 ep5:{ask:['what happened?','where does it hurt?','can you feel your legs?'],
      exam:['primary survey','check the chest','check for bleeding'],
      lab:['blood gas','crossmatch'],
      drug:['transfuse','two litres of saline'],
      proc:['triage','tourniquet','needle decompression','intubate','damage control'],
      dx:['haemorrhagic shock','to theatre']},
};
/* ── every option in a tab is written at the same level ───────────────────────
   A tray of orders is a multiple choice whether it was meant to be one or not, and a
   candidate reads it the way anybody reads a multiple choice: the option written differently
   from its neighbours is the one the writer was thinking about. Half of these tabs told you
   which button to press without knowing a thing about the patient.

   Three shapes of tell, and all three were taken off the twenty-four buttons in the table
   above:

     * the arithmetic printed on the button, on a station that marks the arithmetic. The two
       doses stay — they are the choice — and the working-out comes off;
     * a label that argues for or against its own option, so the differential reads as four
       diagnoses and one editorial;
     * the tail nobody else in the tab has. Length is a tell on its own: on a list of five
       bare nouns the phrase with a clause after it is the one somebody thought hardest about.

   What is left on every button is what a drug chart or a differential actually contains — a
   name, a dose, a route — written the same way for the option that is right and the option
   that is wrong, so that reading the tray tells you nothing that reading the patient would not.
   The two doses on osce-d3 are the exception that proves it: they stay because they *are* the
   question that station asks.

   The coat came off. This was first shipped as a second table — the tray drew a clean label
   over a `data-x` that still carried the tell — on the reasoning that leaving `data-x` alone
   kept every anchored tape replaying. It did, and it also left all twenty-four originals in
   the DOM, one right-click from view-source and one line from devtools: the tray was clean to
   a candidate reading it and an answer key to a candidate inspecting it. An exam seal that a
   browser's own menu walks around is not one. (No example is quoted here, for the same reason:
   this comment is served with the page, so a worked example of the tell would be the tell.)

   So the clean words *are* the `data-x` now, and the second table is gone. What that costs is
   named rather than assumed: the text a chip fires is what lands on the tape and what feeds
   `vitals_replay::leaf`, so a run played after this writes a different leaf from the same run
   played before it. That is a new run, not a broken one. Nothing already anchored moves —
   demo/ is untouched, the matcher is untouched, and a verifier replaying an old tape resolves
   its old wording to exactly the intervention it always did.

   And what the mark sheet sees does not move at all, in either direction: the event log records
   the *intervention id* the matcher resolved, never the words that reached it, so scoring reads
   nothing this table ever touched. Every one of the twenty-four was put through its own
   station's matcher, before and after, and resolved to the identical id; the twelve competent
   tapes re-marked to the same 40/40, the same zero penalty, and the same outcome.

   The ask row is not in here and never was: it is translated, and `PACK.asks` still wins over
   the button's own text in `chipLabel`. */

/* ─── who the patient is ──────────────────────────────────────────────────────
   The season is not one woman. Seven of its seventeen patients are men — a seventy-one-year-old
   whose face is still swelling, a sixty-two-year-old vomiting blood, a five-year-old who will not
   lie down — and the event log used to call every one of them "she", because these lines were
   written for EP1 and never asked who was on the trolley. It is not only the log: the same table
   writes the outcome line in the result panel, so a man who survived was told "she goes home" on
   the one frame that carries the hash, the mark sheet and the anchor.

   `who` is the authority — "Name · SEX AGE", e.g. "Somchai · M 71" — because it is the same string
   the card prints and the bay captions, so the pronoun can never disagree with the name beside it.
   Two fields and it stops: this file is served to the candidate, so a third field in a `who`
   string — or in an example of one written down here — is a mark sheet's hidden number published
   in view-source. `osce-d3` pays three points for asking the weight and six more for the dose
   drawn off it, and both were readable out of this comment before it said so.
   Presentation only: no beat key, no chip, no order and no tape entry changes with it. */
/* A pronoun at the start of a sentence. Here rather than inline because the ward says several. */
const Cap=s=>String(s).charAt(0).toUpperCase()+String(s).slice(1);
const PRO_F={s:'she',o:'her',p:'her'}, PRO_M={s:'he',o:'him',p:'his'},
      PRO_N={s:'the patient',o:'the patient',p:"the patient's"};
/* A `who` string that names neither M nor F gets `they`, not a coin flip weighted to EP1.

   The default used to be PRO_F, which is a guess that is right about ten of seventeen patients
   and silently wrong about the other seven — and wrong in the way that is hardest to see, because
   a failure to match reads on screen exactly like a woman. A card added without a sex marker, a
   `who` string edited into another shape, a case reached before SEASON loads: every one of those
   printed "she goes home" over whoever was on the trolley and looked completely normal doing it.
   "the patient" is not a claim about anybody, so a miss now looks like a miss — and it stays
   grammatical, which bare "they" would not: thirteen lines in the table below read "{s} is
   holding", "{s} goes home", "{s} dies", and singular they turns every one of them into a typo on
   the frame that carries the hash. The server takes the same way out in patient.rs `pronouns()`,
   which answers an unknown sex with "this patient" rather than guessing.

   No shipped case reaches this branch — `every_case_on_the_shelf_states_a_sex` in tests/page.rs
   is what keeps it that way, so the fallback is a smoke alarm and not a design. */
const pro=e=>{
  const who=((e||ep()||{}).who)||'';
  if(/·\s*M\b/.test(who))return PRO_M;
  if(/·\s*F\b/.test(who))return PRO_F;
  return PRO_N;
};
/* How old the patient is, read off the same `who` string the bed label prints — "Name · SEX AGE",
   e.g. "Somchai · M 71". The monitor needs it because alarm limits are a function of age and it
   had only the adult set: a three-year-old at 118 and 28 is a normal three-year-old, and the
   screen alarmed on her from the first second. `null` when the string does not say, which the
   monitor reads as "use the adult limits" — the behaviour it had before it was told anything. */
const ageOf=e=>{ const m=/·\s*[MF]\s*(\d{1,3})\b/.exec(((e||ep()||{}).who)||''); return m?+m[1]:null; };
/* Which bed this is and which unit it is in, for the monitor's banner — it printed a hard-coded
   "BED 3 · ER" over every case in the season, including the paediatric bay in EP3 and twelve OSCE
   stations that are not bed 3 and not the ER. A station wears its own name. */
const bedOf=e=>{
  e=e||ep()||{};
  const p=(e.place||'').toLowerCase();
  const unit = e.spec==='paediatrics'||p.includes('paediatric') ? 'PAEDS'
             : p.includes('resus')  ? 'RESUS'
             : p.includes('opd')||p.includes('clinic') ? 'OPD'
             : 'ER';
  /* On the ward it is a ward bed. Which number is the board's to say and the payload does not
     carry it yet; "BED 3" over every patient on a public ward is the hard-coded lie this whole
     function exists to undo. */
  return (REVIEW ? 'REVIEW' : WARD ? 'WARD' : e.station ? (e.n||'OSCE') : 'BED 3')+' · '+unit;
};
/* `{s}` subject, `{o}` object, `{p}` possessive. A line with no placeholder is a line that never
   needed one — "the reaction comes back" is true whoever it comes back in. */
const fillPro=(t,e)=>{ const g=pro(e);
  return typeof t==='string' ? t.replace(/\{([sop])\}/g,(_,k)=>g[k]) : t; };

/* ─── the engine speaks in beats; a person hears a story ────────────────────── */
const SAY={
 "status:Stable":"{s} is holding","status:Deteriorating":"{s} is getting worse",
 "status:Critical":"{s} is crashing","status:Improving":"{s} is turning around",
 "status:Recovered":"{s} settles","status:Arrest":"{s} arrests","status:Dead":"{s} is gone",
 "threshold:biphasic":"the reaction comes back","threshold:going_quiet":"{s} goes quiet — that is worse",
 "threshold:stemi_recognised":"the ECG tells the truth","threshold:collapse":"{s} collapses",
 "threshold:code_blue":"the monitor alarms — code blue","threshold:rosc":"a rhythm comes back",
 /* Written by the engine, not by a case, and identical whatever the shock did — the rhythm it
    went into and what happened next are on the chart and on the strip. A line that only appeared
    when the shock was right would be the answer key. */
 "threshold:shock":"the shock goes in",
 "terminal:WinDischarge":"{s} goes home","terminal:WinIcu":"{s} lives — ICU",
 "terminal:DeathArrest":"{s} dies","terminal:DeathBiphasic":"{s} dies at home",
 /* Not a beat. `render_beat` cannot produce this string and the engine never emits it — the
    run simply has no terminal, and this is what the panel says instead of leaving its
    headline empty. It is here beside the four real endings so the wording is read against
    them: time was called, and nothing is being claimed about how she ended up. */
 "terminal:TimeCalled":"time — the station ends"
};
/* The line a beat is read in: the run's own translation first, then the page's English, then the
   raw beat with its kind stripped. Three fallbacks deep and none of them can be empty, because a
   case with no translation must read exactly as it did before any of this existed. */
const say=b=>fillPro(TR[b] || SAY[b] || (b.startsWith("harm:") ? "harm — "+b.slice(5) : b.replace(/^[a-z]+:/,"")));

/* ─── the language the patient speaks ─────────────────────────────────────────
   Presentation, all of it. Nothing below reaches the tape, the leaf or the mark sheet: the chips
   keep firing the English phrase they always fired, the beats keep their canonical form for
   everything the page *thinks* with (which cutscene to roll, which line is a harm, which one to
   unseal), and only the words a human reads change. `docs/internal/LANGUAGE_LAYER.md` is the
   argument in full; the tests in main.rs are the argument as an assertion.

   The server owns the tables — one file to edit to add Bahasa Indonesia — so the page holds no
   list of languages of its own. Until /api/lang answers, LANG is empty and every request simply
   omits the parameter, which the API reads as "the language the cases are written in". */
let LANG='', LANGS=[], PACK={asks:{},ui:{},kit:{}};
/* Beat → the line to read it in, for this run. Filled from each view, which carries only the
   beats the run has already earned — never the ones it has not, because a harm line names the
   drug and the deadline the rubric is about to pay for. */
let TR={};
const langQ=()=>LANG?'&lang='+encodeURIComponent(LANG):'';

function renderLangPickers(){
  for(const sel of ['#lang-lb','#lang-gm']){
    const n=$(sel); if(!n) continue;
    n.innerHTML=LANGS.map(l=>`<option value="${l.id}">${l.native}</option>`).join('');
    n.value=LANG;
    n.onchange=()=>setLang(n.value,true);
  }
}
/* Switching is a fetch and a repaint, never a reload: a case in progress is a patient in
   progress, and dropping a live run to change a label would be its own kind of harm. Lines
   already in the transcript keep the words they were said in — a transcript is a record of what
   was said — and everything from here on arrives in the new language. */
async function setLang(id,remember){
  LANG=id||'';
  if(remember) { try{localStorage.setItem('vitals.lang',LANG)}catch(e){} }
  PACK={asks:{},ui:{},kit:{}};
  try{
    const d=await (await fetch('/api/lang'+(LANG?'?lang='+encodeURIComponent(LANG):''))).json();
    if(d.languages&&d.languages.length) LANGS=d.languages;
    PACK={asks:d.asks||{},ui:d.ui||{},kit:d.kit||{}};
    if(d.lang) LANG=d.lang;
  }catch(e){ /* no pack is the English pack: every string below falls back to the original */ }
  /* The two beat seals, stand-ins the exam draws over words it will not say yet: a seal that
     falls back to English on a Thai bedside announces itself as a seal. The fallback stays
     anyway — a build served with no pack at all still has to seal. There used to be a third,
     `harm_sealed`; it no longer has a line of its own to draw itself over. */
  BEAT_DECLINED = PACK.ui.beat_declined || 'the order is declined';
  BEAT_NOTED = PACK.ui.beat_noted || 'noted on the record';
  TR={};
  renderLangPickers();
  if($('#chips')) renderChips();
}
/* On boot: what you chose last, else what your browser is already set to if we speak it, else
   the language the cases are written in. A browser default is not a choice, so it is not written
   to storage — pick from the menu once and that sticks instead. */
async function bootLang(){
  let want=null; try{want=localStorage.getItem('vitals.lang')}catch(e){}
  if(!want){
    try{
      const d=await (await fetch('/api/lang')).json();
      if(d.languages) LANGS=d.languages;
    }catch(e){}
    const nav=(navigator.language||'').slice(0,2).toLowerCase();
    if(LANGS.some(l=>l.id===nav)) want=nav;
  }
  await setLang(want||'', false);
}

/* A harm beat is written to teach: one line naming the drug that was not given, the disease it
   treats and the deadline that has just gone past, fired at the exact moment the candidate has
   not thought of it. In practice that is the whole point.
   During an exam it is a coach leaning over the desk — the rubric is at that moment paying for
   the very thing the beat just named. No example is written out here: the page is served to the
   candidate, so a quoted harm line in this comment is the same leak one file earlier.
   So: while a station is running the sentence is not said, and the patient shows you the rest —
   which is the signal an examiner would leave in the room. The full sentence is not deleted, it
   is deferred: the debrief prints every harm in full for both modes, because
   feedback after the bell is what an exam is for. `over` and `v.outcome` are both consulted so
   the last paint of a run — the one that draws the chart under the result panel — is already
   the after picture.
   The scenario files themselves are never touched: demo/ is the case, and a case must read the
   same to the engine, the replayer and the chain no matter who is looking at the screen. */

/* ── the same rule, one step further: an exam does not tutor ──────────────────
   A beat is the case talking back, and most of what it says is observation — what the film
   showed, what the drug did, what the mother answered. Two kinds of talking back are not
   observation but marking:

     * the order the case declined, whose sentence explains the gate it failed and what has to
       happen first;
     * the reply to a differential the candidate named, which is the reasoning that settles it.

   Under exam, before the bell, both are facts without their sentence: the order was declined,
   or the answer is on the record. Deferred, never deleted — `unsealBeats` rewrites every one of
   them the moment the case ends, so the transcript a candidate reads afterwards is the whole
   thing, in order, with the teaching in it. That is what a debrief is.

   Note what is *not* held: a status change, a threshold the case named, the harm line (which
   has its own seal, on the server) and every beat in practice mode. And nothing here touches
   demo/ — the case says the same words to the engine, the replayer and the chain as it always
   did; this is the last step before the screen, exactly like the chart's own translation. */
let BEAT_DECLINED='the order is declined';
let BEAT_NOTED='noted on the record';

/* Why this patient is kept in — the line under the disposition box. Practice only, and only
   where it is true: the box is on screen for all seventeen cases and the reason is not. */
const DISPO_WHY={ep1:'biphasic reactions come back hours later'};

/* What the case actually was — printed on the debrief, next to the provenance line, and
   nowhere else. The station title is a stem now (SEASON above), which is the only way a mark
   sheet that pays for "named the diagnosis" can mean anything; this is where the candidate
   finally gets told, at the one moment it costs nothing. Episodes are not listed: their titles
   are drama, not diagnosis, and they give nothing away. */
const REVEAL={
 'osce-a':'anaphylaxis',
 'osce-a2':'anaphylaxis, arriving in a gastrointestinal disguise',
 'osce-b':'acute ST-elevation myocardial infarction',
 'osce-b2':'acute pericarditis',
 'osce-b3':'croup — mild',
 'osce-c':'croup — not epiglottitis',
 'osce-c2':'acute asthma exacerbation',
 'osce-c3':'community-acquired pneumonia, right lower lobe',
 'osce-d':'upper gastrointestinal bleeding',
 'osce-d2':'pulmonary embolism',
 'osce-d3':'anaphylaxis, paediatric',
 'osce-d4':'septic shock from an obstructed, infected kidney',
};

let id=null,timer=null,over=false,seen=0,mode='ask',asking=false,hard=false;
/* How far into the chart the feed has already looked — `seen`'s opposite number, and reset in
   exactly the same places. Only drainBeats moves it. */
let CHARTSEEN=0;
/* ── the sequence `seen` walks ───────────────────────────────────────────────
   The same list on both sides of the bell, so the bell stops being a moment when the array a
   counter is counting silently changes shape underneath it. One monotonic list, one watermark.
   What the exam seal covers, and why it holds the way it does, is docs/RISKS.md §11.

   Only under exam. A practice episode is never sealed at either end — the server sends the harm
   beat the moment it happens and the feed prints it there, because that line is the lesson and a
   coach who will not say what went wrong is not coaching. The filter would have swallowed it and
   handed it back at the bell, which is the seal leaking *into* practice by the back door. */
const visBeats=v=>examMode()?(v.beats||[]).filter(b=>!b.startsWith('harm:')):(v.beats||[]);
/* Where each paint left the feed. Pushed once per drainBeats and read only by unsealHarm, which
   has to put back lines that were never written — so it needs to know which paint they belonged
   to, and that is a fact about this page's own history, not about the case.

   `vis` and `chart` are the two watermarks the paint ended on and `clock` its scenario time;
   between them they pin a harm to one paint. An *ordered* harm is preceded in the chart by the
   order that caused it, so `chart` jumps in the paint that carries it and nothing earlier can
   claim it — which matters because an order does not advance the clock and the tick before it
   reads the same second. A *clock-fired* harm moves no chart row, and there `clock` is what
   separates one tick from the next. `tail` is the node to hang it off when the paint that owned
   it wrote no beat of its own. */
let PAINTS=[];
const fmt=s=>`${Math.floor(s/60)}:${String(Math.round(s%60)).padStart(2,'0')}`;

let clockNow=0;
function wake(){ const c=$('#chat'); c.classList.remove('empty'); const e=c.querySelector('.empty-in'); if(e)e.remove(); }
/* Speech — hers or yours. A bubble, because someone said it. */
function turn(cls,who,text,raw){
  wake();
  const d=document.createElement('div'); d.className='turn '+cls;
  d.innerHTML=(who?`<span class="who">${who}</span>`:'')+`<span class="say"></span>`;
  d.querySelector('.say').textContent=text; if(raw)d.title=raw;
  $('#chat').appendChild(d); $('#chat').scrollTop=1e6; return d;
}
/* Everything that is not speech — an order you gave, a beat the patient produced, the end.
   A line with a time on it, because that is how it will read in the chart afterwards. */
function ev(kind,icon,text,raw){
  wake();
  const d=document.createElement('div'); d.className='ev '+kind;
  d.innerHTML=`<span class="t">${fmt(clockNow)}</span><span class="ic">${icon}</span>`+
              `<span class="tx"></span><span class="rule"></span>`;
  d.querySelector('.tx').textContent=text; if(raw)d.title=raw;
  $('#chat').appendChild(d); $('#chat').scrollTop=1e6; return d;
}
function emptyState(){
  const c=$('#chat'); c.innerHTML=''; c.classList.add('empty');
  const E=ep();
  c.innerHTML=`<div class="empty-in"><b>${E.who}</b>${E.line}.<br>
    Ask ${pro(E).o} something, or start treating. The clock is the patient — ${pro(E).s} does not wait.</div>`;
}
/* ─── the kit ────────────────────────────────────────────────────────────────
   Embla's device tray, not a row of buttons: every item carries what it actually is,
   what the flowmeter reads, whether it can come off again, and whether it should ask
   first. 999 mL/hr is not a bug — it is an IV run wide open, and it is one of the
   presets a learner picks from. */
const KIT=[
 /* Presets, not a slider, and this is the one that has to be read carefully: 2 and 4 are a nasal
    cannula, 6 is a simple mask, 10 and 15 are a non-rebreather. They are the settings a hand
    actually reaches for — a 0–15 slider in steps of one invites a learner to choose 7 for no
    reason, and "7 L/min" is not a thing anybody hangs on a wall. The detail line is the mapping
    itself, because the flowmeter is where that lesson lands. Ported from Embla, where the same
    five have been in front of candidates. */
 {id:'o2',  label:'Oxygen', device:'O₂', detail:'cannula 2–4 · mask 6–8 · NRB 10–15', unit:'L/min', presets:[2,4,6,10,15], def:10,
  detach:true,  group:'breathing'},
 {id:'iv',  label:'IV + fluid',   device:'IV · 0.9% NaCl', detail:'0.9% NaCl', unit:'mL/hr', presets:[80,125,500,999], def:999,
  detach:true,  group:'circulation'},
 {id:'ett', label:'Intubate',     device:'ETT 7.0 cuffed', detail:'ETT 7.0 · cuffed', unit:null, presets:[], def:null,
  detach:true,  group:'airway',      confirm:'Intubate now?'},
 {id:'supine', label:'Lay flat, legs up', device:'positioned supine', detail:'once, not a device', unit:null, presets:[], def:null,
  detach:false, group:'circulation'},
 {id:'defib', label:'Defibrillate', device:'defibrillator', detail:'VF / pulseless VT only', unit:'J', presets:[120,150,200,360], def:200,
  detach:false, group:'circulation', confirm:'Deliver the shock?'},
];
const KITLBL=Object.fromEntries(KIT.map(k=>[k.id,k]));
/* The tray's words in the language the bay is being played in — and only its words. The id, the
   setting and the phrase the server mints from them are untouched, so this relabels exactly the
   way a chip relabels: two learners who attach the same device at the same number write the same
   tape whichever language they read it in. No row, or English, gives the original back. */
const kitL=(k,f)=>PACK.kit[k.id+'.'+f]||k[f];
let picking=null;

/* Simple line art, drawn rather than photographed: a learner should recognise the thing on
   the trolley, and a stock photo of a mask teaches nothing about the flowmeter.
   One grammar for all five: 90×104, stroke 2.2 on currentColor, fills only from the tokens
   below, and never more than four parts — at 38px a fifth part turns the icon to mush. */
const ART={
 o2:`<svg viewBox="0 0 90 104" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
   <path d="M28 20h34a6 6 0 0 1 6 6v18c0 13-10.5 23-23 23S22 57 22 44V26a6 6 0 0 1 6-6z" fill="#EAF3F1"/>
   <path d="M22 27H9M68 27h13"/>
   <circle cx="45" cy="40" r="4.5" fill="#2A9D7E" stroke="none"/>
   <path d="M45 67v6"/>
   <path d="M45 73c-9 0-14 6-14 13s6 13 14 13 14-6 14-13-5-13-14-13z" fill="#DCEDE7"/></svg>`,
 iv:`<svg viewBox="0 0 90 104" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
   <path d="M45 6v7"/><path d="M38 8a7 7 0 0 1 14 0"/>
   <path d="M32 13h26a3 3 0 0 1 3 3.3l-3.4 34A11 11 0 0 1 46.7 60h-3.4a11 11 0 0 1-10.9-9.7L29 16.3A3 3 0 0 1 32 13z" fill="#E7F1F6"/>
   <path d="M35 25h20"/><path d="M45 60v5"/>
   <rect x="39" y="65" width="12" height="15" rx="3" fill="#EAF3F1"/>
   <circle cx="45" cy="71" r="2.2" fill="#2A9D7E" stroke="none"/>
   <path d="M45 80v6c0 9-14 8-14 17"/>
   <rect x="38.5" y="88" width="8" height="7" rx="2.5" fill="#DCEDE7"/></svg>`,
 ett:`<svg viewBox="0 0 90 104" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
   <rect x="30" y="8" width="30" height="16" rx="4" fill="#DCEDE7"/>
   <path d="M36 24v34c0 16 4 26 14 32l10-8c-6-5-6-12-6-24V24"/>
   <ellipse cx="45" cy="62" rx="16" ry="10" fill="#EAF3F1"/>
   <circle cx="53" cy="83" r="2" fill="currentColor" stroke="none"/></svg>`,
 supine:`<svg viewBox="0 0 90 104" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
   <path d="M58 72l22-18v18z" fill="#DCEDE7"/>
   <path d="M6 72h76"/><path d="M12 72v13M76 72v13"/>
   <circle cx="19" cy="62" r="7" fill="#EAF3F1"/>
   <path d="M26 65h24l10-3"/><path d="M60 62l16-11"/>
   <path d="M30 66c5 5 12 5 16 1"/></svg>`,
 defib:`<svg viewBox="0 0 90 104" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round">
   <rect x="10" y="22" width="27" height="21" rx="6" fill="#FBE8E6"/>
   <rect x="53" y="61" width="27" height="21" rx="6" fill="#FBE8E6"/>
   <path d="M23.5 22V13c0-4 4-5 7-4"/><path d="M66.5 82v9c0 4-4 5-7 4"/>
   <path d="M50 34l-9 15h8l-9 16" stroke="#C2453B"/></svg>`,
};

/* The catalogue, grouped the way a resuscitation is taught. */
const GROUPS=[['A · airway','airway'],['B · breathing','breathing'],['C · circulation','circulation']];

function closeSheet(){ $('#veil').hidden=true; $('#sheet').innerHTML=''; }
$('#veil').addEventListener('click',e=>{ if(e.target===$('#veil')) closeSheet(); });
addEventListener('keydown',e=>{ if(e.key==='Escape'&&!$('#veil').hidden) closeSheet(); });

let kitNow=[];
function renderTray(kitOn){ kitNow=kitOn||[]; }

/* step 1 — which device */
function openPicker(){
  const on=new Set(kitNow.map(k=>k.id));
  $('#sheet').innerHTML=`
    <div class="sheet-h"><b>Choose a device</b><span class="sp"></span>
      <button class="btn" data-close data-ga="sheet-close">close</button></div>
    <div class="sheet-b">`+GROUPS.map(([title,g])=>{
      const rows=KIT.filter(k=>k.group===g); if(!rows.length)return '';
      return `<div><div class="grp-h">${title}</div>`+rows.map(k=>`
        <button class="dev-row" data-dev="${k.id}" data-ga="dev:${k.id}">
          <span><span class="n">${kitL(k,'label')}</span><span class="d">${kitL(k,'detail')}</span></span>
          ${on.has(k.id)?'<span class="on">✓ on</span>':''}
        </button>`).join('')+`</div>`;
    }).join('')+`</div>`;
  $('#veil').hidden=false;
  $('#sheet').querySelector('[data-close]').onclick=closeSheet;
  // Already-on is shown, not hidden — hiding it makes a learner hunt for something they did.
  $('#sheet').querySelectorAll('[data-dev]').forEach(b=>b.onclick=()=>openSetting(b.dataset.dev));
}

/* step 2 — set it, or take it off */
function openSetting(devId){
  const k=KITLBL[devId];
  const cur=kitNow.find(x=>x.id===devId);
  const mins=cur?Math.max(0,Math.floor((clockNow-cur.since)/60)):null;
  const val=cur&&cur.setting!=null?cur.setting:k.def;
  $('#sheet').innerHTML=`
    <div class="sheet-h"><b>${kitL(k,'label')}</b><span class="sp"></span>
      <button class="btn" data-back data-ga="sheet-back">← back</button></div>
    <div class="sheet-b">
      <div class="set-top">
        ${ART[devId]?`<div class="art">${ART[devId]}</div>`:''}
        <div class="meta"><div class="n">${kitL(k,'label')}</div><div class="d">${kitL(k,'detail')}</div>
          ${cur?`<div class="since">on for ${mins} min${cur.setting!=null?` · ${cur.setting} ${k.unit}`:''}</div>`:''}
        </div>
      </div>
      ${k.presets.length?`<div><div class="grp-h">${k.unit}</div><div class="presets">`+
        k.presets.map(v=>`<button class="btn${v===val?' on':''}" data-v="${v}" data-ga="set:${devId}:${v}">${v}${v===999?' · wide open':''}</button>`).join('')+
        `</div></div>`:''}
    </div>
    <div class="sheet-f">
      ${cur&&k.detach?`<button class="btn danger" data-off data-ga="sheet-takeoff">take it off</button>`:''}
      <span class="sp"></span>
      <button class="btn go" data-apply data-ga="sheet-apply">${cur?'apply':'attach'}</button>
    </div>`;
  let chosen=val;
  const S=$('#sheet');
  S.querySelector('[data-back]').onclick=openPicker;
  S.querySelectorAll('[data-v]').forEach(b=>b.onclick=()=>{
    chosen=+b.dataset.v; S.querySelectorAll('[data-v]').forEach(x=>x.classList.toggle('on',x===b)); });
  const off=S.querySelector('[data-off]'); if(off) off.onclick=()=>{ closeSheet(); detach(devId); };
  S.querySelector('[data-apply]').onclick=()=>{ closeSheet(); commit(k, k.presets.length?chosen:null); };
}

async function commit(k, value){
  // Support that can only ever be added teaches that nothing you attach can be got wrong, so the
  // two that cannot be undone ask first.
  if(k.confirm && !confirm(kitL(k,'confirm'))) return;
  const v=await (await fetch(`/api/kit?id=${id}&dev=${k.id}`+(value!=null?`&set=${value}`:'')+asMe()+langQ())).json();
  if(v.error) return ev('note','—',v.error);
  ev('order','▸', kitL(k,'label')+(value!=null?` · ${value} ${k.unit}`:''));
  paint(v);
}
async function detach(devId){
  const v=await (await fetch(`/api/kit?id=${id}&dev=${devId}&off=1`+asMe()+langQ())).json();
  if(v.error) return; ev('order','✕','remove '+kitL(KITLBL[devId],'label')); paint(v);
}

/* ─── film ──────────────────────────────────────────────────────────────────
   Story Mode does two things with video and I had done neither: it loops a clip per
   clinical state — and the loop changes again when equipment goes on, so the mask is
   visible — and it cuts to a full-frame cutscene on a beat, then returns to the loop.
   The Director's priority is terminal > harm > biphasic > status change, ported as-is. */
const CUT={ cold_open:'ep1_cold_open', trigger:'ep1_trigger', harm:'ep1_harm', biphasic:'ep1_biphasic', code_blue:'ep1_code_blue',
  deteriorate:'ep1_deteriorate', adrenaline_calm:'ep1_adrenaline_calm',
  WinDischarge:'ep1_win_discharge', WinIcu:'ep1_win_icu',
  DeathArrest:'ep1_death_arrest', DeathBiphasic:'ep1_death_biphasic' };
const LOOPS=new Set(['deteriorating','critical','improving','recovered','arrest']);
let loopNow='', cutting=false, film=true, lastShown='stable';
/* Clip names that came back 404, so each one is asked for once and no more. Declared here with
   the rest of the film state rather than beside the handler that fills it: `show()` reads it,
   and a `const` read above its own declaration line only works by accident of call order. */
const missing=new Set();

/* Only EP1 was filmed with a clip per piece of equipment on the patient. The others are built
   from their key art, so asking for a `_o2` variant they do not have would cost a 404 and the
   loop for the rest of that state. */
const KIT_VARIANTS=new Set(['ep1']);
function stateClip(status,kit,epId){
  const s=(status||'stable').toLowerCase();
  if(!LOOPS.has(s)) return null;
  const on=new Set(kit.map(k=>k.id));
  const suffix = KIT_VARIANTS.has(epId)
    ? (on.has('ett') && s==='deteriorating' ? '_ett' : on.has('o2') ? '_o2' : '') : '';
  return epId+'_state_'+s+suffix;
}
/* How far down the room has gone, for an episode whose one frame cannot go anywhere. Only
   the states EP1 has a different still for: everything else is the shot as it was taken. */
const STILLGRADE={deteriorating:'worse',critical:'worse',arrest:'gone',dead:'gone'};
let stillNow='/img/stable.jpg', stillKey=false;

/* ── the station's own patient ────────────────────────────────────────────────
   What EP1 has had since the first build: a frame of *this* patient, in this bay, that
   changes as she does. A station had a stem and then whatever film was ordered, and the
   patient herself was two lines of text under an empty frame.

   The files are `/img/cases/states/<station>_<state>.jpg`, shot per station by the same
   Embla pipeline that shot EP1, and the server reads its own disk for them: the set table
   carries `states` — the list of shots that actually exist for that station today. So this
   never asks for a picture that is not there, and a station whose art has not landed yet
   keeps its stem, which is the whole reason the stem was put on the stage in Phase 13.

   The treatment is EP2-5's, not EP1's: `setStill(src,true)` crosses on the new file's own
   load event and grades the frame with `.still`, so a patient going down is a light coming
   down over eight-tenths of a second rather than a cut. It says nothing the status line
   under the frame has not already said in words, so it leaks nothing into an exam that the
   bay was not already showing — the same reasoning the key-art grade was cleared under. */
const STSTATES=['stable','deteriorating','critical','arrest'];
/* Seven clinical states, four shots. `improving` and `recovered` are a patient who is coming
   back and wear the shot she came in as; `dead` wears the arrest. Nobody is asked for seven
   frames a station when three of them would be the same room. */
const STATIONSTATE={stable:'stable',improving:'stable',recovered:'stable',
  deteriorating:'deteriorating',critical:'critical',arrest:'arrest',dead:'arrest'};
/* A still that failed to load is struck off for the session — a cached set table from before
   a file moved must not put the bay back on a black frame every tick. */
const STILLGONE=new Set();
const stateStills=id=>{ const info=memberOf(id);
  return (info&&info.m.states||[]).filter(x=>!STILLGONE.has(id+'_'+x)); };
/* The shot for the state she is in — or the nearest **milder** one that was shot, never a
   worse one. A station with only `stable` on disk shows the patient as she arrived for the
   whole run, which is honest; hanging an arrest over a patient who is talking to you would
   be the frame telling a lie the mark sheet then marks. Nothing at all → '' → the stem. */
function stationStill(id,status){
  const have=stateStills(id); if(!have.length)return '';
  for(let i=STSTATES.indexOf(STATIONSTATE[(status||'stable').toLowerCase()]||'stable');i>=0;i--)
    if(have.includes(STSTATES[i]))return '/img/cases/states/'+id+'_'+STSTATES[i]+'.jpg';
  return '';
}
/* Put an image in the frame, or take the frame back to black. `key` says it is an episode's
   key art rather than one of EP1's status stills, which is the whole difference in treatment:
   the art is cropped, dimmed and graded (`.still`, in the stylesheet) and EP1's stills are
   not touched at all. */
function setStill(src,key){
  const f=$('#fallback'), a=document.querySelector('.pt-art');
  if(!src){ f.classList.remove('on'); a.classList.remove('still'); a.removeAttribute('data-st'); return; }
  a.classList.toggle('still',!!key);
  if(key)a.dataset.st=STILLGRADE[lastShown]||'ok'; else a.removeAttribute('data-st');
  if(src===stillNow){ f.classList.add('on'); return; }
  /* A swap with a key art on either end of it is a swap between two patients, and the one
     leaving must not be what is on the frame while the one arriving decodes — that is the
     wrong-patient frame, briefly, which is the thing this whole path exists to avoid. So it
     is uncovered by its own load event. EP1 cutting between EP1's stills is one patient in
     one bay and cuts exactly as it always did. */
  const cross=key||stillKey;
  stillNow=src; stillKey=!!key; f.setAttribute('src',src);
  if(!cross){ f.classList.add('on'); return; }
  f.classList.remove('on');
  f.onload=()=>{ if(stillNow===src)f.classList.add('on'); };
  if(f.complete&&f.naturalWidth)f.classList.add('on');
}
/* The frame with no film in it. EP1's stills are EP1's patient and nobody else's — over
   Prasit or Khaopun they are a wrong-patient error a clinician clocks instantly — so every
   other episode gets its own key art instead, and a station gets its own patient once one
   has been shot. Until then it gets the black it is expecting underneath the stem. */
function paintStill(E){
  /* On a shift the frame is the patient in the bed, and the server said which picture that is —
     chosen from the same word it publishes as her status, by the same ladder the board uses. The
     page holds no opinion about states or sizes: it draws the URL that arrived, and when a
     different one arrives the face changes. No ward view, no WARDFACE, and everything below is
     the season's, untouched. */
  if(WARDFACE){ setStill(WARDFACE,true); return; }
  if(E.station){ const src=stationStill(E.id,lastShown); setStill(src,!!src); return; }
  setStill(E.id==='ep1'?'/img/'+lastShown+'.jpg':(E.art||''), E.id!=='ep1');
}
/* The one failure the stem exists to catch. A still that will not load — a stale set table,
   a half-copied file — must not leave the biggest panel in the bay black: the state is struck
   off, the frame goes back to the sheet, and no later tick asks for it again. */
$('#fallback').addEventListener('error',()=>{
  const m=/^\/img\/cases\/states\/(.+)\.jpg$/.exec(stillNow||''); if(!m)return;
  STILLGONE.add(m[1]); stillNow=''; stillKey=false;
  /* One state is struck off, not the station. If a milder shot is still there the frame keeps
     the patient and drops back to it; the stem is the answer only when nothing resolves. */
  const E=ep();
  if(STAGE==='pt'&&!stationStill(E.id,lastShown))STAGE='stem';
  stageKey=''; paintStill(E); renderStage();
});
function show(status,kit){
  lastShown=(status||'stable').toLowerCase();
  const E=ep();
  /* No film on the ward. The clips are the season's — shot for the season's patients, named for
     the season's cases — and `/clip/osce-a2_state_improving.mp4` over Salma Gaber is somebody
     else's face in motion. It 404s on the ward host, so nothing played and the still stayed; what
     the founder saw ("อยู่ๆ ตัดไป clip แพ้อาหาร") was the sheet, but the request was real and the
     day that volume is mounted there it would not be. The frame is her portrait, full stop. */
  if(WARD){ $('#loop').classList.remove('on'); paintStill(E); return; }
  // Film where there is film, stills everywhere else. A deploy without the clips must degrade
  // to the stills, not to a black frame, which is what it did on Cloud Run.
  if(!film){ $('#loop').classList.remove('on'); paintStill(E); return; }
  const want=stateClip(status,kit||[],E.id);
  if(!want || missing.has(want)){ $('#loop').classList.remove('on'); paintStill(E); return; }
  setStill('');
  if(want!==loopNow){ loopNow=want; $('#loop').src='/clip/'+want+'.mp4'; $('#loop').play().catch(()=>{}); }
  $('#loop').classList.add('on');
}
/* A failed clip is remembered by name, not taken as proof that there is no film at all.
   EP1 was shot with a clip for every state; the episodes after it are built from their key art
   and only have the two states that art can honestly play. Killing film on the first miss meant
   the film stopped the moment a patient improved — the player got half a reel and then silence,
   which is worse than stills throughout. Each name is asked for once; the still keeps tracking
   status either way. */
$('#loop').addEventListener('error',()=>{ if(loopNow) missing.add(loopNow); loopNow='';
  $('#loop').classList.remove('on'); paintStill(ep()); });
$('#cut').addEventListener('error',()=>{ film=false;
  $('#cut').classList.remove('on'); $('#cut-tag').classList.remove('on'); cutting=false; });
/* Play a cutscene over the loop, then hand the frame back. */
function playCut(key){
  /* EP1's cutscenes, and the ward has no episodes at all. */
  const asset=CUT[key]; if(WARD||!asset||ep().id!=='ep1'||cutting||!film)return;
  cutting=true;
  const c=$('#cut'); c.src='/clip/'+asset+'.mp4';
  c.muted=!sound; c.currentTime=0; c.classList.add('on'); $('#cut-tag').classList.add('on');
  c.play().catch(()=>{ c.classList.remove('on'); $('#cut-tag').classList.remove('on'); cutting=false; });
  c.onended=()=>{ c.classList.remove('on'); $('#cut-tag').classList.remove('on'); cutting=false; };
}
/* The Director's own order of precedence. */
function direct(newBeats,outcome){
  if(outcome) return playCut(outcome);
  const harm=newBeats.find(b=>b.startsWith('harm:'));         if(harm) return playCut('harm');
  if(newBeats.includes('threshold:biphasic'))                 return playCut('biphasic');
  for(const b of newBeats){
    if(b==='status:Arrest')    return playCut('code_blue');
    if(b==='status:Recovered') return playCut('adrenaline_calm');
    if(b==='status:Critical')  return playCut('deteriorate');
  }
}
/* The season's six rows are the season's: every one of its cases has drugs, labs, a procedure and
   a differential, so the kit draws all six. A compiled case has what its author wrote and no more,
   and a row with nothing in it is an empty shelf a stranger opens looking for the thing to reach
   for. So on a shift the kit is the rows this case actually has. */
const modeRows=()=>WARDSURFACE&&WARDCHIPS?MODES.filter(m=>(WARDCHIPS[m.id]||[]).length):MODES;
function renderModes(){
  $('#modes').innerHTML=modeRows().map(m=>`<button class="btn${m.id===mode?' on':''}" data-m="${m.id}" data-ga="mode:${m.id}">${m.icon} ${m.label}</button>`).join('');
  $('#modes').querySelectorAll('button').forEach(b=>b.onclick=()=>{mode=b.dataset.m;renderModes();renderChips();});
  document.querySelector('.acts').classList.toggle('order-mode', mode!=='ask');
}
/* Position was an answer too. The authored order teaches — the thing to reach for is written
   first, and it is first in almost every list — which at a station means the mark can be earned
   by reading the top-left corner instead of the patient. Under exam the tray is not sorted for
   you: one permutation per run, derived from the run id, so it is stable while the case is open
   (switching modes and coming back must not reshuffle the drugs under your hand) and different
   the next time the same station is sat. Practice keeps the taught order. */
const seedOf=s=>{let h=2166136261;for(let i=0;i<s.length;i++){h^=s.charCodeAt(i);h=Math.imul(h,16777619);}return h>>>0;};
const shuffled=(list,seed)=>{const a=list.slice();let s=seed||1;
  for(let i=a.length-1;i>0;i--){s=(Math.imul(s,1664525)+1013904223)>>>0;const j=s%(i+1);[a[i],a[j]]=[a[j],a[i]];}
  return a;};
/* A translated label is a coat over the button; the button's own words are not. `data-x` — what
   the chip fires, what lands on the tape, what the matcher resolves to an intervention id — is
   what the tray now reads out loud, and `PACK.asks` is the one thing allowed on top of it. A
   Thai learner and an English one who press the same chip still write byte-identical tapes and
   earn the same marks, because the translation relabels and does not re-fire. The analytics id
   stays English for the same reason: it is the same button.
   Only the `ask` row translates. The drugs, the labs, the procedures and the differential stay in
   professional English because that is the language the order is written in and the language the
   rubric pays for — you ask the patient in her language and you write the chart in yours. */
/* A World case brings its own labels: `PACK.asks` is the season's phrasebook, keyed by the
   season's own question strings, and a compiled case's `ask_` id is not in it. The case author's
   label wins where there is one — the words that case was written in — and the rule underneath is
   unchanged: the label is a coat over the button and `data-x` is what fires. */
const chipLabel=x=>WARDLABEL[x]||PACK.asks[x]||x;
/* What a chip says, from what the case calls it. The compiler prefixes each label with the row it
   belongs to — "Ask: …", "Examine: …" — and under a tab that already says ASK, twenty chips opening
   with the same word are twenty chips a learner reads the fourth word of. Display only: `data-x` is
   the intervention id and what fires is untouched. */
const chipText=label=>{
  const t=String(label==null?'':label);
  const m=/^(ask|examine|order|give)\s*:\s*(\S.*)$/i.exec(t);
  return m?m[2]:t;
};
/* The tray, from the case in front of this shift or from the season's table. Same reason as
   `epOf`: on the ward the table is the wrong sixteen questions about the wrong patient. */
const chipRows=()=>(WARDSURFACE&&WARDCHIPS)||CHIPS[ep().id]||{};
/* Which rows have been opened out. Per row, because a learner who wanted every question does not
   want every drug, and remembered while the page lives so a mode change and back does not fold the
   tray under their hand. */
const chipsOpen={};
function renderChips(){
  const c0=chipRows()[mode]||[];
  const c=examMode()?shuffled(c0,seedOf((id||'')+':'+ep().id+':'+mode)):c0;
  const m=mode;
  /* Four, then the rest behind one press.
     A compiled case brings its own tray — twenty questions, twenty-five investigations — and a
     wall of them is a wall to read before the first thing can be done. The first few are where a
     case is started; the rest are there, one press away, in the order the author wrote them.
     Kept whole on the season's six-chip rows, where there is nothing to fold. */
  const FEW=4;
  const folds=WARDSURFACE&&c.length>FEW+1&&!chipsOpen[m];
  const shown=folds?c.slice(0,FEW):c;
  $('#chips').innerHTML=shown.map(x=>`<button class="btn" data-x="${x}" data-ga="chip:${x}">${chipText(chipLabel(x))}</button>`).join('')
    +(folds?`<button class="btn more" id="chipmore" data-ga="chip:more">+ ${c.length-FEW} more</button>`:'');
  /* The label goes with the press for everything except the ask row, whose label is a
     translation — the chart and its echo are English by design (docs/internal/LANGUAGE_LAYER.md),
     and a Thai sentence in the order column would be this file quietly deciding otherwise. */
  const no=takeFirst(WARD, id, pro().o);
  const more=$('#chipmore');
  if(more){ more.onclick=()=>{ chipsOpen[m]=true; renderChips(); }; more.disabled=false; more.title=''; }
  $('#chips').querySelectorAll('button[data-x]').forEach(b=>{
    b.onclick=()=>fire(b.dataset.x, askedShown(WARD, m, b.dataset.x, chipLabel(b.dataset.x)), m==='dx');
    /* Greyed and titled rather than missing: a stranger who can see what they will be able to do
       knows they are one press away from doing it. */
    b.disabled=!!no; if(no)b.title=no;
  });
  /* The one line a stranger reads when they try to type. `no` wins: an ask bar that says "ask her
     anything…" and refuses to be typed in is the bug the founder found, dressed as an invitation. */
  $('#cmd').placeholder = no ? no : (mode==='ask'
    ? (PACK.ui.ask_placeholder || `ask ${pro().o} anything…`)
    : (PACK.ui.order_placeholder || 'or type the order yourself…'));
}
/* What the transcript reads when a chip is pressed, which is not always what the chip fires.
   At a station an ask chip *is* the question — "any allergies?" is the button and the line — so the
   line is what fired and a translated label stays a coat over the button: the chart is written in
   the language the order is written in. A compiled case's ask chip is an intervention id, because
   that is what the tape records and the rubric pays for, and `ask_black_tarry_stool` in the middle
   of a conversation at a bedside is a database key where a sentence should be. So on the ward the
   line is the case author's own label. An order always says what it was, on either host. */
function askedShown(ward, mode, x, label){ return mode==='ask' ? (ward?label:null) : label; }
/* At a station every chip is an order — the asks included. Step::Ask never reaches the tape
   (vitals-replay treats it as inert), so an ask routed through /api/say would leave the
   history-taking marks unearnable. The scenario answers with scripted patient words instead. */
function fire(text,shown,named){
  /* Before the head is taken there is no run: `step` returns at its first line, but `doOrder` has
     already written the order into the transcript — a line at 0:00 that nobody answered, on a page
     whose whole promise is that the transcript is what happened. So the refusal belongs here,
     where every press arrives, and it is said out loud rather than swallowed. */
  const no=takeFirst(WARD, id, pro().o);
  if(no){ if(typeof wardSay==='function')wardSay('<b>'+no+'</b> — nothing you do is on '+pro().p+' chart until the head is yours'); return; }
  (mode==='ask' && !ep().station) ? askHer(text,shown) : doOrder(text,shown,named);
}
/* Is this order the candidate naming a diagnosis? The station's own differential is the list,
   and a typed answer counts — "epiglottitis" in the order box is the same answer as the chip.
   Used for one thing only: whether the case's reply to it is teaching, and therefore whether it
   waits for the bell. It never reaches the tape, the matcher or the score. */
const namesADiagnosis=t=>{ const l=String(t).trim().toLowerCase();
  return l.length>2 && ((chipRows().dx)||[]).some(o=>{ const k=o.toLowerCase();
    return l.indexOf(k)>=0 || k.indexOf(l)>=0; }); };

function paint(v,named){
  const E=ep();
  /* Her face, as the server chose it for the state it is publishing in the same breath. `undefined`
     on the Eternal entry — no ward, no field — and the frame then behaves exactly as it always
     has. */
  if(v.portrait!==undefined){
    WARDFACE=v.portrait||'';
    /* Drawn where the founder looked for it — with her name, not in the frame, which on a station
       is behind the stem sheet until the first order is given. `alt` is her name because that is
       what the picture is of; when there is no picture the element is not there at all. */
    const f=$('#pt-face');
    if(f){
      f.hidden=!WARDFACE;
      if(WARDFACE&&f.getAttribute('src')!==WARDFACE)f.setAttribute('src',WARDFACE);
      f.alt=WARDSHIFT&&WARDSHIFT.name?WARDSHIFT.name:'the patient';
    }
  }
  /* The lines this run has earned, in the chosen language. Merged rather than replaced: beats
     only ever accumulate, and a view is a full snapshot. Absent for English, and absent for a
     case with no translation — both of which mean "show what the case author wrote". */
  if(v.tr) Object.assign(TR, v.tr);
  /* B9 · the player bar: on air while the case runs, ended when it is over. */
  const onAir=!over&&!v.over;
  /* On a shift the bar reads the patient, not the station (producer, 16 ก.ย.): a stranger is
     standing at a bed, and "S1 · OSCE D4" is a shelf number. The case stays, one line down and
     quieter, because which case she is is true and useful — it is just not the point. */
  $('#ep-name').innerHTML=(WARD&&WARDSHIFT
      ? esc(WARDSHIFT.name||('patient '+WARD))
        +(WARDSHIFT.age?' · '+WARDSHIFT.age:'')
        +(WARDSHIFT.country_name?' · from '+esc(WARDSHIFT.country_name):'')
      : (E.sn||E.n)+' · '+E.t)
    /* The pill is the bay's own word for a run in progress. On this host the strip above says what
       is happening in a sentence a stranger reads, and a second badge saying LIVE is one more
       thing to decode at a bedside. */
    +(WARDSURFACE?'':' · <span class="live'+(onAir?'':' end')+'">'+(onAir?'LIVE':'ENDED')+'</span>')
    +(EXAMLIVE?' <span class="live exam">OSCE EXAM</span>':'');
  $('#ep-name').title='scenario '+v.sce_hash;
  $('#pt-name').textContent=E.who; $('#pt-line').textContent=E.line; $('#place').textContent='📷 '+E.place;
  $('#pt-status').textContent=v.status; $('#pt-status').className='pt-status '+v.status;
  show(v.status, v.equipment);
  /* Through setVital, not straight to the DOM: while the monitor is booting these are
     the boot's moving target rather than the screen, so a count-up can never land on a
     reading that was already two seconds stale. */
  /* A saturation and a cuff pressure are measurements of flowing blood. In PEA, VF or
     pulseless VT there is none, so the server sends null and this rail prints `--` — the same
     answer the bedside device has always given, which it used to contradict by printing
     "SpO2 80%" and "BP 54/54" over a patient in cardiac arrest. HR keeps its number: PEA is
     complexes at a countable rate with no pulse, and that is the whole trap. */
  setVital('#m-hr',v.hr); setVital('#m-spo2',v.spo2==null?'--':v.spo2+'%');
  setVital('#m-bp',v.sbp==null||v.dbp==null?'--':v.sbp+'/'+v.dbp); setVital('#m-rr',v.rr);
  drawTrace(v);
  /* The strip says which rhythm it is drawing, the way the bedside lane does. Only when it is
     not sinus — a label that is always on is a label nobody reads, and the point of this one is
     that it appears at the moment the shape of the trace changes. */
  $('#mini-status').textContent=v.status
    +(v.rhythm&&v.rhythm!=='sinus'?' · '+v.rhythm.toUpperCase():'');
  /* She is getting worse. A class changing colour is a fact and nobody looks up for a
     fact; the room going red twice is the alarm the bay would actually be making. Only
     downward, and never after the bell — the border is a warning, not a verdict. */
  const rk=RANK[v.status]||0;
  if(rk>lastRank&&rk>=2&&!over&&!v.over)alarmPulse();
  lastRank=rk;
  $('#clock').textContent=fmt(v.elapsed); clockNow=v.elapsed;

  /* A2 · the shelf remembers where you left the shift: vitals.seen drives the hero's
     Continue and each card's bar. Throttled to one write per 10s of case time. */
  if(v.elapsed<seenMark)seenMark=-1e9;
  if(v.elapsed-seenMark>=10||v.outcome){seenMark=v.elapsed;markSeen($('#ep').value,v.elapsed);}

  /* NEWS2 — the score a ward escalates on. It is a total out of 20 rather than a percentage,
     and it does not average: one observation scoring 3 raises the band on its own, which is the
     whole reason the invented "stability" meter had to go. */
  const n=v.news;
  if(!n){
    /* She has died. An early warning score answers "does somebody need to come, and how fast" —
       there is nothing left to answer, and printing one beside "Dead" is the same nonsense as a
       heart rate on a corpse. */
    $('#news-n').textContent='—';
    $('#news-band').textContent = v.outcome && v.outcome.startsWith('Death') ? 'deceased' : 'no score';
    $('#news-do').textContent = say('terminal:'+(v.outcome||'TimeCalled'));
    $('#newsbox').dataset.band='none';
    $('#news-bar').innerHTML = Array.from({length:20},()=>'<i></i>').join('');
    $('#clock').textContent = fmt(v.elapsed);
    /* A death is still the end of a run, and the end of a run is where the debrief lives. This
       branch used to `return` straight out of paint(), so a case that ended in a death raised no
       result panel at all — no leaf, no score, no "what the case asked for", and, once the exam
       started holding harm back, no unsealing either. The candidate who killed the patient is
       exactly the one the feedback is for. Beats are drained but the Director is not called: a
       death cutscene that has never fired is not something to switch on from a bug fix. */
    renderChart(v, true);
    drainBeats(v, true);
    finish(v);
    return;
  }
  /* ── a patient this instrument does not read ────────────────────────────────
     NEWS2 is an adult score and the RCP does not validate it under 16 — a well
     three-year-old breathes 28 with a pulse of 118, which the adult table charges 5 for.
     The server stopped scoring them (`news2::applies_to_age`) and now sends the panel with
     `applies:false`, `total:null`, `band:"none"` and the sentence to print instead. It must
     not come through the dead branch above: a null `news` means dead and the page does
     considerably more than blank a number for that.

     Three things this must not do. It must not print a number, which is why the server
     sends null rather than a zero. It must not print twenty unlit ticks, which is a
     picture of the best NEWS2 there is — the blank that "reads as reassurance" is exactly
     what the sentence exists to refuse, and drawing it under the sentence would put it
     back. And it must not wear the deceased branch's `band="none"` red, which says
     emergency about a child a paediatrician would walk past. So: a dash, the caution
     colour, a rail that is visibly not a reading, and the server's own sentence saying
     which instrument is missing and why. `total==null` is read as well as `applies` so a
     server that says one without the other still cannot print a bogus score. */
  if(n.applies===false||n.total==null){
    $('#news-n').textContent='—';
    $('#news-band').textContent='not scored';
    $('#news-do').textContent=n.response||'NEWS2 is not validated for this patient';
    $('#newsbox').dataset.band='na';
    $('#news-bar').innerHTML='<i></i>';
  }else{
    $('#news-n').textContent = n.total;
    $('#news-band').textContent = n.band + ' risk';
    $('#news-do').textContent = n.response;
    $('#newsbox').dataset.band = n.band;
    /* Twenty ticks, filled to the score. A bar that fills as the patient worsens reads the
       right way round: more bar is more trouble. */
    $('#news-bar').innerHTML = Array.from({length:20},(_,i)=>
      `<i class="${i < n.total ? 'on':''}"></i>`).join('');
  }

  /* A tray, not a row of tags. What a bedside shows is the thing on the patient, its setting,
     and how long it has been there — "Intubate ×" is an order you typed, not a tube in a throat. */
  $('#kit').innerHTML = v.equipment.length
    ? v.equipment.map(k=>{ const d=KITLBL[k.id];
        const val = k.setting!=null ? `${k.setting}${d&&d.unit?' '+d.unit:''}` : '';
        const x = (!d||d.detach) ? `<button class="t-x" data-off="${k.id}" data-ga="kit-remove:${k.id}" title="take it off">remove</button>` : '';
        const since = fmt(Math.max(0, v.elapsed - (k.since||0)));
        return `<div class="t-row"><span class="t-dot"></span>
          <span class="t-n">${d?(d.device||d.label):k.id}</span>
          <span class="t-v">${val}</span><span class="t-t">${since}</span>${x}</div>`; }).join('')
    : '<span class="muted">nothing on the patient</span>';
  $('#kit').querySelectorAll('[data-off]').forEach(x=>x.onclick=()=>detach(x.dataset.off));
  renderTray(v.equipment);

  /* The rail used to read "⚠ no <drug> yet" with the drug the rubric was marking spelled
     into it, printed on screen until the candidate typed it. A station is an exam: an
     examinee who does not know the answer got it from the furniture, and a score that can be
     earned that way does
     not mean what this whole project claims a score means. So: nothing at all under exam, and in
     practice a cue about the chart rather than about the case — an empty chart is a fact the
     player can see for themselves, and the name of the drug to give is an answer they cannot.
     It clears on the first thing done to the patient of any kind, drug or device, because the
     sentence it makes is "nothing given yet" and a mask on a face is something given. */
  const acted = v.chart.some(c=>c.kind==='action'||c.kind==='equipment');
  /* And not before the head is taken: "nothing given yet" to somebody who may not give anything
     yet is an alarm about a shift that has not started. */
  const mine = !takeFirst(WARD, id, null);
  $('#nudge').textContent = (examMode()||acted||over||!mine) ? '' : '⚠ nothing given yet';
  /* Same rule, one panel down: the reason for admitting is a teaching line in practice and an
     answer at a station — a reason to admit belongs to one diagnosis and to no other, so it is
     said only on the case it is true of. */
  $('#dispo-why').textContent = examMode() ? '' : (DISPO_WHY[E.id]||'');

  /* The chart repaints every tick, so it flips to the full harm text by itself on the last
     paint of the run — the one that raises the result panel. */
  const ended = over || !!v.over;
  renderChart(v, ended);
  renderFilms(v);
  const fresh = drainBeats(v, ended, named);
  if(fresh.length) direct(fresh, v.outcome);
  finish(v);
}

/* Orders and events are different kinds of fact and a real chart never runs them together in
   one column of lowercase fragments. The tag says which, and the rule down the left carries it
   at a glance. */
const KIND={action:'ORDER', action_refused:'REFUSED', equipment:'DEVICE', harm:'HARM', outcome:'OUTCOME',
 /* A shock is not an order in the sense the mark sheet means — `vitals-osce` reads `action`
    rows as intervention ids — so it carries its own kind all the way to here. On the chart it
    reads like what it is: a thing that was done, at a second, at an energy. */
 shock:'SHOCK'};
function renderChart(v,ended){
  $('#chart').innerHTML = v.chart.length
    ? v.chart.slice(-40).map(c=>
        `<div class="c ${c.kind}"><span class="t">${fmt(c.t)}</span>`
        +`<span class="c-k">${KIND[c.kind]||'EVENT'}</span>`
        +`<span class="c-x">${c.text}</span></div>`).join('')
    : '<span class="muted">nothing recorded yet</span>';
  $('#chart').scrollTop=1e6;
}
/* ── the films ────────────────────────────────────────────────────────────────
   Order the imaging and the picture comes back. The server keys each one on
   (station, resolved intervention id) — the same id the tape already carries — so
   every phrasing the matcher understands reaches the picture too, and nothing here is
   ever written to the tape, the leaf or the replay. A station with no film in the
   table behaves exactly as it did before any of this existed.

   Two of the bank's images are held back pending a clinician's read. They are not in
   the server's table *and* not compiled into the binary, so there is nothing here for
   this code to show even by accident. A station that is holding its only film — osce-b's
   ECG, osce-c3's chest — orders it, gets no picture, and keeps the stem on the stage. It
   never falls back to the black frame, which is the whole point of the stem being there.

   Captions, intervention ids and the credit are server constants and page constants —
   no player text reaches this markup, which is why none of it is escaped. The stem is
   built from the same two authorities the shelf card uses (the SEASON entry and the
   server's set table) and carries no player text either. */
let SHOWN=[];
/* Copied from static/img/cases/ATTRIBUTION.md rather than composed here. CC-BY 4.0 §3(a)
   wants four things and all four are in this line: who made it, the licence, the licence
   URI (on the link) and the fact that we modified it — these are teaching renders of PTB-XL
   records, re-encoded again for the web, so "rendered and re-encoded" is the required
   modification notice and not a flourish. NIH ChestX-ray14 imposes no such condition; it is
   credited in the same breath because a teaching product that hides where its films came
   from has no business asking learners to trust them. */
const CREDIT='ECG: PTB-XL (Wagner et al., 2020), '
  +'<a href="https://creativecommons.org/licenses/by/4.0/" target="_blank" rel="noopener">CC-BY 4.0</a>, '
  +'rendered and re-encoded · CXR: NIH ChestX-ray14 (Wang et al., 2017)';
const orderName=s=>String(s||'').replace(/_/g,' ');
/* What the switch strip calls each one. The order id spelled out is the honest label and
   the short one is what a clinician says, so the strip reads ECG / CXR / X-RAY rather than
   "xray neck" wrapped onto two lines in a corner chip. */
const FILMTAB={ecg:'ECG',cxr:'CXR',xray_neck:'X-ray'};
const filmTab=f=>FILMTAB[f.intervention]||orderName(f.intervention);

/* ── the stage ────────────────────────────────────────────────────────────────
   The frame above the patient's name is the biggest thing on the screen. An episode
   fills it with film; a station had nothing to put in it and showed a black rectangle
   across half the bay for the length of the exam, which reads as a broken build.

   A station has three things worth that much of the screen, so it shows them there:

     * the patient, whenever she has been shot — the same frame EP1 has always had, and
       the one thing here that is *not* compiled into the binary: the files land in
       `static/img/cases/states/` per station per state and the server reads them off the
       disk, so a station has a patient the moment art drops one in. This is what the
       frame opens on when it exists, because a bay opens on a bed.
     * the stem, before anything has been ordered — the sheet on the door of the
       station, and the only text in the game a candidate is *supposed* to read before
       touching the patient. Everything on it is already public: it is the shelf card's
       own copy plus the room and the clock. Nothing the mark sheet marks goes on it —
       no diagnosis, no drug, no plan, and not the shelf blurb either, which is written
       to sell the case and used to point at the answer from inside the exam room.
     * the film, once one has been ordered — full frame, because a 12-lead at 69 pixels
       is a grey smudge, and the mark sheet expects the trace to have been *read*.

   `STAGE` is what the frame is showing: 'pt', 'stem', or an index into SHOWN. A film that has
   just arrived takes the frame by itself — ordering the ECG and then hunting for it is
   not what ordering it means — and after that the strip in the corner is the player's.
   `stageKey` keeps the repaint idempotent: paint() runs every tick and rebuilding this
   markup a second would drop focus and restart the image decode. */
let STAGE='stem', stageKey='';
/* What a station opens on: its patient if it has one, the sheet on the door if it does not.
   Both are always one tap apart in the corner strip. */
const openStage=id=>stationStill(id,'stable')?'pt':'stem';
function stemHtml(e){
  /* Same two authorities, same precedence as the shelf card: the server's set table
     names the stem, the band and the tier, and the SEASON entry is the copy that paints
     before it arrives. A station whose table entry has not landed still gets a full sheet. */
  const info=WARDSURFACE?null:memberOf(e.id);
  /* A station's title is the manifest's and carries the case's own age — "…worse at night — F 6"
     over the ward's eight-year-old. Retold here rather than written back into the manifest: the
     manifest is the authored case and stays what its author wrote. */
  const title=wardAged(info?info.m.title:e.t, WARD&&WARDSHIFT?WARDSHIFT.age:null);
  const band=(info?bandOf(info.m):e.spec)||'';
  const tier=(info?info.m.tier:e.tier)||'';
  /* The place is written for the film's corner tag, where "OSCE station · Nurse Mali
     marking" has to say which room it is. On the sheet the header has already said that,
     so the prefix comes off and what is left is what the row is actually for: who else is
     in there with you. A place that is not a station's keeps every word. */
  const room=String(e.place||'').replace(/^OSCE station · /,'');
  /* The band and the level are said once. On a station they ride the header beside the station's
     letter; on the ward there is no letter, the line below carries them, and a header that said
     them too printed "ER · intern" twice on one sheet. */
  const head=REVIEW?'reading the case':WARD?'on the ward':'OSCE station '+e.n.replace(/^OSCE /,'');
  return `<div class="stem-h"><b>${head}</b>`
      +'<span class="sp"></span>'
      +`<span>${WARDSURFACE?'':specShort(band)+(tier?' · '+tier:'')}</span></div>`
    +`<h3 class="stem-t">${title}</h3>`
    /* The case, said once, quietly, on a shift — the header above it now names the person. */
    /* The band and the level, and not the station's letter: "station A2" is the season's own
       shelf label, and a stranger at a bed on a public ward has no shelf to place it on. */
    + (WARDSURFACE?`<p class="stem-case" style="color:var(--ink-3,#7b8a86);margin:-.2rem 0 .6rem">${specShort(band)}${tier?' · '+tier:''}</p>`:'')
    +'<dl class="stem-g">'
      +`<dt>patient</dt><dd>${e.who||''}</dd>`
      /* The bedside card two rows above says what she is presenting with, on this host. One
         statement per fact: the sheet keeps the row on a station, where there is no card. */
      +(WARDSURFACE?'':`<dt>presents</dt><dd>${e.line||''}</dd>`)
      +`<dt>in the room</dt><dd>${REVIEW?(room||'the case\u2019s own room'):WARD?'a public ward — anybody may take the next shift':room}</dd>`
      +`<dt>time</dt><dd>${REVIEW?'as long as you want — nothing here is timed against you':WARD?'your shift, until you hand over':(e.rt||'')}</dd>`
    +'</dl>'
    /* Deliberately the generic candidate instruction and not a per-case one: a task
       written for this station would have to say what the station is about. */
    +'<p class="stem-task">Assess and manage this patient.</p>'
    /* The wording here is narrower than it reads, deliberately. docs/RISKS.md §11 before
       widening it. */
    /* On the ward there is no examiner, no bell and no mark sheet — there is a patient somebody
       else will inherit. Saying "the mark sheet stays sealed until the bell" to a stranger on a
       public ward promises a thing that does not happen there, which is the sort of small false
       claim this page is otherwise careful about. */
    +(REVIEW
      ? '<p class="stem-f"><b>★ review run</b> — the case is being read, not played on anybody. '
        +'Nothing here is recorded, counted or anchored, and the board does not move.</p>'
      : WARD
      ? '<p class="stem-f"><b>★ on the ward</b> — this shift is declared on chain before it is '
        +'played, and what you do is on '+pro(e).p+' chart under your key. Hand over when you are '
        +'done: '+pro(e).s+' stays, and the next stranger starts where you left '+pro(e).o+'.</p>'
      : '<p class="stem-f"><b>★ exam conditions</b> — this attempt is declared on chain before '
        +'it is played, and the score is derived from the tape afterwards. The harm sentences '
        +'and the mark sheet stay sealed until the bell.</p>');
}
function renderStage(){
  const E=ep(), art=document.querySelector('.pt-art');
  /* An episode's frame is the film's, exactly as it was: nothing here paints over it,
     and the place tag goes back where the Director left it. A shift is not an episode and has no
     film: what a stranger at a bed reads before the first order is the sheet, which is why the
     ward comes down the same path a station does without being one. */
  if(!E.station&&!WARDSURFACE){
    art.classList.remove('doc','film');
    $('#stem').classList.add('hide'); $('#filmv').classList.add('hide');
    $('#sgt').classList.add('hide'); $('#place').classList.remove('hide');
    stageKey=''; return;
  }
  /* Whether there is a patient to switch to at all — not which one, which is paintStill's
     job and changes with her status. A frame that rebuilt this markup on every state change
     would restart the film's decode and drop the focus off the strip. */
  const hasPt=!!stationStill(E.id,lastShown);
  if(STAGE==='pt'&&!hasPt)STAGE='stem';
  const pt=STAGE==='pt';
  const i=pt||STAGE==='stem'?-1:Math.min(STAGE,SHOWN.length-1);
  const f=i>=0?SHOWN[i]:null;
  const key=E.id+'|'+(pt?'pt':f?f.file:'stem')+'|'+(hasPt?'p':'')
    +SHOWN.map(x=>x.file).join(',')+(over?'|bell':'');
  if(key===stageKey)return;
  stageKey=key;
  /* The room is a line on the sheet now; two of it would be one too many. And the corner the
     tag wants is the strip's. */
  $('#place').classList.add('hide');
  /* `doc` is the document's shape, and a patient is not a document: with her on the frame it
     is 16:9 like every other frame in this bay. `film` is the height claim, not the
     visibility: after the bell the frame keeps the film and drops the claim, so the debrief
     is not read through a slot. */
  art.classList.toggle('doc',!pt); art.classList.toggle('film',!!f&&!over);
  if(pt){
    /* Nothing over the frame. The still is already in it — paintStill put it there and keeps
       it there as she changes — and the scrim and the grade come with it. */
    $('#filmv').classList.add('hide'); $('#filmv').innerHTML='';
    $('#stem').classList.add('hide');
  }else if(!f){
    $('#filmv').classList.add('hide'); $('#filmv').innerHTML='';
    $('#stem').innerHTML=stemHtml(E); $('#stem').classList.remove('hide');
  }else{
    $('#stem').classList.add('hide');
    $('#filmv').innerHTML=
       `<button type="button" class="fv-i" title="${f.caption}" aria-label="enlarge this film">`
      +`<img src="/img/cases/${f.file}" alt="${f.caption}" decoding="async"></button>`
      +`<figcaption class="fv-c"><span class="fv-who">${orderName(f.intervention)}</span>`
      +`<span class="fv-cap">${f.caption}</span>`
      +`<span class="fv-cr">${CREDIT}</span>`
      +'<span class="fv-z">⤢ tap the film for full screen</span></figcaption>';
    $('#filmv').classList.remove('hide');
    $('#filmv').querySelector('.fv-i').onclick=()=>openFilm(f);
  }
  /* The strip is a way out of wherever the frame is, so it appears when there is somewhere
     to go. The stem is on it whatever else is: it is the sheet the candidate is *supposed*
     to have read, and it must stay one tap away for the whole station however good the
     picture in front of it is. */
  const tabs=(hasPt?[`<button type="button" data-v="pt" class="${pt?'on':''}" aria-pressed="${pt}">patient</button>`]:[])
    .concat(`<button type="button" data-v="stem" class="${!pt&&!f?'on':''}" aria-pressed="${!pt&&!f}">stem</button>`)
    .concat(SHOWN.map((x,n)=>`<button type="button" data-v="${n}" class="${i===n?'on':''}" aria-pressed="${i===n}">${filmTab(x)}</button>`));
  if(tabs.length<2){ $('#sgt').classList.add('hide'); $('#sgt').innerHTML=''; return; }
  $('#sgt').innerHTML=tabs.join('');
  $('#sgt').classList.remove('hide');
  $('#sgt').querySelectorAll('[data-v]').forEach(b=>b.onclick=()=>{
    const v=b.dataset.v;
    STAGE=(v==='stem'||v==='pt')?v:+v; renderStage(); });
}
function renderFilms(v){
  const was=SHOWN.map(f=>f.file).join(',');
  SHOWN=v.films||[];
  const now=SHOWN.map(f=>f.file).join(',');
  if(SHOWN.length&&now!==was)STAGE=SHOWN.length-1;
  renderStage();
}
/* One light box, two callers: a film from the bay, and the credits from the shelf, which is
   where a visitor who never sits a station can still read where the images came from. */
function lightbox(html){
  $('#lightbox').innerHTML=html
    +'<button type="button" class="lb-x" data-lb-x aria-label="close">✕</button>';
  $('#lightbox').classList.remove('hide');
}
function closeLightbox(){ $('#lightbox').classList.add('hide'); $('#lightbox').innerHTML=''; }
function openFilm(f){
  if(!f)return;
  lightbox(`<figure class="lb-fig"><img src="/img/cases/${f.file}" alt="${f.caption}">`
    +`<figcaption><div class="lb-who">${orderName(f.intervention)}</div>`
    +`<div class="lb-cap">${f.caption}</div><div class="lb-cr">${CREDIT}</div>`
    +`<div class="lb-hint">tap anywhere to close</div></figcaption></figure>`);
}
function openCredits(){
  lightbox('<div class="lb-fig"><div class="lb-who">image credits</div>'
    +'<div class="lb-cap">The radiographs and ECGs in this bay are real clinical images from '
    +'open research datasets, re-encoded for the web. None of them is a patient of ours, and '
    +'none of them was generated.</div>'
    +'<div class="lb-cr">'+CREDIT+'</div>'
    +'<div class="lb-hint">tap anywhere to close</div></div>');
}
/* Anywhere closes it — except the licence link, which is the one thing in there somebody
   might actually be reaching for. */
$('#lightbox').onclick=e=>{ if(!(e.target.closest&&e.target.closest('a')))closeLightbox(); };
/* The lobby's own control, and the ward's shift page has no lobby. `onLobby` binds only when the
   element is on this page — the bay is shared by two pages now, and a handler bound to something
   that is not there stops the whole script before it reaches anything else. */
onLobby('#credits','onclick',openCredits);
/* Ahead of the cinema layer's own Escape handler only in the sense that it guards on its own
   state: the light box never opens over a running cutscene, so the two cannot both answer. */
addEventListener('keydown',e=>{
  if(e.key==='Escape'&&!$('#lightbox').classList.contains('hide'))closeLightbox(); });

/* Everything the engine has said since the last look, as lines in the feed. Returns the new
   beats so the caller can decide whether the Director gets a turn. */
function drainBeats(v,ended,named){
  /* Not `v.beats` — see visBeats. The two lists are not the same length either side of the
     bell, and slicing the longer by a watermark taken against the shorter re-prints the run. */
  const vis=visBeats(v);
  const fresh=vis.slice(seen);
  /* Whether the case declined the order these answer is not inferred from the sentence — it is
     read off the chart, where the server already wrote it. An order the scenario answered with
     words and changed nothing by is recorded `action_refused`; that judgement is made in
     `EffectTrace::refused` and the rubric reads it too, so the screen and the mark sheet agree
     about what happened. `CHARTSEEN` walks the same way `seen` walks the beats. */
  const chart=v.chart||[];
  const declined=chart.slice(CHARTSEEN).some(c=>c.kind==='action_refused');
  CHARTSEEN=chart.length;
  const quiet = examMode() && !ended && (declined || !!named);
  fresh.forEach((b,n)=>{
    /* There is no `sealed` branch left on this line: what arrives at the bell arrives whole.
       Held beats keep their old discipline — the index, which is a number, and never the
       sentence, because a title attribute is one hover away from being the answer key. */
    const k = b.startsWith('harm') ? 'harm' : b.startsWith('terminal') ? 'end' : 'beat';
    /* A line the page has its own words for — a status change, a threshold the case named — is
       a fact about the patient, not a mark, and it is never held. */
    const held = k==='beat' && quiet && !SAY[b];
    const node=ev(k+(held?' sealed':''), k==='harm'?'⚠':k==='end'?'■':'·',
      held?(declined?BEAT_DECLINED:BEAT_NOTED) : say(b),
      held?null:b);
    /* Every beat node, not only the held ones: unsealHarm counts these to find where a line
       that was never written belongs. `bi` stays the held-beat lookup and is now an index into
       the visible list, which is the list it is read back out of. */
    node.dataset.bn=seen+n;
    if(held)node.dataset.bi=seen+n;
  });
  seen=vis.length;
  PAINTS.push({vis:seen, chart:CHARTSEEN, clock:clockNow, tail:$('#chat').lastElementChild});
  return fresh;
}
/* The bell. Raised from both ends of paint() — the living patient's path and the dead one's —
   because a run that ended is a run that gets its score, its leaf and its debrief either way. */
function finish(v){
  /* `v.over` and not `v.outcome`. They are different facts: a run can be over with no terminal
     at all — time was called on a patient the case was never going to resolve — and reading
     `outcome` here is exactly what left `osce-b2` and `osce-c` running for ever with the mark
     sheet shut. The server owns the predicate (`Session::over`); this reads its answer. */
  if(!v.over || over) return;
  /* A shift is a few minutes of somebody else's stay, and the season's endings are written for
     the season's patients — their words, their faces, their cinema. The founder's shift reached a
     discharge two sim-minutes in and was shown all three. On the ward the ending is a sentence
     and a hand-over. */
  if(WARD) return wardFinish(v);
  over=true; stop(); disarmEnd();
  /* The film stops being the thing the screen is for. It borrowed height from the transcript
     while it was being read; the verdict is what wants the room now, and the frame gives it
     back rather than holding a hundred-and-fifty-pixel radiograph over a squeezed debrief.
     The film stays on the stage — just at the frame's ordinary share of it. */
  renderStage();
  /* Three endings, not two. She lived, she died, or the clock ran out on a patient who is
     exactly as she was left — and that last one must not be dressed as either of the others:
     a flatline over a living patient is a lie, and a green panel over a station nobody
     finished is a different one. */
  const win=!!v.outcome&&v.outcome.startsWith('Win');
  const died=!!v.outcome&&v.outcome.startsWith('Death');
  /* The panel is built now and shown later. Everything below fills it — including two
     async fetches — while it is still display:none, so by the time the sweep uncovers
     it the verdict is whole rather than assembling itself in front of the player. */
  $('#result').className='result '+(win?'win':(died?'lose':'called'));
  /* Every sealed line in the feed now says what it was. Matched by position against the run's
     own harm beats, which is the order they were appended in. */
  unsealHarm(v); unsealBeats(v);
  showDebrief(); showProvenance(null); showMarks(); showReveal();
  $('#outcome').textContent=say('terminal:'+(v.outcome||'TimeCalled'));
  $('#outnote').textContent=(v.harm.length?'Harm on the record: '+v.harm.join(', '):'No harm recorded.')+'  ·  '+(v.outcome||'no terminal — time called');
  $('#leaf').textContent=v.leaf;
  /* Kept for the debrief's payout line, which asks about this run's own leaf. */
  window.LASTLEAF=v.leaf||'';
  $('#verdict').innerHTML=''; $('#anchor').textContent='anchor this run on chain'; $('#anchor').disabled=false;
  ['#c1','#c2','#c3','#c4'].forEach(s=>onLobby(s,'disabled',true));
  $('#cmd').disabled=true; $('#send').disabled=true; $('#pause').disabled=true; $('#endrun').disabled=true;
  if(win){ const c=cleared(), e=$('#ep').value;
    if(!c.includes(e)){c.push(e);localStorage.setItem('vitals.cleared',JSON.stringify(c));} }
  chainState();
  /* ── the two endings, which must not feel the same ─────────────────────────────
     She lived: the monitor calms for 600ms — the trace drops its amplitude, the room
     stops being an emergency — and then the sweep hands over the verdict.
     She died: the flat line eats the wave, the dark closes in from the edges and the
     sound goes, over 900ms. It is longer than anything else in this game on purpose;
     the brief exempts it from the 600ms ceiling for exactly this reason. A death that
     cuts to a scorecard at the same speed as a save is a game telling you it did not
     notice. Both are skippable, both are after the clock has stopped. */
  /* Guarded on the run, like endFlow: restart pressed during the ending would otherwise
     open a result panel that #start has just emptied, over a case that has just begun. */
  const runId=id;
  const live=()=>id===runId&&over;
  const reveal=()=>{
    if(!live())return;
    $('#result').style.display='block';
    $('#result').scrollIntoView({behavior:'smooth',block:'nearest'});
    $('#vig').className='vig hide';
  };
  const hand=()=>{ if(!live())return; sweep(reveal,()=>{ if(live())endFlow(win); }); };
  if(died)flatline(hand); else settle(hand);
}

/* Deferred, not deleted — and now written rather than rewritten.
   ------------------------------------------------------------------------------------------
   This used to walk `#chat .ev.harm.sealed` and swap the text of each node for the sentence at
   the matching position. There are no such nodes any more, so the bell has to put the lines
   *in*, at the place each of them would have been written.

   Three facts locate one harm, and all three are already tracked:

     k  how many visible beats came before it        — its slot among the beat nodes
     c  how many non-harm chart rows came before it   — which paint carried it, when an order
                                                        caused it and the clock did not move
     t  the scenario second it happened               — which paint carried it, when the clock
                                                        caused it and the chart did not move

   Find the paint, then place it: before the beat node of visible beat #k when that beat was
   written by the same paint, and otherwise on the end of that paint's own block. The anchor
   moves as we go so two harms in one paint keep their order. */
function unsealHarm(v){
  /* Practice already printed every one of these where it happened. */
  if(!examMode()) return;
  const all=v.beats||[];
  const want=[]; let k=0;
  for(const b of all){ if(b.startsWith('harm:')) want.push({b,k}); else k++; }
  if(!want.length) return;
  /* The chart is whole from the bell onwards, so this is where the clock and the running count
     of everything-that-is-not-harm come from. Same order as the harm beats: the automaton
     records the harm the moment it emits the beat. */
  const rows=(v.chart||[]); const at=[]; let c=0;
  for(const r of rows){ if(r.kind==='harm') at.push({t:r.t,c}); else c++; }
  const chat=$('#chat');
  want.forEach((w,i)=>{
    const a=at[i]||{t:Infinity,c:w.k};
    let p=0;
    while(p<PAINTS.length-1 && !(PAINTS[p].chart>=a.c && PAINTS[p].clock>=a.t)) p++;
    const before=PAINTS[p-1] ? PAINTS[p-1].vis : 0;
    /* Written by this paint? Then the line goes in front of it. Otherwise this paint said
       nothing after the harm and the line goes on the end of what it did say. */
    const own = w.k>=before && w.k<PAINTS[p].vis;
    const node=document.createElement('div');
    node.className='ev harm';
    node.innerHTML=`<span class="t">${fmt(a.t===Infinity?PAINTS[p].clock:a.t)}</span>`+
                   `<span class="ic">⚠</span><span class="tx"></span><span class="rule"></span>`;
    node.querySelector('.tx').textContent=say(w.b); node.title=w.b;
    const mark=own ? chat.querySelector('.ev[data-bn="'+w.k+'"]') : null;
    if(mark) chat.insertBefore(node,mark);
    else if(PAINTS[p].tail && PAINTS[p].tail.parentNode===chat)
      chat.insertBefore(node,PAINTS[p].tail.nextSibling);
    else chat.appendChild(node);
    /* Two harms in one paint: the second hangs off the first, not off the same anchor, or the
       pair would come out backwards. */
    PAINTS[p]={...PAINTS[p], tail:node};
  });
}
/* The same, for the lines the exam held back because they were teaching rather than observing.
   By index rather than by position: these are interleaved with everything else the case said,
   and the index is what the node was given when it was written. The sentence itself was never
   in the DOM until now — which is the point of keeping a number there instead. */
function unsealBeats(v){
  const all=visBeats(v);
  document.querySelectorAll('#chat .ev.beat.sealed[data-bi]').forEach(n=>{
    const b=all[+n.dataset.bi];
    if(b!=null){ n.querySelector('.tx').textContent=say(b); n.title=b; }
    n.classList.remove('sealed'); delete n.dataset.bi;
  });
}
async function step(q,named){
  if(!id)return;
  const v=await (await fetch('/api/step?id='+id+q+asMe()+langQ())).json();
  if(v.error)return ev('note','—',v.error);
  paint(v,named);
}
/* `shown` is what the button said, when a button said it — the feed is a transcript of the
   station and a transcript that quotes words the candidate never read is not one. The order
   itself (`text`) is untouched: it is what the matcher reads and what the tape keeps.
   `named` is threaded, not stashed in a module variable, because the tick loop paints too and a
   flag set here would be consumed by whichever paint happened to land first. */
function doOrder(text,shown,named){ disarmEnd(); ev('order','▸',shown||text);
  step('&do='+encodeURIComponent(text), named===undefined?namesADiagnosis(text):named); }
async function askHer(q,shown){
  if(asking||!id)return; asking=true; $('#send').disabled=true;
  /* What was asked, as a person would read it back. `q` is what goes to the server — on the ward
     an intervention id, which is what her case is keyed by — and typing gets the same string
     both ways. */
  turn('you','you',shown||q);
  const d=turn('her think', ep().who.split(' · ')[0], '…');
  step('&tick=5');                                   // talking costs time
  const r=await (await fetch('/api/say?id='+id+'&q='+encodeURIComponent(q)+asMe()+langQ())).json();
  d.classList.remove('think');
  d.querySelector('.say').textContent = r.reply || ('— '+(r.error||'no answer'));
  if(!r.reply) d.classList.add('think');
  /* She was asked for one language and answered in another. Her answer still stands — it is
     still true about the case, and swallowing it would cost the learner the only reply the
     month's compute paid for. A note beside it, so a learner who chose Thai is told rather than
     left wondering whether the setting did anything. */
  if(r.reply && r.off_language) ev('note','·', PACK.ui.off_language || `— ${pro().s} answered in another language`);
  /* The month is spent: the reply carries the whole meter, and the card takes it from here. */
  if(r.ceiling) showCeiling(r.ceiling);
  $('#chat').scrollTop=1e6; asking=false; $('#send').disabled=false; $('#cmd').focus();
}
const run =()=>timer=setInterval(()=>step('&tick='+(hard?3:2)),700);
const stop=()=>{clearInterval(timer);timer=null};

/* ── "I have finished" ────────────────────────────────────────────────────────
   Two presses, because there is one thing on this page that cannot be taken back and this is
   it. The first arms and says so; the second, inside ARM_MS, ends the attempt. Anything else
   the candidate does — an order, a restart, the window of time running out — disarms it, so a
   press left lying around does not fire into the run five minutes later.

   The button does not decide anything about the patient. It calls /api/finish, and the server
   runs the encounter on to wherever what the candidate did was going to take her. Pressing it
   one second before an arrest and standing there through the arrest produce the same tape, the
   same leaf and the same marks — which is why it needs no penalty and can carry no advantage. */
const ARM_MS=6000;
let armT=null;
const ENDLABEL='I have finished';
/* What that button says on the ward, and the line under it — `null` in the bay, where the words are
   the pack's and are not this function's business.
   A shift is not an attempt and does not end: it is handed to whoever comes next, and whether the
   stay ends is the engine's to decide and the chain's to record. So the ward says "hand over"
   twice rather than "I have finished", and the pronoun is the pronoun of the person in the bed. */
function endWords(ward, armed, g){
  if(!ward)return null;
  const Cap=w=>w.charAt(0).toUpperCase()+w.slice(1);
  return armed
    ? { label:'press again to hand over',
        note:'Hands '+g.o+' to whoever comes next. Your shift is written to '+g.p+
             ' chain and cannot be taken back.' }
    : { label:'hand over',
        note:'Ends your shift and writes it to '+g.p+' chain. '+Cap(g.s)+
             ' stays on the ward, and the next stranger starts where you stopped.' };
}
/* The other way out, and the one that costs a stranger everything they have done: the shift is
   released and nothing is written. Named by what it does — "hand her back" and "hand over" are one
   word apart and opposite in consequence (UX review C4) — and asked once before it acts, in the
   same press-again idiom as the end button. A browser dialogue would be dismissed without reading;
   the strip is where this page already speaks. */
let LEAVEARM=null, LEAVEWAS=null;
/* Six seconds and the question is withdrawn, the same as the end button's. A page that keeps a
   confirmed state after the stranger has looked away is a page that leaves a loaded press behind
   the next thing they do. */
function disarmLeave(){
  clearTimeout(LEAVEARM); LEAVEARM=null;
  /* The strip asked the question, so the strip takes it back. A button that has withdrawn its
     question over a page still asking it is the page contradicting itself, and the stranger who
     looks up at that moment reads the sentence rather than the button. */
  if(LEAVEWAS!==null&&$('#wardsay')){ $('#wardsay').innerHTML=LEAVEWAS; LEAVEWAS=null; }
  const b=$('#wardback-shift'); if(!b)return;
  b.classList.remove('armed'); b.textContent=leaveWords(false).label;
}

function leaveWords(armed){
  return armed
    ? { label:'press again to leave', say:'nothing you did will be kept — leave?' }
    : { label:'Leave without recording', say:'' };
}

function disarmEnd(){
  if(armT){clearTimeout(armT);armT=null}
  const b=$('#endrun'); if(!b)return;
  b.classList.remove('armed');
  const w=endWords(WARD, false, pro());
  if(w){ b.textContent=w.label; $('#endnote').textContent=w.note; return; }
  b.textContent=ENDLABEL;
  $('#endnote').textContent=PACK.ui.end_note||'Ends the attempt. The case plays out from here and the marks are computed.';
}
async function endRun(){
  if(!id)return;
  /* On the ward the end of a shift is not the end of her stay: she is handed to whoever comes
     next, and whether the stay ends is the engine's to decide and the chain's to record.
     Checked before `over`, deliberately. `over` means the engine has finished with her — she is
     ready to go home, or she has died — and that is precisely the shift with something left to
     do: hand it over so the chain carries it. Guarding hand-over behind `over` closed the whole
     discharge path on the ward, so only the ticker's deaths ever ended a stay. */
  if(WARD)return handOver();
  if(over)return;
  disarmEnd();
  $('#endrun').disabled=true;
  ev('note','·',PACK.ui.time_called||'time — the station ends');
  gtag('event','case_finish',{ep:$('#ep').value,at:Math.round(clockNow)});
  const v=await (await fetch('/api/finish?id='+id+asMe()+langQ())).json();
  if(v.error){$('#endrun').disabled=false;return ev('note','—',v.error)}
  paint(v);
}
$('#endrun').onclick=()=>{
  if(!id)return;
  const b=$('#endrun');
  /* A shift the engine has already ended goes straight through: there is nothing left to lose,
     and asking a stranger to press twice to confirm is one more chance to press once and walk
     away from a patient who cannot be closed by anybody else. */
  if(WARD&&over)return endRun();
  if(over)return;
  if(b.classList.contains('armed'))return endRun();
  b.classList.add('armed');
  const w=endWords(WARD, true, pro());
  b.textContent=w?w.label:(PACK.ui.end_confirm||'press again to end');
  $('#endnote').textContent=w?w.note
    :(PACK.ui.end_warn||'The case plays out from where you leave it — this cannot be undone.');
  armT=setTimeout(disarmEnd,ARM_MS);
};

/* ── the two buttons a station does not have ──────────────────────────────────
   `hold` stops the clock. `easy/hard` picks how fast the patient deteriorates. Both are
   exactly right for practice — a learner who wants to stop and think should be able to, and a
   learner who wants the harder version should be able to ask for it — and neither belongs in
   something that ends in a claim anchored on a chain.

   Hold is the worse of the two: a station is a ten-minute clock, half the rubric is timed — an
   item that pays only if the thing was done inside its own window — and a candidate who can
   freeze the patient can take as long as they like over every one of those windows and still
   be inside them. That is not a slower exam, it is a different exam. Difficulty is the same
   argument one step along: choosing the rate at which she crumples is choosing how much of the
   mark sheet is reachable, after the run has already been declared to the chain.

   So at a station both come off the bar and the speed is pinned to the one the run was declared
   at. The handlers below refuse as well, because a hidden button is still a button — the shelf
   is HTML and the console is right there.

   Practice keeps both, unchanged. */
function examControls(){
  const x=examMode();
  if(x){ hard=false; $('#diff').textContent='easy'; $('#diff').classList.remove('on'); }
  $('#pause').classList.toggle('hide',x);
  $('#diff').classList.toggle('hide',x);
  $('#pause').disabled=x||!id;
  $('#diff').disabled=x;
}

async function chainState(){
  const me=await identity();
  const c=await (await fetch('/api/chain'+(me?'?player='+me.pub:''))).json();
  $('#chainstate').textContent = c.connected
    ? `${c.cluster||'?'} · tree #${c.tree_id} · ${c.anchored} anchored`+(c.proven!=null?` · ${c.proven} yours`:'')
    : 'no validator';
  onLobby('#anchor','disabled', !c.connected || !over);
  if(me && c.connected){
    const a=await accountState();
    if(a && a.started!=null) TALLY.started=a.started;
    TALLY.anchored=c.proven||0;
    showTally();
  }
  return c;
}

onLobby('#start','onclick',async()=>{
  // Before the case exists, not after. A case opened while the key was still being generated
  // would belong to nobody, and a case that belongs to nobody answers to anyone holding its id.
  await identity();
  stop(); over=false; seen=0; CHARTSEEN=0; PAINTS=[]; clockNow=0; TR={}; emptyState(); $('#result').style.display='none';
  disarmEnd(); $('#endrun').disabled=false;
  /* Wiped, not just hidden: a mark sheet left in the DOM from the previous run is the answer
     key one devtools inspection away, and #result is display:none rather than gone. */
  $('#marks').innerHTML=''; $('#db').innerHTML=''; $('#reveal').textContent=''; $('#verdict').innerHTML='';
  const e=$('#ep').value;
  const r=await (await fetch('/api/new?ep='+e+asMe()+langQ())).json();
  if(r.error)return ev('note','—',r.error);
  gtag('event','case_start',{ep:e,difficulty:$('#diff').dataset.d});
  id=r.id; mode='ask'; loopNow=''; cutting=false; $('#cut').classList.remove('on');
  /* The frame goes back to the door of the station. Wiped rather than left: a film from
     the previous run still on the stage is a picture of a patient who is not in the bed. */
  SHOWN=[]; STAGE=openStage(e); stageKey='';
  /* Declare the run before playing it. The whole integrity claim is that the chain knew you
     started before you knew how it would go — so the declaration has to land here, at the top,
     not at anchor time when the outcome is already on the screen. If it fails the run still
     plays; it just cannot anchor, and the anchor button will say why. */
  commitRun(EXAMRUN);
  renderModes(); renderChips(); paint(r.view);
  /* The monitor wakes up: the trace runs in from the right and the four numbers climb
     from zero to the readings paint() has just put there. Five hundred milliseconds,
     entirely inside the 700ms before the first tick, and it animates values that are
     already true — the clock is not held for a frame of it. */
  lastRank=0; $('#mini').classList.remove('calm'); $('#vig').className='vig hide';
  bootMonitor();
  if(ep().id==='ep1') playCut('cold_open');
  $('#device').src=deviceSrc('monitor');
  document.querySelectorAll('.dev').forEach(b=>b.classList.toggle('on',b.dataset.dev==='monitor'));
  $('#cmd').disabled=false; $('#send').disabled=false;
  $('#pause').textContent='hold'; examControls(); $('#cmd').focus();
  const voice=(await (await fetch('/api/chain')).json()).voice;
  emptyState();
  if(!voice) ev('note','—',`no gateway, so ${pro().s} cannot answer — orders still work`);
  run(); chainState();
});
/* Leaving is not a sweep. The sweep is the signature of going forward — running it
   backwards would say the story moved on when the player just stepped out of it. Two
   hundred milliseconds through black instead. DOORWATCH opens here because the shelf
   the player is about to see is the one a finished run may have unlocked, and that is
   the only moment a door opening deserves a fanfare. */
onLobby('#back','onclick',()=>{ stop(); EXAMRUN=false; EXAMLIVE=false; DOORWATCH=true; examControls();
  fadeSwap(()=>{ $('#game').classList.add('hide'); $('#lobby').classList.remove('hide');
    renderSeason(); }); });
$('#pause').onclick=()=>{ if(examMode())return;
  if(timer){stop();$('#pause').textContent='resume';} else{run();$('#pause').textContent='hold';} };
$('#diff').onclick=()=>{ if(examMode())return;
  hard=!hard; $('#diff').textContent=hard?'hard':'easy'; $('#diff').classList.toggle('on',hard);
  if(timer){stop();run();} };
$('#cmdf').onsubmit=e=>{ e.preventDefault(); const x=$('#cmd').value.trim(); if(!x)return;
  $('#cmd').value=''; fire(x); };
$('#mic').onclick=()=>{
  const SR=window.SpeechRecognition||window.webkitSpeechRecognition;
  if(!SR)return turn('sys note','','this browser has no speech recognition');
  const r=new SR(); r.lang='en-GB'; r.onresult=ev=>{ $('#cmd').value=ev.results[0][0].transcript; }; r.start();
};
/* A bedside device knows the session it is watching, who is in the bed, and whether the room is
   an exam. The first two because a monitor with someone else's name at the top is a wrong-patient
   error a clinician clocks in half a second — the pane used to print "Ing · F 19" over every
   case in the season, including the 71-year-old man in OSCE A. The third because the ventilator
   panel interprets its own numbers out loud — it names causes for the pressure it is showing
   — which is teaching in practice and the answer read off the wall at a station. The devices
   stay dumb about the case itself: no diagnosis, no rubric, nothing but a name, an age and a flag. */
/* No exam flag rides on this URL. A pane used to be told whether to withhold its teaching by a
   query parameter appended here, which put the seal in the candidate's own address bar; the
   panes are told by the server now, on the feed they already poll. */
const deviceSrc=dev=>{
  const e=ep();
  const a=ageOf(e);
  return '/device/'+dev+'?sid='+encodeURIComponent(id)
    +'&pt='+encodeURIComponent(e.who||'')
    +(a==null?'':'&age='+a)
    +'&bed='+encodeURIComponent(bedOf(e));
};
document.querySelectorAll('.dev').forEach(b=>b.onclick=()=>{
  document.querySelectorAll('.dev').forEach(x=>x.classList.toggle('on',x===b));
  if(id)$('#device').src=deviceSrc(b.dataset.dev);
});
$('#addkit').onclick=openPicker;
/* One press, one order, exactly like a chip — and the same words on the button as in the order,
   so the transcript and the tray agree. Which of the four a station can actually act on is the
   station's own business: its matcher decides, and a station with no bed of that kind simply
   does not have one, which was already true of the checkbox. */
$('#dispo-opts').querySelectorAll('[data-dispo]').forEach(b=>b.onclick=()=>doOrder(b.dataset.dispo));
/* Autoplay only survives while muted, so the loop stays silent and the cutscenes get the
   sound — which is where the room tone actually is. Off by default: a bay that starts
   making noise on load is a bay somebody closes. */
let sound=false;
$('#sound').onclick=()=>{
  sound=!sound;
  $('#sound').textContent = sound ? '🔊 sound on' : '🔇 sound off';
  $('#sound').classList.toggle('on',sound);
  $('#cut').muted=!sound;
  $('#loop').muted=true;
};

onLobby('#anchor','onclick',async()=>{
  $('#anchor').disabled=true; $('#anchor').textContent='anchoring…';
  const r=await chainDo('/api/anchor?id='+id); const v=$('#verdict');
  if(r.error){v.innerHTML+=`<span class="r">${r.error}</span>`;$('#anchor').textContent='anchor this run on chain';$('#anchor').disabled=false;return;}
  v.innerHTML+=`<span class="g">anchored at index ${r.index} · score ${r.score} · ${r.proven?'proven':'PROOF FAILED'}</span>`;
  v.innerHTML+=`<span class="n">root ${r.root.slice(0,24)}… · ${r.leaves} leaves · ${r.counted} counted</span>`;
  if(r.det) v.innerHTML+=`<span class="g">OSCE ${r.det.score}/${r.det.max} — deterministic, re-derivable by anyone</span>`;
  /* The star the station card wears: earned here, by an anchor that really landed. */
  const st=starred(), e=$('#ep').value;
  if(!st.includes(e)){st.push(e);localStorage.setItem('vitals.starred',JSON.stringify(st));}
  /* An exam that cleared a bar is a star the chain will certify in a moment. The tier was
     already floored locally when the sheet was marked (see `bankTier`, called from
     `showMarks`) — banking it again here is a no-op for a run that has not improved, and
     the point of the call is the line below it: ask /api/stars until the chain's answer
     catches up. Three-tier: ≥95% of the rubric floors ★★★, ≥85% ★★, ≥70% ★; only upward. */
  if(EXAMLIVE&&r.det&&r.det.max>0){
    const bps=bpsOf(r.det.score,r.det.max);
    const t=tierFor(bps);
    if(t) bankTier(e,t);
    /* What the next star costs, in the rubric's own points. A percentage tells you how you
       did; "3 points from your third star" tells you whether to sit it again — and the
       arithmetic is the server's, so the number is a promise the door will keep. It is the
       same sentence the mark sheet prints, from the same function, because two spellings of
       one verdict is two verdicts. */
    const pct=(bps/100).toFixed(bps%100?1:0);
    v.innerHTML+=`<span class="g">${starGlyph(t)} ${pct}% — `
      +`${nextLine(r.det.score,r.det.max,true)}</span>`;
  }
  refreshStars();
  onLobby('#anchor','textContent','anchored');
  ['#c1','#c2','#c3','#c4'].forEach(s=>onLobby(s,'disabled',false));
  chainState();
});
/* The four claim buttons live in the season's result panel, which the ward host does not compose —
   so they bind where they exist, like every other control that differs by host. A real browser
   found this one: `$(sel).onclick` on a null threw while the script was still setting itself up,
   and every handler after it never bound. */
[['#c4',4],['#c3',3],['#c2',2],['#c1',1]].forEach(([sel,lv])=>{
  onLobby(sel,'onclick',async()=>{ const r=await chainDo('/api/claim?level='+lv);
    $('#verdict').innerHTML += r.granted?`<span class="g">✓ ${r.message}</span>`:`<span class="r">✗ ${r.message}</span>`;
    /* The wallet button's level is the thing a grant just changed — say so without a reload. */
    if(r.granted) refreshRecord(); });
});
/* The score says what happened. This says why — and every line of it is a time or an ordering
   taken from the tape, so the person reading it could check it themselves. */
async function showDebrief(){
  const d=await (await fetch('/api/debrief?id='+id+asMe())).json();
  if(d.error){ $('#db').innerHTML=''; return; }
  const F=s=>`${Math.floor(s/60)}:${String(Math.floor(s%60)).padStart(2,'0')}`;
  let h='';

  if(d.expected.length){
    h+='<div><h4>what the case asked for</h4>'+d.expected.map(e=>{
      let w,c;
      if(e.done_at==null){ w = e.within!=null ? 'never · '+F(e.within)+' target' : 'never'; c='never'; }
      else if(e.late){ w = F(e.done_at)+' · '+F(e.late_by)+' late'; c='late'; }
      else { w = F(e.done_at); c='ok'; }
      /* The reason is shown only where it was missed. Told what you did right, you skim; told
         what it costs to be four minutes late, you remember. */
      const why = (e.done_at==null||e.late) && e.why ? `<span class="db-y">${e.why}</span>` : '';
      return `<div class="db-r"><span class="db-n">${e.label}${why}</span>`
            +`<span class="db-w ${c}">${w}</span></div>`;
    }).join('')+'</div>';
  }

  if(d.avoided.length){
    h+='<div><h4>what it asked you not to do</h4>'+d.avoided.map(a=>
      `<div class="db-r"><span class="db-n">${a.label}`
      +(a.why?`<span class="db-y">${a.why}</span>`:'')
      +`</span><span class="db-w never">${F(a.done_at)}</span></div>`).join('')+'</div>';
  }

  if(d.harms.length){
    h+='<div><h4>harm</h4>'+d.harms.map(x=>
      `<div class="db-h"><b>${F(x.at)} — ${x.text}</b>`
      +(x.caused_by?`<span class="src">after your order: ${(KITLBL[x.caused_by]||{}).label||x.caused_by}</span>`
                   :'<span class="src">not from anything you ordered</span>')
      +`</div>`).join('')+'</div>';
  }

  const spans=d.statuses.filter(s=>s.seconds>=1);
  if(spans.length){
    h+='<div><h4>time in each state</h4>'+spans.map(s=>
      `<div class="db-r"><span class="db-n">${s.status}</span>`
      +`<span class="db-w">${F(s.seconds)}</span></div>`).join('')+'</div>';
  }
  /* Who wrote it, if anybody has published that they did. Last, because it is about
     the case rather than about the run just played — and absent, not blank, when
     there is no attribution to show. */
  const by=authorLine((await ledger()).byEp.get($('#ep').value));
  if(by)h+=`<div class="db-by">${by}</div>`;
  /* The payout for *this* run, when there was one. Asked for after the debrief is drawn rather
     than raced with it: the transfer is a second transaction that lands a moment after the
     proof, and a line that said "not paid" because it asked too early would be worse than no
     line. Absent when nothing was paid — see `authorLine` on why silence beats a zero. */
  paidLine();
  $('#db').innerHTML=h;
}

/* ── the mark sheet ────────────────────────────────────────────────────────────
   The missing half of the loop. A failed station used to say "Death · arrest" and stop,
   so an unlimited-retry model was an unlimited-guess model: nothing on the screen told
   you which of the ten items you dropped. This prints every one of them, worst first.

   Three rules it must not break.

   1. **After the bell only.** It names every action the rubric pays for, with its window
      — mid-run that is the answer key, not feedback. `over` is checked here and the
      server refuses to answer before the outcome (`/api/marks`), so the seal survives
      someone editing this file.
   2. **The server's arithmetic, not ours.** Every number below is printed as sent; the
      page adds nothing up. The sheet comes off `sheet_for_run`, which is `det_for_run`'s
      own body, so the total here and the det score on chain are the same walk over the
      same tape.
   3. **Practice gets it too.** A formative run is where the feedback is worth most; the
      only thing exam mode changes is that the sheet waits for the bell like everything
      else. */
const MKG={hit:'✓',partial:'◐',miss:'✗'};
const mmss=s=>`${Math.floor(s/60)}:${String(Math.floor(s%60)).padStart(2,'0')}`;
/* What a row says under its label. Only where it adds a fact the label does not already
   carry: a window that was missed or met, or the moment an avoided harm actually fired. */
function markNote(it){
  if(it.within!=null){
    if(it.at==null) return 'never given · the window was '+mmss(it.within);
    return 'given at '+mmss(it.at)+' · the window was '+mmss(it.within);
  }
  if(it.kind==='no_harm'&&it.at!=null) return 'it happened at '+mmss(it.at);
  if(it.mark==='hit'&&it.at!=null&&it.kind!=='outcome') return mmss(it.at);
  return '';
}
async function showMarks(){
  const el=$('#marks'); if(!el) return;
  el.innerHTML='';
  /* The seal, said twice. The server is the one that holds. */
  if(!id||!over) return;
  let m; try{ m=await (await fetch('/api/marks?id='+id+asMe())).json(); }catch(e){ return; }
  if(!m||m.error) return;
  /* Provenance rides in on the same sealed answer, and is printed from here rather than from
     the shelf's set table — see showProvenance(). It goes up even for a case with no rubric,
     because where the case came from is true whether or not there was a sheet to mark. */
  showProvenance(m);
  if(!Array.isArray(m.items)||!m.items.length) return;
  const bps=m.bps||0, t=tierFor(bps);
  const pct=(bps/100).toFixed(bps%100?1:0);
  /* ── the star is banked here, by the sheet, not by the anchor ────────────────
     This is the moment the run is *scored* — the server has walked the tape and sent back the
     total the chain would carry — so this is the moment the tier is earned. Writing it here is
     what lets a player who never anchors keep what they won: the anchor stays the on-chain
     proof and stops being a toll gate on local progression. `bankTier` only ever raises the
     floor, so a worse replay cannot take a star back, and `tierOf` still prefers the chain's
     answer over this one wherever the chain has caught up. */
  if(t&&m.max>0&&examMode()) bankTier($('#ep').value,t);
  /* The same sentence the anchor prints, available before anchoring and in practice —
     the arithmetic is the server's either way, so the two can never disagree. */
  let next;
  if(m.capped_from!=null){
    /* The floor under everything else. The rows below still show every point that was earned —
       and there are usually a lot of them, because these are good runs — so the head has to say
       why the total is not their sum, in the one sentence that is actually the lesson. */
    next=`The patient died. ${m.capped_from} of ${m.max} were earned; a station where she dies `
      +'cannot pass, whatever else was right.';
  }
  else next=nextLine(m.score,m.max,!!m.exam);
  /* The stars land one at a time here too. This is the first place the run's own result
     is stated, and three glyphs appearing together is a number; three arriving in turn
     is the thing being awarded. */
  const head=`<div class="mk-h"><span class="mk-star">${starRow(t,true)}</span>`
    +`<b>${m.score} / ${m.max}</b><span class="mk-pct">${pct}%</span>`
    +`<span class="mk-k">mark sheet</span><span class="mk-next">${next}</span></div>`;
  el.innerHTML=head+m.items.map(it=>
    `<div class="mk-r ${it.mark}"><span class="mk-i">${MKG[it.mark]||'·'}</span>`
    +`<span class="mk-n">${it.label}`
    +(markNote(it)?`<span class="mk-y">${markNote(it)}</span>`:'')
    +`</span><span class="mk-p">${it.earned}/${it.points}</span></div>`).join('');
}

/* The answer, at the only moment it is free to give. A station's card, title card and player
   bar carry the stem now — age, sex, complaint, what you could see from the doorway — because
   the mark sheet pays for naming the diagnosis and a title that names it pays first. Here the clock has stopped and the leaf is computed, so the candidate is
   simply told, beside where the case came from: this is what it was. Episodes have no entry —
   their titles are drama, not diagnosis — and they get no line, exactly like provenance. */
function showReveal(){
  const el=$('#reveal'); if(!el)return;
  const dx=REVEAL[$('#ep').value];
  el.textContent = dx ? `This was: ${dx}` : '';
}

/* Where the case came from — printed once the score exists, which is the only moment it means
   anything to the person reading it. An episode — written for this season, not converted from
   anything — gets no line at all.
   It says "deterministic", not "validated": the mark sheets in demo/rubrics still carry
   `status: provisional … point weights pending clinical review`, and a screen that will be
   filmed must not promise a sign-off nobody has given yet.

   The bank id arrives on the /api/marks payload now, not on the shelf's set table. It used to
   be read out of `memberOf()`, which meant every station's bank id — and a bank id spells its
   own diagnosis into itself — was sitting in the one GET the lobby makes before anybody
   sits anything. Called with the sealed answer, or with nothing to clear the line between
   runs. */
function showProvenance(m){
  const el=$('#prov'); if(!el)return;
  el.textContent = m&&m.bank_case
    ? `case ${m.bank_case} · clinical case bank · deterministic score — anyone can re-derive it`
    : '';
}

/* Straight back into the same case, in the mode it was just sat in — the same entrance the
   up-next card uses, so the recap and title card behave identically whichever door you came
   through. A station is an exam by definition; an episode keeps whatever this run declared. */
onLobby('#runback','onclick',()=>{ cineHide(); enterEpisode($('#ep').value, EXAMRUN); });

onLobby('#copy','onclick',async()=>{ const t=await (await fetch('/api/tape?id='+id+asMe())).json();
  await navigator.clipboard.writeText(JSON.stringify(t,null,1)); $('#copy').textContent='tape copied'; });

/* ─── lobby ────────────────────────────────────────────────────────────────── */
const cleared=()=>JSON.parse(localStorage.getItem('vitals.cleared')||'[]');
/* Which episodes this browser has anchored on chain. Written only from a real anchor result —
   the star is a fact about the chain, cached here so the lobby shows it without a per-case
   query, which the chain does not offer yet. */
const starred=()=>JSON.parse(localStorage.getItem('vitals.starred')||'[]');

/* ─── what opens a door ────────────────────────────────────────────────────────
   Station Sets v2: from EP2 on, an episode door is priced in the stars of ITS OWN set of
   stations and nothing else's. A case's star is three-tier — best proven det ≥70% is ★,
   ≥85% is ★★, ≥95% is ★★★ — and the server computes it from proven attempts (/api/stars),
   so a door that opens here opened because the record earned it. The set table itself
   (members, needs, ceilings) lives on the server (/api/chain) and is only cached here; an
   episode's own exam is a replay for the record — XP and the chain, never a key to a door. */
let SETSRV=null;   /* the set table, from /api/chain — cached so a revisit paints doors instantly */
try{SETSRV=JSON.parse(localStorage.getItem('vitals.sets')||'null')}catch(e){}
let SETSTARS=null; /* {station: 0..3} — the tiers the chain has certified */
let STARS=null;    /* the legacy flat count — kept for the pre-set fallback paths */
let STARBAR=7000;  /* pass bar in bps; refreshed from /api/stars so the two cannot drift */
let EXBAR=8500;    /* the ★★ bar, same discipline */
let FLBAR=9500;    /* the ★★★ bar — a flawless run, not a perfect one: 95%, not 100% */
const TIERMAX=3;   /* what one case can be worth; a set's ceiling is members × this */
/* Which star a deterministic score in basis points has earned, and where the next one sits.
   Floored the way the server's integer arithmetic floors, so the page and the chain never
   disagree by a fraction of a basis point about which side of a bar a run landed on. */
const bpsOf=(score,max)=>max>0?Math.floor(score*10000/max):0;
const tierFor=bps=>bps>=FLBAR?3:bps>=EXBAR?2:bps>=STARBAR?1:0;
const nextBar=bps=>bps>=FLBAR?null:bps>=EXBAR?FLBAR:bps>=STARBAR?EXBAR:STARBAR;
/* What is still on the table, in one sentence — the mark sheet's head and the anchor's
   verdict both print this, so the two can never say different things about the same run.

   **"Flawless" means the sheet is full, and nothing less.** The third star is priced at 95%,
   not 100%, and the head used to ask `nextBar` — which has no bar above ★★★ to name and so
   answers null for everything from 95% up. A 38/40 therefore read exactly like a 40/40:
   "Flawless. There is nothing above this one." printed directly above
   `✗ Supporting workup — ECG, CBC or CXR 0/2`. A headline may not contradict a row below it,
   and a candidate told they were flawless does not come back for the two marks.

   So the top of the ladder is two sentences rather than one. The star is won either way —
   that is what the glyphs beside this say — and where the sheet is not full, this says what
   is left instead of claiming there is nothing. `max - score` is the honest count: it is the
   sheet's own arithmetic, so anything over-ordering took off is on the table too, which it is. */
function nextLine(score,max,exam){
  const again=exam?' Sit it again and the better run is the one that counts.':'';
  const bps=bpsOf(score,max), t=tierFor(bps), nb=nextBar(bps);
  if(nb==null){
    const left=Math.max(0,max-score);
    return left===0
      ? 'Flawless. There is nothing above this one.'
      : `The top tier — and ${left} mark${left===1?'':'s'} still on the table.`+again;
  }
  const need=Math.max(1,Math.ceil(nb*max/10000)-score);
  return `${need} point${need===1?'':'s'} from your ${['first','second','third'][t]} star.`+again;
}
/* The rule, in the words a player reads it in — one sentence, built from the live bars so
   it can never claim a threshold the server is not actually using. */
const barRule=()=>`${STARBAR/100}% pass · ${EXBAR/100}% excellent · ${FLBAR/100}% flawless`;
/* Stars this browser watched being earned — optimistic floors under the chain's answer,
   never substitutes: the anchor takes two signatures to land, and a demo must not stand
   in front of a shut door waiting for finality. vitals.tiers is the per-station floor and
   its values are simply tiers, so a 2 written under the two-tier rules still reads as ★★;
   vitals.stars is the older one-tier list, still read so nobody's door slams shut. */
const localStars=()=>{try{return JSON.parse(localStorage.getItem('vitals.stars')||'[]')}catch(e){return[]}};
const localTiers=()=>{try{return JSON.parse(localStorage.getItem('vitals.tiers')||'{}')}catch(e){return{}}};
/* Bank the tier a run just earned, here, in the browser that watched it happen.
   **This is called when the sheet is marked, not when the run is anchored.** It used to live
   inside the anchor handler alone, and the effect was that the ordinary way out of a station —
   the bell, the up-next card, "← episodes" — threw the stars away: a 40/40 came back to a shelf
   reading ⭐0, a shut door and three empty glyphs, and `vitals.stars` was still null. Anchoring
   is the *proof*, and it stays exactly that; it was never supposed to be the receipt a player
   needs to keep their own progress. The chain remains the higher authority — `tierOf` takes the
   max of this and `/api/stars` — so a floor written here is only ever an optimistic floor under
   an answer that will overwrite it upward, and never a claim of provenance: the "anchored on
   chain" ★ on the card is `vitals.starred`, which only the anchor writes.
   Only ever upward, so sitting a station again and doing worse cannot take a star back. */
function bankTier(e,t){
  if(!e||!t)return false;
  let moved=false;
  try{
    const ls=localStars();
    if(!ls.includes(e)){ls.push(e);localStorage.setItem('vitals.stars',JSON.stringify(ls));moved=true;}
    const m=localTiers();
    if((m[e]||0)<t){m[e]=t;localStorage.setItem('vitals.tiers',JSON.stringify(m));moved=true;}
  }catch(err){ /* a kiosk with storage off still plays; it just cannot remember */ }
  /* A star was just earned here. Tell the next shelf paint to hand it over one glyph at a
     time, and to watch for the door it may have opened. */
  if(moved){ POPFOR.add(e); DOORWATCH=true; }
  return moved;
}
/* One station's effective tier: the best of what the chain certifies and what this browser
   watched land, and never above the ceiling — a corrupted cache must not be able to open a
   door. Only ever asked about set members, so an episode's exam cannot leak in. */
const tierOf=id=>Math.min(TIERMAX,
  Math.max((SETSTARS&&SETSTARS[id])||0, localTiers()[id]||0, localStars().includes(id)?1:0));
const setTotal=s=>s.members.reduce((n,m)=>n+tierOf(m.id),0);
/* What a set is worth today: its playable members × 3. The server sends this; an older
   cached table has to be measured instead, so an upgrade never paints a wrong denominator. */
const setCeiling=s=>s.ceiling!=null?s.ceiling
  :s.members.filter(m=>m.playable!==false).length*TIERMAX;
/* Which set an id belongs to, and which gate an id stands behind: an episode is opened by
   its own set; a station opens alongside the episode it follows (its set's teaching just
   happened), so gate2's stations are free and gate3's wait on gate2 — the old ladder, in
   set language. */
const memberOf=id=>{ if(!SETSRV)return null;
  for(const s of SETSRV){const m=s.members.find(x=>x.id===id);if(m)return{set:s,m};} return null; };
const gateReq=id=>{ if(!SETSRV)return null;
  const own=SETSRV.find(s=>s.opens===id); if(own)return own;
  const gi=SETSRV.findIndex(s=>s.members.some(m=>m.id===id));
  return gi>0?SETSRV[gi-1]:null; };
const starsKnown=()=>SETSRV?SETSRV.reduce((n,s)=>n+setTotal(s),0)
  :Math.max(STARS??0, localStars().length);
/* Which stations can sit an exam — served by /api/chain from the server's rubric map, never
   copied here; and whether the run being started is one. The mode chosen here is what the
   ceremony binds into the commitment. */
let EXAM_EPS=[]; let EXAMRUN=false; let EXAMLIVE=false;
/* Is the thing on screen an exam? Three answers had to agree before anything on the page is
   allowed to hint: EXAMRUN is what the entrance chose, EXAMLIVE what the chain actually bound
   (false on a demo box with no validator, which is exactly when a leak would be filmed), and a
   station is an exam by definition — that is what a station is. Any yes is a yes: everything
   that could give away a marked answer asks this one question, so there is a single place to
   audit rather than a condition per widget. */
const examMode=()=>EXAMRUN||EXAMLIVE||!!(ep()||{}).station;
async function fetchStars(){
  try{
    const me=await identity(); if(!me)return null;
    const r=await (await fetch('/api/stars?account='+acctOf())).json();
    if(r.error||r.stars==null)return null;
    if(r.pass_bps)STARBAR=r.pass_bps;
    if(r.excellent_bps)EXBAR=r.excellent_bps;
    if(r.flawless_bps)FLBAR=r.flawless_bps;
    if(r.sets){const t={};for(const s of r.sets)Object.assign(t,s.tiers);SETSTARS=t;}
    return r.stars;
  }catch(e){ return null }
}
/* Ask again a few times after an anchor: the tiers are computed from what has landed, and
   landing takes seconds. Retries stop once the chain has caught up with every local floor. */
function refreshStars(tries=4){
  fetchStars().then(s=>{
    if(s!=null){ STARS=s; paintSeason(); }
    const lag=()=>{const m=localTiers();
      return Object.keys(m).some(k=>((SETSTARS&&SETSTARS[k])||0)<m[k])
        || localStars().some(k=>memberOf(k)&&(((SETSTARS&&SETSTARS[k])||0)<1));};
    if(tries>0&&(s==null||lag())) setTimeout(()=>refreshStars(tries-1),2500);
  });
}
const open_=i=>{
  const e=SEASON[i];
  if(SETSRV){
    const g=gateReq(e.id);
    return !g || setTotal(g)>=g.need_now;
  }
  /* The set table has not arrived yet (very first paint on a fresh browser, or an offline
     kiosk): the ladder — clear the previous card to open the next, which still walks
     EP1 → OSCE A → EP2 → … left to right. */
  return i===0 || cleared().includes(SEASON[i-1].id);
};
/* Paint now, ask the chain after: a stranger's first click must never wait on an RPC. The
   lobby appears instantly under whatever was last known, and repaints only if the chain's
   answer changes a door or a star. */
async function renderSeason(){
  paintSeason();
  const before=JSON.stringify([STARS,SETSTARS]);
  const s=await fetchStars(); if(s!=null)STARS=s;
  let repaint = JSON.stringify([STARS,SETSTARS])!==before;
  await ledger();
  if(!EXAM_EPS.length||!SETSRV){
    try{ const c=await (await fetch('/api/chain')).json();
      if(c.exam_eps&&c.exam_eps.length){ EXAM_EPS=c.exam_eps; repaint=true; }
      /* The bars arrive here too, so a visitor with no account and no chain still reads
         the real thresholds on the shelf rather than a number baked into this file. */
      if(c.star_bars){ const b=c.star_bars;
        if(b.pass)STARBAR=b.pass; if(b.excellent)EXBAR=b.excellent; if(b.flawless)FLBAR=b.flawless;
        repaint=true; }
      if(c.sets&&c.sets.length){ SETSRV=c.sets; repaint=true;
        try{localStorage.setItem('vitals.sets',JSON.stringify(c.sets))}catch(e){} } }catch(e){}
  }
  if(repaint) paintSeason();
}
/* the three-tier star as a card wears it — always three glyphs wide, so a shelf of cards
   reads as one column of stars rather than a ragged edge — and the circuit band as the art
   monogram. Trimmed to fit the tile: PAEDIATRICS at .3em of letter-spacing runs off it. */
const starGlyph=t=>'★'.repeat(Math.max(0,Math.min(TIERMAX,t)))+'☆'.repeat(Math.max(0,TIERMAX-t));
/* The same three glyphs, wearable. `starGlyph` stays a plain string because several
   callers put it somewhere markup would be wrong; this is the version for the two
   places a star is a reward being handed over — the card on the shelf and the head of
   the mark sheet — where they land one at a time, 120ms apart, instead of the number
   simply being different than it was. */
const starRow=(t,pop)=>[...starGlyph(t)]
  .map((c,i)=>`<i class="sg${pop?' pop':''}" style="--i:${i}">${c}</i>`).join('');
/* Which stations should pop on the next shelf paint, and whether a door opening is
   news. Both are one-shot: filled by the moment that earned them, drained by the paint
   that shows them, so a repaint for any other reason never replays the fanfare. */
const POPFOR=new Set();
let DOORWATCH=false, LOCKED_PREV=null, RINGWAS=null;
const specShort=sp=>(sp||'').replace(/^eir-/,'').replace('gastroenterology','gastro')
  .replace('pulmonology','pulmo').replace('paediatrics','paeds').toUpperCase();
/* What the card is allowed to say about the case's field. The circuit band, and only the
   circuit band: GASTRO over "vomited blood, black stool" tells the candidate it is not the
   heart, which is the exact question osce-d's mark sheet is asking, so the organ specialty is
   not on this endpoint at all any more — it arrives with the mark sheet, after the bell. A
   member with no band (an old cached set table, a member added without one) falls back to the
   widest label there is rather than to anything that names an organ. */
const bandOf=m=>m.band||'medicine';
/* ─── who wrote the case ─────────────────────────────────────────────────────
   The author ledger, indexed by case hash and by shelf id. Fetched once, and
   only ever read — the page shows attribution, it never asserts any.

   **A case with no attribution renders nothing.** Not "author: —", not an empty
   slot, not a dash. An absent line says "we have not published this yet"; a
   placeholder says "this case has no author", and one of those is a claim we
   have no business making about somebody's work.

   The count is of *proven* replays and the word is on screen, because anchored
   and proven are different numbers and the difference is the whole reason the
   ledger can be checked at all. */
let LEDGER=null;
async function ledger(){
  if(LEDGER)return LEDGER;
  LEDGER={byPath:new Map(),byEp:new Map()};
  try{
    const r=await (await fetch('/api/authors')).json();
    /* By lineage, not by version. A case is every archive entry sharing a path, so osce-a
       shows the replays of the version people actually played even though the file on the
       shelf has been revised since. Versions are what get signed; cases are what get read. */
    for(const c of (r.cases||[])){
      const row={authors:c.authors||[],proven:c.proven_replays,paid:c.paid||0};
      LEDGER.byPath.set(c.path,row);
      if(c.ep)LEDGER.byEp.set(c.ep,row);
    }
  }catch(e){ /* no ledger is not an error, it is a page with nothing to add */ }
  return LEDGER;
}
/* A key is a token you move, not text you read — the same eight characters the
   rest of the page shows a key by. */
const authorShort=k=>k.slice(0,8);
/* The payout for the run just finished, once the chain has it.

   Polled rather than asked once: the transfer is a second transaction that lands a moment after
   the proof, and a line that said nothing because it asked too early would be worse than no line.

   **Silence when nothing was paid.** A run that earned the author nothing — no attribution, an
   unlisted key, the day's cap reached — shows no line at all, not a zero. The absence is the
   honest report; a zero reads like a failure that belongs to the learner, and it does not.

   The amount says devnet SOL every time it appears. A number shaped like money, on the screen
   where somebody has just been examined, is the last place to leave that ambiguous. */
async function paidLine(){
  const leaf = (window.LASTLEAF || '').trim();
  if (!leaf) return;
  for (let i = 0; i < 12; i++) {
    await new Promise(r => setTimeout(r, 2500));
    let r;
    try { r = await (await fetch('/api/payout?leaf=' + encodeURIComponent(leaf))).json(); }
    catch (e) { return; }          /* offline: say nothing rather than guess */
    if (r.unknown) return;         /* payouts are off, or the chain would not answer */
    if (!r.paid) continue;
    const sol = (Number(r.lamports || 0) / 1e9).toFixed(5).replace(/0+$/, '').replace(/\.$/, '');
    const sig = String(r.signature || '');
    const el = $('#db');
    if (!el) return;
    const link = sig
      ? ` · <a href="https://explorer.solana.com/tx/${sig}?cluster=devnet" target="_blank" rel="noopener">${sig.slice(0, 8)}</a>`
      : '';
    el.insertAdjacentHTML('beforeend',
      `<div class="db-by"><span class="byline">author paid ${sol} devnet SOL${link}</span></div>`);
    return;
  }
}

function authorLine(row){
  if(!row||!row.authors.length)return '';
  const n=row.proven;
  /* Every key that signed any version of this case — a revised case can have more than one
     author of record, and naming only the latest would take the earlier one's credit. */
  const who=row.authors.map(k=>`<span class="mono">${authorShort(k)}</span>`).join(', ');
  /* Paid is a separate count and is only ever shown when it differs from the replays: a case
     where every proven replay was paid says nothing extra by repeating the same number, and one
     where they differ is the only interesting case — a replay proven and not paid means the cap,
     the allowlist, or no attribution, and hiding that would be the ledger flattering itself. */
  const paid = row.paid !== n ? ` · ${row.paid} paid` : '';
  return `<span class="byline">author ${who}`
    +` · ${n} proven ${n===1?'replay':'replays'}${paid}</span>`;
}

function paintSeason(){
  /* Nothing to draw on a page without the lobby — the ward's shift page has no #eps. */
  if(!$('#eps'))return;
  const seasonIds=new Set(SEASON.map(x=>x.id));
  /* One shut door explains itself; every later one just wears the count — the same lock
     sentence used to repeat on every card down the shelf. */
  let lockNamed=false;
  const card=(e,i)=>{
    const un=open_(i), done=cleared().includes(e.id), star=starred().includes(e.id);
    /* A station's face is a clinic card: the server's set table names the stem, the circuit
       band and the tier; the SEASON entry is only the instant-paint copy, so the server copy
       wins the moment it arrives. */
    const info=e.station?memberOf(e.id):null;
    const title=info?info.m.title:e.t, spec=info?bandOf(info.m):e.spec;
    const tierName=info?info.m.tier:e.tier;
    const pr=progOf(e);
    /* Empty unless this case has a published author — see `ledger`. Computed here, above both
       card templates: a locked case still has whoever wrote it, and a byline that appeared only
       once a door opened would read as authorship being something the reader earns. */
    const by=LEDGER?authorLine(LEDGER.byEp.get(e.id)):'';
    /* A station card is typographic and stays that way. Two of them wore an ECG crop for a
       while, and it was the wrong picture for the job twice over: a card has to say *who the
       patient is*, and a 12-lead says nothing about that — it is one investigation out of a
       case, hung on the door before the candidate has decided to order it. The films stay
       exactly where they belong, in the bay, when they are asked for. */
    const art=e.art?artOf(e)
      :e.station?`<div class="art-ph art-cl"><span>${specShort(spec)}</span></div>`
      :`<div class="art-ph ph-t-wrap ph-${e.id}"><span class="ph-t">${e.t}</span></div>`;
    /* No provenance badge on the face. In the game nobody has heard of the case bank — it read
       as a second brand on a card that only has to say what the case is, who it is, and what
       your star on it is. Where provenance actually buys credibility is under a score somebody
       just earned (#prov, after the run) and on the landing page; both say it there. */
    const chip=e.station
      ?`<span class="st-stars" title="your best exam here — ${barRule()}">${starRow(tierOf(e.id),POPFOR.has(e.id))}</span>`
      :`<span class="tier">${e.tier}</span>`;
    const chips=e.station
      ?`<span class="chiprow"><span class="spec">${spec||''}</span><span class="rt">${tierName} · ${e.rt||''}</span></span>`
      :`<span class="rt">${e.rt||''}</span>`;
    const no=e.station?e.n:(e.sn||e.n);
    const bar=(!e.station&&pr>0)?`<span class="prog"><i style="width:${Math.round(pr*100)}%"></i></span>`:'';
    /* A locked card is a <div>, not a disabled <button>: a disabled button swallows
       its children's clicks, and the trailer door inside has to stay clickable. A door's
       price is its own set's stars — no claim button here: the count moves when an
       anchored exam clears a bar, so the door opens the moment the record earns it and
       not a click sooner. */
    if(!un){
      const g=gateReq(e.id);
      const tease=g?`⭐${setTotal(g)}/${g.need_now}`:'locked';
      let go='';
      if(!lockNamed){
        lockNamed=true;
        go=`<span class="go">${g
          ?`open at ⭐${g.need_now} from its stations — you have ${setTotal(g)}${g.need_now<g.need?' · full set coming':''}`
          :`clear ${SEASON[i-1]?SEASON[i-1].n:''} to open`}</span>`;
      }
      return `<div class="tile lock${e.station?' st':''}" data-ep="${e.id}">
      <div class="art">${art}<span class="no">${no}</span>${chip}<span class="tease">${tease}</span></div>
      <div class="body"><h3>${title}</h3>${chips}<p>${e.d}</p>${by}
      ${go}${TRAILER[e.id]?`<button class="go trail" data-trailer="${e.id}" data-ga="trailer:${e.id}">▶ watch trailer</button>`:''}</div></div>`;
    }
    /* The exam door on an open episode is a replay for the record — XP and the anchored
       tape, never a key: doors are priced in station stars alone. */
    const replay=!e.station&&EXAM_EPS.includes(e.id)
      ?`<span class="go exam" data-exam="${e.id}" data-ga="exam:${e.id}" title="an exam replay for XP and the record — doors open on station stars">↻ replay for the record</span>`:'';
    return `<button class="tile${e.station?' st':''}" data-ep="${e.id}" data-ga="tile:${e.id}">
      <div class="art">${art}<span class="no">${no}${star?' <span title="anchored on chain">★</span>':''}${done?' · watched':''}</span>${chip}${bar}</div>
      <div class="body"><h3>${title}</h3>${chips}<p>${e.d}</p>${by}
      <span class="go">${e.station?'★ sit the station — exam conditions':(done?'watch again':'enter the bay')}</span>${replay}</div></button>`;
  };
  /* A declared member whose files are still being fitted (Phase 5b) is a card, not an
     error — it sits with its set and says what it will be. */
  const comingCard=(m,s)=>`<div class="tile st cs" data-ga="coming:${m.id}">
      <div class="art"><div class="art-ph art-cl"><span>${specShort(bandOf(m))}</span></div>
        <span class="no">${m.id.replace('osce-','OSCE ').toUpperCase()}</span>
        <span class="st-stars" title="${barRule()}">${starRow(0,false)}</span></div>
      <div class="body"><h3>${m.title}</h3>
        <span class="chiprow"><span class="spec">${bandOf(m)}</span><span class="rt">${m.tier}</span></span>
        <p>Being fitted for the bay now.</p>
        <span class="go">coming soon${s.need_now<s.need?` · door opens at ⭐${s.need_now} until the full set lands`:''}</span></div></div>`;
  const cards=[];
  for(const [i,e] of SEASON.entries()){
    cards.push(card(e,i));
    /* after a set's last on-shelf station, its declared-but-unpublished members follow */
    if(e.station&&SETSRV){
      const info=memberOf(e.id);
      if(info){
        const onShelf=info.set.members.filter(m=>seasonIds.has(m.id));
        if(onShelf.length&&onShelf[onShelf.length-1].id===e.id)
          for(const m of info.set.members.filter(x=>!seasonIds.has(x.id)))
            cards.push(comingCard(m,info.set));
      }
    }
  }
  $('#eps').innerHTML=cards.join('');
  /* The deck places what was just written, in the same frame: without this the
     seventeen cards spend one paint stacked on top of each other in the middle. */
  cfSync();
  /* ── the door opens ───────────────────────────────────────────────────────────
     A card that was locked the last time this shelf was painted and is not now had a
     key turn in it, and that is a moment: it comes up out of its own blur, lifts eight
     pixels, carries a glow for most of a second and then settles into the ordinary
     shadow of a card you can reach. Gated on DOORWATCH so the two or three repaints a
     page load takes — the cached table, then the chain's answer — cannot fake one.

     All of it is gated on the shelf actually being on screen. refreshStars() repaints
     while the player is still reading the debrief in the bay, and a door that opened
     behind a hidden element would be "already seen" by the time they walked back out —
     LOCKED_PREV is deliberately the state the player last *looked* at, not the last
     state painted. */
  if(!$('#lobby').classList.contains('hide')){
    const nowLocked=new Set();
    $('#eps').querySelectorAll('.tile.lock').forEach(t=>nowLocked.add(t.dataset.ep));
    if(DOORWATCH&&LOCKED_PREV){
      for(const wasId of LOCKED_PREV){
        if(nowLocked.has(wasId))continue;
        const t=$('#eps').querySelector('.tile[data-ep="'+wasId+'"]');
        if(!t)continue;
        t.classList.add('just-open');
        anim(950,()=>t.classList.remove('just-open'));
      }
    }
    LOCKED_PREV=nowLocked; POPFOR.clear();
  }
  $('#eps').querySelectorAll('.tile:not(.lock):not(.cs)').forEach(b=>b.onclick=()=>{
    /* A station card is only ever an exam — that is what a station is. Episodes enter as
       practice unless the exam door below is the thing clicked. */
    const entry=SEASON.find(x=>x.id===b.dataset.ep);
    /* The card is handed to the entrance, not just its id: the shared element measures
       this exact rectangle, so the thing that grows into the screen is the thing the
       thumb was on. */
    enterEpisode(b.dataset.ep, !!(entry&&entry.station), b); });
  /* The exam door on the same card: same bay, but the ceremony will bind exam into the
     commitment before the first order is possible. */
  $('#eps').querySelectorAll('[data-exam]').forEach(x=>x.onclick=(ev)=>{
    ev.stopPropagation(); enterEpisode(x.dataset.exam,true,x.closest('.tile')); });
  /* A5 · the trailer door on a locked card. */
  $('#eps').querySelectorAll('[data-trailer]').forEach(x=>x.onclick=(ev)=>{
    ev.stopPropagation(); playTrailer(x.dataset.trailer); });
  /* A4 · motion on the open cards: hover on a pointer, first touch on a phone —
     the touch starts the loop and the tap still enters, so nothing costs a second tap. */
  $('#eps').querySelectorAll('.tile:not(.lock):not(.cs)').forEach(b=>{
    b.addEventListener('pointerenter',()=>motionOn(b),{passive:true});
    b.addEventListener('pointerleave',()=>motionOff(b));
    b.addEventListener('touchstart',()=>motionOn(b),{passive:true});
  });
  $('#coin-n').textContent=cleared().length;
  renderHero(); paintSets(); paintRing(); teaserWatch();
}

/* ── A2b · the deck ───────────────────────────────────────────────────────────
   Seventeen cards on a coverflow. paintSeason() owns what a card says; this owns
   only where it stands, and the two never argue because they touch different
   properties: the template writes the innerHTML, this writes transform, opacity,
   z-index and one custom property for the shade.

   The numbers, and why they are these numbers. The gap between card centres is
   .95 of a card width — the landing page runs at .39 and reads as a fan of edges,
   which is right for four posters a buyer glances at once and wrong for a shelf a
   player works in. At .95, with 38° of lean on the first neighbour and a scale that
   drops 9% a step, the neighbour's inner edge lands a few pixels clear of the middle
   card's outer one: air between them, every side card whole and readable, and the
   lean carrying the depth on its own instead of the overlap. The lean is capped at
   52° and never approaches the 90° where a card turns edge-on and disappears.

   Nothing here wraps. A season has a first card and a last one, and a deck that
   rolled EP5 round to EP1 would say the shelf is a loop when it is a ladder. */
const CFV={i:0,key:null,moved:false,drag:null,wheel:0,seeded:false,fit:0};
/* No shelf, no cards. The ward's shift page has no #eps at all. */
const cfList=()=>$('#eps')?[].slice.call($('#eps').children):[];
const cfKey=(el,i)=>el.dataset.ep||el.dataset.ga||('#'+i);
/* Two neighbours a side on a desk, one on a phone — past that the card is off the
   screen anyway, and an off-screen card that is still in the focus order is a tab
   stop into nowhere. */
const cfNear=()=>innerWidth<=560?1:2;
function cfPaint(){
  /* Nothing to draw on a page without the lobby — the ward's shift page has no #eps. */
  if(!$('#eps'))return;
  const cards=cfList(); if(!cards.length)return;
  const w=cards[0].offsetWidth||240, gap=Math.round(w*0.95), near=cfNear();
  cards.forEach((c,i)=>{
    const off=i-CFV.i, a=Math.abs(off), far=a>near;
    /* an episode outranks the stations that open its door, and stands taller in the
       slot to say so — the width is shared now, so the rank moved into the scale. */
    const rank=c.classList.contains('st')?.94:1;
    const lean=off===0?0:(off<0?1:-1)*Math.min(52,38+(a-1)*7);
    c.style.transform='translateY(-50%) translateX('+(off*gap)+'px) '
      +'translateZ('+(-a*110)+'px) rotateY('+lean+'deg) scale('+(rank*(1-a*0.09))+')';
    c.style.opacity=far?'0':'1';
    c.style.pointerEvents=far?'none':'auto';
    c.style.zIndex=String(100-a);
    c.style.setProperty('--cf-shade',String(Math.min(a*0.3,0.72)));
    c.dataset.side=off<0?'l':'r';
    if(off===0)c.setAttribute('aria-current','true'); else c.removeAttribute('aria-current');
    /* Only the card in front is a tab stop. Seventeen of them in the tab order meant
       Tab out of the deck rewound it to the first card and walked the whole season a
       press at a time; now Tab reaches the card you are looking at and then leaves,
       and the arrows — which is what the deck's label offers — do the browsing. */
    if(off===0)c.removeAttribute('tabindex'); else c.setAttribute('tabindex','-1');
    /* A card nobody can see must not be a place the keyboard can get to, and it must
       not be read out either. `inert` does both in one word; where it is missing, the
       aria half is still worth having. But inerting the element that currently holds
       focus drops focus on the floor — three arrow presses and the keyboard had lost
       the deck entirely — so the deck takes it back first. */
    if(far&&c.contains(document.activeElement)){
      try{ $('#eps').focus({preventScroll:true}); }catch(err){} }
    if('inert' in c)c.inert=far; else c.setAttribute('aria-hidden',far?'true':'false');
  });
  cfRail(cards); cfWhere(cards);
}
/* The deck is as tall as its tallest card, measured rather than guessed: the shut
   door that names its price runs to three lines and a fixed box would crop it. */
function cfFit(cards){
  /* Nothing to draw on a page without the lobby — the ward's shift page has no #eps. */
  if(!$('#eps'))return;
  let tall=0;
  for(const c of cards){ const h=c.offsetHeight; if(h>tall)tall=h; }
  if(tall&&tall!==CFV.fit){ CFV.fit=tall; $('#eps').style.height=(tall+40)+'px'; }
}
function cfGo(n){
  const cards=cfList(); if(!cards.length)return;
  CFV.i=Math.max(0,Math.min(cards.length-1,n));
  CFV.key=cfKey(cards[CFV.i],CFV.i);
  cfPaint();
  /* The deck stopping somewhere is the only thing that makes one card worth asking the
     server a question about — see `teaserWatch`. */
  teaserWatch();
}
/* Where you are, said in words. Seventeen dots would be seventeen targets and no
   answer; this is one line that names the card and admits when it is shut. */
function cfWhere(cards){
  /* Nothing to draw on a page without the lobby — the ward's shift page has no #eps-where. */
  if(!$('#eps-where'))return;
  const c=cards[CFV.i]; if(!c)return;
  const e=c.dataset.ep?SEASON.find(x=>x.id===c.dataset.ep):null;
  const h=c.querySelector('h3');
  const state=c.classList.contains('lock')?' · locked'
    :c.classList.contains('cs')?' · coming soon':'';
  const w=$('#eps-where'); w.textContent='';
  const line=document.createElement('span');
  line.textContent=(CFV.i+1)+' of '+cards.length+(e?' · '+e.n:'')+state;
  const name=document.createElement('b');
  name.textContent=h?h.textContent:'';
  name.title=name.textContent;
  w.appendChild(line); w.appendChild(name);
  $('#eps-prev').disabled=CFV.i<=0;
  $('#eps-next').disabled=CFV.i>=cards.length-1;
}
/* The rail: the season's shape, not its inventory. Rebuilt only when the deck's
   contents actually change — a repaint for a star that moved must not throw away
   the buttons under the player's cursor. */
function cfRail(cards){
  /* Nothing to draw on a page without the lobby — the ward's shift page has no #eps-rail. */
  if(!$('#eps-rail'))return;
  const rail=$('#eps-rail');
  const sig=cards.map((c,i)=>cfKey(c,i)+(c.classList.contains('lock')||c.classList.contains('cs')?'!':'')).join('|');
  if(rail.dataset.sig!==sig){
    rail.dataset.sig=sig; rail.textContent='';
    cards.forEach((c,i)=>{
      const e=c.dataset.ep?SEASON.find(x=>x.id===c.dataset.ep):null;
      const door=!!e&&!e.station;
      const t=document.createElement(door?'button':'span');
      t.className='cf-tick '+(door?'tk-ep':'tk-st')
        +(c.classList.contains('lock')||c.classList.contains('cs')?' lk':'');
      if(door){
        t.type='button';
        t.title=e.n+' · '+e.t;
        t.setAttribute('data-ga','shelf_jump:'+e.id);
        t.setAttribute('aria-label','go to '+e.n+' — '+e.t);
        t.addEventListener('click',()=>cfGo(i));
      } else t.setAttribute('aria-hidden','true');
      rail.appendChild(t);
    });
  }
  [].forEach.call(rail.children,(t,i)=>t.classList.toggle('at',i===CFV.i));
}
/* Called by paintSeason the moment it has written the cards. The deck keeps its
   place across a repaint by the card's own id rather than its index, so a
   coming-soon member arriving mid-season cannot shuffle the shelf under the thumb. */
function cfSync(){
  const cards=cfList(); if(!cards.length)return;
  if(!CFV.seeded){
    CFV.seeded=true;
    /* First paint opens on the same entry the billboard is offering — the shift you
       left half-done, or EP1 on a spotless browser. The shelf and the hero should not
       have to be reconciled by the player. */
    try{ const h=heroPick();
      if(h&&h.e){ const j=cards.findIndex(c=>c.dataset.ep===h.e.id); if(j>=0)CFV.i=j; } }catch(err){}
  } else if(CFV.key){
    const j=cards.findIndex((c,i)=>cfKey(c,i)===CFV.key);
    if(j>=0)CFV.i=j;
  }
  CFV.i=Math.max(0,Math.min(cards.length-1,CFV.i));
  CFV.key=cfKey(cards[CFV.i],CFV.i);
  cfFit(cards); cfPaint();
}
/* Bound once, on the deck itself — paintSeason replaces the cards inside it several
   times a visit and would take any handler bound to a card with them. */
function cfWire(){
  /* Nothing to draw on a page without the lobby — the ward's shift page has no #eps. */
  if(!$('#eps'))return;
  const deck=$('#eps');
  /* A click on a card that is not the middle one turns the deck instead of entering
     the bay, and a click that is really the end of a drag does nothing at all. Both
     have to happen in the capture phase: paintSeason binds enterEpisode straight onto
     the card, and by the target phase it is already too late to say no. */
  deck.addEventListener('click',e=>{
    const c=e.target.closest&&e.target.closest('.tile');
    if(!c||c.parentElement!==deck)return;
    const i=cfList().indexOf(c);
    if(CFV.moved||i!==CFV.i){ e.stopPropagation(); e.preventDefault();
      if(!CFV.moved&&i>=0)cfGo(i); }
  },true);
  /* There is deliberately no focus-follows rule here, and it cost two regressions to
     learn why. Turning the deck when a card takes focus reads well until you notice
     what focuses a card: the press that is about to click it. The deck would arrive at
     the card first, the click would then find nothing to intercept, and a side card
     would open a case instead of centring — on a mouse it happened between pointerdown
     and click, on a touchscreen the focus lands after pointerup, so no pointer-state
     guard covers both. The roving tabindex in cfPaint makes the rule unnecessary
     anyway: the only card Tab can reach is the one already in front of you. */
  deck.addEventListener('keydown',e=>{
    if(e.key==='ArrowLeft')cfGo(CFV.i-1);
    else if(e.key==='ArrowRight')cfGo(CFV.i+1);
    else if(e.key==='Home')cfGo(0);
    else if(e.key==='End')cfGo(cfList().length-1);
    else return;
    e.preventDefault();
  });
  $('#eps-prev').addEventListener('click',()=>cfGo(CFV.i-1));
  $('#eps-next').addEventListener('click',()=>cfGo(CFV.i+1));
  deck.addEventListener('pointerdown',e=>{
    if(e.button>0)return;
    CFV.drag={x:e.clientX,id:e.pointerId,cap:false}; CFV.moved=false;
    deck.classList.add('dragging');
  });
  deck.addEventListener('pointermove',e=>{
    if(!CFV.drag)return;
    const dx=e.clientX-CFV.drag.x;
    /* Capture only once this is a drag and not a click: a captured pointer retargets
       the click to the deck, which would cost us click-a-card-to-centre-it. */
    if(!CFV.drag.cap&&Math.abs(dx)>10){
      try{deck.setPointerCapture(CFV.drag.id);}catch(err){} CFV.drag.cap=true; }
    if(Math.abs(dx)>40){ cfGo(CFV.i+(dx<0?1:-1)); CFV.drag.x=e.clientX; CFV.moved=true; }
  });
  const release=()=>{
    if(!CFV.drag)return;
    if(CFV.drag.cap){try{deck.releasePointerCapture(CFV.drag.id);}catch(err){}}
    CFV.drag=null; deck.classList.remove('dragging');
    if(CFV.moved)setTimeout(()=>{CFV.moved=false;},60);
  };
  deck.addEventListener('pointerup',release);
  deck.addEventListener('pointercancel',release);
  /* A trackpad's sideways flick is the same gesture as the drag and the shelf used to
     answer it, so it still does. The vertical one is the page's and is left alone. */
  deck.addEventListener('wheel',e=>{
    if(Math.abs(e.deltaX)<=Math.abs(e.deltaY))return;
    e.preventDefault();
    const now=Date.now(); if(now-CFV.wheel<220)return;
    CFV.wheel=now; cfGo(CFV.i+(e.deltaX>0?1:-1));
  },{passive:false});
  let rz=null;
  addEventListener('resize',()=>{ clearTimeout(rz); rz=setTimeout(()=>{
    const cards=cfList(); if(!cards.length)return;
    CFV.fit=0; cfFit(cards); cfPaint(); },120); });
  /* The cards are as tall as their type, and the type arrives after the first paint. */
  if(document.fonts&&document.fonts.ready)document.fonts.ready.then(()=>{
    const cards=cfList(); if(cards.length){ CFV.fit=0; cfFit(cards); cfPaint(); } }).catch(()=>{});
}
cfWire();

/* ── the set strip ── the door prices, said in the only unit that opens them.
   A station card wears its own star; nothing on the shelf added them up, so a player
   three stars into a six-star door had to count cards to find out. One chip per set:
   what you have of what the set is worth ("6 / 9 ⭐"), the door's price marked on the
   bar, and the rule underneath — 70% pass · 85% excellent · 95% flawless. */
function paintSets(){
  /* Nothing to draw on a page without the lobby — the ward's shift page has no #setbar. */
  if(!$('#setbar'))return;
  const el=$('#setbar'); if(!el)return;
  if(!SETSRV||!SETSRV.length){ el.innerHTML=''; return; }
  el.innerHTML=SETSRV.map(s=>{
    const got=setTotal(s), cap=setCeiling(s), need=s.need_now!=null?s.need_now:s.need;
    const ep=SEASON.find(x=>x.id===s.opens)||{};
    const open=got>=need;
    /* Against the ceiling, not against the price: a bar that fills at the door would say
       a full set and an opened door are the same thing, and they are not. */
    const fill=cap?Math.min(100,Math.round(got*100/cap)):0;
    const mark=cap?Math.min(100,Math.round(need*100/cap)):0;
    return `<div class="setchip${open?' done':''}">
      <span class="sc-h"><span class="sc-g">${(ep.sn||s.opens||'').toUpperCase()} door</span>
        <span class="sc-n"><b>${got}</b> / ${cap} ⭐</span></span>
      <span class="sc-b" title="${got} of ${cap} · opens at ${need}"><i style="width:${fill}%"></i><u style="left:${mark}%"></u></span>
      <span class="sc-d">${open?'open — '+(ep.n||s.opens)+' is yours'
        :'opens at ⭐'+need+' · '+(need-got)+' to go'}${s.need_now!=null&&s.need!=null&&s.need_now<s.need?' · full set coming':''}</span>
    </div>`;
  }).join('')+`<span class="sc-rule">a station's star: ${barRule()}</span>`;
}

/* ── A1/A2 · what the shelf remembers ─────────────────────────────────────────
   vitals.seen: per entry, the furthest the case clock ever ran here and when it last
   did. Written by paint(), read by the hero and the per-card bars. A convenience
   cache like vitals.cleared, never an authority on anything the chain certifies. */
const seenMap=()=>{try{return JSON.parse(localStorage.getItem('vitals.seen')||'{}')}catch(e){return{}}};
let seenMark=-1e9;
function markSeen(epId,t){
  const m=seenMap(), cur=m[epId]||{t:0};
  m[epId]={t:Math.max(t,cur.t||0),at:Date.now()};
  try{localStorage.setItem('vitals.seen',JSON.stringify(m))}catch(e){}
}
/* How much of the card's bar to fill: watched fills it, a part-run shows where it
   stands against the shift's runtime, and a run that never really began shows nothing. */
const progOf=e=>{
  if(cleared().includes(e.id))return 1;
  const s=seenMap()[e.id];
  if(!s||!e.mins)return 0;
  const f=s.t/(e.mins*60);
  return f>0?Math.min(.94,Math.max(.06,f)):0;
};
/* Which entry the billboard wears. A part-watched shift wins; failing that, the next
   open door on the mainline — stations included, because the mainline runs through
   them; a spotless browser gets EP1 wearing the marketing line. */
function heroPick(){
  const m=seenMap(), cl=cleared();
  let cont=null;
  for(const e of SEASON){ const x=m[e.id];
    if(x&&!cl.includes(e.id)&&(!cont||x.at>(m[cont.id]||{}).at)) cont=e; }
  if(cont)return{e:cont,mode:'continue'};
  const nxt=SEASON.find((e,i)=>!cl.includes(e.id)&&open_(i));
  if(nxt)return{e:nxt,mode:(cl.length||Object.keys(m).length)?'next':'new'};
  return{e:SEASON[SEASON.length-1],mode:'rewatch'};
}
function renderHero(){
  /* Nothing to draw on a page without the lobby — the ward's shift page has no #hero. */
  if(!$('#hero'))return;
  const p=heroPick(), e=p.e, pr=progOf(e);
  const art=e.art?artOf(e,true)
    :`<div class="hero-void ph-${e.id}"><span>${e.sn||e.n}</span></div>`;
  const eye = p.mode==='continue'?`Continue watching · ${e.sn||e.n}`
    : p.mode==='next'?`Up next · ${e.sn||e.n}`
    : p.mode==='rewatch'?`Watch again · ${e.sn||e.n}`
    : `${e.sn} · ${e.t}`;
  const h1 = p.mode==='new'
    ? 'A patient is dying on a clock<br>and you decide what happens next' : e.t;
  const line = p.mode==='new'
    ? 'Talk to her, treat her, and watch the monitor tell you whether you were right. Anyone can play. Nobody can fake the replay.'
    : e.d;
  const play = p.mode==='continue'?`▶ Continue ${e.sn||e.n}`
    : p.mode==='new'?'▶ Play S1:E1':`▶ Play ${e.sn||e.n}`;
  $('#hero').innerHTML=`<div class="hero-art">${art}<div class="hero-shade"></div></div>
    <div class="hero-txt"><div class="hero-eye">${eye}</div><h1>${h1}</h1><p>${line}</p>
      <div class="hero-act"><span class="hero-play">${play}</span><span class="hero-rt">${e.rt||''}</span></div>
      ${pr>0&&pr<1?`<div class="hero-prog"><i style="width:${Math.round(pr*100)}%"></i></div>`:''}</div>`;
  /* The billboard is a rectangle like any other card, so it takes the same move — it is
     simply a very large card, and the screen it grows into is nearly its own size. */
  $('#hero').onclick=()=>enterEpisode(e.id,e.station,$('#hero'));
}
/* C10 · the ring: five episodes round the dial, stars under it. Stations are the
   season's interludes, so only the five shifts count toward the circle. The star line
   is read against the season's whole ceiling — every playable station × 3 — so the
   number has something behind it instead of climbing toward nothing. */
const starCeiling=()=>SETSRV?SETSRV.reduce((n,s)=>n+setCeiling(s),0):0;
function paintRing(){
  /* Nothing to draw on a page without the lobby — the ward's shift page has no #ring. */
  if(!$('#ring'))return;
  const n=SEASON.filter(e=>!e.station&&cleared().includes(e.id)).length;
  const st=starsKnown(), cap=starCeiling();
  const C=97.4;
  /* The arc runs itself in rather than appearing at its new length \u2014 one full dash with
     the offset walked back to the target, which is the compositor's own way of drawing a
     circle. It only animates when the number actually moved: paintSeason runs two or
     three times on an ordinary load, and a ring that refilled on each of them would be a
     tic rather than a reward. */
  const grew=RINGWAS!=null&&RINGWAS!==n&&!REDUCE();
  const off=(C-C*n/5).toFixed(1);
  $('#ring').innerHTML=`<svg viewBox="0 0 36 36" aria-hidden="true">
      <circle cx="18" cy="18" r="15.5" class="ring-bg"/>
      <circle cx="18" cy="18" r="15.5" class="ring-fg" stroke-dasharray="${C} ${C}"
        stroke-dashoffset="${grew?C:off}"${grew?' style="transition:stroke-dashoffset 600ms var(--ease)"':''}/>
      <text x="18" y="21.5" class="ring-n">${n}</text></svg>
    <span class="ring-t"><b>${n} of 5</b> episodes<br>\u2b50 ${cap?`${st} of ${cap}`:st} star${st===1&&!cap?'':'s'}</span>`;
  if(grew){ const a=$('#ring').querySelector('.ring-fg');
    requestAnimationFrame(()=>{ if(a)a.setAttribute('stroke-dashoffset',off); }); }
  RINGWAS=n;
}

/* ── A4 · motion previews ─────────────────────────────────────────────────────
   One failed clip turns previews off for the visit, the way the film flag does —
   every later hover takes the Ken Burns path instead of re-requesting missing files. */
let PVOK=true;
function motionOn(tile){
  /* On the deck only the card facing you moves. The four leaning behind it are
     scenery — five clips playing at once to serve one hover is a frame nobody
     asked for, and a Ken Burns drift on a card turned 38° away is invisible. */
  if(tile.parentElement&&tile.parentElement.id==='eps'
     &&tile.getAttribute('aria-current')!=='true')return;
  if(tile.dataset.pv)return; tile.dataset.pv='1';
  const e=SEASON.find(x=>x.id===tile.dataset.ep)||{};
  const img=tile.querySelector('.art img');
  if(e.preview&&PVOK){
    const v=document.createElement('video');
    v.className='pv'; v.muted=true; v.loop=true; v.playsInline=true; v.autoplay=true;
    const dead=()=>{ if(!v.isConnected)return; PVOK=false; v.remove(); if(img)img.classList.add('kb'); };
    v.onerror=dead;
    const t=setTimeout(()=>{ if(v.readyState<2)dead(); },1500);
    v.addEventListener('playing',()=>clearTimeout(t),{once:true});
    v.src='/clip/'+e.preview+'.mp4';
    tile.querySelector('.art').appendChild(v);
    v.play().catch(()=>{});
  } else if(img) img.classList.add('kb');
}
function motionOff(tile){
  delete tile.dataset.pv;
  const v=tile.querySelector('.pv'); if(v)v.remove();
  const img=tile.querySelector('.art img'); if(img)img.classList.remove('kb');
}

/* ── A5 · the trailer flag ────────────────────────────────────────────────────
   The Watch-trailer door exists only when the asset does, and the page has no list to
   read: `/clip/` serves a file if it is on the volume and 404s if it is not, and there is
   no manifest endpoint to ask instead. So the door is still discovered by asking — but it
   asks about one card, when there is a reason to, and remembers the answer.

   **What it used to do.** `probeTrailers()` swept every locked entry on every shelf paint.
   Seventeen fetches, all 404, none of them cached (a 404 carries no `Cache-Control`), and
   `paintSeason` runs two or three times a load — the cached set table, then the chain's
   answer — with no re-entrancy guard on an async sweep, so a single shelf load put 26 to 30
   console errors on the record. Nothing looked broken, because the still-image fallback is
   the design; that is the problem. A console with thirty expected errors in it is a console
   nobody reads the real one out of.

   **What it does now.** Zero requests to paint a shelf. One request when the deck settles
   on a locked card the player has actually browsed to, and only if this browser does not
   already know the answer — `vitals.teasers` keeps it for a day, which is the same day the
   server asks for on a hit (`Cache-Control: public, max-age=86400`), so a teaser that lands
   is found within a day at the outside and usually on the next visit. No list is hardcoded
   and nothing has to be edited when the T2 teasers arrive on the volume (EP5 first, per
   SERIES_UX D): the button still appears by itself, on the card being looked at.

   The fetch is still aborted the moment the verdict is in, so a probe never downloads a
   film, and a negative answer is remembered exactly like a positive one — the whole point
   is to stop asking a question whose answer is already known. */
const TRAILER={};
const TEASER_TTL=864e5;          /* a day, matching the server's own max-age on a hit */
let TEASER_INFLIGHT=null;        /* one probe at a time; a swipe must not open sixteen */
/* What this browser last learned, by id. Reading is best-effort: a kiosk with storage off
   simply asks once per page load instead of once per day, which is still not seventeen. */
function teaserCache(){
  try{ const c=JSON.parse(localStorage.getItem('vitals.teasers')||'null');
    if(c&&c.at&&Date.now()-c.at<TEASER_TTL&&c.ids)return c; }catch(e){}
  return null;
}
function teaserRemember(id,ok){
  try{ const c=teaserCache()||{at:Date.now(),ids:{}};
    c.ids[id]=ok; localStorage.setItem('vitals.teasers',JSON.stringify(c));
  }catch(e){}
}
/* Ask about one entry, at most once. Returns nothing; a hit repaints the shelf, which is
   the only visible consequence there is. */
async function probeTrailer(id){
  if(!id||TRAILER[id]!==undefined||TEASER_INFLIGHT)return;
  const c=teaserCache();
  if(c&&c.ids[id]!==undefined){ TRAILER[id]=c.ids[id]; if(TRAILER[id])paintSeason(); return; }
  TEASER_INFLIGHT=id;
  try{
    const ctl=new AbortController();
    const r=await fetch('/clip/'+id+'_teaser.mp4',{signal:ctl.signal});
    TRAILER[id]=r.ok; ctl.abort();
  }catch(err){ TRAILER[id]=false; }
  finally{ TEASER_INFLIGHT=null; }
  teaserRemember(id,TRAILER[id]);
  if(TRAILER[id])paintSeason();
}
/* The deck settling on a card is the moment that card is worth a question — it is the one
   in front of the player, at full size, with its own trailer door. Debounced, so swiping
   the length of the season asks about where the thumb stopped and not about the fifteen
   cards it went past. */
let TEASER_WAIT=null;
function teaserWatch(){
  clearTimeout(TEASER_WAIT);
  TEASER_WAIT=setTimeout(()=>{
    const el=cfList()[CFV.i];
    const id=el&&el.dataset.ep;
    if(id&&el.classList.contains('lock'))probeTrailer(id);
  },420);
}
function playTrailer(epId){
  const e=SEASON.find(x=>x.id===epId)||{};
  cineShow([{skip:false,html:'<div class="trailer"><div class="tr-h">'+(e.sn||e.n||'')
    +' · '+(e.t||'').toUpperCase()+' — teaser</div>'
    +'<video src="/clip/'+epId+'_teaser.mp4" controls autoplay playsinline></video>'
    +'<button class="btn" data-tr-x data-ga="trailer-close">close</button></div>',
    wire:r=>{ r.querySelector('[data-tr-x]').onclick=()=>cineHide();
      const v=r.querySelector('video'); v.onended=()=>cineHide(); v.onerror=()=>cineHide(); }}]);
}

/* ══ Phase 12 · the motion engine ════════════════════════════════════════════
   Everything that moves between scenes runs through here, and it exists so that
   four promises can be kept in one place instead of seventeen.

   **Skippable, all of it.** Every animation registers a finisher. Any pointerdown
   or keydown anywhere on the page drains the set, and each finisher runs its own
   tail — the scene swap, the reveal, the callback — immediately. Nothing can be
   left half-played, and nothing needs its own escape hatch.

   **Reduced motion is a different animation, not a faster one.** REDUCE() is read
   at call time rather than cached, so a player who changes the system setting mid
   session gets the new behaviour without a reload. The CSS halves its durations
   from the same media query.

   **The patient's clock is never held.** Every call site here is either before
   /api/new or after the outcome has landed. The monitor boot is the one that looks
   like an exception and is not: it animates numbers that are already painted and
   already correct, on a case whose first tick is 700ms away.

   **transform, opacity, clip-path.** The title card's blur and letter-spacing are
   the single named exception, asked for by the brief. */
const REDUCE=()=>matchMedia('(prefers-reduced-motion: reduce)').matches;
/* The phone break, asked in script rather than read off the stylesheet. The query is the
   same string the CSS uses — `(max-width:640px)` — and the two have to stay that way: it is
   what decides whether the up-next step is a curtain or a sheet, and a script that thought
   it was a sheet while the CSS was still painting a curtain would scroll the page behind a
   black screen. One place to change it, here and in the `.cine.at-foot` block. */
const PHONE=()=>matchMedia('(max-width:640px)').matches;
const INFLIGHT=new Set();
/* Drained on any input. A finisher must be idempotent and must remove itself — the
   pattern below (`ended` flag, delete-then-call) is the only one used. */
function skipMotion(){
  const all=[...INFLIGHT]; INFLIGHT.clear();
  for(const f of all){ try{ f(); }catch(e){} }
}
/* Capture phase, so the tap that skips is the same tap the player already made —
   it never costs a second one. A no-op when nothing is running, which is almost
   always, so the cost on an ordinary click is one `.size` read. */
addEventListener('pointerdown',()=>{ if(INFLIGHT.size)skipMotion(); },true);
addEventListener('keydown',()=>{ if(INFLIGHT.size)skipMotion(); },true);
addEventListener('touchstart',()=>{ if(INFLIGHT.size)skipMotion(); },{capture:true,passive:true});

/* A CSS animation runs for `ms`; this is the bookkeeping around it. */
function anim(ms,after){
  let t=null,ended=false;
  const fin=()=>{ if(ended)return; ended=true; clearTimeout(t); INFLIGHT.delete(fin);
    try{ after&&after(); }catch(e){} };
  INFLIGHT.add(fin);
  t=setTimeout(fin,REDUCE()?Math.min(ms,120):ms);
  return fin;
}
/* Restart a CSS animation that may already have run on this element. */
const replay=(el,...cls)=>{ if(!el)return; el.classList.remove(...cls);
  void el.offsetWidth; el.classList.add(...cls); };

/* ── the sweep ────────────────────────────────────────────────────────────────
   `mid` is the scene swap and it fires exactly once, at full cover — whether the
   sweep played out or was skipped on its first frame. `done` fires after. Callers
   never have to care which happened. */
function sweep(mid,done){
  const S=$('#sweep'); if(!S){ mid&&mid(); done&&done(); return; }
  const R=REDUCE(), half=R?60:260;
  let t=null,ended=false,swapped=false;
  const swap=()=>{ if(swapped)return; swapped=true; try{ mid&&mid(); }catch(e){} };
  const fin=()=>{ if(ended)return; ended=true; clearTimeout(t); INFLIGHT.delete(fin);
    swap(); S.className='sweep hide'; try{ done&&done(); }catch(e){} };
  INFLIGHT.add(fin);
  S.className='sweep'+(R?' x':'');
  void S.offsetWidth;                                   /* commit the from-state */
  requestAnimationFrame(()=>{
    if(ended)return;
    S.classList.add('a');                               /* cover, 260ms */
    t=setTimeout(()=>{
      if(ended)return;
      swap();                                           /* the scene changes here */
      S.classList.remove('a'); S.classList.add('b');    /* clear off right, 260ms */
      t=setTimeout(fin,half);
    },half);
  });
  return fin;
}
/* Going back is not a sweep. A quarter-second through black says "we are leaving"
   without claiming the story moved forward. */
function fadeSwap(mid,done){
  const S=$('#sweep'); if(!S){ mid&&mid(); done&&done(); return; }
  const half=REDUCE()?60:100;
  let t=null,ended=false,swapped=false;
  const swap=()=>{ if(swapped)return; swapped=true; try{ mid&&mid(); }catch(e){} };
  const fin=()=>{ if(ended)return; ended=true; clearTimeout(t); INFLIGHT.delete(fin);
    swap(); S.className='sweep hide'; try{ done&&done(); }catch(e){} };
  INFLIGHT.add(fin);
  S.className='sweep x'; void S.offsetWidth;
  requestAnimationFrame(()=>{ if(ended)return; S.classList.add('a');
    t=setTimeout(()=>{ if(ended)return; swap(); S.classList.remove('a');
      t=setTimeout(fin,half); },half); });
  return fin;
}

/* ── the shared element ───────────────────────────────────────────────────────
   The card becomes the screen. The clone is placed at the viewport and transformed
   *back* onto the card's measured rectangle, then released — which is the only way
   to get a 60fps size change out of a browser. Uniform scale keyed on width, so the
   art inside is never squashed on the way up; the small vertical bloom that leaves
   reads as the card blooming, which is the effect wanted anyway. */
function sharedInto(el,after){
  const go=()=>{ try{ after&&after(); }catch(e){} };
  if(!el||REDUCE())return go();
  const r=el.getBoundingClientRect();
  const w=innerWidth,h=innerHeight;
  if(!r.width||!r.height||!w||!h)return go();
  const s=Math.max(.06,Math.min(1,r.width/w));
  const dx=(r.left+r.width/2)-w/2, dy=(r.top+r.height/2)-h/2;
  const box=document.createElement('div');
  box.className='shel';
  const inner=document.createElement('div'); inner.className='shel-in';
  /* Only the art travels. The badges and the body are absolutely positioned against
     a card-sized box and would fly to the wrong corners of a screen-sized one. */
  const src=el.querySelector('.hero-art picture,.hero-art img,.hero-void,'
    +'.art picture,.art img,.art-ph');
  if(src)inner.appendChild(src.cloneNode(true));
  box.appendChild(inner);
  box.style.transform=`translate3d(${dx}px,${dy}px,0) scale(${s})`;
  document.body.appendChild(box);
  let t=null,ended=false;
  const fin=()=>{ if(ended)return; ended=true; clearTimeout(t); INFLIGHT.delete(fin);
    go();
    /* One frame of overlap, so the black the title card arrives on is already up
       before the clone leaves — otherwise the shelf flashes back for a frame. */
    requestAnimationFrame(()=>box.remove()); };
  INFLIGHT.add(fin);
  void box.offsetWidth;
  requestAnimationFrame(()=>{
    if(ended)return;
    box.classList.add('go');
    box.style.transform='translate3d(0,0,0) scale(1)';
    t=setTimeout(fin,320);
  });
  return fin;
}

/* ── the cinema bars ── */
let barTimer=null;
function bars(on){
  const b=$('#cbars'); if(!b)return;
  clearTimeout(barTimer);
  if(on){ b.classList.remove('hide'); void b.offsetWidth; b.classList.add('on'); }
  else{ b.classList.remove('on');
    barTimer=setTimeout(()=>b.classList.add('hide'),REDUCE()?140:340); }
}

/* ── the monitor boots ────────────────────────────────────────────────────────
   The trace runs in from the right and the numbers climb from zero to what they
   already are. `BOOTING` is why this cannot show a stale reading: while it runs,
   every paint writes its value into the target map instead of the DOM, so the
   count-up is always climbing toward the newest truth and lands exactly on it. */
let BOOTING=null;
const VITALSEL=['#m-hr','#m-spo2','#m-bp','#m-rr'];
function setVital(sel,val){
  if(BOOTING){ BOOTING[sel]=val; return; }
  const el=$(sel); if(el)el.textContent=val;
}
/* '120/80' at .5 is '60/40'; '98%' is '49%'; '—' is '—'. */
const countTo=(s,k)=>String(s).replace(/\d+/g,m=>String(Math.round(+m*k)));
function bootMonitor(){
  const tr=$('#mtrace');
  if(tr){ tr.classList.remove('flat'); }
  if(REDUCE()){ if(tr)tr.classList.remove('boot'); return; }
  if(tr)replay(tr,'boot');
  const t={}; for(const s of VITALSEL){ const el=$(s); t[s]=el?el.textContent:''; }
  BOOTING=t;
  const t0=performance.now(), MS=500;
  let raf=null,ended=false;
  const fin=()=>{ if(ended)return; ended=true; cancelAnimationFrame(raf); INFLIGHT.delete(fin);
    const target=BOOTING; BOOTING=null;
    if(!target)return;
    for(const s of VITALSEL){ const el=$(s); if(el)el.textContent=target[s]; } };
  INFLIGHT.add(fin);
  const frame=()=>{
    /* `BOOTING` is cleared by whoever finishes first, and a frame already queued can run after
       that — two boots in quick succession is all it takes, which a restart during the monitor's
       count-up does. It threw "Cannot read properties of null" on the Eternal bay under a driven
       restart; the code predates the bay being split out, so what changed is how reliably the
       race is hit, not the race. */
    if(ended||!BOOTING)return;
    const k=Math.min(1,(performance.now()-t0)/MS), e=1-Math.pow(1-k,3);
    for(const s of VITALSEL){ const el=$(s); if(el)el.textContent=countTo(BOOTING[s],e); }
    if(k>=1)return fin();
    raf=requestAnimationFrame(frame);
  };
  raf=requestAnimationFrame(frame);
  return fin;
}

/* ── the rail's trace follows the rhythm ──────────────────────────────────────
   It did not. One hand-drawn sinus strip was drawn over every state of every case, so a
   station whose patient was in PEA showed four textbook complexes marching along under a
   panel that said "Arrest" — and a clinician reading that screen stops reading the rest of
   it. The rhythm is scenario content the engine already holds (`Rhythm::parse`, declared per
   state), it now rides in the view, and this is the rail drawing what it is told rather than
   what it assumes.

   Four shapes, because four is what the difference between them is *for*:

     sinus     the strip in the markup — organised, spiky, the one a well patient wears
     pea       wide, slow, low complexes. Organised electricity, no output — which is the
               finding, and the reason the drill is to feel for a pulse instead of reading
               the screen. HR keeps its number beside it; SpO2 and BP are `--`.
     vt        broad and regular and fast, the shape a defibrillator is for
     vf        chaotic, no complexes at all — the same three-sine recipe the bedside device
               draws at full size, so the sparkline and the strip cannot disagree
     asystole  a straight line

   The full-size device keeps its wandering asystole baseline (a real lead picks up chest
   movement, which is why the protocol says confirm in two leads); at 28 pixels tall that
   wander is invisible, so this one is drawn flat.

   Presentation only. Nothing here reaches the tape, the replay, the leaf or the rubric. */
const TR_SINUS=(()=>{const p=$('#mtrace');const w=p&&p.querySelector('.tr-w');
  return w?w.getAttribute('d'):'M0 28 H300';})();
/* A polyline across the strip, sampled from a shape function. Deterministic on purpose: the
   path is written into the DOM once, and a trace that reshuffled itself on every paint would
   flicker. */
const tracePath=(f,step)=>{let d='M0 28';
  for(let x=step;x<=300;x+=step)d+=' L'+x+' '+(28-f(x)).toFixed(1);
  return d;};
const TRACES={
  sinus:TR_SINUS,
  /* Two complexes where sinus has four, three times as wide and a third as tall. */
  pea:'M0 28 H40 l10 -6 l12 12 l14 -10 l10 4 H150 l10 -6 l12 12 l14 -10 l10 4 H300',
  vt:tracePath(x=>Math.sin(x*0.20)*10,3),
  vf:tracePath(x=>Math.sin(x*0.55)*5+Math.sin(x*0.31+1.7)*3+Math.sin(x*0.93+0.4)*1.8,3),
  asystole:'M0 28 H300',
};
function drawTrace(v){
  const box=$('#mtrace'); if(!box)return;
  const w=box.querySelector('.tr-w'); if(!w)return;
  /* Unknown or absent rhythm degrades to sinus, not to a flat line: a rail that invents an
     arrest is worse than one that misses it. */
  const d=TRACES[String(v.rhythm||'sinus').toLowerCase()]||TRACES.sinus;
  if(w.getAttribute('d')!==d)w.setAttribute('d',d);
}

/* ── she is getting worse ─────────────────────────────────────────────────────
   Ranked so the border only fires on the way down. Improving back to Stable is not
   an alarm, and an alarm that cries on good news is an alarm nobody reads. */
const RANK={Stable:0,Improving:0,Recovered:0,Deteriorating:2,Critical:3,Arrest:4,Dead:5};
let lastRank=0;
function alarmPulse(){
  const a=$('#alarm'); if(!a)return;
  a.className='alarmv'; void a.offsetWidth;
  a.className='alarmv on'+(REDUCE()?' quick':'');
  return anim(REDUCE()?120:1100,()=>{ a.className='alarmv hide'; });
}
/* ── she turned the corner ── 600ms of the monitor calming down before the verdict. */
function settle(after){
  const m=$('#mini'); if(m)m.classList.add('calm');
  return anim(600,after);
}
/* ── she did not ──────────────────────────────────────────────────────────────
   Deliberately slower than anything else in the game: 900ms, and the brief is
   explicit that this one is allowed to be. The flat line eats the wave from the
   left, the dark closes in from the edges, and the sound goes with her — the
   cutscene's own audio is wound to nothing, and the LIVE dot has already stopped
   beating by the time this runs (paint() marks the bar `ended`). */
function flatline(after){
  const tr=$('#mtrace'); if(tr){ tr.classList.remove('boot','calm'); replay(tr,'flat'); }
  const m=$('#mini'); if(m)m.classList.remove('calm');
  const v=$('#vig'); if(v){ v.classList.remove('hide'); void v.offsetWidth; v.classList.add('on'); }
  const c=$('#cut'); if(c){ try{ c.volume=0; }catch(e){} }
  return anim(900,after);
}
/* The white splice before SEASON 2 — a cut, not a dissolve. */
function flash(){
  const f=$('#flash'); if(!f||REDUCE())return;
  /* The class comes off before the reflow, not after: offsetWidth on a display:none
     element is zero and forces nothing, so the animation would never restart. */
  f.classList.remove('hide'); replay(f,'on');
  return anim(400,()=>{ f.className='flash hide'; });
}

/* ── B6-B8 · the cinema layer ─────────────────────────────────────────────────
   A queue of steps on one black surface. A tap (or Escape) advances any skippable
   step; a card with its own buttons sets skip:false and drives itself. The queue's
   done() runs exactly once, when the last step ends or the whole thing is skipped
   through — which is what lets the intro hand the frame to the run and nothing else. */
let cineQ=null;
function cineShow(steps,done){
  if(cineQ)clearTimeout(cineQ.timer);
  if(!steps.length){cineHide();return done&&done();}
  cineQ={steps,i:-1,done,timer:null};
  $('#cine').classList.remove('hide');
  cineNext();
}
function cineNext(){
  if(!cineQ)return;
  clearTimeout(cineQ.timer);
  cineQ.i++;
  if(cineQ.i>=cineQ.steps.length){const d=cineQ.done;cineHide();return d&&d();}
  const st=cineQ.steps[cineQ.i];
  /* B7 · a real cross-dissolve, not a fade through black. The outgoing recap frame is
     kept underneath the incoming one for a quarter second and fades out beneath it —
     which is what "dissolve" means, and what three stills cutting on black did not do.
     Only between recap frames: everything else in the queue wants a clean cut. */
  const out=$('#cine').querySelector('.rc-still');
  $('#cine').innerHTML=st.html;
  if(out&&!REDUCE()&&$('#cine').querySelector('.rc-still')){
    out.classList.add('rc-out');
    $('#cine').insertBefore(out,$('#cine').firstChild);
    setTimeout(()=>out.remove(),260);
  }
  $('#cine').dataset.skip=st.skip===false?'no':'yes';
  /* A step may ask not to be a curtain. Only one does — see `upNext` — and only on a phone,
     where the CSS turns this class into a sheet at the foot of the screen instead. */
  $('#cine').classList.toggle('at-foot',!!st.sheet);
  if(st.ms)cineQ.timer=setTimeout(cineNext,st.ms);
  if(st.wire)st.wire($('#cine'));
}
function cineHide(){
  if(cineQ)clearTimeout(cineQ.timer);
  cineQ=null; $('#cine').classList.add('hide'); $('#cine').innerHTML=''; $('#cine').dataset.skip='';
  $('#cine').classList.remove('at-foot');
  /* The bars belong to the cinema layer and leave with it, from whichever of the
     several exits was taken. */
  bars(false);
}
/* The cinema is the season's, and the ward host composes none of it — so these bind only where
   there is something to bind to, like every other control that differs by host. */
onLobby('#cine','onclick',()=>{ if(cineQ&&$('#cine').dataset.skip!=='no')cineNext(); });
addEventListener('keydown',e=>{ const c=$('#cine'); if(c&&e.key==='Escape'&&cineQ&&c.dataset.skip!=='no')cineNext(); });

/* B6 · the title card: the name of the episode pulling into focus out of a blur over
   600ms, then holding for the rest of the step. It is never faded out — the sweep is
   what lifts it, which is half of why the sweep exists. The bars go out here: the recap
   they framed is over and the bay is next. */
const titleCard=e=>({ms:1500,html:'<div class="tc"><div class="tc-k">'
  +(e.station?'night shift training · '+e.n:(e.sn||e.n))+'</div><div class="tc-t">'
  +e.t.toUpperCase()+'</div></div><span class="cine-skip">tap to skip</span>',
  wire:()=>bars(false)});
/* B7 · the recap: an intro card, then three frames with a line each, under cinema bars
   that slide in at the head and out again at the title card. */
function recapSteps(e){
  const R=RECAP[e.id]; if(!R)return[];
  const steps=[{ms:1400,html:'<div class="tc"><div class="tc-prev">Previously on <b>VITALS</b>\u2026</div></div><span class="cine-skip">tap to skip</span>',
    wire:()=>bars(true)}];
  for(const[img,line]of R)steps.push({ms:2600,html:'<div class="rc-still">'
    +(img?'<img src="'+img+'" alt="">':'<div class="rc-void"></div>')
    +'<div class="rc-line">'+line+'</div></div><span class="cine-skip">tap to skip</span>'});
  return steps;
}
/* ── the front door of an episode ─────────────────────────────────────────────
   Every entry from the shelf routes here: the recap for EP2-5, the title card for
   everything, and only then the run — the case clock never ticks under the recap.
   The kiosk hash path (#play) keeps calling #start directly and stays overlay-free,
   and so does the restart button: a restart is not an entrance. */
function enterEpisode(epId,exam,from){
  const e=SEASON.find(x=>x.id===epId)||SEASON[0];
  EXAMRUN=!!(exam||e.station);
  $('#ep').value=e.id;
  /* Before the title card, not after the first tick: the bar is on screen behind the cinema
     layer, and a hold button visible for one frame of a station is a hold button. */
  examControls();
  /* The whole entrance, in order, and none of it on the clock:
       the card you pressed grows into the screen (320ms, shelf entrances only)
       → previously on… under cinema bars (EP2-5)
       → the title card pulls into focus and holds
       → the sweep lifts it and puts the bay behind it, opening the case at full cover.
     The scene swap is inside the sweep's `mid` rather than up here, so the shelf stays
     where it was until the moment the trace covers it. `/api/new` is fired at that same
     instant — the first tick of the patient's clock is 700ms after this, and every
     frame above it has already been spent. */
  const run=()=>{
    const steps=e.station?[]:recapSteps(e);
    steps.push(titleCard(e));
    cineShow(steps,()=>sweep(()=>{
      $('#lobby').classList.add('hide'); $('#game').classList.remove('hide');
      $('#start').click();
    }));
  };
  if(from)sharedInto(from,run); else run();
}
/* ── B8 · the end-of-episode flow ─────────────────────────────────────────────
   Outcome → stinger → up next. The terminal cutscene keeps the frame until it ends;
   a restart mid-wait cancels the whole thing (the run id is the guard). */
function endFlow(win){
  const runId=id, eId=$('#ep').value, exam=EXAMLIVE;
  let waited=0;
  const go=()=>{
    if(id!==runId||!over)return;
    /* Walked out to the shelf while this was still pending: the flow does not follow.
       Without this, "← episodes" during the beat between the verdict and the stinger
       drops the up-next card on top of the lobby a second later — a fullscreen card,
       skip:false, sitting over a shelf the player is already trying to use. The end of
       an episode belongs to the bay; leaving the bay ends it. */
    if($('#game').classList.contains('hide'))return;
    if(cutting&&waited<12000){waited+=400;return void setTimeout(go,400);}
    const steps=[];
    if(win&&STINGER[eId])steps.push({ms:3600,
      html:'<div class="sting">'+STINGER[eId]+'</div><span class="cine-skip">tap to skip</span>',
      wire:()=>bars(true)});
    cineShow(steps,()=>{ if(id===runId&&over&&!$('#game').classList.contains('hide'))
      upNext(win,exam,eId); });
  };
  /* Measured from the verdict landing, not from the bell: the settle-or-flatline and the
     sweep now sit between the two, and the old delays were being eaten by them. This is
     the beat the player has with the mark sheet before the season asks for them back. */
  setTimeout(go,win?1800:1600);
}
/* The next door that is actually open, walking the shelf left to right. */
function nextOpen(eId){
  const i=SEASON.findIndex(x=>x.id===eId);
  for(let j=i+1;j<SEASON.length;j++)if(open_(j))return SEASON[j];
  return null;
}
function upNext(win,exam,eId){
  const i=SEASON.findIndex(x=>x.id===eId);
  let nxt=win?nextOpen(eId):SEASON[i], note=null;
  if(win&&exam&&!nxt){
    /* The star is not banked until the anchor lands, so the door it opens still reads
       shut here — show the door anyway and say what opens it. No auto-advance: leaving
       an exam un-anchored by countdown would cost the star the run just earned. Only a
       station's star prices a door; an episode's exam replay banks XP and the record. */
    nxt=SEASON.slice(i+1).find(x=>!x.station)||null;
    const ent=SEASON[i];
    note=ent&&ent.station
      ?'Anchor this run first — its stars are what open this door.'
      :'Anchor this run to bank it — doors open on station stars.';
  }
  if(!nxt)return epilogue(exam);
  const auto=win&&!exam;
  /* A station has no key art — it is not an episode and never gets a billboard — so this card
     used to be a stem beside an empty gradient, which is the one place in the flow where the
     next patient is announced and nobody is shown. It has a face on disk already: the `stable`
     shot the bay itself opens on. `stationStill` is the same reader the bay uses, and it returns
     '' unless the server's set table says that file is really there, so a station whose art has
     not landed keeps the gradient rather than a broken frame.
     `stable` and only `stable`: it is the patient as they arrive, which is what the candidate is
     about to see through the door anyway. A later state would be the card telling them how this
     run ends before they have taken it. */
  const unArt = nxt.art || (nxt.station ? stationStill(nxt.id,'stable') : '');
  cineShow([{skip:false,sheet:true,html:'<div class="un"><div class="un-k">'+(win?'up next':'run it back')+'</div>'
    +'<div class="un-card">'+(unArt?'<img src="'+unArt+'" alt="" onerror="this.replaceWith(Object.assign(document.createElement(\'div\'),{className:\'un-ph ph-'+nxt.id+'\'}))">':'<div class="un-ph ph-'+nxt.id+'"></div>')
    +'<div><div class="un-n">'+(nxt.sn||nxt.n)+(nxt.station?' · night shift training':'')+'</div>'
    +'<div class="un-t">'+nxt.t+'</div><div class="un-rt">'+(nxt.rt||'')+'</div></div></div>'
    +(note?'<p class="un-note">'+note+'</p>':'')
    +'<div class="un-acts"><button class="btn go" data-un-go data-ga="upnext:play">▶ '+(win?'play now':'run it back')+'</button>'
    +'<button class="btn" data-un-stay data-ga="upnext:stay">'+(exam&&win?'review &amp; anchor':'read the debrief')+'</button></div>'
    /* The countdown reads as a ring walking backwards \u2014 the five seconds are a shape
       shrinking rather than a digit ticking, which is what a broadcast does with the
       same five seconds. The digit stays inside it: the ring is the feeling and the
       number is the fact, and neither is a substitute for the other. */
    +(auto?'<div class="un-count"><span class="un-dial">'
      +'<svg viewBox="0 0 36 36" aria-hidden="true"><circle cx="18" cy="18" r="15.5"/>'
      +'<circle cx="18" cy="18" r="15.5" class="un-arc"/></svg>'
      +'<b data-un-n>5</b></span> starting\u2026</div>':'')+'</div>',
    wire:r=>{
      /* On a phone this card is a sheet at the foot of the screen rather than a curtain
         over it, so there is something to see behind it — and what should be behind it is
         the sheet the run just earned, not the top of the bay. `review & anchor` used to
         be the only way to reach it and the player had to notice that to get there at all;
         it is simply already up now, and that button and the debrief link below still
         scroll the panel exactly as they did. `block:'start'` rather than `'nearest'`:
         nearest is a no-op when the panel is taller than the viewport, which on a phone
         it always is, and the head of the mark sheet is the part worth landing on. */
      if(PHONE()&&$('#marks')&&$('#marks').children.length)
        $('#marks').scrollIntoView({behavior:REDUCE()?'auto':'smooth',block:'start'});
      const go=()=>{cineHide();enterEpisode(nxt.id,nxt.station);};
      r.querySelector('[data-un-go]').onclick=go;
      r.querySelector('[data-un-stay]').onclick=()=>{cineHide();
        $('#result').scrollIntoView({behavior:'smooth',block:'nearest'});};
      /* The countdown is the binge loop; either button cancels it (cineHide clears the
         handle — clearTimeout and clearInterval share an id space by spec). */
      if(auto){let n=5;const tid=setInterval(()=>{
        const el=r.querySelector('[data-un-n]');
        if(!el||!cineQ){clearInterval(tid);return;}
        if(--n<=0){clearInterval(tid);go();}else el.textContent=n;},1000);
        cineQ.timer=tid;}
    }}]);
}
/* ── the season 1 epilogue (Phase 3 owns this copy) ───────────────────────────
   Three cards past the finale stinger, playable only after an EP5 win: the wrinkle
   Ing leaves in the queue log (season 2's buried fuse), the transfer letter on the
   desk (season 2's front door), and the title that promises it. Prose layer only —
   nothing here touches the run, the star, or the record; the anchor door on the
   last card is the same scroll-to-result the up-next card already offers. */
function epilogue(exam){
  cineShow([
   {html:'<div class="epi"><div class="epi-k">later \u00b7 the queue log</div>'
    +'<div class="sting">Ing signs off her first volunteer shift and leaves one note on the queue log: '
    +'three of tonight\u2019s patients came in fighting for breath \u2014 and not a burn on any of them. '
    +'\u201cStrange, isn\u2019t it?\u201d Nobody has time to think about it.</div></div>'
    +'<span class="cine-skip">tap to continue</span>'},
   {html:'<div class="epi"><div class="epi-k">the morning after \u00b7 one letter on the desk</div>'
    +'<div class="epi-letter"><div class="epi-lh">office of the provincial health board<br>re: second-year posting</div>'
    +'<p>Your residency year is complete. Effective the first of the month, you are transferred to a district hospital, four hours upcountry.</p>'
    +'<p>Thirty beds. One ambulance. One doctor on the roster.</p></div>'
    +'<div class="epi-punch">There is no resus bay there.<br>There is no next shift coming to take over.<br><b>There is you.</b></div></div>'
    +'<span class="cine-skip">tap to continue</span>'},
   {skip:false,html:'<div class="un"><div class="un-k">end of season 1 \u00b7 the resus bay</div>'
    +'<div class="epi-ret">VITALS will return</div>'
    +'<div class="epi-s2">Season 2<span>The District</span></div>'
    +(exam?'<p class="un-note">Anchor this run \u2014 the finale star belongs on the record.</p>':'')
    +'<div class="un-acts">'
    +(exam?'<button class="btn go" data-epi-anchor data-ga="epilogue:anchor">review &amp; anchor</button>':'')
    +'<button class="btn'+(exam?'':' go')+'" data-epi-x data-ga="epilogue:shelf">back to the shelf</button></div></div>',
    wire:r=>{
      /* The letter cuts to white and the white cuts to black, and SEASON 2 is what is
         standing there when it clears — a splice, the way a title comes up at the end
         of a finale. The one place in the game a flash is the right instrument. */
      flash();
      const a=r.querySelector('[data-epi-anchor]');
      if(a)a.onclick=()=>{cineHide();$('#result').scrollIntoView({behavior:'smooth',block:'nearest'});};
      r.querySelector('[data-epi-x]').onclick=()=>{cineHide();$('#back').click();};
    }}]);
}
renderSeason(); renderModes();
/* The panel does three different jobs for three different situations. Showing all three at
   once is what made it unreadable: two paste fields side by side, when only one of them is
   ever the one you want, and the account and the machine printed as the same 44 characters
   with nothing saying why. */
/* Not `show` — the page already has one, for swapping the film behind the patient. Shadowing
   it threw at parse time and took the whole script with it. */
const vis=(sel,on)=>$(sel).classList.toggle('hide',!on);

async function refreshRecord(){
  /* Nothing to draw on a page without the lobby — the ward's shift page has no #me-lv. */
  if(!$('#me-lv'))return;
  const me=await identity();
  const c=await (await fetch('/api/chain'+(me?'?player='+me.pub:''))).json();
  $('#lb-chain').textContent=c.connected
    ? `${c.cluster||'?'} · tree #${c.tree_id}`+(me?` · ${shortKey(me.pub)}`:' · no key')
    : 'no chain';
  if(!me){
    $('#rc-pill').className='pill none'; $('#rc-pill').textContent='no key';
    $('#rc-level').textContent='This browser cannot make a key';
    $('#rc-sub').textContent='Ed25519 in WebCrypto — try Chrome, Safari 17+ or Firefox 129+';
    // `show` here was the page's *other* show — the one that swaps the film behind the patient.
    // It took `'#rc-list'` as a clinical status and `false` as the kit list. Nothing threw, which
    // is why it survived the rename that caught every other call.
    ['#rc-list','#rc-acts','#rc-flow','#rc-joinbox','#rc-handover'].forEach(x=>vis(x,false));
    $('#me').dataset.state='none';
    $('#me-lv').textContent='no key';
    $('#me-k').textContent='this browser cannot sign';
    return;
  }
  const mine=acctOf()===me.pub;
  const a=await accountState();
  const open_=a&&a.open, linked=a?a.linked:mine;

  $('#rc-acct').textContent=acctOf();
  $('#rc-dev').textContent=me.pub;
  $('#rc-title').textContent = mine?'My record':'Someone else\u2019s record';

  /* One word for where this machine stands. */
  const pill=$('#rc-pill');
  if(!open_){ pill.className='pill none'; pill.textContent='not opened yet'; }
  else if(linked && mine){ pill.className='pill here'; pill.textContent='this machine'; }
  else if(linked){ pill.className='pill here'; pill.textContent='linked'; }
  else { pill.className='pill view'; pill.textContent='watching only'; }

  /* The level is the thing anyone came here for, so it goes first and it goes big. */
  const pr=await (await fetch('/api/progress?account='+acctOf())).json();
  /* The bar carries the two things worth knowing without opening anything: what you have earned,
     and which record you are looking at. */
  $('#me').dataset.state = !open_ ? 'none' : (linked ? 'own' : 'watch');
  $('#me-lv').textContent = pr.level==null ? (open_?'no level yet':'my record') : pr.level_name;
  $('#me-k').textContent = mine ? shortKey(me.pub) : 'watching '+shortKey(acctOf());
  if(pr.level==null){
    $('#rc-level').textContent = open_?'Nothing claimed yet':'No record yet';
    $('#rc-sub').textContent = open_
      ? 'Finish a case and claim a level'
      : 'Finish a case and the account opens itself';
  }else{
    $('#rc-level').textContent=pr.level_name;
    $('#rc-sub').textContent=`${pr.attempts} attempt${pr.attempts===1?'':'s'} · `
      +`${pr.distinct} case${pr.distinct===1?'':'s'} · ${pr.xp} xp`;
  }

  /* Which machines can play into it. Truncated: a key is a token you move, not text you read. */
  vis('#rc-list', open_);
  if(open_){
    const n=a.devices;
    $('#rc-devices').innerHTML =
      `<div class="dev"><span class="dot${linked?'':' off'}"></span>`
      +`<span>${mine?'this browser':(linked?'this browser':'this browser — not linked')}</span>`
      +`<span class="k">${shortKey(me.pub)}</span></div>`
      + (n>1 ? `<div class="dev"><span class="dot"></span><span>${n-1} other`
               +`${n-1===1?'':'s'}</span><span class="k">linked</span></div>` : '');
  }

  /* Only ever offer the action that applies here. */
  vis('#rc-acts', true);
  vis('#rc-open', linked);                 // hand the record to another machine
  $('#rc-join').textContent = mine?'Use another account':'Back to my own record';
  vis('#rc-handover', open_ && !linked);   // this machine is watching and wants in
  if(!linked){ vis('#rc-flow',false); }
}

$('#rc-open').onclick=()=>{ vis('#rc-joinbox',false);
  vis('#rc-flow', $('#rc-flow').classList.contains('hide')); };
$('#rc-join').onclick=()=>{
  if(acctOf()!==(ME&&ME.pub)){ setAcct(null); $('#rc-msg').textContent='Back to this machine\u2019s own record.'; refreshRecord(); return; }
  vis('#rc-flow',false);
  vis('#rc-joinbox', $('#rc-joinbox').classList.contains('hide'));
};
const copyTo=async(btn,text)=>{ await navigator.clipboard.writeText(text);
  const was=btn.textContent; btn.textContent='copied'; setTimeout(()=>btn.textContent=was,1200); };
/* Its own sheet, not the picker's. The picker rewrites its innerHTML every time it opens, so
   sharing one would tear the record's handlers off their elements. */
function openRecord(){ $('#recveil').hidden=false; refreshRecord(); }
function closeRecord(){ $('#recveil').hidden=true; vis('#rc-flow',false); vis('#rc-joinbox',false); }
onLobby('#me','onclick',openRecord);
$('#rec-close').onclick=closeRecord;
$('#recveil').addEventListener('click',e=>{ if(e.target===$('#recveil')) closeRecord(); });
addEventListener('keydown',e=>{ if(e.key==='Escape'&&!$('#recveil').hidden) closeRecord(); });

$('#rc-copy').onclick=e=>copyTo(e.target,acctOf());
$('#rc-devcopy').onclick=async e=>copyTo(e.target,(await identity()).pub);
$('#rc-link').onclick=async()=>{
  const other=$('#rc-add').value.trim(); if(!other)return;
  $('#rc-msg').textContent='Linking…';
  const r=await linkDevice(other);
  $('#rc-msg').innerHTML = r.error ? `<span class="bad">${r.error}</span>`
    : `<span class="ok">Linked.</span> That machine now plays into this record — ${r.devices} in total.`;
  if(!r.error){ $('#rc-add').value=''; vis('#rc-flow',false); }
  refreshRecord();
};
$('#rc-adopt').onclick=async()=>{
  const a=$('#rc-use').value.trim(); if(!a)return;
  setAcct(a); $('#rc-use').value=''; vis('#rc-joinbox',false);
  // The handover hint below already says what to do with the key; repeating it here just made
  // the card say the same sentence twice.
  $('#rc-msg').textContent='';
  refreshRecord();
};
/* ─── the meter ────────────────────────────────────────────────────────────────
   The bay is free; donations pay for its compute. Both halves of that sentence are shown:
   the pill counts the month in the open, and when the ceiling is reached the card says what
   the money funded — the constraint is part of the product, not an error state. */
function showCeiling(m){
  const c=$('#ceiling'); c.classList.remove('hide');
  const used=(+m.used).toLocaleString(), clicks=(+m.clicks||0).toLocaleString();
  c.innerHTML=
    `<b>The bay has spent its month of compute.</b>
     <span>${used} conversations with a dying patient ran free in ${m.month} — every one paid
     for by donations. Orders and the monitor still work; the patient's voice returns on the 1st.</span>`
    +(m.donate?`<a class="cl-give" href="/donate" target="_blank" rel="noopener">Keep the bay running →</a>`:'')
    +`<span class="cl-count">♥ ${clicks} ${+m.clicks===1?'person':'people'} followed the donate link this month</span>`;
}
async function meterState(){
  /* Nothing to draw on a page without the lobby — the ward's shift page has no #meterpill. */
  if(!$('#meterpill'))return;
  let m; try{ m=await (await fetch('/api/meter')).json(); }catch(e){ return; }
  if(!m) return;
  if(m.donate) $('#donate').classList.remove('hide');
  if(m.cap!=null){
    $('#meterpill').classList.remove('hide');
    $('#meterpill').textContent=`◎ ${(+m.used).toLocaleString()} / ${(+m.cap).toLocaleString()} this month`;
    if(m.used>=m.cap) showCeiling(m);
  }
}
meterState();
/* Before anything paints: the picker has to be filled and the chips have to know what language
   they are wearing. It is one small fetch and everything downstream degrades to English if it
   never answers. */
bootLang();

/* ─── a shift on the ward ──────────────────────────────────────────────────────
   The bay is one bay. Opened at /ward/<patient_id> it plays a stranger's patient
   from the state the chain says she is in, and everything below this line is the
   only difference: four transactions the browser signs, and a strip that says
   whose shift this is.

   The bar is built here rather than written into the markup because it exists
   only on the ward host, and the Eternal entry's page should not carry a control
   that never shows for it.

   Why `id` is left unset until the head is taken: every control in this page
   already guards on `id`, so a patient opened but not taken is a chart you can
   read and not a patient you can treat. That is the honest state — you have not
   taken the shift yet — and it needs no second flag to enforce. */
let WARDSHIFT=null, WARDPENDING=null;

/* What a stranger may not do yet, said in the words the page will show them.
   `null` on the Eternal entry, always: there is no head to take there, and a gate that reached
   it would be this file quietly turning a single-player bay into a ward. Tested in
   tests/shift_logic.mjs. */
function takeFirst(ward, runId, whom){
  return ward && !runId ? 'take the shift to treat '+(whom||'the patient') : null;
}

/* An authored line, retold with the age of the person actually in the bed.
   The case says "F 6" because somebody wrote a six-year-old; the ward admitted an eight-year-old
   onto it, and the door has already refused any pack whose sex or band contradicts the case — so
   only the number can differ, and when it does, hers is the true one. Without an age nothing is
   rewritten: a ward that does not know is not a ward that may invent. */
function wardAged(text, age){
  if(!age)return text;
  return String(text).replace(/\b([MF])\s*\d{1,3}\b/g, (m,sex)=>sex+' '+age);
}

/* The case in front of this shift, turned into the card the bay already knows how to draw.
   Everything in it is the payload's: the title is the case's own headline, `line` is its presenting
   line (which `stemHtml` prints as PRESENTS, the last thing read before the clock starts), `who` is
   the person actually in the bed, and the tray is the case's own interventions.
   `null` when the payload carried no case content — the page says so rather than drawing whatever
   the page's own table answers, which was EP1's patient over everybody. */
function wardCard(content, reviewing){
  if(!content||!content.case_id)return null;
  /* The compiler's vocabulary on the left, the kit's rows on the right: `ix_` is an investigation
     and the tray calls that row labs, `tx_` is a treatment and the tray calls it drugs. Nothing is
     renamed on the wire — what fires, lands on the tape and is marked is the intervention id. */
  const ROWS={ask:'ask',exam:'exam',lab:'lab',treat:'drug',dx:'dx'};
  const chips={}, labels={};
  for(const row of Object.keys(ROWS)){
    const ids=[];
    for(const item of ((content.chips||{})[row]||[])){
      if(!item||!item.id)continue;
      ids.push(item.id);
      if(item.label)labels[item.id]=item.label;
    }
    if(ids.length)chips[ROWS[row]]=ids;
  }
  /* No `station` on this card, whatever it looks like on screen. `station` is the season's word
     for an exam: it shuffles the tray, seals the marks, and turns the ask bar from a conversation
     into an order column — `askHer` is reached only when `!ep().station` — so a ward card that
     claimed to be one would take away the thing the founder asked for by name ("ทำไมกด chat
     ไม่ได้"). The sheet a station also draws is `renderStage`'s to give the ward directly. */
  const entry={
    id:content.case_id,
    /* What the bar calls this run. "the ward" over a case nobody is in is the wrong half of the
       truth on a review page, where the patient is invented and the case is the subject. Passed in
       rather than read off `REVIEW`, so this function is a function of what it is given — which is
       also how it is tested. */
    n:reviewing?'reviewing':'the ward', sn:reviewing?'reviewing':'the ward',
    t:content.title||'',
    who:content.who||'',
    line:content.presents||'',
    d:content.story||'',
    place:content.setting||content.care_setting||'',
    /* Where the patient presented, never the case's specialty: SURGERY over a three-week fever
       answers the question the diagnosis chips are asking. The season took the organ specialty off
       this sheet for that reason (`bandOf`), and the payload does not carry it. */
    spec:content.care_setting||'',
    tier:content.difficulty||'',
  };
  return {entry, chips, labels};
}

/* A page where the bay would be: the ward has something to say and nothing to play.
   `#game` ships hidden and the bay unhides it as a run starts, so writing a sentence into it
   without this was a blank screen with the words in it — which is what the "Not this bed" page
   was until this line existed. `plain` turns the bay's grid back into a page. */
function wardPage(html){
  const g=$('#game');
  g.classList.remove('hide','waiting'); g.classList.add('plain');
  g.innerHTML='<div class="wardpage">'+html+'<p><a href="/">← the globe</a></p></div>';
}

/* The strip's sentence: which bed, which shift, what the chart was rebuilt from, and what a
   stranger has to do before any of it is theirs. Every gendered word comes from `pro()` at the
   moment it is said — the ward admits men, and this line is the most-read one on the page.
   `bed` is the board's number, not a second one computed here. */
function shiftLine(bed, shift, before, g){
  /* Short, because the button under her face is what is being asked for now and this line is the
     context beside it. It was one sentence ending in the instruction, and a stranger read past all
     of it looking for something to press. `g` stays in the signature: the day this line carries a
     pronoun again, it will be the patient's. */
  return (bed?'bed '+bed+' · ':'')
    +'shift '+shift+' · chart rebuilt from '+before+' anchored shift'+(before===1?'':'s');
}

/* Which bed she is in, from the board — the one place that works it out. It is a position in a
   walk of every patient on the ward in admission order, not a field on a patient, so the shift
   payload would have to redo the board's whole per-request read to answer it; and a second answer
   is how a page comes to disagree with the board beside it. Best-effort by design: the bed is a
   courtesy on a strip, and a board that cannot be read must not stop a shift from opening. */
async function wardBed(){
  try{
    const w=await (await fetch('/api/ward')).json();
    /* How long the lease runs *today*. The program counts in slots — 3,450 of them — and how long
       that takes is the chain's business: devnet was at 0.166 s a slot on 17 ก.ย., which made the
       lease nine and a half minutes while every sentence on this page said twenty-three. The ward
       measures it and publishes it; this page repeats it and works nothing out. */
    LEASEMIN=(w.policy&&w.policy.lease&&w.policy.lease.minutes_now)||null;
    LEASESEC=(w.policy&&w.policy.lease&&w.policy.lease.seconds_now)||null;
    const her=(w.patients||[]).find(p=>String(p.patient_id)===String(WARD));
    return her&&her.bed?her.bed:null;
  }catch(e){ return null; }
}
let LEASEMIN=null, LEASESEC=null, LEASEENDS=null, LEASETIMER=null;
/* The countdown itself. Started at the take, because that is when the lease starts — the program
   stamps `lease_until_slot` in the block the take lands in, and the page's own clock from that
   moment is the honest local reading of it. Stopped when the shift ends, in either of its ways. */
function leaseClock(){
  clearInterval(LEASETIMER);
  const paint=()=>{
    const el=$('#leaseclock'); if(!el)return;
    const left=LEASEENDS===null?null:Math.round((LEASEENDS-Date.now())/1000);
    el.textContent=leaseLine(left);
    el.classList.toggle('soon', left!==null&&left<=300);
  };
  paint();
  if(LEASEENDS!==null)LEASETIMER=setInterval(paint, 1000);
}
function leaseStop(){ clearInterval(LEASETIMER); LEASETIMER=null; LEASEENDS=null;
  const el=$('#leaseclock'); if(el){ el.textContent=''; el.classList.remove('soon'); } }
/* What the clock is counting, in words, for the strip.
   The lease is a fixed span and the program refuses an anchor past it — `anchor_shift` checks
   `slot >= lease_until_slot` — so a shift that runs out is a shift that cannot be recorded. In the
   last five minutes the line says what to do about it, because that is when saying so can still
   change the outcome. At zero it says the two true things: the bed is free, and this is no longer
   theirs to record. */
function leaseLine(left){
  if(left===null||left===undefined)return '';
  if(left<=0)return 'the lease has run out — the bed is free';
  const m=Math.floor(left/60), s=Math.floor(left%60);
  const clock='shift ends in '+m+':'+String(s).padStart(2,'0');
  return left<=300 ? clock+' — hand over to record it' : clock;
}

/* What the ward says about how long a head is held for, or nothing at all.
   Never a fixed number: the only honest sentence is the one the chain's own rate produces, and a
   ward that has not measured its rate says nothing rather than twenty-three minutes. */
function leaseWords(minutes){
  return minutes ? 'The lease runs about '+minutes+' minutes today.' : '';
}

/* What the one button under the bedside card says.
   Before the head is taken there is one thing to ask for and it is said in her name; after, the one
   act left is the one that writes the shift to the chain. A finished shift keeps it — that is the
   shift with something left to do, and the discharge path was closed for a day because a guard
   read "finished" as "nothing more to do".
   `taken` and `over` are passed rather than read off the page, so the sentence can be read without
   one — and positionally rather than as an object, because the test that runs this function pulls
   it out of the file by brace matching and a destructured parameter list closes the first brace. */
function primaryLabel(taken, over, name, g){
  if(!taken) return 'Take the shift · treat '+(name||g.o);
  return 'Hand over · record this shift';
}

/* The bedside card's own button: the page's one action, under her face and her name, where the
   founder looked for it ("ปุ่มเข้ารักษาคนไข้มันไม่ค่อยเด่น"). The strip keeps a second copy — a
   stranger who has scrolled to the tray should not have to come back up. */
function paintPrimary(){
  const b=$('#wardprimary'); if(!b)return;
  if(!WARD){ b.hidden=true; return; }
  const taken=!takeFirst(WARD, id, null);
  b.hidden=false;
  b.textContent=primaryLabel(taken, over, (WARDSHIFT&&WARDSHIFT.name)||'', pro());
  b.className='btn go primary'+(taken?' handover':'');
  b.onclick=()=>{ if(!taken){ takeShift().catch(e=>wardSay(esc(e&&e.message?e.message:e))); }
                  else { endRun(); } };
}

/* How tall the thing pinned over the transcript is, in the transcript's own column.
   The action bar is sticky at the foot of the stage, so the stage has to end above it — and the
   bar's height is not a number a stylesheet can hold: it grows when the tray wraps and when the
   chips open. So the page measures it and keeps measuring it, and the stylesheet spends the answer
   (`--actsh`, in `html.is-ward .stage`). Measured on the demo capture at 1440×810: the bar was 184
   px tall over 14 px of reservation, and the last 269 px of the transcript were behind it. */
function reserveTheBar(){
  const bar=document.querySelector('.acts');
  if(!bar)return;
  const h=Math.ceil(bar.getBoundingClientRect().height);
  if(h>0)document.documentElement.style.setProperty('--actsh', h+'px');
}
if(typeof ResizeObserver==='function'){
  /* The bar changes height without the window changing size — a tray that wraps, a "+ N more"
     that opens — which is why this watches the element rather than the viewport. */
  addEventListener('load',()=>{
    const bar=document.querySelector('.acts');
    if(bar)new ResizeObserver(reserveTheBar).observe(bar);
    reserveTheBar();
  });
}
addEventListener('resize', reserveTheBar);

/* The monitor, where a thumb can see it.
   On a phone the rail is under the transcript and the tray, six thousand pixels down a shift page:
   a learner deciding what to do next had to scroll past everything they had already read to see a
   pulse. So on a narrow screen the monitor moves into the bedside card and stays at the top of the
   screen while the case is read. One element, moved — a second copy would be two things claiming
   the same four numbers, and one of them would go stale. */
function monitorWhereItIsRead(){
  const mon=$('#mini'), pt=document.querySelector('.stage .pt');
  if(!mon||!pt||!WARDSURFACE)return;
  const phone=matchMedia('(max-width:760px)').matches;
  const rail=document.querySelector('.rail');
  if(phone){ if(mon.parentElement!==pt) pt.insertBefore(mon, pt.firstChild); }
  else if(rail&&mon.parentElement!==rail) rail.insertBefore(mon, rail.firstChild);
}
addEventListener('resize', monitorWhereItIsRead);

/* Every control that treats her, opened or closed in one place.
   Called when the page opens her and again when the head is taken, so there is one answer to
   "may I do this yet" and one sentence saying why not. */
function wardGate(){
  const no=takeFirst(WARD, id, pro().o);
  document.documentElement.classList.toggle('untaken', !!no);
  const cmd=$('#cmd'), send=$('#send'), mic=$('#mic');
  if(cmd)cmd.disabled=!!no;   // the placeholder is renderChips's, and it runs below
  if(send)send.disabled=!!no;
  if(mic)mic.disabled=!!no;
  if($('#chips'))renderChips();
  paintPrimary();
  monitorWhereItIsRead();
}

/* prepare on the server → sign here → submit to the ward's own program. The Eternal
   bay's chainDo is the same shape against a different program, and they stay apart
   on purpose: one refactor between them is one refactor away from Eternal's anchors. */
async function wardDo(path){
  const me=await identity();
  if(!me)return {error:'this browser has no Ed25519 — try Chrome, Safari 17+ or Firefox 129+'};
  const r=await (await fetch(path+(path.includes('?')?'&':'?')+'player='+me.pub)).json();
  if(r.error||!r.sign)return r;
  return await (await fetch('/api/ward/submit?player='+me.pub+'&sig='+await sign(r.sign))).json();
}
function wardSay(html){ const b=$('#wardbar'); if(b)$('#wardsay').innerHTML=html; }
/* The strip at the top of the bay, for both kinds of run this host has.
   A shift's carries the two controls that put a stranger on the chain — take, and hand back. A
   review run has neither: nobody is in the bed, so there is no head to take and nothing to hand
   back, and what a reviewer wants instead is the way to the next case. One function, because the
   ids are read by `wardSay` and by `wardGate` and a second copy of them would be two elements with
   one name — which is what `page.rs::no_id_is_declared_twice` refuses. */
function wardBar(){
  /* The strip a shift page is *served* with is the one a stranger reads first: on a 1.5 Mbit link
     the script does not run for about two seconds, and until it does the page is a header over
     nothing (measured 18 ก.ย. on the globe's own panel, which had the same shape of bug). So the
     server writes this one into `/ward/<id>` and the page adopts it; a review run, which the server
     cannot tell apart from a shift by its markup alone, still gets its own built here.

     Adopting means wiring, not returning: the early return that used to be here left a served
     strip with no handler on its button, which is a page that looks finished and does nothing. */
  if(!$('#wardbar')){
    const tint=REVIEW?'rgba(198,143,31,.09)':'rgba(15,110,92,.06)';
    const controls=REVIEW
      ? '<a class="btn" href="/ward/review">open another case</a>'
      : '<button class="btn go" id="wardtake" disabled>take this shift</button>'
        +'<button class="btn quiet" id="wardback-shift" style="display:none">'
        +leaveWords(false).label+'</button>';
    $('#game').insertAdjacentHTML('afterbegin',
      '<div id="wardbar" style="display:flex;gap:.8rem;align-items:center;flex-wrap:wrap;'+
      'padding:.6rem .9rem;margin-bottom:.6rem;border:1px solid var(--rule,#d8ded9);'+
      'border-radius:.5rem;background:'+tint+'">'+
      '<b id="wardwho">…</b><span id="wardsay" style="flex:1">reading the ward…</span>'+
      '<span id="leaseclock" class="leaseclock"></span>'+controls+
      '<a class="btn" id="wardback" href="/">← the globe</a></div>');
  }
  /* `#game` ships hidden and the strip goes inside it, so a way back in there is a way back
     nobody can press. `waiting` shows the strip and hides the cockpit under it, which has no
     patient in it yet; the run drops the class when the case lands. */
  const g=$('#game'); g.classList.remove('hide'); g.classList.add('waiting');
  if(REVIEW||!$('#wardback-shift'))return;
  /* Wrapped, because a button that throws is a button that does nothing and says nothing. One
     driven run ended with the strip showing its opening line and no sign that the press had been
     received; I could not reproduce it, and the fix for the class is to make any failure in here
     arrive on the strip rather than in a console nobody has open. */
  $('#wardback-shift').onclick=()=>{
    const b=$('#wardback-shift');
    if(!b.classList.contains('armed')){
      /* Asked once. The strip carries the question because it is where the page speaks and
         because a stranger reading the button is already looking at it. */
      b.classList.add('armed'); b.textContent=leaveWords(true).label;
      LEAVEWAS=($('#wardsay')||{}).innerHTML;
      wardSay(leaveWords(true).say);
      clearTimeout(LEAVEARM); LEAVEARM=setTimeout(disarmLeave, ARM_MS);
      return;
    }
    disarmLeave();
    handBack().catch(e=>{
      wardSay('that did not go through: '+esc(e&&e.message?e.message:e)+' — try again');
    });
  };
  $('#wardtake').onclick=()=>takeShift().catch(e=>{
    $('#wardtake').disabled=false;
    wardSay('that did not go through: '+esc(e&&e.message?e.message:e)+' — try again');
  });
}
/* A case, opened to be read. The same surface a shift uses and the same card renderer: what
   differs is that nobody is in this bed, so there is nothing to take, nothing to hand over and
   nothing to anchor — and the strip says so before anything is played. */
async function openReview(){
  wardBar();
  const r=await (await fetch('/api/new?review='+encodeURIComponent(REVIEW)+langQ())).json();
  if(r.error){
    wardPage('<p class="bed">case '+esc(REVIEW)+'</p>'
      +'<h1>Not a case here</h1><p>'+esc(r.error)+'</p>'
      +'<p><a href="/ward/review">← the cases the ward holds</a></p>');
    return;
  }
  const rv=r.review||{};
  const built=wardCard(rv.content, true);
  if(!built){
    wardPage('<p class="bed">case '+esc(REVIEW)+'</p>'
      +'<h1>Not on this page yet</h1>'
      +'<p>This case is held, and there is nothing here yet that can draw it.</p>'
      +'<p><a href="/ward/review">← the cases the ward holds</a></p>');
    return;
  }
  WARDCARD=built.entry; WARDCHIPS=built.chips; WARDLABEL=built.labels;
  /* The run is live from the first paint: there is no head to take, so the gate that holds a
     stranger at a bed has nothing to hold here. */
  id=r.id;
  $('#wardwho').textContent='Review run';
  $('#fallback').alt=rv.title||'the case';
  $('#lobby').classList.add('hide'); $('#game').classList.remove('hide','waiting');
  const sel=$('#ep');
  if(sel&&!sel.querySelector('option[value="'+rv.case+'"]')) sel.add(new Option(rv.case, rv.case));
  $('#ep').value=rv.case;
  SHOWN=[]; STAGE='stem'; stageKey='';
  if(!(built.chips[mode]||[]).length){
    const first=MODES.find(m=>(built.chips[m.id]||[]).length);
    if(first)mode=first.id;
  }
  renderModes(); renderChips();
  paint(r.view);
  bootMonitor();
  run();
  /* The badges, and only the badges: what this run is is said once, in the note the server put in
     the page before any of this ran. Saying it twice is two lines a reviewer has to read to learn
     one thing. */
  const badges=[rv.provisional?'not clinically reviewed':'', rv.withdrawn?'withdrawn from placement':'',
                rv.endemic&&rv.country?('endemic · '+rv.country):'',
                rv.difficulty?('level: '+rv.difficulty):''].filter(Boolean).join(' · ');
  wardSay(esc(badges||rv.case||''));
  wardGate();
}

/* One patient's whole stay, for a patient who is not in a bed any more.
   Every state but "on the ward right now" was "Not this bed": no name, no face, no outcome, and no
   way to the shifts that treated her. The chart is the chain — this is where a stranger reads it. */
function chartPage(c){
  const g=/^m/i.test(c.sex||'')?PRO_M:(/^f/i.test(c.sex||'')?PRO_F:PRO_N);
  const when=t=>t?String(t).replace('T',' ').replace(/\.\d+Z?$/,'').replace('Z','')+' UTC':'';
  const face=c.portrait?'<img src="'+esc(c.portrait)+'" alt="" width="128" height="128" '+
      'style="border-radius:12px;object-fit:cover;display:block;margin:0 0 .9rem">':'';
  const shifts=(c.shifts||[]).map((s,i)=>
    '<li>shift '+(i+1)+' · <a href="/shift/'+encodeURIComponent(s.run_hash)+'">the receipt</a>'
    +' · key '+esc(s.signer)+'…'+(s.kept?'':' · <i>tape not kept here</i>')+'</li>').join('');
  const what=c.state==='went_home'?'went home':c.state==='died'?'died'
            :c.state==='off_ward'?'is on the chain and has no case here':'is on the ward';
  wardPage(
    '<p class="bed">patient '+esc(c.patient_id)+'</p>'
    +face
    +'<h1>'+esc(c.name||('patient '+c.patient_id))+'</h1>'
    +'<p>'+[c.age?esc(c.age):'', c.country_name?'from '+esc(c.country_name):'',
            c.case_title?esc(c.case_title):'', c.difficulty?esc(c.difficulty):'']
        .filter(Boolean).join(' · ')+'</p>'
    +'<p><b>'+esc(cap(g.s))+' '+esc(what)+'</b>'
      +(c.closed_at?' · '+esc(when(c.closed_at)):'')
      +(c.admitted_at?'<br>admitted '+esc(when(c.admitted_at)):'')+'</p>'
    +(shifts?'<h2 style="font-size:1rem;margin:1.4rem 0 .4rem">'
        +(c.shifts.length===1?'one shift':c.shifts.length+' shifts')
        +' on '+esc(g.p)+' chain</h2><ol style="line-height:1.9">'+shifts+'</ol>'
      :'<p>No shift was ever anchored on '+esc(g.p)+' chain.</p>'));
}
const cap=w=>String(w||'').charAt(0).toUpperCase()+String(w||'').slice(1);

async function openShift(){
  /* First, before anything is awaited: the strip is this page's only way back to the globe, and
     until the case lands it is the only thing on the page a stranger can act on. A ward that
     answers slowly — staging blocked for 49 seconds on 17 ก.ย. while the chain was read — was a
     header over nothing until this line moved above the awaits. */
  wardBar();
  const me=await identity();
  const r=await (await fetch('/api/new?patient='+WARD+(me?'&player='+me.pub:'')+langQ())).json();
  if(r.error){
    /* A bed that cannot be opened is not a refusal, it is a patient whose stay is over — or one
       nobody can open yet. Her own chart is the page: who she is, what happened, and every shift
       that treated her with its receipt. The sentence from `/api/new` is kept for the case where
       even the chart cannot be read, because then there is nothing else true to say. */
    try{
      const c=await (await fetch('/api/ward/patient/'+encodeURIComponent(WARD))).json();
      if(c&&!c.error&&c.patient_id!==undefined){ chartPage(c); return; }
    }catch(e){ /* the chart is not needed to say what happened */ }
    wardPage('<p class="bed">patient '+esc(WARD)+'</p>'
      +'<h1>Not this bed</h1><p>'+esc(r.error)+'</p>');
    return;
  }
  /* The case, built from what the payload said about it and from nothing else on this page.
     A payload that could not describe the case is a sentence and no controls: a stranger who is
     told the truth walks away, and a stranger shown another patient's name treats the wrong one. */
  const built=wardCard(r.ward.content);
  if(!built){
    wardPage('<p class="bed">patient '+esc(WARD)+'</p>'
      +'<h1>Not on this page yet</h1>'
      +'<p>This patient’s case is not on this page yet. The chart is on chain and the bed is on '
      +'the board — there is nothing here yet that can draw the case.</p>');
    return;
  }
  WARDCARD=built.entry; WARDCHIPS=built.chips; WARDLABEL=built.labels;
  WARDPENDING=r.id; WARDSHIFT=r.ward;
  $('#wardwho').textContent=(r.ward.name||('patient '+WARD))+' · '+(r.ward.country||'—');
  /* The frame is about to hold her face, so what a screen reader is told about it is her name. */
  $('#fallback').alt=r.ward.name||'the patient';
  $('#lobby').classList.add('hide'); $('#game').classList.remove('hide','waiting');
  $('#wardtake').disabled=false;
  /* The select is the bay's own record of which case is running — `markSeen`, the star, the
     author line and the analytics event all read it — so the case is added as its only option.
     Nothing about the case is *looked up* from it any more: `epOf` answers that off the card. */
  const sel=$('#ep');
  if(sel&&!sel.querySelector('option[value="'+r.ward.case+'"]')){
    sel.add(new Option(r.ward.case, r.ward.case));
  }
  $('#ep').value=r.ward.case;
  SHOWN=[]; STAGE=openStage(r.ward.case); stageKey='';
  /* The kit opens on a row this case actually has. `ask` for almost every case, and the one that
     has no questions opens on whatever it does have rather than on an empty shelf. */
  if(!(built.chips[mode]||[]).length){
    const first=MODES.find(m=>(built.chips[m.id]||[]).length);
    if(first)mode=first.id;
  }
  renderModes(); renderChips();
  paint(r.view);
  bootMonitor();
  /* Said here rather than above, because `pro()` reads the case the select now names: the ward
     admits men and women, and until this line runs the page would be speaking about EP1's
     patient. Driving a shift on Rafael Moreira printed "shift 1 of her stay" over a man. */
  wardSay(shiftLine(null, r.ward.shift, r.ward.shifts_before, pro()));
  /* And again with the bed when the board answers, which it does from its own fifteen-second
     cache. Said twice rather than awaited, so a slow board never holds the page — and dropped if
     the head has been taken in the meantime, because by then the strip is saying something newer
     and a late promise overwriting it would be the page talking over itself. */
  wardBed().then(bed=>{
    if(bed&&!id)wardSay(shiftLine(bed, r.ward.shift, r.ward.shifts_before, pro()));
  });
  /* Read, not treat. Every control that would touch the patient says which of the two this is. */
  wardGate();
  /* Two controls that mean something in the bay and nothing here: "restart" would quietly open a
     practice run of her case and lose the shift, and "← episodes" is a shelf this patient is not
     on. The strip's own link is where a stranger goes back to. */
  /* Nothing to hide: the ward host composes neither. "restart" would quietly open a practice run
     of her case and lose the shift; "← episodes" is a shelf this patient is not on. */
}
async function takeShift(){
  const b=$('#wardtake'); b.disabled=true;
  wardSay('opening an account for this browser…');
  const opened=await wardDo('/api/ward/open');
  /* Already open is not a failure: a key that has played here before has an account, and the
     program says so rather than making a second one. */
  if(opened.error&&!/already/i.test(opened.error)) { b.disabled=false; return wardSay(esc(opened.error)); }
  wardSay('taking the head…');
  const took=await wardDo('/api/ward/take?id='+WARDPENDING);
  if(took.refused){ b.disabled=false; return wardSay('<b>refused.</b> '+esc(took.refused)); }
  if(took.error){ b.disabled=false; return wardSay(esc(took.error)); }
  wardSay('declaring the shift before it is played…');
  const said=await wardDo('/api/ward/declare?id='+WARDPENDING);
  if(said.error&&!said.declared){ b.disabled=false; return wardSay(esc(said.error)); }
  /* Now the bay is live: every control in the page guards on `id`. */
  id=WARDPENDING; over=false; $('#endrun').disabled=false; $('#endrun').textContent='hand over';
  b.style.display='none'; $('#wardback-shift').style.display='';
  /* Everything the bay's `start` does at the bottom, which a shift on the ward needs just as
     much: the controls open, and the clock runs. Without the clock a shift sits at 0:00 for ever
     and the idle span is the only time she has — the founder watched exactly that. */
  wardGate(); $('#cmd').focus(); run();
  /* Two short sentences rather than one long one: this is the line a stranger reads at the moment
     the controls open, and `plain_words.rs` holds every sentence on the strip to twelve words. */
  wardSay('the head is yours until you hand over. What you do here is on '+pro().p+
          ' chart, under your key. '+leaseWords(LEASEMIN));
  /* The head is theirs from this second, so the countdown starts from this second. */
  LEASEENDS=LEASESEC?Date.now()+LEASESEC*1000:null;
  leaseClock();
  armTheExit();
}

/* The end of a shift on the ward: what happened, in the ward's words, and the one thing left to
   do about it. No result panel, no mark sheet, no sweep, no cinema — those are the season's, and
   every one of them speaks about the season's patient. The chart stays on screen exactly as the
   next stranger will inherit it. */
function wardFinish(v){
  over=true; stop(); disarmEnd();
  const win=!!v.outcome&&v.outcome.startsWith('Win');
  const died=!!v.outcome&&v.outcome.startsWith('Death');
  /* The controls close because there is nothing more to do to her: what is on the tape is what
     the chain will carry. Handing over stays open, and is now the only thing that is. */
  const cmd=$('#cmd'), send=$('#send'), mic=$('#mic');
  if(cmd){ cmd.disabled=true; cmd.placeholder='this shift is finished — hand over'; }
  if(send)send.disabled=true;
  if(mic)mic.disabled=true;
  $('#chips').querySelectorAll('button').forEach(b=>{ b.disabled=true; b.title='this shift is finished — hand over'; });
  $('#pause').disabled=true;
  disarmEnd();
  $('#endrun').disabled=false; $('#endrun').textContent='hand over';
  armTheExit();
  wardSay(died
    ? '<b>'+esc(nameNow())+' died on your shift.</b> Hand over — the chain records it, under your key.'
    : win
      ? '<b>'+esc(nameNow())+' is ready to go home.</b> Hand over to close '+pro().p+' stay on chain.'
      : '<b>this shift is finished.</b> Hand over, and the next stranger starts where you stopped.');
}

/* What the chain says happened to her, as a clause. The state is one word — went_home, died,
   on_ward — and each already carries its own verb: "she is went home" was the page gluing "is"
   onto a past tense. Anything the chain says that this build does not know is not an ending, so
   it reads as the state it is rather than as a verb the page invented. */
function stateSentence(state, p){
  if(state==='went_home')return p.s+' went home';
  if(state==='died')return p.s+' died';
  return p.s+' is on the ward';
}

/* Who is in the bed, for a sentence: the ward's name for her when the page has one. */
function nameNow(){ return (WARDSHIFT&&WARDSHIFT.name)||('patient '+WARD); }

/* Handing her back: the head goes down, nothing is anchored, and this shift's tape is discarded.
   Said plainly, because a stranger deserves to know that walking away costs her nothing and
   records nothing of theirs. */
async function handBack(){
  if(!id)return;
  $('#wardback-shift').disabled=true;
  wardSay('leaving without recording…');
  const r=await wardDo('/api/ward/release?id='+id);
  if(r.refused){ $('#wardback-shift').disabled=false; return wardSay('<b>refused.</b> '+esc(r.refused)); }
  if(r.error){ $('#wardback-shift').disabled=false; return wardSay(esc(r.error)); }
  stopBeating(); leaseStop(); id=''; stop();
  /* She is somebody else's patient from this second, so the controls close the way they were
     closed before the head was taken — and say the same thing about why. */
  wardGate();
  wardSay('<b>left without recording.</b> the next person finds '+pro().o+' as you did. '+
          '<a href="/">back to the globe</a>');
}

/* A stranger who closes the tab should free the bed in seconds rather than in the length of a
   lease. Two halves, and neither of them depends on a signature surviving anything:

     * while this page holds a head it says so, every `beat_again_in_seconds` the ward asks for.
       The ward takes the head back when the beats stop — two missed beats — on its own authority,
       which is what `FreeShift` is for;
     * and on the way out the page says it is leaving, which frees the bed now rather than in two
       missed beats. `sendBeacon` because it is the only thing a closing page can still send, and a
       POST because that is the only thing `sendBeacon` sends — the old exit posted a signed
       release to a route that only answered GET, so it had never once landed.

   What this replaces: a release signed in advance and refreshed every 45 seconds. It could not
   have worked. A blockhash on this chain is worth about 26 seconds (measured 17 ก.ย.), so the
   signature the page was holding had expired before the page could ever use it. */
let BEATTIMER=null;
function armTheExit(){
  clearTimeout(BEATTIMER);
  const beat=async()=>{
    if(!id||!WARD)return;
    try{
      const r=await (await fetch('/api/ward/beat?id='+encodeURIComponent(id),
                                 {method:'POST', keepalive:true})).json();
      const s=Number(r&&r.beat_again_in_seconds)||30;
      BEATTIMER=setTimeout(beat, s*1000);
    }catch(e){
      /* A beat that did not go through is not a shift that ended: try again on the same rhythm,
         and let the ward's own grace decide what a silence means. */
      BEATTIMER=setTimeout(beat, 30000);
    }
  };
  beat();
}
/* The shift is over and the head is already back on chain — handed over, or handed back. The ward
   forgets this page rather than sweeping a head nobody holds and paying for a transaction to say
   what the chain already says. */
function stopBeating(){
  clearTimeout(BEATTIMER); BEATTIMER=null;
  if(id&&WARD)navigator.sendBeacon('/api/ward/beat?id='+encodeURIComponent(id)+'&done=1');
}
addEventListener('pagehide',()=>{
  if(!id||!WARD)return;
  navigator.sendBeacon('/api/ward/left?id='+encodeURIComponent(id));
});
async function handOver(){
  if(!id)return;
  try{ await handOverInner(); }
  catch(e){
    /* The tape is on the server either way: a shift that fails to hand over is not a shift that
       was lost, and telling somebody to open the patient again is better than a dead button. */
    $('#endrun').disabled=false;
    wardSay('that did not go through: '+esc(e&&e.message?e.message:e)+
            ' — <a href="/ward/'+WARD+'">open '+pro().o+' again</a>');
  }
}
async function handOverInner(){
  disarmEnd(); $('#endrun').disabled=true;
  wardSay('reducing your shift…');
  const over_=await (await fetch('/api/handover?id='+id+asMe())).json();
  if(over_.error)return wardSay(esc(over_.error));
  /* The tape has been reduced and the leaf named, so the clock stops here. It used to keep
     ticking through the anchor — each tick another step posted to /api/step — and the anchor then
     reduced a longer tape than the one the hand-over had filed: two hashes for one shift, and the
     chain took the one nobody kept. The server refuses those steps now; this is the page not
     making them. */
  stop();
  wardSay('anchoring '+over_.shift.beats+' beat'+(over_.shift.beats===1?'':'s')+' onto '+pro().p+' chain…');
  const a=await wardDo('/api/ward/anchor?id='+id);
  if(a.refused){
    /* The refusal is the mechanic, and it is shown in the words the server chose — a person at a
       bed cannot read a program error code. Their work is not lost: it is on their tape. */
    return wardSay('<b>refused.</b> '+esc(a.refused)+
                   ' <a href="/ward/'+WARD+'">open '+pro().o+' again</a>');
  }
  if(a.error)return wardSay(esc(a.error));
  stopBeating(); leaseStop();
  /* The receipt is the point of the whole thing and it had no link: a stranger who just anchored
     a shift was told it landed and given a way back to the globe, and nothing that shows what
     they did. `/shift/<run hash>` is public and needs no key — see the receipt page. */
  /* Addressed by the leaf when the chain gave us one: that is this shift and no other, and it is
     what a judge reads off her account as the head. The run hash is the tape's, which two people
     who did the same things would share — a good fallback, a worse name. */
  const name=a.head||over_.run_hash;
  const receipt=name?' <a href="/shift/'+encodeURIComponent(name)+'">the receipt for this shift</a> ·':'';
  wardSay('<b>handed over.</b> '+pro().p+' chain is '+a.shifts+' shift'+(a.shifts===1?'':'s')+
          ' long and '+esc(stateSentence(a.state||'', pro()))+'.'+receipt+
          ' <a href="/">back to the globe</a>');
}
if(WARD) addEventListener('load',openShift);
if(REVIEW) addEventListener('load',openReview);

refreshRecord();
if(location.hash.startsWith('#play')){
  const e=(location.hash.split('=')[1]||'').split('&')[0]; if(e)$('#ep').value=e;
  /* &script runs a canned opening — for a kiosk left playing, and for checking that the log
     actually renders what the code says it does. */
  if(location.hash.includes('&kit')) setTimeout(()=>openPicker(),3000);
  if(location.hash.includes('&script')) setTimeout(async()=>{
    await new Promise(r=>setTimeout(r,1200));
    await askHer('What happened to you?');
    await new Promise(r=>setTimeout(r,600));
    commit(KITLBL.o2, 15);
    await new Promise(r=>setTimeout(r,900));
    /* Not "let her stand up": this canned opening runs against whichever case the #play hash
       names, and seven of the seventeen are men. The scenario matchers key on "stand", so the
       neutral phrasing reaches the same intervention in every case it reaches at all. */
    doOrder('let the patient stand up');
  }, 2500);
  $('#lobby').classList.add('hide'); $('#game').classList.remove('hide');
  addEventListener('load',()=>{ const b=$('#start'); if(b)b.click(); });
}
