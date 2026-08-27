# The stations' patient stills

One frame of the patient in the bay, per station, per state — the same thing EP1 has and the
same Embla pipeline that shot it. Drop a file in here and the station has it: the server reads
this directory at request time and the set table tells the page which states exist, so nothing
needs rebuilding and no table needs editing.

    <station>_<state>.jpg

`<station>` is the station id exactly as the shelf spells it (`osce-a`, `osce-a2`, `osce-b`, …)
and `<state>` is one of **stable · deteriorating · critical · arrest**. Anything else in here is
ignored — both halves of the name are checked against the server's own tables before a file is
opened.

Shot notes, the frame's own grading, and what happens while a file is missing:
`docs/internal/STATION_STILLS_WIRING.md`.
