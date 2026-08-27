# Case media — attribution

Every file in this directory is a clinical image taken from the **embla-cases** media bank
(`embla-cases` v2026.08.0, commit `e933ad24`, `media/`) and re-encoded for the web. Only the
datasets we actually ship are listed here; the full bank's attribution lives in that repo's
`media/ATTRIBUTION.md`.

**This is a public repository and the images are served to end users, so attribution travels
with the pixels.** Two of the six files are CC-BY 4.0, which makes credit a licence condition,
not a courtesy — see *What the licences require of us* at the bottom.

---

## 1. `ecg-*.png` — 12-lead electrocardiograms

- **Source:** PTB-XL, a large publicly available electrocardiography dataset (PhysioNet).
- **Licence:** **Creative Commons Attribution 4.0 International (CC-BY 4.0)** —
  <https://creativecommons.org/licenses/by/4.0/>
- **Cite:** Wagner P, Strodthoff N, Bousseljot R-D, Kreiseler D, Lunze FI, Samek W, Schaeffter T.
  *PTB-XL, a large publicly available electrocardiography dataset.* Scientific Data 7, 154 (2020).
- **Link:** <https://physionet.org/content/ptb-xl/>
- **Modified:** yes, twice. The PNGs are **teaching renders** of PTB-XL waveform records (a
  derivative work made in embla-cases, not a scan of an original trace), and this copy is further
  **palette-quantised to 64 colours** for web delivery. Pixel geometry, paper speed and gain are
  unchanged: 25 mm/s, 10 mm/mV, 10 s, full disclosure. No resampling, no crop, no lossy pass —
  the ST segment is bit-for-bit where the render put it.

| file | record | finding | size |
|---|---|---|---|
| `ecg-sinus-tachycardia-04408.png` | PTB-XL `records500/04000/04408_hr` | Sinus tachycardia, no ischaemic change | 192 KB (from 674 KB) |
| `ecg-st-elevation-anterior-01278.png` | PTB-XL `records500/01000/01278_hr` | Anterior ST elevation (PTB-XL SCP class `ASMI`/`AMI`) | 36 KB (from 911 KB) |

## 2. `cxr-*.png` — chest radiographs

- **Source:** NIH ChestX-ray14 (NIH Clinical Center).
- **Terms:** *"There are no restrictions on the use of the NIH chest x-ray images."* Commercial and
  research use permitted. **Not** a formal CC0 dedication and **not** CC-BY; NIH asks that users
  cite the source and acknowledge the NIH Clinical Center, which we do here and on the page.
- **Cite:** Wang X, Peng Y, Lu L, Lu Z, Bagheri M, Summers RM. *ChestX-ray8: Hospital-scale Chest
  X-ray Database and Benchmarks on Weakly-Supervised Classification and Localization of Common
  Thorax Diseases.* IEEE CVPR 2017.
- **Link:** <https://nihcc.app.box.com/v/ChestXray-NIHCC>
- **Modified:** re-encoded as optimised 8-bit greyscale PNG at the original 1024×1024. No crop, no
  resample, no lossy pass, no window/level change.

| file | bank ref | label | size |
|---|---|---|---|
| `cxr-normal-1.png` | `cxr/normal-1.png` | No finding | 388 KB |
| `cxr-normal-3.png` | `cxr/normal-3.png` | No finding | 367 KB |
| `cxr-normal-4.png` | `cxr/normal-4.png` | No finding | 325 KB |
| `cxr-consolidation-pneumonia-1.png` | `cxr/pneumonia-1.png` | Pneumonia | 309 KB |

## 3. `card/*.jpg` — shelf card art

Crops of the six images above, resized to 720×450 and saved as quality-82 JPEG for the clinic
shelf. **Derivative works — they inherit the licence of the file they were cut from**, so the two
`card/ecg-*.jpg` files are CC-BY 4.0 and carry the same obligation as the PNGs. Never present a
card crop as a diagnostic image: it is deliberately partial.

---

## Provenance — source hashes

Verify a file against the bank with `shasum -a 256` on the embla-cases side:

```
694623c1f25979e96bcd066f503664429813df08be412e5c5f31cd7176fb2a7b  media/ecg/04408_hr.png
4d723a57e4753c2a41e91962514bb8e8973d4493a7e22b631603431fa3674b69  media/ecg/01278_hr.png
84cb473f69dbf9dca2fb946de92545031f03d6501be510555067aee46c17033e  media/cxr/normal-1.png
9a6be22a769638cb67c527039c0bd0d7a960704c95b93d33df44e82cafd55769  media/cxr/normal-3.png
8e6d80172a1fc3a58719d3ead3360433880e940e77913d54564e46fae62b7331  media/cxr/normal-4.png
316896df8ea39cd66b0474d460e8b347933cb0e9e1c4fd9894612a3839f3d6d3  media/cxr/pneumonia-1.png
```

---

## What the licences require of us

**CC-BY 4.0 (the two ECGs, and their card crops) — attribution is a condition of the licence.**
Section 3(a) requires that we keep, *in any reasonable manner for the medium*: the creator's name,
a copyright notice, the licence notice and its URI, and **a statement that the material was
modified**. Ours is modified twice over (rendered, then quantised), so the modification notice is
not optional.

Practically, that means **two places**:

1. **This file, shipped beside the images** — satisfies the repository half. Anyone who clones or
   redistributes the directory gets the credit with it.
2. **The served page** — a viewer who never opens the repo must still be able to reach the credit.
   CC-BY 4.0 accepts a link for this ("you may satisfy the conditions … by providing a URI or
   hyperlink to a resource that includes the required information"), so a one-line credit under
   the film, or a persistent "image credits" link in the footer pointing at a credits page, is
   enough. It does **not** have to be a wall of text over the image.

Suggested caption, short enough to sit under a film in-game:

> ECG: PTB-XL (Wagner et al., 2020), CC-BY 4.0, rendered and re-encoded ·
> CXR: NIH ChestX-ray14 (Wang et al., 2017)

`ecg-st-elevation-anterior-01278.png` already burns `ECG: PTB-XL (PhysioNet, CC-BY 4.0)` into its
own header strip. `ecg-sinus-tachycardia-04408.png` does **not** — its strip says only
"PTB-XL, teaching render" — so the page-level credit is the one that carries it. Do not rely on
the burned-in text.

**NIH ChestX-ray14 (the four CXRs)** imposes no attribution condition, so nothing here is a legal
requirement. We credit it anyway, in the same line, because a teaching product that hides where
its films came from has no business asking learners to trust them.

---

## Clinical caveats — read before wiring these to a station

- **`ecg-st-elevation-anterior-01278.png`** is flagged in the bank's curated
  `media/mappings/ecg-mapping.tsv` as `have⚠ KOL pick leads+verify acute` — the PTB-XL SCP class
  is `AMI`/`ASMI`, and **ASMI can be an old anterior MI rather than an acute STEMI**. The film
  shows convincing coved ST elevation in V2–V4, but "acute" is asserted by the case text, not
  proven by the record. A KOL sign-off is outstanding.
- **`cxr-consolidation-pneumonia-1.png`** carries the ChestX-ray14 *Pneumonia* label, and those
  labels are NLP-mined from radiology reports, not read by a radiologist for this dataset. Unlike
  the ECG bank there is **no KOL-reviewed CXR mapping** in embla-cases — there is only
  `tools/media-check.py`, which proves a reference resolves to a file and says nothing about
  whether the film shows the disease. This particular film does not obviously show the "wedge of
  white in the right lower lobe with air bronchograms" that osce-c3's beat describes. Keep the
  case's own report text beside the image so the teaching point is carried by the words, and put
  the CXR bank in front of a KOL before public launch.
- The four `normal` films are interchangeable in every clinical sense; each station keeps the one
  its source case named purely so the trail back to embla-cases stays exact.
