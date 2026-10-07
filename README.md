# TeamsFast

**Microsoft Teams, native and fast.** A Linux Teams client in Rust with
[egui](https://github.com/emilk/egui), built on the protocol core extracted
into [`teams-core`](https://github.com/YacineSahli/teams-core) (the patched
`ost` library: OAuth → Skype token, chat-service REST, Trouter websocket,
ICE/TURN/RTP/SRTP calling). No browser engine.

## What works today

- **Sign-in** — Microsoft device-code flow entirely in the app (code + link
  panel with Open/Copy); work/school and personal accounts
- **Chat** — list (live-sorted, pinned/muted chats, unread badges,
  per-chat notification levels, ghost mode for read receipts), history with paging, send (with pending +
  failed-retry bubbles), reply, edit, delete, reactions, "Seen by"
  receipts, typing indicators, forward, locally pinned messages,
  jump-to-latest, contact cards (click a sender), roster-resolved names
- **Rich messages** — Teams HTML subset (bold/italic/links/code/mentions/
  quotes), inline images with lightbox, file attachments (upload with
  progress, download & open), **Adaptive Cards** (Opsgenie/bot cards render
  with facts and actions), colour emoji
- **Live** — Trouter push: incoming messages, notifications (click opens
  the chat), typing; desktop notifications with per-chat levels, mute and
  quiet hours
- **Sections** — icon rail: Chat, Teams & channels (create/rename/delete
  channels, join public teams, create teams), **Calendar** (week of
  meetings with join links), **Files** (OneDrive recents + each
  conversation's shared files, download/open), **To Do** (lists,
  complete/reopen, quick add), **Planner** (boards, buckets, tick tasks),
  **Shifts** (this week's schedule), **OneNote** (browse notebooks, read
  pages, append notes), **Activity** feed (mentions and messages)
- **Presence** — own status with a set-status menu (Available/BRB/Busy/
  DND/Away/Offline)
- **Calls & meetings** — 1:1 audio calls from any 1:1 chat header, call
  banner with timer + hang-up, **meeting join** (Join buttons on calendar
  meetings and a paste-a-link dialog: join URL, thread ID or meeting ID),
  **Meet now** (instant meeting, created and joined in one click), and an
  echo-bot test call in Settings
- **Settings** — full window (General/Appearance/Notifications/Account/
  Storage/About): theme picker + zoom, notification rules, start-in-tray,
  close-to-tray, sign-out (keyring wipe), clear local archive
- **Search** — message search online (Graph) merged with a local index of
  your archive, so search also works offline or on tenants that block it
- **New conversations** — 1:1 by email/UPN, group chats with topic
- **Tray icon** — show/hide window, quit

Also: local encrypted archive (SQLCipher), keyring tokens, offline mode,
user-editable JSON themes.

Video/screenshare is the next milestone.

Agent/architecture hand-off notes live in `AGENTS.md`.

## Install

Grab a portable build from [the releases](https://github.com/YacineSahli/teamsfast/releases)
— unpack it and run the binary inside. The release carries an update
signature: a portable install keeps itself current (checks once a day,
downloads in the background, swaps on restart, and rolls back
automatically if the new version fails to start).

```sh
tar xzf teamsfast-v*-x86_64-unknown-linux-gnu.tar.gz
cd teamsfast-v*-x86_64-unknown-linux-gnu
./teamsfast
```

For a system-wide-feeling install, drop the binary and
`teamsfast-portable.txt` anywhere on your `PATH` (e.g.
`~/.local/opt/teamsfast/`) and symlink it.

## Run from source

```sh
cargo run
```

Build needs Rust 1.85+ and `alsa-lib-devel` (the calling stack links ALSA
through cpal); the vendored OpenSSL also wants `perl` + `make`.

Signing in uses Microsoft's device-code flow: click **Sign in**, then open a
browser at the URL printed in the terminal that launched teamsfast and enter
the code. Tokens are cached by the ost core in `~/.config/teams-cli/`.

## Layout

- `src/backend/` — tokio worker thread; owns the `ost` client, talks to the
  UI only through `Command`/`Event` channels (zapfast architecture).
  `mod.rs` dispatch; `conv.rs` conversations; `directory.rs` search/teams/
  new chats; `media.rs` images/files; `live.rs`+`live_parse.rs` Trouter
  pushes (with unit tests); `headless.rs` debug helpers.
- `src/ui/` — egui views; never touch protocol types. `widgets.rs` (Teams
  HTML renderer, avatars), `sidebar.rs`, `conversation.rs`, `panels.rs`,
  `media.rs` (lightbox).
- `src/model.rs` — time formatting, avatar colours.
- `teams-core/` — local checkout of the protocol crate (git-patched in
  `Cargo.toml` while the `[patch]` block is active; delete that block to
  build against the pushed revision).
- `FEASIBILITY.md` — the investigation and plan.
- `reference/` mirrors of spotifast/zapfast, the architecture donors.
- `.upstream/better-teams` — upstream-of-record clone for future syncs.
- `qa_shot.sh` — headless UI screenshot harness (Xvfb) used for QA.

## Logs

Every run writes a fresh log to `~/.local/state/teamsfast/teamsfast.log`
(app + protocol layer, including every endpoint call and fallback), with
panic backtraces in `panic.log` next to it. When testing for a bug report,
just run the app, reproduce, and share that file. `--logs` prints the path;
`RUST_LOG=trace` or `RUST_LOG=ost=trace` raises verbosity. Quick tail:

```sh
tail -f ~/.local/state/teamsfast/teamsfast.log
```

## Debug tools

```sh
./target/debug/teamsfast --dump-chats          # raw conversation list
./target/debug/teamsfast --probe-chat <id>     # member roster of one chat
./target/debug/teamsfast --search "query"      # message search, headless
./target/debug/teamsfast --teams               # teams + channels, headless
```

QA env hooks: `TEAMSFAST_OPEN=<chat_id>` (open on launch),
`TEAMSFAST_SIZE=WxH` (window size).

Unofficial client. Not affiliated with Microsoft. Use at your own risk.
