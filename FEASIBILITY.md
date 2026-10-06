# TeamsFast — Feasibility Plan

**A native Microsoft Teams client for Linux, following the Spotifast/ZapFast recipe.**
Status: investigation complete · 2026-10-06

---

## 1. Verdict

**Feasible — with staged scope.** Chat, presence, notifications, teams/channels and
1:1 audio calls are achievable today without a browser engine, because the hardest
part already exists: a MIT-licensed Rust protocol library for Teams
([eisbaw/ost](https://github.com/eisbaw/ost)) that was built Linux-first and has
working audio calls over ALSA. The ZapFast architecture maps onto it almost 1:1.

The two honest caveats, up front:

1. **Protocol maintenance is the permanent cost.** Teams' client-facing APIs are
   undocumented and version-pinned. They work (ost proves it; Teams' own web client
   depends on the same surface), but Microsoft can change them at any time and we
   own the fix. Same treadmill whatsapp-rust runs on.
2. **Enterprise identity is a different risk model than WhatsApp.** Nobody gets
   their phone banned for linking a companion device quietly; a Teams login shows
   up in the tenant's Entra sign-in logs as an unknown public client. In tenants
   with Conditional Access / Intune app-protection policies, third-party clients
   may be blocked outright — those users can never be our users. This must be a
   documented limitation, not a surprise.

---

## 2. What the reference projects teach (the recipe)

| Layer | Spotifast | ZapFast | → TeamsFast |
|---|---|---|---|
| UI | egui/eframe (glow) | egui/eframe (glow) | egui/eframe (glow) |
| Protocol | librespot + Spotify Web API | whatsapp-rust (companion device) | **ost** (OAuth → Skype token → chat REST + Trouter + RTP/SRTP) |
| Shell | fastframe-* crates (tray, theme, fonts, i18n, update, instance, scroll, log) | same | same |
| Storage | directories + keyring | SQLCipher archive per account | SQLCipher archive per account |
| Threading | tokio worker + Command/Event + Waker | same | same |

ZapFast's measured value proposition (150–200 MB idle RAM, ~152 ms to first
window vs 1.13 GB / 528 ms for WhatsApp Web in Chromium) is exactly the pitch
against teams-for-linux (Electron + the full Teams web app; typically well over
1 GB and seconds to usable).

Two structural differences that work **in our favour** vs WhatsApp:

- **No end-to-end message crypto.** Teams chat is TLS + bearer-token protected,
  server-side. No Signal protocol, no key storage, no message-padding protobuf
  gymnastics. The archive is plain (encrypted-at-rest by us) content.
- **A second authoritative surface exists**: Microsoft Graph (calendar, files,
  user data) is documented and stable. ost already uses both surfaces together —
  Graph for calendar/files, Teams' own APIs for chat/calls.

---

## 3. The discovery that changes the answer: the protocol layer exists

### 3.1 `eisbaw/ost` — "Open Source Teams client" (MIT)

Rust CLI/TUI Teams client, created Feb 2026, ~640 KB of Rust, Linux-first
(ALSA audio, V4L2 camera). Upstream stalled since ~April 2026 (10★, 102 open
issues) — but the core is real and was demonstrated live:

| Subsystem | Status in ost | Notes |
|---|---|---|
| OAuth2 device-code flow (work/school **and** personal `teams.live.com`) | stable | refresh-token → multiple audiences (Graph, IC3) |
| Skype-token exchange (`teams.microsoft.com/api/authsvc/v1.0/authz`) | stable | this is the real "login" for all Teams APIs |
| Chat service REST (`{region}.ng.msg.teams.microsoft.com`, chatsvcagg, middle-tier) | stable | list/read/send messages, teams, channels |
| **Trouter v4** WebSocket push (registrar → socketio session → reconnect/backoff) | stable | real-time messages, presence, call invitations |
| **Calling**: signaling (two-phase `epconv` conversation API), ICE, TURN, SDP (incl. compression), RTP/RTCP, **SRTP (pure-Rust AES-128-CM + HMAC-SHA1-80)**, Opus audio, mic/speaker via cpal | **audio calls working on Linux** | video WIP (V4L2 + OpenH264 + SDL2) |
| Docs | excellent | protocol glossary, GUID/client-ID reference |

### 3.2 Fork politics: upstream is dead, the living core lives in better-teams

Verified 2026-10-06 via the GitHub API:

- **Upstream `eisbaw/ost` is abandoned in practice.** Last code commit **6 Feb
  2026**. The owner (eisbaw / Mark Ruvald Pedersen) is active on GitHub (pushing
  other projects days ago) but has not touched ost in 8 months.
- **104 PRs, 0 engagement.** mrowlinson filed **104 pull requests** against
  upstream (Sept 22–30, 2026): 103 still open, 1 closed unmerged, **zero
  comments, zero labels, zero merges**. Each PR = one feature branch on his
  fork `mrowlinson/ost` (~100 branches). The fork's `main` is still at
  upstream shape — integration never happens there.
- **The real code lives in better-teams' vendored `rust/ost/`** (a copied
  tree, not a submodule), documented in `OSTMAC-PATCHES.md`: **92 numbered
  patches (35 [major], 56 [minor], ~105 KB of changelog)**. Patch #1 is a fix
  for an upstream build bug (SDP dictionary size — upstream HEAD didn't
  compile). The vendored copy **deletes the entire TUI** (11 files — the GUI
  replaces it) and **adds 21 modules**: 16 new API surfaces (calendar, files,
  recordings, transcripts, planner, todo, tags, tabs, search, apps…),
  `event_hub.rs` (Trouter event channel for embedders), `lib.rs` (library
  surface), `calling/macav.rs` (macOS A/V bridge), plus test fakes.
- **License:** NOTICE keeps the vendored `rust/ost/` tree under upstream MIT
  (original LICENSE preserved); the Microsoft-exclusion license covers only
  better-teams' original code (`swift/`, `rust/ostmac-core/`). The protocol
  core we need is therefore MIT — including the 92 patches, as best the
  NOTICE reading goes (verify file-by-file before shipping).

Consequence for us: **base TeamsFast on the better-teams vendored ost (or a
subscription of mrowlinson's PR branches), not on upstream.** Upstream is a
snapshot; mrowlinson is the de-facto maintainer of the protocol code.

### 3.3 Everything else surveyed

- `teams-for-linux` (5.1k★, active): Electron wrapper around the official web
  app. The incumbent we're displacing. Zero protocol risk, maximal resources.
- `msteams-lite-client` (Rust + Iced): read-only **Graph API** client; needs a
  tenant admin to register an app. Shows why pure-Graph is a dead end for a
  daily driver (below).
- `patois/teams` (Go, the old reverse-engineered library): deleted by author.
- `thichcode/rust_teams`, `mjul/rust-ms-teams`: toys.
- `weirdapps/teams-access` (TypeScript CLI, active): accessibility angle,
  TypeScript — not usable as a Rust core, but another live reference.

---

## 4. Options considered

| Option | Verdict | Why |
|---|---|---|
| **A. Pure native: egui + ost as protocol crate (the ZapFast shape)** | ✅ **recommended** | All layers exist; measured upside is huge; MIT throughout; differentiates as the only true-native Linux Teams client |
| B. Graph API client | ❌ as primary | Needs per-tenant app registration (admin consent walls); no calling; no Trouter (no real-time); personal accounts mostly unsupported; message fidelity partial. Fine as a *supplementary* surface (ost already mixes it in) |
| C. Lighter wrapper (Tauri/wry) | ❌ off-mission | Still a browser engine; ~half the RAM at best; doesn't match the fastframe-family quality bar |
| D. Contribute to teams-for-linux | ❌ | Wrong architecture; its existence is useful only as an API-breakage early-warning system |

---

## 5. Recommended architecture

```
teamsfast/
├── Cargo.toml            # workspace
├── crates/
│   └── teams-core/       # fork of eisbaw/ost, reshaped as a library:
│                         #   AuthManager (device-code + refresh, keyring-stored)
│                         #   ChatClient (REST), Trouter (ws), Caller (RTP/SRTP)
│                         #   → emits typed Event, accepts Command  (zapfast shape)
├── src/
│   ├── main.rs           # fastframe-shell + eframe
│   ├── app.rs            # process shell: theme, tray, updates, accounts
│   ├── backend.rs        # tokio worker thread, Command/Event bridge, Waker
│   ├── archive.rs        # SQLCipher store: chats, messages, contacts, presence
│   ├── model.rs          # app types; worker translates raw Teams JSON here
│   └── ui/               # chat list, conversation, teams/channels, calls,
│                         # calendar, settings; pushes model::Action only
└── packaging/            # copy zapfast's: arch, flatpak, appimage, release-notes
```

Key mappings from zapfast to keep:

- **Views never touch protocol types.** `model.rs` owns app types; the worker
  parses Teams' (often gnarly) JSON/HTML into them. Views push `Action`s.
- **One `Backend` per account**, SQLCipher archives under
  `state/accounts/<id>/`, keys in the OS keyring, nothing cross-account.
- **fastframe crates** give tray, single-instance, self-update, themes
  (Omarchy integration!), i18n, scrolling, logging for free.
- **fastframe-audio** (cpal) is the same library ost uses for audio — calls and
  the shell share one audio stack.

Differences from zapfast to respect:

- Teams messages are **HTML**, not WhatsApp text. We need a hardened
  HTML-subset renderer in egui (allowlist tags/styles; zapfast's `markup.rs` is
  the starting point, not the endpoint).
- **Presence is central** in Teams (it's a work tool) — live Trouter presence
  in the roster and hover-cards is a day-one feature, not a nice-to-have.
- Auth is **interactive OAuth** (device code, or browser loopback like
  msteams-lite-client) — no QR pairing. Token storage must be keyring-only,
  never plain JSON (zapfast/spotifast already set this pattern).

---

## 6. Phasing

### Phase 0 — Spike (1–2 wk) — kill-or-continue gate
Fork ost, build as lib. egui window with: device-code login → Skype token →
chat list → open chat → read history → **send a message** → live incoming via
Trouter with notification. Measure RAM/startup vs teams-for-linux with the
zapfast benchmark methodology (paired runs, medians, publish results).
**Gate:** if Skype-token or Trouter flows prove unstable over two weeks, stop.

### Phase 1 — Daily-driver chat (4–8 wk)
Read receipts, typing indicators, reactions, replies/threads, edit/delete,
presence in roster, mention detection + notifications, image/file download
(asm.skype.com objects), file *upload*, HTML-subset renderer, SQLCipher
archive + scrollback, multi-account, tray, autostart, single-instance,
self-update, themes/i18n. Teams & channels browsing.

### Phase 2 — Calls (4–6 wk)
1:1 audio calls (ost's stack: ICE/TURN/SRTP/Opus) through PipeWire/ALSA, then
**meeting join** (meeting chat → `epconv` join, roster, mute). Meetings on
Linux with working audio is the single biggest feature teams-for-linux users
complain about.

### Phase 3 — Video & screen share (open-ended, ship when solid)
Camera send (V4L2 + OpenH264, ost has WIP), receive path, screen share
(X11 capture; Wayland via xdg-desktop-portal PipeWire streams — the same wall
every Linux client hits; honest docs).

### Ongoing (the real cost)
Protocol treadmill: watch Teams web client releases (teams-for-linux issues are
the canary), keep `SkypeSpaces/TsCallingVersion` headers and endpoints fresh,
CI that logs in weekly against a real test tenant.

---

## 7. Risk register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Microsoft changes undocumented chat/call APIs | **certain, eventually** | feature outage until patched | treat ost as owned fork; canary CI; ship fast patches; document, don't hide it (zapfast model) |
| Conditional Access / Intune blocks non-approved clients | low per user, high in hardened orgs | those users can't log in | explicit supported-tenant docs; device-code + browser flows both offered; "try the spike" onboarding |
| Tenant SOC flags unknown client in sign-in logs | medium | user IT conversation, worst case blocked | name the app honestly in docs; recommend a personal/dev tenant for evaluation |
| Consumer-account (teams.live.com) drift | medium | personal users break | ost supports both authz endpoints; test both in CI |
| Calling quality (jitter buffers, device quirks, PipeWire) | high | calls are unforgiving | phase 2 separately; feature-flag calls; lean on ost's live-call test harness |
| HTML renderer XSS-adjacent bugs | medium | crash/leak | strict allowlist parser, fuzz it, never eval styles/URLs |
| Living ost code only exists inside better-teams' repo | **true today** | fork-lock-in: our core tracks another app's vendor tree | track `mrowlinson/ost` PR branches; keep our fork of the core as a first-class repo; raise extraction with mrowlinson early |
| ToS | nonzero | account action (mostly consumer side) | same disclaimer posture as zapfast: unofficial, own risk, no affiliation, no telemetry |

---

## 8. Effort & resourcing

- Solo, evenings pace: **MVP (Phase 0+1) ≈ 2–3 months**; usable 1:1 calls ≈ +1–1.5 months.
- The single biggest schedule risk is **Phase 2 audio quality**, not protocol plumbing.
- Reuse budget is unusually high: fastframe (~14 crates), ost (auth+chat+trouter+SRTP),
  zapfast's UI patterns (chat list, conversation view, archive, notifications,
  packaging, docs site) — this is the third sibling in the family, and it shows.

## 9. Success metrics (mirror zapfast's published benchmarks)

- Idle PSS RAM: **< 250 MB** (target < 200 MB) vs teams-for-linux measured baseline
- Process start → first window: **< 300 ms**; chat visible < 1 s from archive
- Zero browser-engine processes; binary < 40 MB
- Message send → Trouter echo round trip < 300 ms p50
- 1:1 call MOS judged acceptable on ALSA + PipeWire test matrix

## 10. Open questions

1. Does meeting *join* (as opposed to ad-hoc 1:1 calls) work end-to-end with ost's
   `epconv` flow against a real tenant? (better-teams README suggests yes on macOS;
   must verify on Linux audio stack.)
2. Personal-account parity: which chat features differ on `teams.live.com`?
3. E2EE-encrypted 1:1 Teams calls (optional MS feature) — ost presumably negotiates
   the normal, non-E2EE path; confirm and document.
4. ~~Do we approach the ost upstream?~~ **Resolved: upstream is dead (104
   unanswered PRs, 8 months silent).** Approach **mrowlinson** instead — the
   de-facto maintainer: propose (a) spinning the patched ost core out of
   better-teams into its own maintained crate/repo that better-teams and
   teamsfast both consume, or failing that (b) tracking his vendored tree as
   our upstream. His PR backport attempt to eisbaw shows he values a proper
   library surface (`lib.rs`, event_hub) — a Linux client is evidence that
   core deserves independence. Coordinate before forking; verify the MIT
   status of every file we take.
