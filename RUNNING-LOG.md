# RUNNING-LOG.md — run 4 (multi-day, started 2026-10-17)

Continuity file: if context compacts, resume from here + SESSION-REPORT.md
(run 3 ledger) + AGENTS.md. HEAD at run start: fbfd9e1 (64 tests),
teams-core 9b7cbda (pushed).

## Ground rules (decay-proof recap)

- Never tag/release/push teamsfast. teams-core: commit AND push, minimal.
- One fix = one commit; `cargo build && cargo test` green first; never
  `git add -A`; stage per-file; split mixed files via /tmp backup dance.
- Only I edit files. Subagents read/QA/research.
- Images → `vision` subagent only. Verify before code (7-deep false-
  positive class). `pgrep -ax teamsfast` before any QA launch.
- QA binary: ./target/debug/teamsfast. Xvfb caps 1100x760. Light QA:
  ~/.config/teamsfast/settings.json + rm app.ron, restore after.

## Day 1 — 2026-10-17

- 09:1x Bootstrap done: read AGENTS/SESSION-REPORT/memory ×3; git state
  verified (fbfd9e1 / 9b7cbda pushed / clean / no live process).
- Baseline build+test green; found 1 app warning (mark_read `tx` unused
  since d3e1b25) → fixed, committed 7844576. App crate back to 0
  warnings. (teams-core path-dep shows 28 warnings of its own — noted
  for §D, would need a teams-core commit+push.)
- NEXT: §A.1 red-team wave (3 agents on 71f431c..HEAD + teams-core
  9b7cbda) while I build the fuzz corpus from the real archive (§A.2).

## Findings ledger (agent → claim → disposition)

Agent 1 (invariants, 71f431c..fbfd9e1):
- P2 media.rs decode join parks serial worker → **FIXED 30aa55d** (watcher task owns the join).
- P2 cards.rs table Grid id = content hash only (dup cards collide) → **FIXED 7ed8d7e** (ui.id().with).
- P3 table cells drop bold/italic → **FIXED 7fb5dc8**.
- P3 chat table re-parses per frame → open (cheap, cached segs bound it; house-style cache = P3).
- P3 midnight-crossing meetings render into clipped space below grid → open (P3; wrap-to-next-day design Q).
- P3 is_sep_row false-positives on all-dash data rows → open (P3, rare).
- P3 settings ScrollArea no id_salt → open (only ScrollArea in window; P3).
- P3 TABLE/CALDUMP env read per event → open (event-handler frequency; P3).
- Everything else CHECKED CLEAN (tick gate, culling, batches, paint ordering,
  refresh semantics, tint_over, char safety, r2l, selectables, bold sweep,
  br/p flush, CallSlot guard, separators, theme-at-startup).

Agent 2 (parser inputs, hand-traced):
- parse_html drops `<br/>` (name "br/") and closing block tags never break
  ("/div") → **FIXED f1eb8e7** (+ close-pattern `</b>` vs `</blockquote>`
  precision + strip_tags `br/` + trailing-space trim). 6 tests.
- strip_tags `</p>` trailing space → **FIXED (same commit)**.
- `>` inside quoted attributes mangles tag cut → **DEFERRED with evidence**:
  zero occurrences in 1503-payload corpus; degrades to mangling, no panic.
- pipe inside markdown-link URL in table cell splits cell → **DEFERRED P3**
  (no occurrence in tenant corpus; link-aware table splitter = design Q).
- `\|` at end of row, CRLF `\r` retention, all-dash rows, `***bold italic***`,
  multiline emphasis, nested emphasis, entities beyond the 6 named →
  **DEFERRED**: corpus shows only the 5 named entities (`&nbsp; &amp; &quot;
  &gt; &lt;`), no numeric refs; rest are degradation-not-bug shapes.
- lane_geometry: no panics (clamp chain); multi-day/malformed drop silently →
  open P3 (calendar robustness), negative duration → 15-min stub.
- extract_card: 789/789 real Swift payloads extract cleanly → **leniency
  (URL-safe b64 etc.) DEFERRED with evidence**.

Agent 3 (teams-core 9b7cbda):
- Payload shapes CORRECT vs sdp_dictionary.bin captures (PascalCase
  ["Audio","Video"]; web sends superset with ScreenViewer). No action.
- Intel for §C: join_conversation_with_modalities can declare ScreenViewer;
  camera fail degrades to black frames (modality decoupled from pipeline).
- Pin test is fragment-only (acceptable — body is a json! literal).
- teams-core warnings: ~610 default-features (mostly serde_json::Value
  fields in api/) — NOT minimal to clean; defer. 28 show in our audio build.

Fuzz (§A.2): 1503 payloads × truncation/injection/tag-swap/pipe-flood →
**0 panics** (1.48s release). Harness committed 2d0e09a (env-gated).

## Day 1 close (2026-10-17 ~14:00 local) — CHECKPOINT GREEN

- 19 commits (7844576..1ebcc5a), 75/75 tests, 0 app warnings.
- Soak §A.3: 173 samples / 2h52m — RSS 155MB→101.7MB then FLAT (no leak),
  fds 48 stable, threads 61 stable. PASS.
- Fuzz §A.2: 1503 real payloads × mutations, 0 panics (twice: before and
  after the parser changes). Harness committed, env-gated.
- Battery §B: dark 6/6 PASS at HEAD; light E2E all surfaces (P2s fixed:
  weak-text AA, search slab; canvas-gray deviation deferred — design
  decision for the user); offline mode FIXED (cached list paints, banner,
  no latch — red-team follow-ups included).
- Day-1 red-team (full diff): P1 offline latch + P2 stale clobber + P2
  <b/> consume FIXED same-session; 5 P3s fixed, 4 accepted w/ evidence.
- Keyboard nav: DEFERRED — XTEST fake events never materialize in this
  sandbox (verified with a raw X listener; wtype is Wayland-only).
- teams-core: untouched, still 9b7cbda = origin/main.

### Day-2 plan
§C video modality: join-preview dialog (camera/mic toggles) → flip
JoinMeeting video=true → echo-verify; camera-on-later control slot.
Then §D remainder (offline-guard sweep central fix, lane/month polish),
§E platform.

## Commit log (run 4)

| Commit | What/why |
|---|---|
| 7844576 | mark_read: underscore unused tx (left by d3e1b25) — 0 warnings restored |
| f1eb8e7 | parse_html: self-closing <br/> + closing block tags break lines; close-pattern precision; strip_tags br/ + trailing space. 6 tests |
| 30aa55d | Image decode no longer parks the worker (watcher task owns the join) |
| 7ed8d7e | Card table Grid ids mixed with bubble id (dup-card collision) |
| 7fb5dc8 | Pipe-table cells render bold/italic pieces |
| 2d0e09a | Fuzz harness (env-gated corpus test); 1503 payloads 0 panics |
| f45bc74 | Calendar lane index = matched lane, not lane COUNT (overlap bug from fddbfc4; CALGEOM hook + 3 tests) |
| bdd5dd7 | Week now-line paints above chips (was broken dashes) |
| 0227aac | Week opens scrolled to today/now (scroll_target consumed next begin → needs request_repaint) |
| 8ffdb87 | Calendar compact pills prefer the time range when it fits |
| f0ca701 | Light: weak text from palette (AA fix; placeholder/send 1.8→5.9:1) |
| dadf44b | Light: search recesses toward white (Palette::is_light) |
| c771e27 | Offline: cached chat list actually paints (bwrap-verified 3/3) |
| 2013138 | Offline: no latch (Ready clears), no stale clobber (ChatsCached merge rule) |
| 46ea1c3 | parse_html: self-closing inline tags drop, not consume (red-team) |
| 4bf91fa | Fuzz harness: empty corpus skips |
| 27f2ac0 | is_light keys on panel |
| 1ebcc5a | stale now-line comment dropped |

### §B battery round 1 (dark) — critic verdicts
- chat/table: 5/5 PASS (tables bold headers, zebra, no clipping).
- calendar: 2 P1s claimed → BOTH REAL (lane overlap root-caused via new
  TEAMSFAST_CALGEOM=1 dump: `geo[i].0 = lanes.len()-1` bug, fixed f45bc74;
  today clipped → scroll-to-now 0227aac; now-line under chips → bdd5dd7).
  All three vision-verified after fix.
- month "09:30 I" lone-glyph pill: subject likely literally "I" — pending
  CALDUMP check (suspected false-positive class #8, variant of one-letter).
- sections (todo/activity/teams/files): all PASS; P2s are feature-scope
  (todo smart lists, teams landing selection, people-picker) → roadmap,
  not bugs. P3s noted (flag glyph, feed chrome, sort menus, dup rows
  seeding artifact).
- chrome/dialogs (settings/join/newchat/ring/typing/draft): all PASS,
  zero P1/P2 except newchat people-picker (by design).

### New QA lesson
- The soak app's append-writer + any second app launch = the log gets
  truncated by the launcher then progressively OVERWRITTEN by the soak's
  appends at the new EOF. Log-evidence QA runs must grep the log DURING
  the app's lifetime (before kill), not after.

## Day 2 (2026-10-17 afternoon) — §C video modality COMPLETE

| Commit | What/why |
|---|---|
| 8c8b067 | Backend: JoinMeeting camera/mic flags; pre-join preview capture (forwarder chain) |
| 5e2497b | Pre-join dialog: live camera tile + Mic/Camera toggles (defaults mic on, camera off); calendar/meet-now route through it. Vision 5/5 (live 4:3 webcam pillarboxed) |
| 09a3dfd | Camera-on-later: stage camera button on ALL calls; audio leg flip starts mid-call capture into CameraSlot. Echo-verified: video_packets_sent=1554 after +15s flip, no renegotiation |
| 4411c6d | Share gates on the live video leg (late-camera + join-with-camera shareable); preview tee always on. Echo+vision 4/4 (bars monitor tile, Sharing label, lit control) |

teams-core PUSHED: c987dec (CameraSlot — slot-over-pre-spawn resolution in
the send loop, VideoFrame-typed to stay feature-clean; 407/407).

Echo-call regression after mic-param plumbing: call_accepted=true,
audio 1633 / video 2129 packets (45s run).

QA hooks added: TEAMSFAST_JOINCAM (pre-arm dialog toggle),
TEAMSFAST_LATECAM=<secs> (mid-call camera flip + optional chained share
+ stage open).

§C status: C.1 ✅ C.2 ✅ C.3 ✅ (renegotiation/card-submits/caller-cancel
stay capture-gated per policy). Next: §D offline-guard central sweep.

## Day 2 close — CHECKPOINT GREEN (2026-10-17 evening)

- 30 commits total run-to-date, 82/82 tests, teams-core PUSHED to 7ceef15.
- Battery 6/6 PASS (dark; light verified in the two light fixes).
- Deferrals: teams-for-linux benchmark (user sign-in), keyboard nav
  (no injection), inline notif replies (notify-rust 4), P3 skips
  (scroll thumb = egui-internal; focus rings/separator rhythm need
  interactive or multi-row data; narrow-lane truncation inherent).

## Day 3 close — CHECKPOINT GREEN (2026-10-17 late)

- 33 commits run-total, 82/82, teams-core 7ceef15 pushed. Battery 8/8.
- Error-path QA found the invisible-Retry bug (fixed e8a875f);
  camera on-state polish (112537c). Design docs local in docs/drafts/.
- Soak #2 runs into the evening; stop with `kill $(cat /tmp/soak5/app.pid)`.
- New QA hook: TEAMSFAST_FAILSEND=1 (offline-only usage) drives the
  pending→failed→retry bubble path.
- +6c1d517: P3 batch (settings scroll salt; QA env flags once-locked —
  CALDUMP re-verified 104 lines post-refactor). 34 commits run-total.

## Run 5 (2026-10-17 late evening) — §3.A layout shift, day 1 part 1

HEAD 6c1d517 → 3e496bf (unused mut) → 12b87ab (RECTLOG/REACT/settlediff
harness) → 4b32967 (width stability) → 9451037 (reserved reaction strip).
84/84 tests. Soak #2 verdict: NO leak (RSS flat 226,360KB 7.3h).

Shift-source ledger (all rectlog-proven):
1. Reactions conditional row ABOVE bubble (+46px on arrival) → FIXED
   9451037: reserved 18px strip BELOW bubble, bubble-right anchored,
   meta merged into the row; own-slot 150px floor; unit + live proof.
2. Horizontal overflow → central min_rect grows past screen (1116 vs
   1100) → wrap width differs cached-vs-network (+16px rewraps) AND
   offline rendered 24px wide forever. Sources: header title (raw
   thread-id offline, unbreakable 17px heading), attachment buttons
   (full filename), body tokens >48 chars (URLs, Jira media params).
   FIXED 4b32967 (truncating budgeted title label, ellipsized buttons,
   LayoutJob break_anywhere). Width now 727/1092 EVERY frame, both modes.
3. egui scrollbars here are FLOATING (reserve ZERO width — verified);
   AlwaysVisible attempt reverted (no effect under floating).
4. Settle byte-diff t1-vs-t2 IDENTICAL on the Jira chat (the bar).
5. t0 crop at 0.7s often pre-paint (blank) — timing note.

Vision QA (2 rounds): chips below bubble right-aligned within 1px;
timestamp shares the row; header truncates; no collisions. Vision
false-positive #10: "sidebar divider identical online/offline" — it
wasn't (the 24px delta lived in the conversation pane, panel probes
disproved the vision x-ranges).

Pending next: A2 image-size audit (image_sizes persisted; guess 340x200
on first-ever view), A6 load-older prepend anchor (needs a QA auto-click
hook), light battery, adversarial review, checkpoint.

## Day-1 checkpoint (2026-10-18 ~00:40) — GREEN

Workstream A COMPLETE: f25fffe (red-team+rhythm) on top of 42932af,
9451037, 4b32967, 12b87ab, 3e496bf. 84/84. Settle byte-diff IDENTICAL
dark+light. Report + memory written. NEXT (day 2): §3.B manual battery
wave (12+ surfaces incl. light), §3.C backlog (month chip ellipsis, week
hour-gutter, chat-table parse cache, is_sep_row corpus check), §3.D
Wayland raise design note + soak #3 after layout work. Stop any soak
before QA (gotcha).

## Day-1 part-2 close (2026-10-18 ~02:15) — GREEN

Battery round 5: 16 shots → 3 critics → 7 verified fix commits (e7c7e44
typing/header/chips, 04111f7 week fixed gutter, 003e8ca pill ellipsis,
a2d1c9a files dedupe, 94bf29c+0eba22f dialogs, c85bcc3 ring). §3.C done
(week gutter, month ellipsis, is_sep_row corpus-checked, table cache
deferred). Wayland raise design note in docs/drafts/ (LOCAL). Soak #3
RUNNING overnight (/tmp/soak6 — app.pid + soak.log; display :79).
Day-2 menu: own-avatar column wobble fix, adaptive typing label,
full-day-diff adversarial review, §3.E stretch, harvest soak.
Day total: 14 commits, 82→84 tests.

## DAY 1 CLOSED (2026-10-18 ~02:30) — GREEN at d807b4c

Full-day red-team done (15th commit batch): panic-free width math
(sub-66px panes crashed clamp!), per-chat prepend key, exact +K reserve,
qa_flag hot env reads. Settlediff IDENTICAL re-confirmed at EOD.
Day 1 totals: 15 commits (3e496bf..d807b4c), 82→84 tests, 0 app
warnings, soak #3 alive (230MB). Memory + report current. Day-2 resume:
harvest soak, own-avatar column wobble, adaptive typing label, §3.E
stretch (docs/drafts polish only). BETWEEN DAYS: re-read §1-2, pgrep,
kill soak before log-heavy QA.

## Day 2 start (2026-10-18 ~02:20) — own-avatar wobble: DEFERRED after 4 verified-broken attempts

The ±12px own-avatar wobble (battery P2): tried fixed slot + cap
(avatar ✓ but bubbles stretched to cap — render_segments'
horizontal_wrapped expands to available), fixed slot + r2l allocate
(bubbles anchored 121px PAST the pane — allocate_ui inside r2l
mis-anchors over-wide children), set_max_width(measured) (re-anchors
the region LEFT on this egui — avatars vanished), allocate_ui +
top_down(Max) (identical broken rects — the anchor is dominated by
something upstream not yet isolated). All 4 shot+rect-verified broken;
REVERTED to d807b4c (pre-attempt state, verified). The wobble stays
deferred — the egui layout primitive needed is a right-anchored
shrink-to-fit that this stack doesn't compose cleanly from
allocate_ui/with_layout/set_max_width. Next attempt should instrument
the SLOT allocation rect itself first (my probes always measured
downstream of it).

Soak #3: running since 01:33 (harvest after 03:33). Tree green at
d807b4c; 84/84; no day-2 commits yet.

## Day-2 close (2026-10-18 03:50) — GREEN at 1ef606d

Soak #3 PASS (2h flat). Typing fallback + activity @you committed.
Avatar wobble deferred (4 broken attempts documented above). Final
suite: 7 settlediffs IDENTICAL (one mid-window DIFFERS = real chat
activity, pixel-located + re-run clean). 84/84, 0 warnings. NEXT
STRETCH: avatar wobble with slot-rect instrumentation, header-wrap,
P3 chrome, user-gated items.

## Run 6 day 1 (2026-10-18) — GREEN at a9c59d2 (soak #4 running)

HEAD 1ef606d → a9c59d2: 11 commits, 84→86 tests, 0 warnings, core
7ceef15 untouched.
- OWN-ROW FIX (the run-5 deferred item): 397466e had left-anchored own
  messages since Oct 6 (avatar wobble + pending/confirmed jump). Fix =
  computed leading space (slot cancels) + content-aware slots + RECTOWN
  probe; pipe tables adaptive cap (+red-team: wide-glyph measure, 3%
  grace). All gates green (settlediff IDENTICAL d+l, vision 5/5+3/3).
- Battery round 6: typing P1 (fallback bug), scroll-gated Latest pill
  (at-bottom signal; State.offset trap; SCROLLBENCH WAS INERT — fixed),
  own-link AA, light chips edge (clip-rect ate the Inside stroke),
  activity mentions AA, Join parity (disabled buttons use inactive
  visuals wholesale — scoped), ring below chip, grouped gutter 43.
  HIDDEN: settings "phantom thumb" = half the General tab below an
  invisible fold (rail-derived modal height) — real height budget now.
- C: offline coverage NON-ISSUE (80/80 in DB; viewport artifact).
- False positive #13 (disabled modal_primary reads secondary).
- Soak #4 running since 11:52 (/tmp/soak7). POST-SOAK TODO: k8s card
  re-shot (grace 1.03), light battery spot, soak harvest + log grep,
  memory note, commit RUNNING-LOG.
- Wayland raise DEFERRED (own-session item; soak blocks the live run).

## Run 6 day 1 CLOSE (2026-10-18 13:58) — GREEN at grace-revert commit

- Soak #4 PASS: 117 samples / 1.9h, RSS last-hour band 40 KB (no leak);
  threads 60-62, fds 48-53; log = 2 known Xvfb quirks + 1 benign trouter
  max-age reconnect; 0 panics, 0 app warnings.
- Post-soak: k8s card re-shot caught a REAL regression from the 3% grace
  (cells truncated) → reverted to 10% (vision PASS full cells); settlediff
  IDENTICAL re-confirmed.
- Day totals: 13 commits (13ecaa0..HEAD), 84→86 tests, 0 warnings,
  teams-core untouched. Memory + report + AGENTS gotchas updated.
- NEXT RUN: Wayland raise (own session, design note current); possibly
  re-cite cull-margin perf with the FIXED bench; user-gated items
  unchanged (mid-scroll prepend manual pass, benchmarks, capture-gated
  shapes, light-canvas rebaseline).

## Run 6 day 2 close (2026-10-18 ~15:30) — GREEN

Day-2: +1 commit battery fixes (87 tests). Wayland raise deferred w/
upgraded design note (background_frame factoring = the prep step).
Everything green; soak #4 harvested PASS earlier. Run-6 totals: 15
commits (13ecaa0..HEAD), 84→87 tests, 0 warnings, core untouched.

## Run 7 day 1 (2026-10-18 evening→night) — GREEN at eaf30c2

User report list A–F ALL dispositioned; 10 commits (fe9a4ec..eaf30c2),
87→95 tests, teams-core PUSHED 0b17a5e+2772600 (marker wire; char-index
panic fix). settlediff IDENTICAL (± REACT). Soak #5 RUNNING (/tmp/soak8).
See SESSION-REPORT §Run 7 for the full ledger + evidence paths.

KEY TRAPS LEARNED TODAY (add to AGENTS next session):
- egui stick_to_bottom re-pins EVERY frame while scroll_stuck_to_end;
  only EXPLICIT scroll adjustments (scroll_to_rect, repeated) escape it;
  raw State writes oscillate forever (jump probe: offset alternating
  0/max per frame). The stuck flag is private.
- A pre-begin offset write of f32::MAX: begin does NOT clamp → round_ui
  overflows to -inf → all rows culled → blank frame → stick re-pin
  computes NaN → f32::max(NaN,0)=0 → view parked at TOP, stick lost.
- QA env hooks that arm at a wall-clock delay must pump their own
  repaints (event-driven loop produces no frames on a quiet chat).
- sqlcipher CLI prints "ok" BEFORE result rows — $( ) captures it into
  the id (the pins seed was "ok\n<id>" → 6 give-up steps).
- The debug-build first content frame can take ~3s under desktop load
  (user's Firefox at 200% CPU) — settlediff now samples at 8s/12s.
- str::find returns BYTE offsets — never feed into a char cursor
  (teams-core P1: `éé` in backticks killed the backend thread).
- vision agents can clobber the very files they inspect (two light-theme
  false alarms today were file-overwrites mid-inspection) — keep
  originals under names the agents aren't told about, or re-capture.

POST-CLOSE CORRECTION (same night): the "light chip missing" alarm was
the SCROLLUP hook frame-starving on quiet chats (no events → no frames
→ the 3s arm gate never evaluated → view stayed at bottom → chip
correctly hidden). Hook now pumps repaints until armed (ad78b22); chip
re-verified BOTH themes ("Mon 27 Jul" readable). Lesson added to
AGENTS: QA hooks arming at wall-clock delays must self-pump.

NEXT: harvest soak (≥2h); day-2 menu = the reaction-wire 404 mystery
(user tries one react in the app; web-client capture unblocks the
picker), composer P3 polish (multi-line B/I, partial-marker
double-wrap, pipes in <li>), straddle-frame polish if a safe mechanism
appears, user-gated: live formatted send.

## Run 8 day 1 (2026-10-18 night→19) — GREEN at 397cae3

HEAD a1942a9 → 397cae3: 4 commits, 95→104 tests, 0 warnings, core
PUSHED 276fa6d (Graph reactions). Soak #5 INVALID (externally killed at
24.5min, evidence clobbered by a 24s manual test.sh run; RSS fell and
sat flat — no leak signature). Soak #6 RUNNING (/tmp/soak9, setsid —
session teardown killed #5).

- BATTERY (13 dark + 8 light, 4 critics + pixel triage): run-7 surfaces
  all PASS. Two P2 claims disproven cheaply (QA seed mimicked data loss;
  1× icon miscounts — high-zoom crops settle both). Real fixes:
  per-run wrap (char-level breaking killed readability), inline-space
  glue ("itItem"), ChatsCached title heal (raw meeting id forever).
- REACTION WIRE UNBLOCKED: native /reactions 404s for everything;
  Graph setReaction/unsetReaction with UNICODE emoji payload works —
  live add/unset round-trip verified on an own message; arbitrary
  unicode accepted, stored as 1f389_partypopper keys (picker+parser =
  next-run unit). Default quick set realigned (🎉→😠).
- COMPOSER P3s: per-line multi-line wrap, inner unbold (close-first
  stack walk), pipe tables regroup across bullet sentinels.
- ADVERSARIAL REVIEW of the day's diff: fence-per-line P1 + 2 P2s +
  3 P3s fixed same-session (397cae3).

SOAK #6 PASS (2h2m): last-hour RSS band 0 KB flat at 169MB after the
login-burst decline (216→169MB); 0 panics/0 warnings; survived 12
concurrent QA launches beside it (setsid + FORCE guard + mid-run log
snapshot per the AGENTS pattern).

STRETCH (same night): unicode reactions COMPLETE (core 002db2e pushed —
1f389_* key parser + widened send gate; live probe parses 🎉 back); FULL
PICKER SHIPPED (0150bc0 — grid + favorites editing, vision-verified both
themes, click-tested); under-composer timestamp UNREPRODUCIBLE under
RECTLOG (closed pending recurrence).

NEXT: harvest soak #6 (≥2h); app-side react-click verification on a
live peer; user-gated: web-client captures, live formatted send.

## 2026-10-19 run 9 — user manual-pass items (929729c..ec32fad)
Five user reports, all dispositioned same-session, 116/116 green
throughout (teams-core at bc84fce, pushed):

- A. "Timestamp still appears in title when scrolling" — root cause was
  run-7's OWN sticky-day chip: the original report said "should be
  hidden under", run-7 shipped interpretation (b) (pin the day IN the
  header). Removed the whole chip machinery; separators scroll under
  and clip. Vision-verified header band clean; settlediff IDENTICAL.
- B. Picker favorites: defaults (classic six) are now PINNED favorites
  — custom stars add to them instead of replacing the quick set
  (favorite_state = defaults ∪ stored picks, deduped for legacy
  settings; display cap 8 never truncates a default; toggling a
  default is a no-op). Vision: five visible defaults gold-starred.
- C. Custom emoji + tabs: directory endpoint CAPTURE-GATED (Graph beta
  403 TeamworkCustomEmoji.Read; native guesses 404), BUT the custom
  REACTION wire is live-verified: Graph beta
  {"reactionType":"custom","displayName"} → 204 → emotions key
  "name;0-csea-…" → unset clears (v1.0 answers 204 and drops the
  fields — silent no-op trap). Shipped: inline CustomEmoji rendering
  in messages (20px, authenticated fetch), custom reaction chips
  (asset URL from objects host learned from message imgs), tabbed
  picker (Favorites/Smileys/Gestures/Objects/Custom — Custom from a
  seen-index, hidden while empty), ReactCustom action/command. Edit
  round-trip preserved (strict tag passthrough in build_message_html).
  teams-core: 2c4d7ce + bc84fce pushed.
- D. WYSIWYG composer: styled galley over the marker model (bold
  wght-700, synthetic italics, strikethrough, code chip; markers dimed
  to 45% — every char stays in the job for caret mapping). 7 tests;
  vision at 3x: bold 2x stroke weight, 13/13 italic stems lean, strike
  exact, code chip. (First shot was a STALE binary — cargo test does
  not rebuild the bin; noted in AGENTS.)
- E. Single-instance: already live since 9a34f63 (per-exe slot, "show"
  verb, QA bypass) — E2E re-verified (second launch exits 0 with the
  message). Parity fix: the show handler now sends Minimized(false)
  like the tray show. NOTE for the user: `cargo run` beside the
  installed copy coexists BY DESIGN (per-exe scoping).

## 2026-10-19 run-9 follow-up — item 1 REAL root cause (9a9267d)

User: "1. is not fixed yet" (third report of timestamps appearing in
the title while scrolling). The user's attached screenshots (found in
the session image cache, 236x166 thumbnails) showed the exact
phenomenon: a group-meta timestamp ("17:05, 15 Oct") painted INSIDE the
chat-title band, baseline on the header rule, plus an avatar-only row.

Root cause: `reaction_strip` did `strip.set_clip_rect(anchor)` — SET
REPLACES the ScrollArea viewport bound. Rows above the viewport still
render inside the 600px cull margin; their strips painted at true
positions up to ~600px above the rule, bounded only by the central
panel's clip (= into the chat header AND up against the app top bar —
matching the original run-7 wording "chat header + app top bar").
Run-7's "zero paint-over" forensics and my first static/sweep pass both
missed it because the artifact is a per-frame transient only visible
while a strip crosses/sits above the viewport top: hashing the header
band of EVERY frame of a 60fps SCROLLBENCH sweep on the tall Jira chat
(23,970px content, ~60 strips) found two one-frame bands — "12:06,
7 Oct" at y=53 and "16:00, 1 Oct" at y=27 (grazing the top bar).
The chip added in run-7 and removed in run-9 was a red herring both
times.

Fix: `shrink_clip_rect(anchor)` (anchor ∩ viewport) — keeps the
red-team chip width-cap and restores the viewport bound. Regression
`reaction_strip_never_paints_above_viewport`: strip fully above the
viewport top must tessellate zero vertices above the clip (38 pre-fix).
Live post-fix: same sweep → 0 transient bands. 117/117. AGENTS part-5
gotcha added (never SET a clip to a content rect inside a scroll).

## 2026-10-19 E2E call rig (7418f43 + harness) — selftest PASS

User asked how to enable autonomous E2E call testing with two accounts.
Account plan first chosen: M365 Developer sandbox — the user's
application was then REJECTED by MS ("you don't currently qualify";
the dev program has tightened since 2024). Revised options presented
(Business trial tenant / federated second work account / dev-program
retry). The RIG itself is account-independent and is now live:
- TEAMSFAST_PROFILE → teams-core per-profile keyring slots (the layer
  existed unused), + suffix on archive key + instance slot; files via
  launcher XDG redirects. Verified: qa1 boots signed-out with isolated
  archive/log/settings; profiled slot answers --ctl, default slot
  finds nobody.
- --ctl control verbs through the single-instance claim handler
  (queue → App::ui drain → same Actions as the buttons). CALLQA
  transition logs. calluser <upn> = cold-start (CreateOneToOne →
  call on CreateChatOk).
- qa_call_e2e.sh: selftest PASS live (ping→echo→connected→mute→
  hangup→ended, "1470 packets sent / 769 received" on the 45s hold).
  pair mode awaits two signed-in profiles.
Next: the user picks the second-account path; then `login` mode
provisions each profile (device code; browser-agent can drive
devicelogin if handed test credentials), then `pair` runs the real
A→B flow and captures the gated payload shapes (caller-cancel,
remote hang-up, video invitations).
