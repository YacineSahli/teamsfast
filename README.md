# TeamsFast

**Microsoft Teams, native and fast.** A Linux Teams client in Rust with
[egui](https://github.com/emilk/egui), built on the protocol core extracted
into [`teams-core`](https://github.com/YacineSahli/teams-core) (the patched
`ost` library: OAuth → Skype token, chat service REST, Trouter websocket,
ICE/TURN/RTP/SRTP calling). No browser engine.

Currently a **Phase-0 spike**: sign in, list chats, read history, send
messages, live Trouter events.

## Run

```sh
cargo run
```

Signing in uses Microsoft's device-code flow: click **Sign in**, then open a
browser at the URL printed in the terminal that launched teamsfast and enter
the code. Tokens are cached by the ost core in `~/.config/teams-cli/`.

## Layout

- `src/backend.rs` — tokio worker thread; owns the `ost` client, talks to the
  UI only through `Command`/`Event` channels (zapfast architecture).
- `src/app.rs` — egui views; never touch protocol types.
- `teams-core/` — local checkout of the protocol crate (git-patched in
  `Cargo.toml` while the `[patch]` block is active; delete that block to
  build against the pushed revision).
- `FEASIBILITY.md` — the investigation and plan.
- `reference/` mirrors of spotifast/zapfast, the architecture donors.
- `.upstream/better-teams` — upstream-of-record clone for future syncs.

Unofficial client. Not affiliated with Microsoft. Use at your own risk.
