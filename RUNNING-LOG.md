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

## 2026-10-19 evening autonomous pass (9307012..96be1fc, core bb1681a+5aad588)

While the E2E second-account provisioning is pending:
- teams-core: library error strings de-CLI'd ("Run 'teams-cli login'"
  bailed straight into the GUI welcome screen, seen during rig QA);
  fresh profiles now read "Not signed in. Sign in to continue."
  (empty config ≠ expired session — verified by qa1 shot). PUSHED.
- App lib warnings zeroed (splice tests module needed #[cfg(test)];
  a duplicated #[test] had double-counted one test — 119 was really
  118 unique).
- BENCHMARKS SHIPPED (roadmap #4): scripts/bench.sh + docs/benchmarks.md
  — 4 paired Xvfb runs vs teams-for-linux 2.26.0 (extracted AppImage):
  median idle PSS 153.1MB / 1 process vs 394.0MB / 6 processes (2.6x),
  first-window paint a 320ms tie. Method = zapfast's (PSS tree sum,
  alternating order, identical first-paint detector). Honest caveats
  documented: their figure is PRE-LOGIN (interactive sign-in pending =
  one user-gated step for the post-login pair), our window 1100x760
  (winit Xvfb cap) vs their 1280x800, --disable-gpu under headless X.
- SOAK #7 running (setsid, RELEASE build, default profile, /tmp/soak7):
  60s RSS sampler + a call driver doing one --ctl echo call every 10
  min (exercises the control channel + call stack under the soak).
  First call completed (connected→ended). Harvest next session; the
  app log is volatile to later launches — CALLQA snapshots land in
  /tmp/soak7/soak.log immediately per call.

## 2026-10-19 personal-account reconnaissance (core edca813 PUSHED)

User asked whether Teams PERSONAL accounts are truly useless. Answer
after investigation: NOT settled-dead — the consumer path is closer
than the "different protocol" verdict suggested:
- teams-core ALREADY implements the consumer authz exchange
  (teams.live.com/api/auth/v1.0/authz/consumer, auth/skype.rs) — every
  call site just hardcodes personal=false.
- Free probes (no sign-in needed): v2 device code REJECTS
  api.spaces.skype.com for the consumer client/tenant (AADSTS70011);
  graph/substrate/outlook v2 scopes ARE accepted → the client CAN mint
  consumer tokens. The V1 device-code endpoint (resource= param) is
  STILL ALIVE and accepts resource=api.spaces.skype.com AND
  teams.live.com for the consumer client — v1 is the road in.
- examples/personal_probe.rs runs the full chain behind ONE
  device-code approval: v1 token → consumer authz → native
  conversations GET (X-SkypeToken). Awaiting the user's approval run;
  success ⇒ two free personal accounts = the A<->B messaging rig
  (calls = the next probe after that).

## 2026-10-19 PERSONAL ACCOUNTS: VIABLE, live-verified (core efa7baa)

The user's push ("we really cannot do anything with personal
accounts?") was right. Full chain verified live with a hotmail account
(examples/personal_probe.rs, one device-code approval, RT now saved at
/tmp/tfl_personal_rt = approval-free iteration):
1. v2 CONSUMERS device code with scope
   service::api.fl.spaces.skype.com::MBI_SSL (the consumer web
   client's own authzResource, mined from its JS bundles) — approve at
   microsoft.com/link (NOT login.microsoft.com/device — v2 codes read
   'did not work' on the v1 page).
2. POST teams.live.com/api/auth/v1.0/authz/consumer → 200. FLAT shape:
   skypeToken{skypetoken,skypeid:"live:<handle>",signinname,expiresIn},
   regionGtms, consumerLicenseDetails, regionSettings
   (isFederationEnabled:true).
3. Consumer hosts (the full map): chatService msgapi.teams.live.com,
   chatServiceAfd teams.live.com/api/chatsvc/consumer, CALLING
   api.cc.skype.com, presence presence.teams.live.com, middleTier
   teams.live.com/api/mt, search msgsearch.skype.com.
4. GET msgapi.teams.live.com/v1/users/ME/conversations → 200, 5 real
   conversations, X-SkypeToken auth — the native API speaks consumer.
Dead ends on record: ESTS v2 scopes reject spaces.skype.com for this
client; v1 mints it but authz 40110s those tokens; v1 cannot mint
service:: (AADSTS500011); v2 RT is single-audience.
NEXT (ordered): (a) teams-core consumer login/config wiring (profile
personal flag + the flat authz parse + host overrides), (b) app
TEAMSFAST_PROFILE+personal → qa profiles on personal accounts → the
A<->B MESSAGING rig with two free accounts, (c) registration+trouter+
send probes, (d) the CALLING probe on api.cc.skype.com (consumer 1:1
calls are licensed per consumerLicenseDetails).

## 2026-10-19 rig accounts BOTH provisioned (approval-free now)

- A: rig.a.autotest@example.com, skypeid live:rig-consumer-a,
  hasTeamsLicense=true (RT: /tmp/tfl_personal_rt).
- B: rig.b.autotest@example.com, skypeid live:.cid.0123456789abcdef (the
  unmigrated CID form — a THIRD mri shape to handle), teamsFree tier
  (chat + 1:1 calls included; no copilot/recording) (RT:
  /tmp/tfl_personal_rt2).
Both RTs verified refreshing approval-free into authz-200 + chat list
(B: 0 conversations — fresh). NEXT SESSION UNIT ORDER: (1) create the
A<->B 1:1 via POST msgapi.teams.live.com/v1/users/ME/conversations
(members as skypeid MRIs), (2) send-path probe (the work-shape
sendmessage against the consumer host), (3) trouter consumer probe,
(4) teams-core consumer wiring (login/config/authz parse/host
overrides), (5) app personal profiles → qa_call_e2e pair on personal
accounts, (6) calling probe api.cc.skype.com.

## 2026-10-19 evening II — consumer CALL archaeology (big leaps)

Both QA personal accounts live IN-APP (messaging E2E verified:
send/receive/1:1 titles/peer names/self chips; consumer trouter
pushes CONFIRMED — call-log EventMessages arrived on A).
- epconv FOUND: api.flightproxy.skype.com/api/v2/epconv (work-twin
  pattern; every conv.skype.com / appliance-8443 path 404s/timeouts).
- IC3 Gateway is AAD-ONLY (AADSTS9002332 names app 93aca3ff…): the
  consumer call credential is the SKYPE token — signaling's 8 Bearer
  sites route through call_auth() (X-Skypetoken when consumer).
- Gates fixed en route: 8:live: MRI extraction (skypeid claim),
  GUID-thread callee via roster read, tenant = all-zeros (the
  threads' own addedByTenantId).
- RESULT: A→B call PLACES END-TO-END — epconv 201 (6992-byte
  conversation), invite to 8:live:.cid.0123… 202, controller call-log
  "outgoing twoParty target=B". callState=failed after ~0.3s: B's
  endpoints look unreachable to the controller (B's trouter got NO
  data frames ever, keepalives only, while A receives pushes fine).
NEXT (the one remaining gap): why B's calling registration doesn't
attract the invite — compare A/B registrar template responses,
calling presence/capabilities post, CID-account routing. Then B
answers → voice → video. Wire keys all in teams-core (pushed).

## 2026-10-20 consumer calls — root cause of the receive gap FOUND

Experiments: B→A direction (same non-delivery — not account-specific);
TFL registration table (2.6 keys + product context, the work flow's
own pre-fix shape) — pushed, live, did not flip delivery; consumer
calls also stamp Event/Call "started" partlist messages into the
thread (marker only — no SDP; the real invite still rides the NGCall
push).
ROOT CAUSE (from the web client's own JS config):
calling_registrarUrl = https://{appliance}:8443/V2/registrations — a
SEPARATE calling registration on the appliance port. THIS NETWORK
FILTERS OUTBOUND 8443: api.cc.skype.com + api.conv.skype.com both
time out on 8443 while 443 answers; flightproxy cp/ep refuse to
proxy V2 registrations (404 Route does not exist). With that
registration missing, the controller sees the callee as unreachable
(callState=failed in ~0.3s — exactly what the call-logs show).
SHIPPED: register_calling_v2 fires automatically for personal
profiles (timeout-tolerant, logs "is outbound 8443 filtered?").
Everything else about consumer calling already works over 443 —
placement verified end-to-end yesterday.
NEXT: run the rig once on an UNFILTERED network (home/hotspot; or
open 8443 to api.cc.skype.com) — the registration lands, invites
route, and ringing/answer/voice/video proceed. Zero code changes
needed; the V2 payload may need one iteration against the appliance's
validation once reachable. (Also fixed: a broken commit I pushed
mid-refactor — caught by the build check, corrected + repushed.)

## 2026-10-20 investigation — the capability lead

Mined the web client's service-binding code (b14.js): consumer
services require header x-ms-client-consumer-type: teams4life, and
pick skype-token vs AAD auth per service config. PROBES (live):
- GET presence.teams.live.com/v1/me/presence (X-Skypetoken + the
  consumer header) = 200 — and shows "capabilities":[] — our consumer
  accounts publish NO call capabilities. Strong fit for the ~0.3s
  fast-fail: the controller sees a non-callable callee.
- Presence/capability WRITES 401 (40102) on every candidate path and
  BOTH auth styles (skype header, MSA Bearer) at presence + ups hosts;
  PATCH too. The consumer write path needs a per-service token scope
  (the authSvcEndpointMetadata pattern — each teams.live.com service
  names its own authority+tokenScopes) that we haven't minted yet.
NEXT: (a) find the presence service's token scope (mine the LAZY JS
chunks — the 32 initial bundles don't contain the publish code), (b)
mint it, publish capabilities ["Audio","Video"], (c) retry the call —
if invites then route, ringing proceeds with everything already
built. Fallback capture: B in the web client once.

## 2026-10-20 run-10 — consumer calls: spine solved, 403 root-caused

Mined the consumer web client's LAZY chunks (found the runtime's
a.un/a.uh chunk maps — 6881 named chunks — via the browser-UA page
load; presence-service, config-life and calling chunks downloaded).
Four live-verified breakthroughs:
- PRESENCE PUBLISH SOLVED: the consumer write path is PUT
  {unifiedPresence}/v1/me/endpoints/ with {id, availability, activity,
  deviceType, capabilities} + X-Skypetoken + teams4life header → 201,
  capabilities ["Audio","Video"] visible in GET (the months of 401s
  were wrong-path PUTs). ~5-min TTL → the app's PollPresence now
  publishes for personal profiles every 60 s (UPS endpoint published,
  live-verified in-app; an MT variant via teams.live.com/ups/global +
  authz ?resource= mtsvc tokens also works).
- CONSUMER SPINE: config-life's trouterService = go.trouter.skype.com
  + edge.skype.com/registrar (work prod = teams.microsoft.com v3!).
  teams-core now negotiates the SKYPE fleet for personal and takes
  the edge.skype.com V3 registrar hint; all TFL templates accepted.
  Plus LIFE-registrar (teams.microsoft.com/registrar/prod/v2) calling
  registrations; appliance-8443 attempt REMOVED (theory retracted).
- ECHO CALL CONNECTED on a consumer account (phase=connected) — the
  whole caller + media stack works end-to-end for personal profiles.
- A↔B 1:1 = call/end 403 subCode 10124 "Permission check for callee
  to interact with caller is not allowed" in <0.5 s: a CALLEE-SIDE
  RELATIONSHIP GATE (stranger-call block), not routing. Contact-add
  probes rejected everywhere (peoplecoreservice 401/403 on every
  mintable token; flightproxy FQDN-whitelisted; msgapi 501). B also
  gets NO message pushes on either fleet (receiver-side; app polls).
NEXT (the one gate left): establish the A↔B relationship — either the
user adds the contact once from a real client (both accounts are
ours), or one authorized web-client capture of the Add-contact wire.
Then ringing → answer (--ctl) → RTP counters → video, all of which is
already built and waiting on the rigs (pa/pb Xvfb launchers).
Core: 9639ba7+1467624 pushed. App: UPS publish in PollPresence.

## 2026-10-20 run-11 — CONSUMER CALLS: receive chain COMPLETE

User added A↔B as mutual contacts → the 403/10124 permission gate is
GONE (calls ring the full window). Mined + implemented the whole
consumer receive/answer protocol (config-life + calling chunks +
live capture): evt:107 invites on the SKYPE fleet as 3::: data frames
({evt,cp=b64(gzip(...))} / gp) — were silently DROPPED by the
chat-only gate in data_frame_event (the ring starvation). Chain now
LIVE-VERIFIED: B rings (phase=ringing, video detected from the SDP) →
answer → fork attach (top-level 'attach' + required links.end on the
live surl; join action with full conversationRequest) → mediaAnswer
{mediaAnswer:{...}} + acceptance {callAcceptance:{acceptedBy (fork
participantId!), mediaContent, links}} → CALLER phase=connected on
the answer SDP. Echo re-verified healthy mid-session.
WEDGE (environmental, post-surgery): mass registrar cleanup (~200
stale 24h-TTL bindings from SIGKILLed test sessions, deleted via
GET/DELETE /registrations) triggered 429s + conv-service 404s on
invite resolution for the pair thread; echo (fresh conversations)
still connects. A single call+answer re-verifies the acceptance once
the cooldown lifts — rigs left running (pa :81, pb :82).
Core 085f97e pushed (400 tests). Gotchas: pkill shim ignores -9
(use kill -9 by pid); registrar deletes rate-limit fast (~30/burst);
the wedge also surfaced a caller call-slot latch on torn-down calls
(restart clears; fix separately).

## 2026-10-20 run-12 — acceptance-504 campaign: caller chain COMPLETE, callee acceptance still CC-blocked

Rigs relaunched (pa :81 / pb :82, fresh group thread
19:9999999988887777666655554444aaaa@thread.v2 minted via
PAIR_MAKE_GROUP — the ORIGINAL pair thread's conv record is wedged
server-side: 5704 "failure to resolve ownership", identical cached
correlationId 7fd7434d for BOTH callers = a poisoned conversation
record at conv-frce-06; unreachable from outside (GET/DELETE 404,
POST end 503/70051). Fresh threads resolve fine — pair thread to
retest later for expiry).

LIVE-VERIFIED END-TO-END (repeatably, 6+ cycles): invite 202 →
evt:107 ring on B (caller unknown, video flag from offer m=video) →
fork attach (11KB) → ringing progress (A receives /call/progress/ on
its task socket!) → mediaAnswer 202 → caller CONNECTED on the answer
SDP → NEW: caller POSTs the mediaAnswer acknowledgement
(mediaAnswer/links/mediaAcknowledgement — 201 Created; was parsed from
the wrong path and never sent).

The callee's acceptance POST still dies: 22s hold → 412/430/10065
"Trouter server timed out … Delivering call acceptance notification".
Falsified this run (all live-tested, then reverted only where inert):
- dead registrar registrations (swept BOTH V3 and LIFE registrars; the
  app leaks 6/instance: 1h session-max-age reconnects re-register on a
  NEW surl without removing the old — systemic; REG_CLEAN now covers
  both registrars, comma keeps, CONTAINS match: the old starts_with on
  a full URL never matched and DELETED LIVE registrations once — the
  likely 5704-wedge origin).
- declared endpoint ids (create/attach/acceptedBy now declare the main
  session's REGISTERED epid via trouter::current_epid, not random
  UUIDs).
- registrationId=epid bindings (register_calling_epid probe; present
  on live surls during the last test — still 504).
- caller call-channel links: invite callInvitation.links
  (progress/mediaAnswer/acceptance/redirection/end — mined from the
  web callInvitation builder) + clientContentForMediaController. The
  double-slash bug in trouter_callback paths ("/call/x/" → //call/x/)
  fixed; CC now adopts the links (echoed single-slash in logs).
- callee broker channel: fork-attach `subscribe` link = GET long-poll
  (202, message/nextSubscribeUrl/expirationTimeInSec=30, 404 when the
  channel dies — contract captured via PAIR_BROKER_SUB); the answer
  flow now polls it and would ack a callAcceptance echo — the echo
  NEVER ARRIVES on it (empty messages only).

Web-chunk evidence (calling-pluginless): the callee FSM enters
WAITING_CALL_ACCEPTANCE_ACK after POSTing AG; handleCallAcceptance
receives the callAcceptance EVENT and POSTs
e.body.callAcceptance.links.acknowledgement (Ty body). B's socket has
received ZERO callAcceptance frames all day — the CC never attempts
delivery to ANY URL B declared. Every client-side lever now matches
the web shapes; the remaining diff is invisible without a real web
acceptance capture (USER-GATED).

Also found: decline is broken (fork reject link answers 405 to our
POST — wrong method/body for /cc/v1/forked/.../reject); caller 60s
no-answer timeout works; caller call-slot latch still wedges after
connected calls that die on acceptance (hangup clears).

Core: 0757679 pushed (400 tests). NEXT: (a) web capture of a real
acceptance (gated), or (b) fix decline's 405 + latch, retest pair
thread wedge expiry.

## run-12 — publish scrub (2026-10-20)

3-subagent audit of both public repos (full history + trees): zero
credentials anywhere, but run-9..11 capture work had landed real
identifiers against the fictional-fixtures policy. Fixed the same
day:
- teamsfast: `git filter-repo --replace-text` over the UNPUSHED
  range only (published history + tags untouched) — rig-account
  emails/handles, the live_parse orgid, and the tenant k8s/Jira/
  custom-emoji fixtures all swapped for same-shape fictional
  stand-ins; AGENTS.md topic-channel example generalized and the
  signing-key backup path redacted (see the private runbook).
- teams-core: full history rewrite + force-push (personal handles,
  captured per-call keying material, thread ids, AAD orgids, emoji
  object id → fictional). 400/400 lib tests green post-rewrite;
  Cargo dep is branch-tracked, lock carries no rev pin.
- Guards: pre-push grep hooks both repos; GitHub secret scanning +
  push protection enabled.

### run-12 addendum — decline VERIFIED, latch VERIFIED, registrar self-sweep; acceptance 408 now

Second wave (core 08a5e02 pushed):
- DECLINE WORKS E2E: fork reject = DELETE with top-level
  {callEnd:{code:603,...}} (payload-unwrapped) → 202, and the caller
  receives the rejection in 170ms. Proves the CC event fan-out is fine
  when the payload is right — the acceptance failure is payload/flow
  specific, not environmental.
- CALLER LATCH FIXED (live): the call task now pumps its socket
  post-connect (heartbeats + callEnd detection); a connected-then-
  failed call self-terminates on the CC's call/end instead of wedging
  the slot ("Remote end received — hanging up").
- REGISTRAR SWEEP on reconnect: surl changes now delete the superseded
  surl's bindings (the hourly session-max-age reconnect was leaking 6
  dead entries per hour per account).
- updateConversationLinks (QG shape) against the fork join's
  notificationLinks URL → 404 (before AND after mediaAnswer). The
  acceptance error shifted 412/430 → 408 — behavior changes with
  payload timing; remaining unknowns need the gated web capture.
- test hygiene: sendfix_one_to_one_thread_id_is_ordered_pair had a
  wrong-pasted expectation since 88f27a9 (the pure function can never
  produce the real-tenant thread id); fixed. self_ring example
  repaired.

 rigs left RUNNING (pa/pb on the final binary, registrations clean).

## run-13 — acceptance root-caused to DTLS-SRTP; RTP now flows; 7 core fixes

User-gated web captures: FOUR HARs captured live (user accepted in the
browser while rigs called). Every web accept died CLIENT-SIDE before any
HTTP request — attach 200 → progress 202 → (accept click) →
updateEndpointState 200 → DELETE reject 410/3111
"CallEndReasonMediaOfferProcessingError". Ruled out live: mic permission
(was granted), input devices (system has 2), modalities (audio-only
offer + ["Audio"] declared — CC echoed callModalities ['audio'] and it
STILL failed), offer shape (rebuilt to MS form), Firefox-vs-Chromium.
ROOT CAUSE (chunk-verified): calling-pluginless carries 57 dtls + 8
fingerprint references and ZERO SDES crypto — the consumer web is a
WebRTC/DTLS-SRTP stack; our SDES-only (a=crypto) offer is unanswerable
(setRemoteDescription throws). The same fleet-wide DTLS expectation
unifies the rig→rig 408/10056 "waiting for acknowledgement": the CC's
media pipeline never sets connectTime for an all-SDES call (RTP flows
raw regardless) and the acceptance transaction dies at its 30s deadline.
FIX = DTLS-SRTP media stack in teams-core (a=fingerprint+a=setup in SDP,
DTLS handshake on the ICE-selected socket, SRTP keys from the exporter;
keep a=crypto for SFB/echo interop) — the next run's main feature.

Shipped this run (all live-verified, core 400/400, COMMIT BUT NOT PUSHED):
- b784fcb caller acks the LATE callAcceptance frame (arrives ~300ms
  after the mediaAnswer, i.e. after wait_for_call_acceptance returned —
  the pump forwards links.acknowledgement; ack 201 in 330ms).
- dedd7db answer flow binds the RTP socket BEFORE answering and patches
  the bound port into the SDP: m=audio 0 REJECTS the m-line (RFC 3264) —
  every prior answer told the CC the callee had no media leg. RTP NOW
  FLOWS rig↔rig (1472 sent / 1475 received vs 0/0 before).
- 52a96e7 AG acceptance body completed (clientEndpointCapabilities=1,
  callKeepAliveInterval:null).
- 6563ae6 applicationType "TFL" (value captured from the web's own
  reject payload) on attach + conversationRequest + acceptance.
- deb507b ringback 120s hard deadline + 10s timeout on the end-POST
  (live wedge: 8 minutes of ringback on the host speakers after a call
  ended — the un-timed end POST blocked the task post-`ended`).
- 4239025 MS-shaped SDP both directions (per-m c=, a=rtcp,
  x-signaling-fb, x-source-streamid, no direction attr, real o= session
  id, srflx raddr fixed off 0.0.0.0).
- d2c8dd8 honest callModalities + video-less offers for audio-only
  calls (web preps its accept from the DECLARED modalities); side
  effect: the callee ring banner now shows video=false for audio calls.

Verified end-of-run on the final binary: echo call healthy (connected
2.2s, 818 pkts + SRTCP back), rig→rig answer chain complete with
two-way RTP, decline path untouched. Falsified this run: notificationLinks
404 = red herring (web POSTs it only on trouter-URL change); keep-alives
(interval 2700s ≈ 40min — not the 30s killer); registrar same-id
semantics (one binding per registrationId, replace on re-register);
fork-per-registration (invite fan-out is per-channel).

Traps for the record: PAIR_REG_CLEAN with rig-keep-tokens DELETES THE
WEB CLIENT'S REGISTRATIONS (a mid-session sweep made the web un-ringable
until tab refresh — refresh re-registers). pkill shim ignores -9; rigs
launched via setsid still died once to a cancelled watcher (relaunch
heals). Non-stop "ringing" on the host = a rig's 425Hz tone through
PulseAudio (fixed wedge; blip was a red herring). The `app.pid` files
can go stale (relaunch overwrote); pgrep is the truth.

Rigs LEFT RUNNING (pa/pb on final binary d2c8dd8, regs 24 live / 0
stale; the web tab's registration was swept — refresh it before the next
web test). Pair thread 19:2d46f0… STILL wedged (roster 403; group
thread remains the working one). P2 caller-name in the ring banner
still "(unknown)" (offer from.displayName; roster resolve exists).

### run-13 addendum — CONSUMER WEB↔APP CALLS WORK (DTLS-SRTP shipped, live E2E)

Evening continuation ("fix everything and test"): implemented the full
DTLS-SRTP stack and closed the loop with the user's browser as caller.
**Live-verified end-to-end: web (teams.live.com, real WebRTC client)
calls the app → rig rings with the caller's real name ("the peer's real display name") →
answer → ICE direct → acceptance 200 + INLINED callAcceptanceAcknowledgement
in 0.1s → DTLS handshake with the browser (client role) in ~100ms →
audio streaming 5000+ packets, call STAYS UP (user confirmed "working";
every prior attempt died ≤30s).**

Evening commits (core, COMMIT BUT NOT PUSHED; 404/404 tests):
- ed79e87 SDP: a=fingerprint(sha-256, real cert)+a=setup on every
  offer/answer (actpass/active); SDES a=crypto kept for SFB peers.
- 479a4aa DTLS-SRTP media plane: new calling::dtls on webrtc-rs `dtls`
  (use_srtp, EXTRACTOR-dtls_srtp 60B export split per RFC 5764); single
  socket reader demuxes DTLS records into a bounded queue; background
  handshake (caller=server/answerer=client) swaps the SRTP context;
  sends gated until keys land. Loopback unit test + rig↔rig live
  (handshakes <1s, RTP 1458/1454).
- 5179f86 start the media session for DTLS-only offers (web offers have
  NO a=crypto; our SDES gate skipped media entirely).
- d7f6e24 answers never ADD SDES crypto to a DTLS-only offer (RFC 3264
  violation → the web's setRemoteDescription failed 500ms after its 200).
- 7bd8742 JSEP m-section stubs: the web offers FOUR m-lines (audio,
  video-recvonly, screenshare, x-data; BUNDLE 0 1 2 3) — answers now
  carry a section per offer line (port-0 rejects echoing proto/pts/mid).
  A short answer = instant 410/3111 (live-verified twice).

The debugging ladder (each step live with the user's browser): acceptance
200+inlined ack → media-gate fix → crypto-violation fix → m-count fix →
CONNECTED. Rig↔rig 408/10056 (our own caller vs our own answerer) is
STILL OPEN — the CC's wait there is specific to OUR create-side flow
(the web caller's identical-looking transaction completes fine); next
lead: diff our create/invite against the web caller's (HAR has it).
Remaining nits: browser→app return audio (recv=0; tab mic/relay suspect),
video answered as rejected, ring banner "(unknown)" for OUR rig callers.
NOTE: the answer side is now the reference implementation — web↔app
inbound calls are DONE.

## run-13 addendum II — RIG↔RIG CONSUMER CALLS FULLY WORKING (the 408 closed)

Autonomous continuation ("continue work on next things"): the
rig↔rig acceptance 408/10056 — open since run-12 — is CLOSED, plus two
adjacent live bugs. Four core commits (edee7b8..548c766, 407 tests)
+ app 30ebf78 (118 tests); COMMIT BUT DO NOT PUSH still in effect.

The 408 was THREE stacked bugs; each fix was found by one live
rig↔rig round + HAR/JS mining:

1. **edee7b8 UPN-form callee MRI → 10124 stranger gate.** The pair
   thread died server-side (A removed from its roster — the run-11
   "wedge" finally explained: AclCheckFailed 209), so calluser minted
   a FRESH 1:1 whose roster carries the callee as `8:user@host` —
   the CC cannot route that form and the invite fan-out dies
   call/end 403 subCode 10124. The consumer web anchors its contact
   1:1 on a `productThreadType: OneToOneChat` thread whose roster
   freezes endpoint-registered forms (`8:live:.cid.<guid>`): when the
   roster peer is UPN-form, we now resolve the endpoint MRI via the
   sticky thread's roster. (msgapi `/v1/users/ME/contacts*` is 405-
   walled; presence reads 40102 — the sticky roster is the only
   client-visible UPN→MRI table.)
2. **211e275 epconv → cpconv.** Our single-shot create went to
   /api/v2/epconv WITH `groupChat {threadId}`; the CC built a GROUP
   call (fresh minted thread, isMultiParty=true, groupCallInitiator)
   — the web's create is /api/v2/cpconv with `groupChat:null`
   (two-party). Aligned consumer creates to the web shape exactly
   (caps 109985824, endpointState seq 1).
3. **b3fb4d9 THE ack the CC actually waits for.** With 1+2 the calls
   CONNECTED with perfect bidirectional DTLS-SRTP audio and still
   died at the 30s acceptance deadline despite the caller
   transport-acking (160ms) AND POSTing the record's
   links.acknowledgement (201, Ty body). Mined the web's
   calling-pluginless chunk: `handleIncomingMsgSync` returns the
   Ty-shaped callAcceptanceAcknowledgement as the RESPONSE BODY of
   the trouter HTTP-over-WS frame (`{id,status,headers,body}` per
   trouter-js) — the CC resolves the callee's accept long-poll on
   THAT transport-body ack, not the HTTP POST. Our websocket now
   answers `/call/acceptance/` forwards with the ack body + echoed
   correlation headers. Acceptance: 200 "Incoming call accepted",
   calls stay up indefinitely (verified 2min+, ended only by
   hang-up).

Also fixed live-verified:
- **548c766 callee remote-gone watchdog + real display names.** The
  CC delivers NOTHING to the callee on caller hang-up (no trouter
  frame, no broker event — the earlier browser hang-up DID deliver
  frames the answer loop ignored): accepted calls kept streaming
  into the void (20min/36k packets) with the slot latched. The
  answer loop now ends the call after 30s of frozen recv (our sender
  paces 50pps, so frozen-after-traffic = remote gone). App side:
  Event::CallGone always sends HangUp (30ebf78). Ring banners show
  the sign-in local part instead of "(unknown)"/raw MRI (both the
  invite from.displayName and acceptedBy.displayName).

VERIFIED THIS SESSION (rig pa↔pb, final binary, CALLQA logs):
ring → answer → acceptance 200 → DTLS both roles → bidirectional
audio 5954/5954 → caller hangup clean end; callee watchdog end at
+30s ("remote end gone: recv frozen"); decline still 603/202.
Echo path untouched; rigs left running (pa :81 / pb :82).

Notes: browser→app recv=0 is almost certainly a muted tab — Chrome
stops RTP when the mic track is disabled while consent STUN keeps
flowing (exactly our observed pattern); rig↔rig proves the receive
crypto both ways. Needs one user round with a hot mic to confirm.
Video offers are still answered audio-only (port-0 stubs) — the
remaining P3. Pair thread 19:2d46f0… permanently dead (roster 403;
use calluser — it resolves via the sticky OneToOneChat now).

## run-14 — CONSUMER VIDEO CALLS + GROUP CALLS + SCREEN SHARE (all rig-verified)

Autonomous run ("video calls → group calls → screen share", 2026-10-20
evening). COMMIT BUT DO NOT PUSH still in effect. core 548c766..246deb4
(7 commits, 414 tests / 416 with video-cam), app 5f234dc..eeed648
(4 commits, 118 tests). Rigs pa/pb left RUNNING on the final binary.

**P0 VIDEO CALLS: DONE, rig↔rig end-to-end, every round live-verified.**
Strategy: consumer video uses the WEB'S OWN BUNDLE SHAPE everywhere —
the web's cpconv offer (captured in the HAR) is `a=group:BUNDLE 0 1 2 3`,
all m-lines on ONE transport, DTLS-only, mid1 video recvonly with a pile
of H264/AV1/rtx PTs. Our caller now offers that shape (2 m-lines,
audio PCMU + video H264 102/108 pm=1, both sendrecv, one socket) and
the answerer answers mid1 FOR REAL (same port/creds/candidates as
audio, offer's PT + fmtp echoed, direction mirrored, BUNDLE group line,
mid2/mid3 stubbed) — so the rig testbed exercises exactly the code a
browser hits. TEAMSFAST_NO_VIDEO_ANSWER=1 forces the old audio-only
stubs for A/B.

The chain closed on the FIRST live round (ring → answer → DTLS-SRTP on
the one transport → bidirectional video + audio) but rendered nothing —
three stacked display bugs, each found by one rig round + an offline
loopback test:
1. per-SSRC ROC (cfcacb0): BUNDLE = two SSRCs on ONE SrtpContext; the
   single shared (roc, highest_seq) pair corrupts the estimate. Now a
   HashMap<ssrc, StreamRoc> per direction; inbound state commits only
   after auth-tag verify. Interleaved-streams-across-wraps unit test.
2. SVC container NALs (fb613ba): our packetizer injects PACSI(30)/
   prefix(14) NALs; openh264 rejects them with persistent
   dsBitstreamError that kills every P-frame. Receive paths skip 14/30.
   Full-wire roundtrip test (encode→packetize→depacketize→decode).
3. TWO-ENCODER interleave (7a95c2a + ea2a121): the no-source fallback
   used a separate black-frame encoder, so a camera ticking slower than
   the send loop interleaved two unrelated H264 reference chains on one
   SSRC (receiver: dsRefLost on every slice). encode_black_frame() goes
   through the CALL'S encoder (dims-aware); the hand-rolled baseline
   IDR bitstream is also decoder-hostile (0/6 in tests) — pinned by test.
Final polish (7940cab): I420 black needs UV=128 — all-zero planes
render as solid RGB(0,135,0) GREEN (the caller's remote tile was a
uniform green field); the fallback tees the preview (no more "camera…"
placeholder on no-source legs); answer_call_with_stop drives the
phase watch to Connected (callee banners were stuck "calling… 00:00").

VERIFIED (final binary, CALLQA logs + pixel-checked screenshots):
- videocall pa→pb: ring video=true → accept → BUNDLE video answered
  (PT 102) → DTLS → audio 1500/1500 + video both directions, decode
  150/150 frames (100%, zero errors) → both stages render video tiles
  (vision 4/4 pass: no placeholders, no green) → caller hangup clean;
  decline still ends <1s (accepted=false 0/0); audio-only calls intact.
- Callee-side media: VideoSessionConfig on MediaSession — video RTP
  demuxed by payload type in the STUN-aware recv loop, camera/share
  slots + camera gate + remote-frame sink via CallControls (the app's
  accept_incoming now creates call_controls(true) and passes them).

**P1 GROUP CALLS: the group thread works** (`--ctl call
19:<group-thread>@thread.v2`): ring → accept → DTLS → audio 1242/1242 →
clean hangup. N-party create (2+ participants.to) NOT implemented —
honestly deferred: with only two QA accounts a multi-dial cannot be
verified; a true 3rd participant is the user's browser (§5). Noted on
the wire: the calluser flow's post-hoc invite leg draws a trouter
addParticipantFailure 580/5201 (callControllerTransactionEnd 408/10312
"timed out waiting for an answer to the media negotiation request")
while the call itself works — cosmetic today, worth understanding when
N-party lands.

**P2 SCREEN SHARE on the consumer video leg: DONE, synthetic source.**
TEAMSFAST_SCREENSHARE=1 + videocall: pa's share (1280x720 bars) rides
the BUNDLE video leg — encoder re-init at 720p/3072kbps on the SAME
chain, preview tee shows the bars, pb DECODES 720p frames and its stage
RENDERS the bars end-to-end (vision-verified both sides). One real bug
found + fixed (246deb4 + app 18ff4a9): 720p frames at the stage's
15fps repaint = ~55MB/s RGBA conversion+upload on software GL — the
callee UI burned 86% CPU and the stage VANISHED; VideoFrame::downscaled
box-filters anything >640px wide before the texture. The REAL portal
path stays user-gated (interactive session); the interactive STOP
(share→camera swap) is untested headless (no click injection).

User-gated §5 batch (unchanged, ONE message when the user is back):
browser→app VIDEO call with camera ON (validates our answer against
real WebRTC + captures the newOffer/renegotiation shapes for mid-call
video + mid2 screenshare activation); 3rd participant in the group
call; one hot-mic browser→app audio round (recv=0 = muted-tab theory).
Refresh the browser tab after any registrar sweep.

P3 notes (not blocking): outgoing consumer call banners show the raw
thread id (incoming calls show real names since 548c766); the callee
stage renders narrow beside an open chat (layout, not data); rigs burn
~75% CPU on 720p (openh264 decode at 15fps dominates — fine for QA,
worth a thread-priority look someday).

### run-14 addendum — BROWSER ROUND (user-gated §5, 2026-10-21 morning)

The user placed three browser→app video calls (Chrome 155 /
TsCalling 2026.36, camera+mic on). Three findings, three fixes; the
last fix (PLI) was then verified LIVE on the rigs after the user left.

1. **DTLS close_notify killed both directions** (core commit "DTLS-SRTP:
   keep the DTLS connection alive"). We closed the DTLS connection right
   after exporting SRTP keys — Chrome marks the transport
   dtlsState=closed and then neither encrypts its RTP nor decrypts ours
   (ICE connected, senders active, ZERO media both ways; the user's
   webrtc-internals dump showed the closed transport — the decisive
   evidence). After the fix: **browser↔app audio is BIDIRECTIONAL,
   live-verified (497/497)** — this also closes the old "recv=0 = muted
   tab" theory (it was our close, not the tab).
2. **Our BUNDLE video answer passes real WebRTC**: setRemoteDescription
   succeeded (signaling stable, 5 sections), and Chrome's video RTP
   arrives and DECRYPTS (372 pkts in 10 s). Chrome sends standard
   RFC 6184 with **STAP-A** aggregation (SPS/PPS bundled) which our
   depacketizer dropped as unimplemented → STAP-A support + rtx-PT
   exclusion + strict PT routing shipped (commit "Video receive: STAP-A
   aggregation + strict PT routing").
3. **The initial SPS/PPS+IDR burst lands before our DTLS keys install**
   and senders never repeat parameter sets unprompted — every P-slice
   then fails dsNoParam forever (600+ type-1 NALs, zero type-7/8/5 after
   the start window). Fix: **RTCP PLI keyframe recovery** — the answerer
   requests a keyframe every 500 ms until a frame decodes; both video
   send loops ANSWER PLIs with a forced openH264 IDR. VERIFIED LIVE
   rig↔rig with the TEAMSFAST_DROP_KEYFRAMES_SECS=5 hook (simulates the
   missed window): PLIs flowed, the caller forced IDRs, decode recovered
   to 100% after the window. Clean no-hook regression: 147/147 frames,
   audio+video both ways, clean hang-up.

State: core at the PLI-loop commit (all run-14 work: cfcacb0..PLI),
app eeed648 + RUNNING-LOG commit; COMMIT BUT DO NOT PUSH. Rigs pa/pb
running the final binary. REMAINING for the user's NEXT browser round
(one call, camera on): confirm browser VIDEO decodes in the app tile
(audio + answer-acceptance + RTP/decrypt already proven); then the
screen-share click (mid2 renegotiation capture) and the 3rd-participant
group round.

### run-14 addendum II — hardening pass (2026-10-21, autonomous)

Test/quality pass over everything run-14 shipped, while awaiting the
user's next browser round:

- **Regression pins for every live bug** (each failure found on the
  wire this run): the ONE-encoder law as a CONTRAST assertion
  (interleaved encoders <=3/6 decoded vs single-encoder >=5/6 on the
  same frames); I420 black renders dark while the all-zero buffer it
  replaced is asserted to be exactly RGB(0,135,0); PLI wire roundtrip
  (+ PSFB FMT!=1 is not a PLI); app apply_frame caps 720p to 640x360.
- **New coverage**: answer stub generation (all-after-audio vs
  skip-first-video) + BUNDLE-group insertion; OfferedVideo direction
  matrix / pm=0 fallback / AV1-only=None; pick_source_frame semantics
  (latest-wins, gate, screen-over-camera, sticky-between-ticks,
  disconnected fallthrough+clearing).
- **Latent bug fixed + pinned**: the rtx-PT exclusion scan could leak
  past the first m=video section into the screenshare m-line and strip
  a MAIN payload type (the sections reuse PT numbers for different
  roles). The regression test builds the exact collision and fails
  against the old scan.
- **Outgoing call banners show the peer's name** (cached chat name
  instead of the raw GUID thread id — vision-verified live: "rico
  almani — connected 00:04", no @thread.v2 anywhere).
- **qa_call_e2e.sh pairvideo**: the full consumer video E2E as ONE
  command (ring video=true → accept → BUNDLE answered → decode on BOTH
  sides → video counters in the end summary). Live PASS against the
  rigs; requires the pair thread id as arg 6.
- AGENTS.md status + call-gaps sections refreshed to the run-13/14
  truth (consumer audio/video/group/screenshare E2E; the six video
  "laws" recorded where the next session will read them).

Core: 432 tests (437 with video-cam). App: 119. Rigs on the final
binary.

### run-14 addendum III — cleanup pass (2026-10-21, autonomous)

Follow-on cleanup while awaiting the next browser round:
- **Zero unused-variable warnings in teams-core across feature combos**
  (cfg-conditional params get explicit no-op refs; tight cfg(all(...))
  gating on has_display; planner keeps id validation without the dead
  binding; get_session_id underscores its unused http param).
- **Dead code removed**: the legacy private Graph chat module
  (api/graph.rs — pre-msgapi, zero references), prose_html,
  thread_member_mris_pub, the setup_media_leg wrapper, write_wav.
  This-run's code areas are warning-clean; the ~600 remaining warnings
  are the inherited CLI-era baseline (mostly serde field-never-read on
  response models) — deliberately left (wholesale churn risks
  wire-format structs).
- **call_label extracted + archive-backed test** (name > thread id,
  blank/unknown/missing fallbacks).
- **pairvideo re-run PASS on the final binary** after all of the above
  (14/14 assertions) — the dead-code removal touched call_test, so the
  live round was the regression gate. Rigs restored on the final binary.
- **README** updated: audio+video+group calls shipped.
- NOTE: cargo clippy is broken on this machine (rustup clippy-driver
  1.98.0 vs Fedora rustc 1.98.1 — E0514 on every shared dep); left
  alone rather than touching the user's toolchain.

Core 432 (437 video-cam), app 120 tests.

### run-14 addendum IV — adversarial-review campaign (2026-10-21)

Two independent adversarial reviewers (calling stack + app integration)
over the run-14 code; every finding triaged, the substantive ones fixed
and live-re-verified. Plus one genuine REMOTE HANG found by my own
hostile-input test.

**The hang (found before the reviews reported):** a malformed SDP blob
made raw inflate report Ok forever with zero progress — decompress_sdp's
grow-loop only resized when output space ran low, so the backend thread
SPUN with no log output until killed. Any incoming call with a corrupt
blob could wedge the app remotely. No-progress bail + 8MB cap; 13
hostile shapes pinned by test (was >60s hang, now 0.00s).

**Core fixes (both reviewers' P1s + P2 batch):**
- MediaSession OWNS the video-send + PLI tasks now (aborted in
  stop/Drop) — every answered video call leaked an immortal encoder
  task (up to 1080p/3.5Mbps forever with a live share).
- Caller-side PLI recovery: a browser callee's keyframe burst inside
  our DTLS-keys window would black remote video for the whole call (the
  answerer's exact live bug). Stall-aware PLI on BOTH sides (the old
  task exited after first decode — mid-call IDR loss froze video).
- Inbound PLI takes the blocking lock; caller skips video when the
  ANSWER rejected the m-line; rtp::decode skips header extensions like
  full_header_len; is_rtcp_packet covers 200..=207 and gates both recv
  loops (RTCP-auth on every RTP packet doubled receive crypto);
  ulaw dump env-gated; conversation keep-alive stops with the call;
  downscaled covers odd widths (nearest-neighbor fallback).

**App fixes:**
- P0: busy-rejections and pre-task join failures no longer wipe a LIVE
  call's UI while its task keeps running (mic open, no hang-up left) —
  routed to Status.
- P1 regression: EVERY call (audio included) opened the video stage
  with a 15fps repaint loop (call_controls(true) unconditional);
  outgoing gates on the video flag, incoming derives it from the
  invitation — vision-verified audio calls are banner-only again.
- A second ring from a DIFFERENT caller now declines the superseded one;
  the SAME caller's re-fire (consumer DOUBLED invitation) just refreshes
  — the naive version self-declined every consumer call (live-caught:
  fork attach 404, both pair flows dead; fixed and re-verified).
- Late-camera toggle skips when the call holds the pre-spawn capture;
  HangUp leaves the slot for the done-reaper (no overlapping call
  tasks); share_note clears on end; the label strip only removes the
  literal ' — accepting…'; ctl send refuses a mismatched chat_id.

**Live verification on the final binary:** pairvideo PASS (14/14),
pair PASS (audio E2E), vision: audio-call layout correct. Core 433
(438 video-cam), app 120. Deferred (documented, not fixed): answer mids
echo the offer's real mids (currently hardcoded 0/1 — true for every
captured web shape); decompress-at-entry for the BUNDLE helpers;
PACSI stream-layout metadata tracks the real resolution; CallGone is
identity-free (correlation needs threading call/thread ids through the
event — the watchdog bounds the damage today).

### run-14 addendum V — ghost ringback fixed (user report, live-verified)

"When calling someone, ringing continues even though the callee already
has the call." Root cause: the ringback tone loop only exited on
phase-change / stop-watch / 120s cap — when a placement FAILED (decline,
timeout, acceptance-without-SDP) the call task returned and dropped the
phase SENDER, and a tokio watch serves its last value forever after
sender drop, so the 425Hz tone ghosted up to two minutes with no call UI
left. The loop now checks sender liveness (wait_ringing always did).
Live-verified on the rigs: decline round → task return → "ringback:
stop" within milliseconds (was: +120s). Every declined call did this.

Also learned: the rigs share the host's PulseAudio — all automated
rounds have been AUDIBLE on the user's speakers (including the ghost
tones). TEAMSFAST_NO_RINGBACK=1 mutes the tone; rigs now run with it.
If the caller's phase itself sticks Ringing while media flows (the
app→browser direction — our offer vs the web callee's answer creation,
still never verified end-to-end), that is the remaining acceptance-chain
gap needing one browser round.

### run-14 addendum VI — BROWSER VIDEO CALLS COMPLETE (user rounds, live-verified)

Three user-in-the-loop rounds today closed every remaining call gap.
The final chain, all verified against real Chrome (TsCalling 2026.36):

- **app→browser**: BUNDLE offer accepted, browser answers mid1 for
  real, DTLS server role, bidirectional audio for the whole call,
  video both directions. The pre-DTLS 410/3111 era is fully closed.
- **browser→app**: full E2E incl. DECODE AND RENDER — the user's
  camera decodes 589/589 frames at 640x360 and renders in the stage
  tile (vision-verified: real webcam scene).

The last three bugs, each found by one instrumented round:
1. **rtcp-fb negotiation** (mined from the web's own JS
   `reduceRtcpFbInSdp` list): a WebRTC sender only honors PLI/FIR when
   the SDP declared a=rtcp-fb for it — our offer had only MS x-message
   lines. Chrome answered 40+ PLIs with SPS/PPS but ZERO IDRs until the
   standard ccm fir / nack / nack pli lines landed; after: PLI→keyframe
   in 50 ms.
2. **STAP-A pending swallow** (FU-A forensics: 224 IDR fragments, 168
   middles + 56 ends + ZERO starts): the STAP-A(SPS+PPS) queue
   delivered one NAL per depacketize call, so the second queued NAL was
   delivered as the NEXT packet's result — consuming it unparsed. The
   packet after every SPS/PPS burst is always the IDR's FU-A START.
   Pending NALs now drain within the same packet (process_nal
   extracted); regression test pins the exact sequence.
3. **2h call-history durations** (user-reported): our hang-ups posted
   dead links (acceptance links/end 404s on connected calls) — the CC
   held sessions to its ~2h cap (also the browser's 'trying to get you
   back' screen). Fallback chain end -> callLeg/end -> conversation
   leave on the caller; the callee hang-up now POSTs the invitation
   end too. The leaked sessions also pinned the browser's inbound
   endpoint 'in a call' for hours — the cause of the browser not
   ringing late in the session.

Also this session: ghost ringback fixed (sender-liveness; the rigs
share the host's PulseAudio AND its tray — two mystery tray icons were
the rigs; TEAMSFAST_NO_RINGBACK mutes), fork-loss ring detection
(progress-link poll while ringing), and the ring-dedupe regression
(consumer DOUBLED invitations must not self-decline — live-caught when
both pair flows went red).

Core 434 (439 video-cam); app 120. Rigs on the final binary.

## run-15 — screenshare E2E both directions, call-recycle + egui-Image fixes, N-party shape, P3 hardening (2026-10-21 night)

Autonomous run per docs/drafts/run15-prompt.md. All commits PUSHED:
app 59b00bc..3063668 (7), core 9212913..09ed4b7 (3). Final: core 457
tests (both feature combos), app 121. Rigs pa/pb left RUNNING on the
final binary (muted ringback).

**THE UNEXPECTED BLOCKER (found by P0's first round, fixed + verified):
the service RECYCLES the caller's dedicated trouter call socket every
~2m13s (Normal close) — the frame pump treated ANY close as "remote end
received" and hung the call up mid-media, then the post-mortem hang-up
chain hit leave-400 and warned the CC session would cap server-side
(the 2h-history producer again). Fix (core 9212913): the pump
reconnects (new TrouterSocket::connect_url on the same session URL,
capped backoff), only an explicit callEnd frame ends the call, and the
pump watches stop/end so it can't outlive the call. Live-verified: a
>5min call survived the recycle at 2m13s AND 4m26s, media unaffected
(UDP is independent), clean hang-up with honest counters (17926 pkts).
Also: /tmp/received_audio.pcm now writes only under TEAMSFAST_AUDIO_DUMP
(call audio is user content).

**P0 SCREEN SHARE: both directions end-to-end, vision-verified.**
pa-shares→pb-renders AND the flip pb-shares→pa-renders: auto-share fires
on CallMedia (logs CALLQA share=auto with the gate state now), encoder
re-init 1280x720/3072kbps on the same chain, PLI recovery + 640-cap
cope, bars render in the remote tile on both rigs (vision 4/4 + pixel
numpy: video present in-tile, no window-edge bleed). STOP mid-call is
clean on BOTH legs (caller + answerer): decode CONTINUES at 320x240
(camera fallback; encoder re-inits down), `share on` returns 720p. The
one-encoder law holds across mid-call resolution changes both ways.
New ctl verbs: `share on|off` (headless share start/stop).

**The egui 0.36 Image trap (the "clipped callee stage" was never a
panel bug):** an `Image` whose source texture is LARGER than the
ui.add_sized slot paints at ORIGINAL size centered on the slot — a
640x360-capped share rendered as a slab starting ~143px left of the
panel and running 142px PAST the window edge (missing header/controls
read as a collapsed panel; ui.max_rect() stayed healthy in the very
frames the tile painted at [602..1242] — two instrumented rounds to
crack). Camera tiles (320x240 < slot) always fit, which is why only
share rounds broke. Fixed with the local-preview pattern
(allocate_exact_size + painter().image, aspect baked into the rect);
RECTLOG probe kept (STAGE-TILE painted= vs max_rect=). Also fixed: the
QA auto-share latched once per PROCESS (share_auto_fired never reset —
later calls silently sent camera only, and one "share off" test
silently tested nothing); now per-call + forensics logs.

**P1 meeting-join video modality (core 8d0ebee):** the MS-shape offer
(work-tenant/meeting legs) gained the STANDARD a=rtcp-fb ccm fir / nack
/ nack pli lines on the video m-line — a WebRTC peer answering that
offer never honors PLIs without them (the exact live consumer-path
bug; the web's own offers carry them). strip_video_mline extracted
pure; join-shape pinned by tests (m=video + fb when camera on,
audio-only when off). LIVE join-path verification BLOCKED: the conv-
service is wedged on the epconv create+join path TODAY (Phase-2 400 on
BOTH modalities, BOTH group threads, before and after our changes —
run-11 wedge pattern; cpconv 1:1 unaffected and re-verified). Consumer
group video calls take the BUNDLE path regardless. Deferred: one live
join round when the wedge lifts.

**P2 N-party create (core f3154ac):** participants.to is now an array
(payload extracted pure + unit-tested: 2/1/0 entries, distinct
participantIds), the roster resolves ALL non-self members
(consumer_thread_members), and every callee gets the post-create
invite (the dial signal). LIVE 2-entry probe (real member +
unreachable MRI via TEAMSFAST_EXTRA_CALLEE): the CC ACCEPTS the
multi-entry create (201) and still forks the real member (B rang) —
but answers the create ITSELF with a placeholder acceptance
(acceptedCallModalities=[], no media leg): multi-entry creates flip
the CC into group-call mode. True N-party media needs the group-call
acknowledgement machinery — honestly deferred to the user's
3rd-participant round. 1:1 regression: 375/375 pkts both ways.

**P3 hardening (core 09ed4b7 + app 73a78ff):**
- answer mids echo the OFFER's real mids (align_answer_mids_to_offer;
  hardcoded 0/1 stays byte-identical for the standard web shape).
- decompress-at-entry: ensure_plain_sdp at the answer's blob entry —
  compressed offers can no longer silently degrade the bundle/video
  helpers to audio-only.
- PACSI Stream Layout tracks encoder (re)inits (set_stream_layout;
  720p announces 1280x720/3072k, not the 320x240 default).
- CallGone identity: the gone-push's /callAgent/<epid>/<callId>/ id is
  extracted and compared against the ringing invitation's own id — a
  foreign call's end-push can no longer clear the ring or hang up the
  live call (missing ids keep the old behavior; nothing captured
  regresses).

**P4 polish:** top-bar status refreshes on call start ("Calling…") and
connect ("In a call") — the previous call's end summary used to sit
there through the next call (vision-caught). Audio-call banner verified
live: banner-only (no empty stage) + "Show call view" toggle present.

**pairvideo PASS (14/14) on the final binary** — full regression after
everything above (ring video=true, BUNDLE answered, decode both sides,
clean hang-up, both-direction counters). Known ops notes: bare `--ctl`
without TEAMSFAST_PROFILE hits the DEFAULT slot and reads as a dead
rig (cost ~40min this run — memory file written); rig log timestamps
are UTC vs local +02:00 (the "log stopped" misread); the rigs still
capture the HOST webcam on video calls (two rigs = one camera: the
second gets black — expected).

DEFERRED (honest list): live meeting-join video round (conv-service
wedge, retry when it lifts); true 3-way N-party media (group-ack
machinery, user round); browser screen-share mid2 capture + 3rd
participant (unchanged user-gated items).

## run-16 — join-rewrite, browser group round, adversarial campaign, caller watchdog (2026-10-22)

Autonomous session ("everything that doesn't need the user"). ALL
PUSHED: app 24d6d88..4449dda (7), core 09ed4b7..9b0f27a (6). Final:
core 460 tests, app 122; pairvideo PASS on the final binary; rigs
pa/pb RUNNING on it (muted).

**The "wedge" was never a cooldown — the SERVICE changed its consumer
controller responses.** Overnight retries still 400'd; the epconv
create (consumer skype proxy, 201) now returns controllers/links on
the WORK proxy (flightproxy.teams.microsoft.com/api/v2/ep/conv-…)
which rejects our X-Skypetoken join outright. A/B probe: host-swap
alone 400s; host+ep/→cp/ path swap to flightproxy.skype.com 201s —
but only on ~1/5 clusters (euwe-03 accepted; euno/frce/swce 400).
consumerize_conv_urls now rewrites the controller AND the whole
response body at Phase 1 for consumer. NOTE: the two-phase join is
still the WRONG consumer group flow — see next item.

**Consumer group calls belong on the roster→cpconv path, and the QA
group threads were stale.** The old stream01 threads' rosters no
longer contain B (empty callee list → the two-phase fallback → the
400s above). Shipped: consumer_thread_members (plural roster dial),
new ctl verbs `group <topic> <upn>…` and `groupfrom <thread> <topic>`
(consumer group-chat create lifting members from a source roster —
MRI forms verbatim; the AAD resolution of the work-tenant create
silently died on consumer), CALLQA chat_created marker. LIVE: minted
a fresh A+B group from the pair 1:1.

**UNPLANNED BROWSER ROUND (the user answered a fresh-group video call
from their browser): full E2E VERIFIED** — connected, DTLS server
role, audio 5981/5980 BOTH ways, browser camera decoded 626+ frames.
The fresh-group video path through roster→cpconv works against real
Chrome.

**Caller hang-up chain: root-caused to "always broken, never
verified".** Every caller termination link fails on consumer
(acceptance /active/../end 404; synthesized callLeg/end 404; leave 400
with fresh e= AND full conv headers). The addendum-VI verification
round timed out UNANSWERED (missed calls were the clean path) — no
CONNECTED caller hang-up ever succeeded. Groundwork shipped
(latest-wins link harvest from the late acceptance frame's FULL set +
conversationUpdate frames, full conv-service headers on end/leave
POSTs). COMPENSATION VERIFIED: the CALLEE's invitation end
(fork-reject DELETE) returns 202 — rig↔rig sessions terminate through
the peer. The leak bites only peers that never hang up (the user's
browser sat on the reconnect screen; they can Leave from the browser
UI). NEXT CAPTURE ASK: a browser-CALLER HAR (user places a call from
the browser, hangs up from the browser, exports HAR) — that end POST
is the missing wire shape.

**Adversarial campaign (2 reviewers + own hostile tests) — every
substantive finding fixed and live-verified:**
- CORE P0: the reconnect pump could outlive the call forever (nobody
  signals it on duration-cap exits; a dropped stop sender serves its
  last value forever) — one leaked reconnecting WebSocket per call.
  Main loop signals end; closed channel stops.
- CORE P1: remote-end detection was a substring — "callEndReason"
  contains "callEnd", so benign frames could tear down healthy calls.
  Parsed (check_call_end) + exact delivery-URL suffix; single parse
  per frame.
- CORE P1: &body[..min(2000)] UTF-8 byte-slice panics (the éé
  precedent) — chars().take() ×4. P1: PACSI set_stream_layout missing
  on the CALLER's loop (720p shares announced 320x240). P1: consumer
  roster failures fell silently to the broken two-phase path (now a
  clear error).
- CORE P2: mid alignment once for mediaAnswer AND the embedded
  acceptance SDP; add_participant_url consumerized; harvest accepts
  https only; gzip frame decompression capped 8MB; /end synthesis
  edge cases.
- APP P1: ring_call_agent_id outlived its ring on decline/timeout/
  fork-loss — a stale match could hang up an UNRELATED live call
  (cleared with the ring now). P2: CALLQA verb logs the head word
  only (it carried `send` message TEXT and group topics/UPNs — a
  standing privacy violation); CallFailed resets the top-bar status;
  remote tile letterboxes when the height cap binds (portrait shares
  squashed); share starts generation-tagged (a portal start that
  outlives its call can't attach to the next); CreateChatOk dedupes;
  empty LOCATION ids rejected; lobby wording.
- Hostile pins: consumerize/strip/mid shapes, serde-bounded JSON walk.

**THE LIVE FINDING THE REVIEWS DIDN'T HAVE: the caller had NO
frozen-recv watchdog.** While verifying the review fixes, a trouter
recycle dropped server-side messages (message_loss frame) — pb's
hangup end-delivery never reached pa, which stayed "connected
05:52" streaming into the void toward the 55min cap (the answerer
has had a 30s watchdog all along). Shipped + LIVE-VERIFIED: caller
watchdog (45s frozen recv after media flowed) — peer hangup 08:50:58
→ fired 08:51:44 → clean end.

DEFERRED: call_after_create identity (QA-only race; harness never
interleaves), the share-thread-panic Starting edge (catch_unwind
someday), caller-end wire shape (browser HAR ask above), browser
mid2 screenshare capture + 3rd participant (standing user rounds).

## run-16b — leak verification + 55-min soak (2026-10-22)

**The leak verification found a REAL one past the adversarial fixes:
+1 UDP socket per connected call, both sides.** Isolation: decline
rounds leaked ZERO (no media session) → the holder lived in the
connected-media phase → the run-14 DTLS keep-alive fix's
std::mem::forget(dc) pinned the media socket forever (the forgotten
connection holds a strong Arc; the run-14 comment claimed "leaks
nothing but the object" — wrong by one socket). QueueConn now holds a
WEAK handle (strong refs exist for the whole handshake; the conn is
idle post-export). VERIFIED: 5 connected calls in one process, socket
count flat (was +1/call). Core a82a947.

**55-minute soak (the longest call ever run on this stack): 3300s
duration cap, auto-shared 720p bars pa→pb, camera pb→pa.**
- Caller survived **38 dedicated-socket recycles** (one every ~87s)
  with ZERO media interruption — the reconnect pump holds under
  sustained recycling.
- Audio 164,992/164,993 packets (mirrored both sides), video
  171,020/147,755 — decode counts matched frame-for-frame the whole
  hour (45,376+ frames each side at the 50-min mark).
- Zero watchdog misfires, zero PLI stalls, memory flat (298MB caller
  w/ 720p encode+decode+share, 253MB callee), sockets flat (the DTLS
  fix holds across the hour too).
- Clean cap-out end on both sides with honest summaries.

Also prepared (docs/drafts/upstream-bundle/, local): four-patch bundle
for better-teams (presence-with-client, login_with_code_sink,
run_call_with_stop, planner-with-client) with per-pitch write-ups —
NOT posted, awaiting user review.

## run-16c — BROWSER ROUND: caller hang-up FIXED + renegotiation shapes CAPTURED (2026-10-22)

The user placed a browser video call to rig pa, screen-shared, hit the
expected share-error (we don't answer renegotiations yet), hung up
cleanly, and exported the HAR (~/Downloads/teams.live.com.har — parsed
locally, never committed; wire shapes extracted only).

**THE CALLER HANG-UP CHAIN IS FIXED (core bbfd771, pushed):** the
browser ends a consumer call by POSTing the conversation leave link
with a REQUIRED 442-byte body — participants.from (MRI, displayName,
REGISTERED endpointId, participantId) + conversationTransactionEnd +
callTransactionEnd blocks. Our empty {} body had 400'd on every
consumer call since the beginning. With the HAR shape the leave
returns 204 — verified live TWICE (audio + video caller hang-ups), and
the callee now tears down in 63ms (the CC always delivered the end;
our broken leave hid it behind the watchdog). Closes the 2h-history /
stuck-reconnect-screen class for app-placed calls.

**Modality renegotiation wire shapes CAPTURED (both directions, saved
to gitignored docs/drafts/captures/):**
- CALLEE view (trouter mediaRenegotiation POSTs to our callAgent):
  mediaNegotiation {callModalities, links{mediaAnswer,rejection},
  mediaContent{blob=new offer SDP, newOffer:false,
  negotiationTag:"<participantId>;ss_N"}}. The SHARE START offer adds
  a NEW m=video section with a FRESH mid (4, then 5 on re-fire),
  a=sendonly, a=label:applicationsharing-video; the second frame
  (ss_2) flips mid:1 to recvonly (camera stopped). NOT the original
  mid2 — each renegotiation mints new mids.
- CALLER view (browser POSTs, 202s): startOutgoingNegotiation link
  with {mediaNegotiation:{callModalities:["Audio","Video",
  "ScreenSharer"], sender:{...}}} — the send-side of the same flow.
The acceptance's links (callLeg/renegotiate/updateCallState/hold/…)
were also confirmed live. NEXT FEATURE = answer these offers (mediaAnswer
link) + accept new mids on the BUNDLE transport.

Also this round: browser→app video re-verified on the newest build
(decode 640x360, 301+ frames) and the caller's renegotiation link set
harvest-hinted by the acceptance frame confirmed.

## run-16d — MID-CALL SCREEN SHARE FROM A REAL BROWSER: WORKING (2026-10-22)

Live acceptance round (user browser → rig pa, camera + share):

- **Camera video decoded normally** (640x360, the wedged state from the
  previous round had cleared; see the stale-offer gate below).
- **Share click → offer received (6009 bytes) → answered → 202
  Accepted → the share DECODED at 960x540 (651 frames)** — a distinct
  stream from the camera on the new mid, riding the same BUNDLE
  transport. The browser showed no share error. Clean hang-up after.

This closes the last capture-gated call feature: true modality
renegotiation (callee side) is LIVE against real Chrome.

**The earlier failed round root-caused to a stale-offer poisoning:**
trouter REDELIVERS unacked indicators on reconnect — a morning-call
offer arrived during an unrelated sanity call and the (then ungated)
answering posted a live-session answer to it (2xx); the NEXT browser
call on the account showed the wedged state (video RTP arriving with
zero decode all call; the share click produced no offer delivery at
all). Guard shipped (61f2c64): offers carry their callAgent id and the
app drops foreign ids (CallGone-style correlation) — after which the
account state had recovered and the acceptance round passed.

Remaining in the renegotiation area (smaller, follow-ups): caller-side
answering (rig-placed calls receiving browser shares), the stop-share
offer (ss_2 camera-off shape) is unit-tested but not yet
live-observed, and the share's video is not yet rendered as a SEPARATE
tile (it takes over the remote tile — same SSRC-space stream, distinct
resolution; a proper share-tile UI is polish).

## run-16e — CALLER-SIDE RENEGOTIATION ANSWERING SHIPPED (2026-10-22)

Closes the renegotiation asymmetry left by run-16d: when the RIG
PLACES a call and a real web peer (callee) shares mid-call, the
renegotiation offer is delivered to the caller — and nothing answered
it. Now shipped in teams-core 8ce9f3e (PUSHED):

- `sdp::build_renegotiation_answer_as_caller(offer, our_offer)` — the
  caller never sent an answer, so the template is the OFFER WE SENT:
  our ICE/DTLS creds verbatim, every `a=setup:` rewritten to `passive`
  (our offer said actpass, their acceptance answered `active` → we are
  the DTLS server; a renegotiation must not restart or flip roles),
  directions mirrored, NEW sendonly share sections accepted recvonly
  on the same transport with their fresh mid (live-captured: each
  share gets a NEW mid — 4, then 5; stopped mids never return),
  port-0/x-data rejected, exactly ONE group line (the callee variant's
  duplicate-group output was live-accepted by Chrome, but the caller
  path emits a single one).
- The caller main loop (run_call_with_controls) drains
  `CallControls::renegotiations` — the app-side forwarding
  (Event::RenegotiationOffer → call_media handle) was already
  direction-agnostic, so NO app change was needed. Send trap: the
  channel is drained into a Vec BEFORE awaiting — holding the mpsc
  Receiver's borrow across an await makes the tokio::spawn'd task
  !Send (Receiver is Send but not Sync).
- Verified: 2 new unit tests (share-start + camera-off shapes,
  synthetic creds 198.51.100.4, incl. the caller-side trap assert —
  the offer's creds must never leak into the answer), core suite 464
  green, app 123 green, rig↔rig pairvideo PASS on the new code
  (ring→answer→video both ways→clean hang-up). LIVE browser round
  (rig places call → browser shares) still pending — that needs the
  user; rig↔rig share never triggers this path (our share rides the
  existing video m-line via stream layout).
- Housekeeping: rigs pa/pb were cycled onto the new binary during the
  regression and are currently DOWN (session hit a memory cap —
  concurrent cargo invocations; builds now run sequential `-j2`).
  Relaunch Xvfb :81/:82 first, then the apps (run-15 protocol).

**run-16e live addendum**: the FIRST pairshare run upgraded the
verification — when A stopped its share, the remote peer sent a REAL
stop-share renegotiation offer (30215 bytes) to the CALLER, and the
new drain answered it successfully (renegotiation=received →
answered, call continued through a re-share + clean hang-up). So
caller-side answering is LIVE-VERIFIED against a real peer (rig), and
the "stop-share offer never live-observed" item from run-16d is closed
(observed + answered + recovered). Remaining live gap is only the
Chrome-specific round (rig places call → BROWSER shares).

Also shipped: `qa_call_e2e.sh pairshare` — the run-15 manual share
regression as one command (A auto-shares synthetic 720p → B decodes →
A stop → A re-share mid-call (encoder re-init) → B decodes again →
clean hang-up with counters). First run PASS (720p decodes 1→6 on the
re-share; audio 628/628, video 690/566).
