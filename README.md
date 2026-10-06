# TeamsFast

**Microsoft Teams, native and fast.** A Linux Teams client in Rust with
[egui](https://github.com/emilk/egui), built on the protocol core extracted
into [`teams-core`](https://github.com/YacineSahli/teams-core) (the patched
`ost` library: OAuth → Skype token, chat-service REST, Trouter websocket,
ICE/TURN/RTP/SRTP calling). No browser engine.

## What works today

- **Sign-in** — Microsoft device-code flow (work/school and personal)
- **Chat** — list (live-sorted by recency), history with paging, send,
  reply, edit, delete, reactions, read receipts, typing indicators,
  roster-resolved names (external/federated users included)
- **Rich messages** — Teams HTML subset (bold/italic/links/code/mentions/
  quotes), inline images with lightbox, file attachments (upload with
  progress, download & open), colour emoji (platform font + bundled Noto)
- **Live** — Trouter push: incoming messages, notifications, typing;
  desktop notifications for background chats
- **Search** — message search (Graph; degrades gracefully when a tenant's
  token lacks `Chat.Read`)
- **Teams & channels** — browse teams, open channel conversations
- **New conversations** — 1:1 by email/UPN, group chats with topic
- **Tray icon** — show/hide window, quit

Phase-0/1 scope: calls (audio/video/screenshare) are the next major
milestone — the protocol core already has working 1:1 audio on Linux.

## Run

```sh
cargo run
```

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
