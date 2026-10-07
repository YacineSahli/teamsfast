# TeamsFast — agent guide

A native Microsoft Teams client for Linux: Rust + egui, no browser engine.
Built on the **fastframe** stack and on **teams-core** — our maintained fork
of the patched `ost` protocol library. Sibling-in-spirit of zapfast and
spotifast (same architecture, same fastframe crates). Read `FEASIBILITY.md`
for the full investigation and `README.md` for the user-facing summary.

**Status: Phase 2 in progress — chat client with sections, calls AND
meeting join.** Chat (send/reply/edit/delete/react/forward/pins,
pending+retry bubbles, seen-by, unread badges, pinned/muted chats,
per-chat notification levels, contact cards, ghost mode), Adaptive Cards,
section rail (Chat/Teams+mgmt incl. channel rename-delete/Calendar+join/
Files incl. per-chat shared files/ToDo/Planner/Shifts/OneNote/Activity),
quiet hours, Meet now,
presence with status menu, full Settings window, GUI device-code sign-in,
notification click→chat, 1:1 audio calls + MEETING JOIN (verified live:
epconv → call_accepted=true → Call active). Offline/local search merged
with Graph. Video/screenshare is the remaining Phase 2 item.

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
- `teams-core/` — the protocol crate (local git checkout for development;
  the build pulls the published copy from
  `https://github.com/YacineSahli/teams-core` — clone the sibling and add
  `[patch."https://github.com/YacineSahli/teams-core"] teams-cli = { path = "teams-core" }`
  to build against local changes). Upstream chain:
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
- Two ScrollAreas/Grids in one view MUST get unique `id_salt`/ids — the
  defaults clash and egui paints "Second use of scrollbar/Grid ID" error
  chips while corrupting the second widget's state.
- Adaptive Card payloads arrive as `<Swift b64="…"/>` (quote form) inside
  a URIObject, wrapped in `{"attachments":[{"content":{card}}]}` — see
  ui/cards.rs extract_card (handles `b64,` param form too).
- `egui::Panel::left` has no `exact_width`; use `default_size` +
  `resizable(false)`.

### Build env (this machine)
- The call stack needs `alsa-sys` → alsa.pc. System has runtime
  libasound only; headers come from the Flatpak GNOME SDK via a local
  shim (`~/.local/lib/teamfast-pc/alsa.pc` + lib/libasound.so symlink to
  /usr/lib64) wired through `[env] PKG_CONFIG_PATH` in ~/.cargo/config.toml.
  After editing the .pc you MUST `cargo clean -p alsa-sys` — build scripts
  cache link paths and stale ones re-break the link.
- teams-core builds with `--features audio`; the app Cargo.toml already
  enables it on the `ost` dep.

### QA harness quirks
- Xvfb reports a 0mm display via XRandR → winit caps the window at
  1100x760 regardless of TEAMSFAST_SIZE. Shoot QA at 1100x760 for full
  paint; black bands at 1240x820 are the harness, never the app.
- Xvfb draws a phantom mouse pointer dead-center that can hold a hover
  bar open or tint one row — ignore in vision QA.

### Threading & crates
- Backend = one thread with a current-thread tokio runtime;
  `worker(cmd_rx, tx)` must be driven by `rt.block_on` (a dropped async fn
  future = silent dead backend, "Starting…" forever).
- Commands channel: tokio unbounded, `rx.recv().await` (root task must
  yield so spawned tasks run). Events channel: std mpsc, drained per frame.
- fastframe-tray `Tray::spawn` must run on a thread with NO ambient tokio
  runtime (ksni nested block_on panics) — see `spawn_tray` in app.rs;
  returns via mpsc with 3s timeout.
- fastframe-log `drain_wait` is a blocking Condvar — only on
  `tokio::task::spawn_blocking`, never on the runtime (starved the whole
  backend once: "Go live" froze everything).
- `fastframe_emoji::get()` must be called after `emoji::setup()`.

### Protocol / ost
- Chat sends to **topic-type channels** (`threadtype: "topic"`, e.g.
  #Alerting) return 2xx but the message never lands. conv::send now
  verifies via clientmessageid and surfaces a clear error. Proper topic
  posting = unsolved upstream question.
- `@unq.gbl.spaces` chats: ost's chat-LIST name resolver fails on them
  ("[Direct message]" placeholder) but the roster works — our open_chat
  resolves names via `list_chat_members_data` (also fixes "?" senders).
  Same fix should be upstreamed to ost's resolver.
- Theme presets from fastframe contain an `outline` color — Palette::set
  must map it or every shared preset warns "unknown color".
- Own-message detection needs `self_id` (Graph /me `id`, bare GUID) —
  wired via `Event::SelfId`; fallback decodes `oid` from the cached Graph
  JWT (base64 padding-robust).

## Local archive (persistence)

- SQLCipher DB `~/.local/state/teamsfast/archive.db`; key in `archive.key`
  (0600, random hex). Keyring upgrade for the archive key = future work.
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

## Testing / verification workflow

- `cargo build && cargo test` (17 tests: click injection, live_parse on
  real production shapes, archive round-trips, formatters).
- `./test.sh` — user-facing manual-test launcher (preserves the session
  log to `last-session.log`).
- `./qa_shot.sh OUT.png [SIZE] [CHAT_ID]` — headless Xvfb screenshot;
  copy of the app log lands in `/tmp/qa_last_run.log`. Send to the vision
  agent for pixel QA (this loop caught every layout bug so far).
- Layout forensics: `TEAMSFAST_LAYOUT_DEBUG=1` prints own-bubble rects.
- Screenshot gotchas: always `env -u WAYLAND_DISPLAY DISPLAY=:N`; kill
  Xvfb/app by PID (pkill -f patterns self-match the bash command line).
  A phantom pointer in Xvfb can hold one hover bar open — ignore in QA.

## Known gaps / deferred

- **Video/screenshare** — the last Phase 2 item; audio 1:1 + meeting join
  work (signaling verified live; this sandbox blocks TURN UDP, so
  full-duplex audio needs a real network to hear).
- **Meeting lobby** — joining a lobby-gated meeting places the leg; an
  explicit lobby/admit UI is future work (teams-core has LobbyState).
- **Adaptive Cards** — basic renderer (TextBlock/FactSet/Image/OpenUrl,
  Submit actions shown disabled); Input.Text and interactive submits are
  future work.
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
