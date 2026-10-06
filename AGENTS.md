# TeamsFast — agent guide

A native Microsoft Teams client for Linux: Rust + egui, no browser engine.
Built on the **fastframe** stack and on **teams-core** — our maintained fork
of the patched `ost` protocol library. Sibling-in-spirit of zapfast and
spotifast (same architecture, same fastframe crates). Read `FEASIBILITY.md`
for the full investigation and `README.md` for the user-facing summary.

**Status: Phase 0/1 — daily-driver chat works end-to-end on a real
account.** Current version renders a polished chat UI (verified via
headless screenshot + vision QA rounds): sidebar, conversations with rich
content, media, search, teams/channels, live push, notifications, themes,
tray, local encrypted archive, keyring tokens, offline mode. Calls are
Phase 2 (not started).

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
- `src/ui/` — sidebar.rs (rows = allocate-first whole buttons),
  conversation.rs (bubbles, hover bar, composer), panels.rs (search,
  new-chat), widgets.rs (Teams-HTML renderer, avatars, day separator,
  icon_button), media.rs (lightbox).
- `src/theme.rs` — Palette (17 named colors, dark/light bases, derive
  rules), Catalog/Omarchy bridge, Settings load/save, emoji raster cache.
- `src/emoji.rs` — bundled Noto Color Emoji setup (MUST run before
  plugin/raster; see gotchas).
- `src/tray.rs` — tray icon (procedural SVG-free icon) + menu.
- `teams-core/` — the protocol crate (local git checkout, wired via
  `[patch]` in Cargo.toml; delete the patch block to build against the
  pushed `https://github.com/YacineSahli/teams-core`). Upstream chain:
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

- **Calls (Phase 2)** — teams-core already has working Linux 1:1 audio
  (ICE/TURN/SRTP/Opus) + WIP video; UI + PipeWire QA needed. Meeting join
  after that.
- **Connector cards** — rendered as a muted "Connector card" placeholder;
  Adaptive Card JSON (`Swift b64` in URIObject) parsing is future work.
- **Unread badges** — needs consumption-horizon state cached locally.
- **Fastframe shell** — close-to-tray/single-instance/autostart/
  self-update (fastframe-instance/update/shell crates not yet wired).
- **Archive key in OS keyring** (currently a 0600 file).
- **GUI device-code login** (ost prints the code to the terminal; needs
  pub surface or reimplemented oauth flow).
- **Channel send to topic channels** fails silently at MS's side — we
  surface an honest error; proper topic posting unresolved.
- Multi-account, i18n, light-theme QA, message search inside the app UI
  is wired but tenant-blocked (Graph 403) for now.

## Roadmap (agreed priority)

1. User manual pass → fix findings same-session.
2. Notification click → focus chat; unread badges (needs local
   consumption-horizon).
3. GUI login + keyring/`--logs` polish.
4. Benchmarks vs teams-for-linux (zapfast methodology, publish numbers).
5. Upstream: ost issue (roster naming for @unq.gbl.spaces, topic-channel
   send, device-code surfacing) + ping better-teams author (mrowlinson).
6. Phase 2: 1:1 audio calls → meeting join → video/screenshare.

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
