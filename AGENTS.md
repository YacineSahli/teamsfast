# TeamsFast — agent guide

A native Microsoft Teams client for Linux: Rust + egui, no browser engine.
Built on the **fastframe** stack and on **teams-core** — our maintained fork
of the patched `ost` protocol library. Sibling-in-spirit of zapfast and
spotifast (same architecture, same fastframe crates). Read `FEASIBILITY.md`
for the full investigation and `README.md` for the user-facing summary.

**Status: Phase 2 in progress — chat client with sections, calls, meeting
join AND incoming-call ringing.** Chat (send/reply/edit/delete/react/forward/pins,
pending+retry bubbles, seen-by, unread badges, pinned/muted chats,
per-chat notification levels, contact cards, ghost mode), Adaptive Cards,
reaction picker with tabs + TENANT custom-emoji reactions (beta custom
wire, live-verified run-9) + WYSIWYG composer,
section rail (Chat/Teams+mgmt incl. channel rename-delete/Calendar+join/
Files incl. per-chat shared files/ToDo/Planner/Shifts/OneNote/Activity),
quiet hours, Meet now,
presence with status menu, full Settings window, GUI device-code sign-in,
notification click→chat, 1:1 audio calls + MEETING JOIN (verified live:
epconv → call_accepted=true → Call active). Incoming calls ring with an
accept/decline banner (auto-answer is disabled app-side; audio answer
verified live end-to-end pipeline, real-payload capture pending — see
gaps). Offline/local search merged with Graph. Video/screenshare is the
remaining Phase 2 item.

## Repository layout

- `src/main.rs` — entry; `--dump-chats`, `--probe-chat <id>`, `--search`,
  `--teams`, `--logs` headless debug flags; `TEAMSFAST_OPEN=<chat_id>` and
  `TEAMSFAST_SIZE=WxH` QA env hooks.
- `src/app.rs` — App state, event drain, Action::apply, top bar (icon
  buttons, theme picker, user chip), offline banner, theme catalog polling,
  reaction popup, lightbox.
- `src/backend/mod.rs` — worker thread; full Command/Event surface;
  Session (client + self_id + older_links + archive); dispatch.
- `src/backend/conv.rs` — open/send/reply/edit/delete/react/mark-read/
  paging; cached-first open (archive paints instantly, network then
  refreshes).
- `src/backend/directory.rs` — search / teams+channels / create 1:1+group.
- `src/backend/media.rs` — image fetch/decode/downscale (spawn_blocking),
  upload (progress events), download-and-open.
- `src/backend/live.rs` + `live_parse.rs` — Trouter session, pointer-based
  sticky hover unaffected; parser (9 unit tests, shapes from real logs).
- `src/backend/archive.rs` — SQLCipher persistence (see "Local archive").
- `src/ui/` — sidebar.rs (rows = allocate-first whole buttons, context
  menu, unread badges), conversation.rs (bubbles, hover bar, composer,
  pending-send bubbles, seen-by, call controls), panels.rs (search,
  new-chat), sections.rs (Calendar/Files/ToDo/Activity panels),
  settings.rs (Settings window), cards.rs (Adaptive Card renderer),
  widgets.rs (Teams-HTML renderer, avatars, day separator, icon_button),
  media.rs (lightbox).
- `src/backend/sections.rs` — calendar/files/todo/planner/shifts/teams-mgmt
  handlers; `src/backend/join.rs` — meeting-join resolution (join URL /
  thread id / meet ID → call leg).
- `src/theme.rs` — Palette (17 named colors, dark/light bases, derive
  rules), Catalog/Omarchy bridge, Settings load/save, emoji raster cache.
- `src/emoji.rs` — bundled Noto Color Emoji setup (MUST run before
  plugin/raster; see gotchas).
- `src/tray.rs` — tray icon (procedural SVG-free icon) + menu.
- `teams-core/` — the protocol crate (nested gitignored checkout for
  development; builds pull the published copy from
  `https://github.com/YacineSahli/teams-core`). To build against local
  changes, this machine carries the `[patch]` in **`~/.cargo/config.toml`**
  (global, machine-local — NEVER put it in Cargo.toml or the committed
  `.cargo/config.toml`; an accidentally committed path patch broke fresh
  clones/CI once). Push teams-core main before relying on published builds.
  Upstream chain:
  eisbaw/ost (dead) → better-teams vendored+patched (92 patches, MIT) →
  us. Sync via `git filter-repo --subdirectory-filter rust/ost` re-run +
  `git merge` (deterministic, shared history).
- `.upstream/better-teams`, `reference/` (spotifast/zapfast clones),
  `qa_shot.sh` (headless screenshot harness).

## Critical gotchas (each one bit us; do not rediscover)

### egui 0.36
- `App` trait: implement `fn ui(&mut self, ui: &mut Ui, frame)` (not
  `update`); the root `Ui` is handed to you — panels are
  `egui::Panel::left/top/bottom(id)` shown *inside* it, CentralPanel last.
- `Ui::horizontal_wrapped` rows EXPAND to full available width — chat
  bubbles inside one render full-width. For shrink-to-fit bubbles give the
  bubble an `allocate_ui(vec2(measured_width, …))` slot
  (see conversation.rs own-row: text measured via painter.layout, slot =
  clamp(text_w + 26, 56, cap)).
- `set_max_width` inside a `right_to_left` layout SHIFTS content left
  instead of capping it. For own bubbles use
  `with_layout(top_down(Align::Max))` on an explicit-width slot.
- Right-anchored shrink-to-fit cluster (own message rows): do NOT rebuild
  with RTL/allocate_ui/set_max_width — all mis-anchor on this stack
  (four verified-broken run-5 attempts). The working primitive is an LTR
  `ui.horizontal` + a computed LEADING space in which the measured slot
  width cancels arithmetically (`own_row_lead`), pinning one shared
  right edge. The slot measurement must be content-aware (text via
  painter.layout, cards=560, images from persisted image_sizes): any
  content that escapes its slot grows the parent `max_rect` and POISONS
  every following row's `available_width` (live-verified cascade).
- `StickArea`/`stick_to_bottom` + late content: earlier blank-pane
  incident was OUR panel-order bug (composer rendered after CentralPanel
  consumed space), not egui. Keep order: top → left → right → bottom
  (composer) → central. Rule: no panel after CentralPanel.
- Late `.response.interact(Sense::click())` after painting children gets
  shadowed by hover-sense labels — clicks silently die. Pattern: allocate
  the clickable Response FIRST (`allocate_exact_size(..., Sense::click())`),
  paint content in `ui.new_child(UiBuilder::max_rect(rect))`, then
  `response.clicked()`. Regression-tested in sidebar.rs tests.
- Labels are text-selectable — they swallow clicks. Rows-as-buttons need
  `.selectable(false)` on every label inside.
- Icon images (`bytes://` URIs from fastframe-icons) need
  `egui_extras::install_image_loaders(ctx)` — without it every icon button
  renders as an empty/broken box (red triangles in screenshots).
- `Ui::scroll_to_cursor(Some(Align::BOTTOM))` and `stick_to_bottom` blank
  the whole scroll content on this stack (verified in our app; plain egui
  min-repro did NOT reproduce — interaction with our layout). The message
  list therefore renders oldest-first top-down and relies on
  stick_to_bottom being safe NOW (panel-order bug fixed) — retest after
  any egui upgrade.

### fastframe-emoji
- `EmojiPlugin::default()` does NOT install the bundled Noto font.
  `crate::emoji::setup()` (OnceLock, bundled BUNDLED) MUST run before
  `ctx.add_plugin(...)` and before any `fastframe_emoji::get().render()`.
  Symptom otherwise: ALL emoji (text + bars + chips) render as white
  monochrome glyphs.
- For bars/chips/raster: `theme::raster_emoji(cluster, px)` renders RGBA
  via the installed font (probe: TEAMSFAST_EMOJI_PROBE=1 logs all sizes).
  Plain `ui.label(emoji)` in Areas/Tooltips falls back to monochrome.

### egui 0.36, part 2 (learned the hard way this phase)
- ScrollArea content INHERITS the parent layout: a vertical ScrollArea
  inside a ui.horizontal row lays its children out HORIZONTALLY. Wrap the
  scroll body in ui.vertical (todo/notes nav columns both hit this).
- Widget-in-button: `Button::new` takes TEXT; putting an image in a
  button is `Button::image(img)` (or `.image_and_text`). Passing a
  widget that pre-adds itself to the Ui (like the old emoji_widget)
  paints it on the PARENT and leaves the button an empty chip — the
  hover reaction bar shipped like that for a release.
- Two ScrollAreas/Grids in one view MUST get unique `id_salt`/ids — the
  defaults clash and egui paints "Second use of scrollbar/Grid ID" error
  chips while corrupting the second widget's state.
- Adaptive Card payloads arrive as `<Swift b64="…"/>` (quote form) inside
  a URIObject, wrapped in `{"attachments":[{"content":{card}}]}` — see
  ui/cards.rs extract_card (handles `b64,` param form too).
- Connector text carries MARKDOWN the web client would render: links,
  `**bold**`, `_italic_` and Jira-wiki `*bold*` (this tenant's dominant
  shape — single asterisks, also around table cells). `split_markdown_links`
  + `scan_emphasis` (widgets.rs) handle all four with word-boundary rules
  (never intraword, no newlines, no `***`; bullets/arithmetic stay literal).
  7 unit tests + live-verified. CRITICAL: in card TextBlocks the pieces MUST
  render inside `ui.horizontal_wrapped` — one `ui.label` per piece in the
  vertical flow tears multi-piece text one fragment per line.
- `egui::Panel::left` has no `exact_width`; use `default_size` +
  `resizable(false)`.

### egui 0.36, part 3 (pixel-audit findings — do not regress)
- `Color32::from_rgba_unmultiplied(...)` tints in **Panel frames render as
  opaque dark slabs** on this stack (the light-theme ring banner was
  near-black). Never tint banners with alpha; blend with
  `crate::theme::tint_over(base, fg, alpha)` over `pal.panel` (dark/light
  ring/offline/call banners all use it).
- `egui::FontWeight` is GONE (0.36 variable fonts): bold painter text =
  `LayoutJob` + `sections[0].format.coords = VariationCoords::new([(b"wght", 700.0)])`
  via `ctx.fonts_mut`. `FontFamily::Name("Bold")` compiles but falls back
  to a non-bold font.
- `RichText::strong()` is a COLOR-ONLY flag in egui 0.36 ("stronger
  color") — there is no `TextStyle::Bold`, and on the Inter stack the
  color bump is invisible: it NEVER changes weight. Real bold =
  `theme::RichTextBold::bold()` (`.family(inter-bold)`, wght 700); every
  former `.strong()` site is `.bold()`. Render TESTS must build their
  Context via `theme::test_ctx()` — a stock `Context::default()` PANICS
  on the unbound `inter-bold` family the moment bold text draws.
- `centered_and_justified` centers HORIZONTALLY only — vertical is
  top-anchored. Center empty states with painter text (see body_note /
  the Ready-branch empty state), not layout.
- Hardcoded `Color32::from_rgb` in ui/ = light-theme breakage waiting to
  happen (30 sites cleaned in the audit). Any new color goes in the
  Palette; "one green for both themes" presence is 0x2f9e63.
- Section panels MUST draw `ui.separator()` BEFORE the empty/loading
  early-return, or empty sections lose the header rule (planner/shifts
  shipped like that).
- Hover action bar anchors with the MEASURED previous-frame bar rect
  (bar_h/bar_w); a hardcoded offset smaller than the real bar digs into
  the bubble's first text line. Bubbles clipped at the viewport top
  anchor tooltip-style above the pointer. TEAMSFAST_HOVER is exclusive
  with pointer hover (Xvfb's phantom pointer otherwise paints a second
  bar mid-shot).
- Hover-bar buttons DEAD for months (reply/forward/react/pin/edit/
  delete): two stacked bugs — the actions collected into a RefCell sink
  nothing drained, and egui's hit test is one frame behind a MOVING
  Area (the test bar re-anchors after the measured-height kick-in, so
  click tests need a settle frame first). `Order::Tooltip` ALLOWS
  interaction (the click-through theory was wrong); the Area is
  Foreground now for modal z-order sanity. Regression test:
  `hover_bar_buttons_push_actions`.
- `add_space` in a `ui.vertical` adds VERTICAL space — indenting needs a
  horizontal wrapper (OneNote section indent shipped as a no-op).
- `ui.with_layout(right_to_left(…))` DIRECTLY in a vertical ui corrupts the
  vertical cursor for everything after it: the measured content grows by
  ~its own height EVERY frame (the new-chat modal climbed +152px/frame
  until it was a full-height empty slab). Wrap the r2l row in
  `ui.horizontal(…)` — or make it the last element (the join dialog only
  ever survived because its r2l row is last). Probe-verified 2026-10-15.
- `scroll_area::State::offset` does NOT reflect the rendered position
  (egui animates a private `offset_target`; see the prepend race) —
  derive at-bottom/visibility signals from observable geometry (e.g.
  the last row's screen bottom vs the ScrollArea viewport), never from
  State::offset. Also: `stick_to_bottom` re-pins a programmatic offset
  drive on every content change — under live traffic SCROLLBENCH's
  sweep never moved the view until the bench disabled the stick.
- Auto-sized popup modals derive height from CONTENT, and a horizontal
  row cannot exceed the parent's available rect — an available_height
  cap on a ScrollArea inside such a row feeds the row's own height back
  into the scroll (the settings modal showed half its General tab below
  an invisible fold this way). Claim the row explicitly:
  allocate_exact_size + new_child(UiBuilder::max_rect).
- Hit-testing detail: a click-sense widget allocated AFTER a hover-sense
  one at overlapping rects wins (month pills over hover cells work), but
  egui hit-tests against the PREVIOUS frame's rects — a test that renders
  each frame in a FRESH `egui::Context` eats every click (the warm-up
  frame must share the Context with the click frame).
- Multiline composer (Enter sends; Shift+Tab/Shift+Enter newline):
  consume keys BEFORE `ui.add(TextEdit)` — a multiline TextEdit turns
  plain Enter into `\n` and Shift+Tab into a DE-INDENT natively.
  `InputState::consume_key` matches LOGICALLY (extra Shift ignored), so
  each combo needs an explicit `modifiers.matches_exact` gate or plain
  Enter swallows Shift+Enter. The field lives inside the ScrollArea's
  child Ui, so its id can't be derived before add: the composer stores
  `field.id` under the fixed context key `teamsfast_composer_field` and
  reads it back next frame. The break itself is spliced by hand
  (char-range replace + `TextEditState` cursor after the break).
  Regression: `composer_enter_sends_shift_tab_newline`. Key-event tests
  MUST send `Event::ModifiersChanged` first — `InputState.modifiers`
  only advances on that event, not on Key events' own modifiers field.
- Call timers: one epoch only — `call_connected_at` (first frame the
  phase watch reads connected). Banner and stage share it; counting from
  placement showed two different durations on screen at once.

### egui 0.36, part 4 (run-7 scroll/stick forensics)
- `stick_to_bottom` re-pins the offset EVERY frame while the (private)
  `scroll_stuck_to_end` flag is set — only REPEATED explicit
  `ui.scroll_to_rect` calls escape it (an explicit adjustment skips the
  end-of-frame re-pin and suppresses the stuck recompute). Raw
  `State.offset` writes oscillate against the re-pin forever (probe:
  offset alternating 0↔max per frame).
- NEVER write `st.offset.y = f32::MAX` pre-begin: begin does NOT clamp;
  `round_ui` overflows ±MAX to ∓inf, every row culls (blank frame), and
  the stick re-pin then computes NaN — `f32::max(NaN, 0.0) == 0.0` parks
  the view at the TOP of history with the stick lost permanently.
- `scroll_to_rect` centers the rect's CENTER; a landing check that
  centers the TOP never passes (permanent repaint busy loop — pixel-
  stable, so settlediff hides it).
- QA env hooks that arm at a wall-clock delay must pump their own
  repaints: a quiet chat produces no frames on the event-driven loop.
- `str::find` returns a BYTE offset — never feed it into a char cursor
  (teams-core: `` `éé` `` in a message panicked build_message_html and
  killed the backend thread; rt.block_on has no catch_unwind).
- sqlcipher CLI prints `ok` BEFORE result rows — `$( )` capture includes
  it (a seeded pin id became "ok\n<id>").

### egui 0.36, part 5 (run-9)
- `Ui::set_clip_rect` REPLACES the clip — inside a ScrollArea that
  DISCARDS the viewport bound. The reserved reaction strip did
  `set_clip_rect(anchor)`; rows above the viewport still render inside
  the 600px cull margin, so their group-meta timestamps (and chips)
  painted at their true positions up into the chat header AND the app
  top bar (bounded only by the central panel's clip) — the user's
  thrice-filed "timestamps appear in the title when scrolling". Two
  passes misdiagnosed it (run-7 shipped a sticky-day chip, run-9
  removed it): the leak is a per-frame transient only visible while a
  strip crosses/sits above the viewport top — static shots and coarse
  sweeps never framed it. Caught by md5-hashing the header band of
  EVERY frame of a 60fps SCROLLBENCH sweep on the tall Jira chat (two
  one-frame bands, timestamps legible at y=53 and y=27). Fix (9a9267d):
  `shrink_clip_rect(anchor)` = anchor ∩ viewport; regression test
  tessellates a strip fully above the viewport and asserts zero
  vertices above the clip (pre-fix: 38). Rule: inside a scroll, always
  SHRINK (intersect) — never SET a clip to a content rect.
- `TextEdit` WYSIWYG via `.layouter(FnMut(&Ui, &dyn TextBuffer, f32) ->
  Arc<Galley>)`: the job must cover EVERY character contiguously
  (caret/selection map by char range) — markers can be DIMED
  (gamma_multiply 0.45), never dropped. LayoutSection exposes
  `byte_range` (ByteIndex tuple `.0`), not text; VariationCoords reads
  via `.as_ref()`. TextFormat in 0.36 has `italics: bool` (synthetic
  oblique — real, pixel-measured ~6.5°), `strikethrough: Stroke`,
  `background: Color32`.
- `cargo test` does NOT rebuild `target/debug/teamsfast` (it builds
  test harnesses under deps/) — after a fix that only tests exercise,
  run `cargo build` before QA shots, or vision reports the OLD binary's
  behavior (the italic false-alarm: scanner fixed, shot stale).
- Two closures both needing `&mut ConvCtx` (image + custom-emoji
  callbacks into render_segments) fight the borrow checker; the
  callbacks run strictly sequentially, so a
  `RefCell<&mut ConvCtx>` borrowed per call at the call site is the
  sound pattern.
- fastframe-instance is LIVE (9a34f63): per-exe slot, "show" verb, QA
  bypass under TEAMSFAST_SIZE. `cargo run` beside the installed copy
  intentionally coexists (scoped) — that is not a bug report.

### Wayland window control (user's session is Wayland)
- winit 0.30 Wayland: `set_visible` is a NO-OP, `set_minimized(false)`
  warns "Unminimizing is ignored", `focus_window` is literally `{}` — an
  app cannot hide-by-unmap, restore, or raise itself. Hide-to-tray
  therefore MINIMIZES on Wayland (`App::hide_window`, `on_wayland` flag
  from `WAYLAND_DISPLAY`); X11 keeps real unmapping. Tray "show" sends
  Visible(true)+Minimized(false)+Focus: correct on X11, a no-op on
  Wayland — restoring is on the compositor (taskbar/overview). A real
  fix needs fastframe-tray waker retargeting + close/reopen-window
  hand-off; not done.
- `Action::LoadCalendarRange` + `Command::LoadCalendarRange` fetch
  arbitrary calendarView windows for week/month navigation; default
  calendar fetch is 35 days. Graph calendar dateTimes are NAIVE — parse
  wall components (`meeting_span`) or they shift by the local offset.

### Build env (this machine)
- SCREENSHARE (teams-core `screen` feature, PUBLISHED since 2a338ed):
  xdg-desktop-portal ScreenCast + PipeWire capture emitting I420
  VideoFrames; compiles, runtime unverified (interactive portal dialog).
  The app gates it behind the non-default `screenshare` cargo feature
  (`cargo build --features screenshare`) because the pipewire/libspa
  sys-crates need headers the release runners don't install — flip it to
  default only together with adding those to release.yml. Local build
  needs the pkg-config shims `~/.local/lib/teamfast-pc/`
  (libpipewire-0.3.pc + libspa-0.2.pc point at the GNOME SDK 50 headers,
  same pattern as the alsa shim) and the machine-local `[patch]` in
  ~/.cargo/config.toml. Clean pw teardown SHIPPED 2026-10-16 (teams-core
  5c63f74): a 150ms loop timer quits the pw main loop on stop() (works
  even when frames stop flowing) and the consumer thread joins on drop
  (1.5s budget, wedged stream falls back to the bounded leak).
- The call stack needs `alsa-sys` → alsa.pc. System has runtime
  libasound only; headers come from the Flatpak GNOME SDK via a local
  shim (`~/.local/lib/teamfast-pc/alsa.pc` + lib/libasound.so symlink to
  /usr/lib64) wired through `[env] PKG_CONFIG_PATH` in ~/.cargo/config.toml.
  After editing the .pc you MUST `cargo clean -p alsa-sys` — build scripts
  cache link paths and stale ones re-break the link.
- teams-core builds with `--features audio`; the app Cargo.toml already
  enables it on the `ost` dep.

### QA harness quirks
- Vision critics MISIDENTIFY/MISCOUNT small glyphs at 1× zoom (a
  "missing Copy button" and a "4-icon bar" were both wrong — high-zoom
  ffmpeg crops of the exact region settle it in one pass). Also: a QA
  SEED that mimics a rendering bug (a truncated table row) will fool
  every critic — keep seeds truthful.
- A soak app launched as a session background task DIES with the
  session (run-8 post-mortem: soak #5 reaped at session end; log
  evidence then clobbered by any later instance). Launch soaks under
  `setsid` (detached process group) with a sampler that exits when the
  app PID dies.
- Xvfb reports a 0mm display via XRandR → winit caps the window at
  1100x760 regardless of TEAMSFAST_SIZE. Shoot QA at 1100x760 for full
  paint; black bands at 1240x820 are the harness, never the app.
- Xvfb draws a phantom mouse pointer dead-center that can hold a hover
  bar open or tint one row — ignore in vision QA.
- TWO app instances share ~/.local/state/teamsfast/teamsfast.log: the
  newer launch TRUNCATES it and the older instance's append-writer then
  progressively overwrites the new lines (a running soak destroys QA
  log evidence and vice versa). Grep log evidence DURING the app's
  lifetime, and stop soaks before log-heavy QA sessions.
- Offline QA: `unshare -n` is blocked by the exec sandbox; use
  `bwrap --unshare-net --dev-bind / / ./target/debug/teamsfast`
  (keyring stays reachable — tokens load; the app must paint cached
  chats + the offline banner — Event::ChatsCached merge rule).
- Keyboard QA is impossible in this sandbox: XTEST fake events never
  materialize (verified with a raw XNextEvent listener), wtype is
  Wayland-only, xdotool/xwininfo are not installed. a11y/keyboard nav
  stays untested until tooling exists.
- egui `scroll_to_me`/`scroll_to_rect` queued mid-ScrollArea-content is
  consumed by the ScrollArea's NEXT `begin` — on the event-driven frame
  loop you MUST `ctx.request_repaint()` beside it or the scroll silently
  never applies (calendar week scroll-to-now, 0227aac).
- SQLCipher ad-hoc queries: the keyring secret IS the key string (64
  ASCII chars of hex text) — use it directly; re-hexing the bytes gives
  a 128-char "not a database" key. Message HTML lives in the `raw`
  column (`content` is the plain projection).

### Threading & crates
- `ses.call_stop` is a `CallSlot { stop, done }`: the `done` flag is set
  by a Drop guard living INSIDE the spawned call task (normal exit AND
  panic unwind), and `call_slot_free()` reaps finished slots — before
  run 3, only an explicit HangUp cleared the slot, so any call that ended
  on its own (remote hang-up, safety cap) wedged "a call is already in
  progress" forever.
- Backend = one thread with a current-thread tokio runtime;
  `worker(cmd_rx, tx)` must be driven by `rt.block_on` (a dropped async fn
  future = silent dead backend, "Starting…" forever).
- Commands channel: tokio unbounded, `rx.recv().await` (root task must
  yield so spawned tasks run). Events channel: std mpsc, drained per frame.
- Worker sync sections stall EVERYTHING (trouter included, same runtime).
  Perf pass 2026-10-16 proved it: archive saves were SQLCipher autocommit
  (one journal fsync per INSERT) = **560ms per 50-message save on real
  disk** — every open/refresh/send/react paused the worker that long, and
  the paint event sent AFTER the save ate the stall too. WAL +
  synchronous=NORMAL + one `unchecked_transaction` per batch = 0.5ms
  (~1000x). Rule: batched archive writes ONLY; benchmark on
  ~/.local/state, never /tmp (tmpfs hides fsync cost). Paint events also
  go out before any bookkeeping. UI side: `chat_mem` (12-chat RAM cache)
  paints re-opens instantly — `open_chat`/`refresh_chat` must never clear
  the visible pane, and live-activity refresh is `Command::RefreshChat`
  (history-only), never a full re-open. Hot render paths use
  `parse_html_cached`/`plain_cached` (per-frame re-parses of multi-KB
  message HTML were 2-3 parses/bubble/frame).
- Frame loop is EVENT-DRIVEN since the second perf pass (2026-10-16,
  40306ee): backend events wake the UI via the event-wake thread; the
  old unconditional 500ms repaint is GONE. Time-based logic must declare
  itself in the tick gate at the end of `App::ui` (call/ring/typing/
  offline/refresh/uploads) or it silently stops updating when idle.
  Exception: env-gated QA loops (TEAMSFAST_SCROLLBENCH) request their
  own repaints.
- Message-list row culling (offscreen rows → placeholder allocations
  sized by previous-frame height) is load-bearing: new per-row work must
  tolerate being SKIPPED for offscreen rows, and the day-label advance
  must stay OUTSIDE the cull (else separators repeat). CULL_MARGIN is
  600 (SCROLLBENCH: p95 frame build 1.84ms vs 2.82ms at 240 — fewer
  in-view height corrections for ~0.4ms median). Sidebar rows use
  `show_rows` (uniform 44px) — don't add variable-height content there.
- Message hover bars must not render while a modal overlay is open —
  they share egui's Foreground order with Modal and z-fight above it
  (`ConvCtx.overlays_open`; reachable via hover-message + Ctrl+N).
- The open-chat refresh debounce is 400ms (was 900) with a
  `refresh_in_flight` guard: bursts re-arm instead of queueing one page
  fetch per message on the serial worker. Cleared by the chat's
  Messages reply, worker errors, and a 5s self-heal (silent worker
  failure paths send no event).
- Serial network loops on the worker are the other classic stall:
  planner/notes/shifts/teams-channels fan-outs are concurrent
  (`join_all`, chunks of 8), the chat-list endpoint winner is sticky
  per process AND persisted on disk (teams-core 254e9c6,
  `~/.config/teams-cli/chats_endpoint` — cold starts no longer pay the
  two 401 probe round-trips, ~160ms on this tenant; a stale hint costs
  one failed RTT once, the full chain re-runs and rewrites it), roster
  names cache 5 min per chat. New multi-item fetches must not be
  `for { x().await }`.
- fastframe-tray `Tray::spawn` must run on a thread with NO ambient tokio
  runtime (ksni nested block_on panics) — see `spawn_tray` in app.rs;
  returns via mpsc with 3s timeout.
- fastframe-log `drain_wait` is a blocking Condvar — only on
  `tokio::task::spawn_blocking`, never on the runtime (starved the whole
  backend once: "Go live" froze everything).
- `fastframe_emoji::get()` must be called after `emoji::setup()`.

### Protocol / ost
- CUSTOM EMOJI (run-9, live-verified): reacting WITH tenant custom
  emoji works via Graph BETA setReaction/unsetReaction
  `{"reactionType":"custom","displayName":name}` (teams-core
  send_custom_reaction_with_client) — v1.0 answers the SAME request 204
  but SILENTLY DROPS the custom fields (no reaction lands; its own 400
  names displayName/reactionId, so the validator knows them — the
  deserializer doesn't). The native read then shows the emotion key
  `"{displayName};{object_id}"` where object_id is the same `0-csea-…`
  id a CustomEmoji img's `itemid` carries. The tenant DIRECTORY is
  capture-gated: Graph /beta/teamwork/messaging/customEmojis 403s
  (needs TeamworkCustomEmoji.Read, absent from the Teams web-client
  token) and native directory guesses 404 — the picker's Custom tab
  runs off a seen-index (message imgs + received custom reactions;
  ui/widgets.rs custom_emoji_seen). CustomEmoji ASSETS (asyncgw
  imgt2_anim) 401 anonymously — they ride the skype-token media path.
  Edit round-trip: build_message_html passes the exact safe CustomEmoji
  img tag through verbatim (strict itemtype/https/charset gate).
- REACTIONS (run-8, live-verified): the native chat-service
  `/v1/users/ME/conversations/{c}/messages/{m}/reactions` 404s for EVERY
  type (better-teams' vendored ost carries the same unverified guess —
  their ledger admits it). The working write path is Graph
  `POST /v1.0/chats/{enc}/messages/{id}/setReaction` /
  `unsetReaction` with `{"reactionType": EMOJI}` — the UNICODE EMOJI,
  not the word (ASCII 'like' → 400). Arbitrary unicode is accepted and
  stored under codepoint-name emotions keys ("1f389_partypopper"); the
  classic six keep word keys. Graph GET of a single message is
  scope-blocked (ChatMessage.Send without Chat.Read) — verify via the
  native read. teams-core 276fa6d.
- parse_html (2026-10-16 run 3): `<br>` flushes text AND pushes
  LineBreak; block opens (`<p>`/`<div>`) FLUSH before pushing their
  LineBreak. Both were broken from day one (br = flush-only with an
  unreachable LineBreak arm; block opens never flushed → `<p>a</p><p>b</p>`
  concatenated to "ab"). `strip_tags` emits a boundary space for BLOCK
  tags only (quote payloads run `</strong><p>` — plain concat fused
  "NameAnd…"). Pipe tables (Jira `||h||`/`|c|`) render as striped grids
  in chat text and card TextBlocks (`split_pipe_tables`/`parse_pipe_run`/
  `render_pipe_table`, widgets.rs); chat rows arrive `<br>`-separated and
  are grouped by a lookahead in render_segments.
- Multi-line sends go out with `<br>` (teams-core `prose_html`, 2026-10-16):
  a literal `\n` inside `<p>` is whitespace per HTML — real clients (and
  our own block-noise collapsing) flatten it to a space. `<pre>` interiors
  keep raw newlines; edit and quote-reply share the same body builder.
- Chat sends to **topic-type channels** (`threadtype: "topic"`, e.g.
  literal:#alerts) return 2xx but the message never lands. conv::send now
  verifies via clientmessageid and surfaces a clear error. Proper topic
  posting = unsolved upstream question.
- `@unq.gbl.spaces` chats: ost's chat-LIST name resolver fails on them
  ("[Direct message]" placeholder) but the roster works — our open_chat
  resolves names via `list_chat_members_data` (also fixes "?" senders).
  ROOT CAUSE (fixed 2026-10-08, upstream me): 1:1 rosters often carry a
  BOT member (Cortana quick-reply, `28:…`) — a resolver that requires
  "exactly one non-self member" bails and falls back to the last-message
  sender, showing the USER'S OWN NAME when they spoke last. Fix:
  filter `28:` MRIs (`resolve_mate_name`); open_chat also heals the
  cached chat list when the roster resolves a different name.
- Read state: the server `consumptionhorizon` (ChatInfo.last_read_ms,
  from the conversations-list properties/threadProperties) is authoritative
  for "read on another client" — unread = last message > max(local
  horizon, server horizon). Horizons are not persisted in the archive
  cache; offline falls back to local-only (pre-2026-10-08 behavior).
- Theme presets from fastframe contain an `outline` color — Palette::set
  must map it or every shared preset warns "unknown color".
- Own-message detection needs `self_id` (Graph /me `id`, bare GUID) —
  wired via `Event::SelfId`; fallback decodes `oid` from the cached Graph
  JWT (base64 padding-robust).

## Local archive (persistence)

- SQLCipher DB `~/.local/state/teamsfast/archive.db`; key lives in the **OS
  keyring** (service `teamsfast`, user `archive-key`) since 2026-10-16 —
  migrated from `archive.key` on first open after a VERIFIED keyring
  readback, file removed only then. Keyring unavailable → the file stays the
  source (fresh installs fall back to it too). Edge: a locked daemon on a
  migrated profile = archive unavailable that session (DB unharmed; network
  still syncs). Ad-hoc DB queries read the key via secret-tool/python
  secretstorage — the key file may no longer exist.
- Written after every successful fetch (chats, newest 50 per open chat,
  refetches); loaded at worker start (instant cached chat list) and on
  open_chat (cached history paints before the network reply).
- Encrypted verified (file header is random bytes). Offline: cached chats
  + conversations render; `load_older` exists for future offline paging.

## Tokens & security

- ost config (tokens) now lives in the **OS keyring**
  (`~/.config/teams-cli/config.toml` is auto-migrated to the keyring and
  deleted on first load — verified live). `delete_for` also wipes the
  keyring copy. If the keyring is unavailable the plaintext file stays as
  fallback (never lose tokens over a missing daemon).
- JWT `oid` fallback decodes the cached Graph token when whoami fails.

## Offline mode

- `CheckReady` failure + cached content ⇒ `Event::Offline` (NOT
  NeedLogin). App stays Ready, shows the warning banner (⚠ + "Retry now"),
  retries every 20 s — network return auto-recovers to Connected.
- No cached content ⇒ `Event::NeedLogin` as before.

## Logging

- Per-run file `~/.local/state/teamsfast/teamsfast.log` (truncated at
  launch) + `panic.log`; dual tracing_subscriber layers (terminal + file,
  EnvFilter both). Default `warn,teamsfast=debug,ost=debug`; `RUST_LOG`
  overrides. `--logs` prints the path. qa_shot.sh copies it to
  `/tmp/qa_last_run.log`.

## E2E call rig (2026-10-19, live selftest PASS)

- **Profiles**: `TEAMSFAST_PROFILE=<id>` selects teams-core's keyring/
  config slot (`tokens-<id>`), suffixes the archive keyring entry and
  the single-instance slot (`teamsfast-<id>`); default identifiers are
  byte-identical to before. Files split via the launcher's per-profile
  `XDG_{CONFIG,STATE,DATA}_HOME` (see qa_call_e2e.sh `launch()`). A
  profiled run CLAIMS its slot even with TEAMSFAST_SIZE set (cannot
  collide with the default instance; `--ctl` needs the claim).
- **Control channel**: `teamsfast --ctl <verb>` (same profile + binary)
  → claim handler → queue → `App::ui` drains each frame. Verbs: `ping`,
  `call <chat_id>`, `videocall <chat_id>`, `calluser <upn>` (CreateOneToOne
  then call on CreateChatOk), `echo`, `answer`, `decline`, `hangup`,
  `mute on|off`. `--ctl` takes the WHOLE rest of the line as one verb.
  `CALLQA` log lines mark verbs/actions/transitions (ringing, caller_
  gone, connected, ended+summary, failed) — metadata only, no content.
- `./qa_call_e2e.sh selftest` — one signed-in instance (default profile,
  REFUSES if an instance is already running), echo-call E2E, asserts
  from the redirected log under `/tmp/tf-<profile>/st/…` (NOT the
  user's ~/.local/state log — every harness profile redirects XDG).
  `login <profile>` — device-code provisioning shot. `pair <aP> <aUPN>
  <bP> <bUPN>` — full A→B ring/answer/hold/mute/hangup flow.
- Echo-service note: `echo` connects and flows RTP (1470 sent / 769
  received over a 45s hold) but never rings or sends video — the
  A↔B pair is the only way to exercise ringing/accept/remote-hangup.

## Testing / verification workflow

- `cargo build && cargo test` (17 tests: click injection, live_parse on
  real production shapes, archive round-trips, formatters).
- `./test.sh` — user-facing manual-test launcher (preserves the session
  log to `last-session.log`).
- `scripts/bench.sh [runs]` — paired idle-RAM/first-paint benchmark vs
  teams-for-linux (zapfast methodology; results in docs/benchmarks.md;
  needs target/release + the extracted AppImage at /tmp/tfl-bench).
- `./qa_shot.sh OUT.png [SIZE] [CHAT_ID]` — headless Xvfb screenshot;
  copy of the app log lands in `/tmp/qa_last_run.log`. Send to the vision
  agent for pixel QA (this loop caught every layout bug so far).
- Layout forensics: `TEAMSFAST_LAYOUT_DEBUG=1` prints own-bubble rects.
- `TEAMSFAST_SCROLLBENCH=1` (with a chat open) drives a synthetic
  150px/frame sweep through the conversation after 300 settle frames
  and logs `SCROLLBENCH frame build: <us>` lines — per-frame layout
  cost without a human scrolling. It self-repaints (env-gated).
- `TEAMSFAST_RING=1` renders the ringing banner with a synthetic caller
  (pixel QA for incoming calls; Accept posts to dead links harmlessly).
- `TEAMSFAST_TYPING="Name"` forces the typing indicator in the chat header
  (pair with TEAMSFAST_OPEN); `TEAMSFAST_ACTIVITY=1` seeds one synthetic
  activity-feed row (pair with TEAMSFAST_VIEW=activity) — the live feed
  only fills from unread/mentions, which headless runs usually lack.
  `TEAMSFAST_DRAFT="a|b"` seeds the composer draft (`|` = newline) for
  multi-line-composer QA (nothing sends).
- `TEAMSFAST_PICKER_TAB=<n>` presets the reaction picker's tab (Custom=4)
  — pair with TEAMSFAST_PICKER (tab clicks are impossible headless).
- `TEAMSFAST_TABLE=1` (pair with TEAMSFAST_OPEN) appends synthetic
  messages after the chat loads — a chat-HTML pipe table and a card with
  the real k8s-scan Jira payload; re-seeds after network full-replaces.
- `TEAMSFAST_CALDUMP=1` logs every calendar meeting's date/start/dur/
  subject/canceled after fetch (lane/size forensics without a debugger).
- Light-theme QA: the settings file is `~/.config/teamsfast/settings.json`
  (NOT the state dir) and egui persists visuals in
  `~/.local/share/teamsfast/app.ron` — `rm -f` it or the old visuals
  override the saved theme.
- `TEAMSFAST_VIDEOCALL=1` + `TEAMSFAST_HANGUP_AFTER=<secs>` place a
  camera-on echo call that hangs up on a timer — bounded headless call
  QA; the run's stdout carries the call summary
  (`video_packets_sent=…`, `echo_detected=…`). NOTE: HANGUP_AFTER counts
  from LAUNCH, not call-connect — slow logins eat the window (bump it).
- `TEAMSFAST_SCREENSHARE=1` swaps the Share button onto a synthetic
  1280x720 animated-bar source AND auto-starts it on video calls — full
  share path (slot swap, encoder re-init, preview tee) headlessly; pair
  with VIDEOCALL. The real portal path needs `--features screenshare`
  and an interactive session (portal picker dialog).
- Screenshot gotchas: always `env -u WAYLAND_DISPLAY DISPLAY=:N`; kill
  Xvfb/app by PID (pkill -f patterns self-match the bash command line).
  A phantom pointer in Xvfb can hold one hover bar open — ignore in QA
  (it also makes plain/HOVER conversation shots byte-identical).

## UI polish (2026-10-08 pass, vision-verified)

Shipped: code-chip/code-block message rendering (extend-mode chips,
blocks for multiline/long code), span-glue previews, full-width
banners with right-aligned actions, pulsing ring banner, call-stage
rework (framed tiles, camera placeholder, bottom-right controls),
stronger dark danger color, egui::Modal dialogs (centered + scrim +
Esc/backdrop close, Settings ✕), readable disabled buttons, settings
active-tab fix, centered painter-based loading/empty body notes
(body_note — ScrollAreas collapse available rects, never center with
layouts inside them), Teams channel indent, Meet-now primary button,
Files metadata separators, Activity Clear guard, rail icon semantics.

## UI polish (2026-10-14 audit round, 4-critic + pixel verify)

15-surface battery → 4 parallel vision critics (native-Teams rubric) →
pixel/code triage (scrim "missing" claim was the KNOWN false positive;
the phantom pointer opens a hover bar in unhooked shots) → fixes:
- Settings: custom SQUARE checkboxes (stock `ui.checkbox` inherits the
  global widget radius 8 → its ~14px box renders as a CIRCLE = radio
  semantics for independent booleans); segmented pill radius 5.
- Calendar: 48px hours (36 crushed 30-min meetings), time RANGES not
  "HH:MM, D Mon" in blocks/pills, char-boundary-safe ellipsize
  (painter text has no truncation; byte-slicing PANICS on multi-byte
  chars — en-dash took down week/month), lane-aware column min-width
  (concurrent meetings were one-letter slivers), now-line spans the
  grid + dot, month cells stretch + today = accent disc on the numeral
  (full-cell outline read as selection), out-of-month pills dim,
  canceled meetings dim/strike/no-Join (Graph `status:cancelled`
  parsed; the tenant really returns them).
- Files: per-type COLORED icons (palette-sourced tints), always
  two-line rows, hover-revealed actions (24 always-on outlined buttons
  was the loudest chrome in the app).
- Ring banner: vertical inset (buttons spanned the band), two-line
  identity, matched Accept/Decline (dark-theme pale green darkened
  0.72 + white text).
- Chat: unread dot never re-badges the OPEN chat from stale server
  horizons; timestamp+seen-by under the LAST bubble of a group; sender
  names secondary gray (accent is for mentions/links only); mention
  chips get their own spaces in plain previews.
- Teams view: body-level loading/empty state (serial ~1s/team fetch
  showed a bare void; the inline spinner arc read as a stray ")").
- Modal X buttons: stroke NONE (transparent fill alone still draws the
  inactive bg_stroke — same ghost trap as the top bar).
- Light theme: active rail tile wash strengthened (0x50 → 0x86 alpha;
  glyph contrast was ~2.8:1).
- Hover bar: never extends into the avatar gutter on incoming bubbles.
- To Do: matched ghost header buttons, Add disabled until text.
- Empty state: ghost icon above "Select a conversation".
Deferred (P3, accepted): calendar List nav chevrons, group-chat glyph
ragged column, search field recessed fill, dialog shadow depth,
Teams per-channel unread dots (needs data), checkbox→toggle switch.

## Known gaps / deferred

- **Incoming-call ringing hardening** — the accept/decline banner ships
  (auto-answer disabled via `TEAMS_MANUAL_CALLS=1`; audio answer path
  shared with the CLI's). Still capture-gated: caller-cancel pushes and
  the accepted-call remote hang-up shape were never seen live (MS rejects
  self-adds: add returns 202 but no invitation fans out — verified), so
  `Event::CallGone` only clears a RINGING banner and the ringing banner
  self-expires after 45 s; an accepted call still ends via Hang-up only.
  Video invitations are answered audio-only. To capture a real payload:
  run the app with `RUST_LOG=ost=debug`, receive any call, grep the log
  for `NGCallManagerWin` frames (`teams-core/examples/self_ring.rs`
  documents the fan-out experiment).
- **Video/screenshare** — camera send (chat-header video button) +
  call-stage panel (remote tile, local preview, mute/camera toggles,
  hang up) + **SCREENSHARE SHIPPED (2026-10-14)**: stage Share button
  (video calls — the share rides the existing video leg, no
  renegotiation), portal ScreenCast via `--features screenshare`, and a
  synthetic QA source. teams-core: `ScreenShareSlot` on the controls
  pair, sticky source preference in the send loop (hold last frame
  between capture ticks — else the encoder flaps 720p↔240p), encoder
  re-init on resolution change (area-scaled bitrate, 256kbps camera →
  3.5Mbps cap), screen frames capped to 1920x1080. Live-verified on an
  echo call (single 1280x720@3072kbps encoder init, bars in the preview
  tile, vision 5/5). Remaining: interactive portal test against a REAL
  peer (user-gated), remote-decode verification (echo sends no video),
  true modality renegotiation (toggles are soft gates), screenshare on
  MEETING joins (join currently places audio-only legs).
- **Adaptive Cards** — basic renderer (TextBlock/FactSet/Image/OpenUrl,
  Submit actions shown disabled). Interactive submits (Input.Text +
  Action.Submit) are CAPTURE-GATED, not mere TODO: the submit wire
  format was never captured, and better-teams ALSO punts (cards with
  inputs get fallback rows there). Do not guess a payload — capture a
  real submit from the web client first.
- **Meeting lobby** — joining a lobby-gated meeting places the leg; the
  lobby UI shipped (joining → lobby → admitted, 12s grace heuristic);
  verify the admit signal against a real lobby-gated meeting.
- **Topic-channel sends** still fail at MS's side — honest error surfaced.
- **Fastframe shell** — single-instance/autostart/self-update
  (fastframe-instance/update/shell crates not yet wired).
- **Archive key in OS keyring** (currently a 0600 file).
- **Multi-account**, i18n; in-app message search is wired but
  tenant-blocked (Graph 403) for now.
- Mute state is mirrored locally (server-side flag not exposed by
  list_chats_data, so it resets to un-muted on a fresh profile).

## Roadmap (agreed priority)

1. Video/screenshare (the last call modality).

### Release + self-update (live since v0.1.0)

- **Release policy (user decision, 2026-10-07): builds run ONLY on
  release.** There is no CI workflow — tags `v*` trigger
  `.github/workflows/release.yml`: x86_64 + aarch64
  tarballs (portable marker inside), attested, `checksums.txt` signed
  with the Ed25519 key whose public half is
  `assets/update-public-key.hex`; the private key lives in the
  `TEAMSFAST_UPDATE_SIGNING_KEY` repo secret (backup copy:
  `redacted-see-private-runbook` — LOSING it breaks
  update verification for shipped installs; keep a copy off-machine).
  **NEVER tag or release autonomously**: the user tests manually until
  the product is feature-complete and polished; every release lands on
  their install via self-update, so cut one only on an explicit
  "release vX" request. Verify with `cargo build && cargo test` locally
  before tagging — there is no CI safety net on main.
- Releases REQUIRE `packaging/release-notes/v<ver>.md` committed first.
- The updater is `src/updates.rs` + the worker in app.rs; installation
  detection needs the marker beside the binary (dev builds from cargo
  correctly refuse). E2E-verified: 0.1.0 portable → signed 0.1.1 →
  helper swap → receipt → no rollback.
- `TEAMSFAST_AUTOUPDATE=1` restarts without the button (headless QA).
2. Meeting lobby UI (LobbyState is in teams-core).
3. User manual pass → fix findings same-session.
4. Benchmarks vs teams-for-linux (zapfast methodology, publish numbers).
5. Upstream: ost issues (roster naming for @unq.gbl.spaces, topic-channel
   send) + push our presence-with-client / login_with_code_sink /
   run_call_with_stop / planner-with-client patches; ping better-teams
   author (mrowlinson).
6. Fastframe shell (single-instance, autostart, self-update), archive key
   in keyring, multi-account.

## Ecosystem notes

- `eisbaw/ost` upstream is dead (8 months, 104 unanswered PRs from
  mrowlinson). The living code is better-teams' vendored ost + patches;
  our teams-core tracks that. mrowlinson is the de-facto maintainer —
  engage him before big forks drift.
- better-teams (macOS, SwiftUI) proves the same core drives a polished
  app; its OSTMAC-PATCHES.md documents all 92 patches we carry.
- Licensing: teams-core tree = MIT (eisbaw's LICENSE kept); better-teams'
  own code (Swift/ostmac-core) is MIT-with-Microsoft-exclusion and must
  NOT be copied.

## Conventions

- Views never touch protocol types: parse in backend/model, render in ui/.
- Actions (ui → App::apply) mutate state then send Commands; raw commands
  that need App state (like OpenChat) must go through Actions.
- No vendoring of egui/epaint/fastframe crates — fix upstream.
- Privacy: never log message contents, tokens, or key material; fastframe
  redaction is available when we wire fastframe-log.
- Commit after every verified fix; screenshot+vision for any visual
  change.
