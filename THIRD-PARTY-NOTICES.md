# Third-party notices

TeamsFast builds on the following open-source projects. Bundled assets
ship inside release packages; the rest are compiled in.

## Compiled dependencies

- **teams-core** (MIT) — the protocol core; a fork of the `ost` library,
  carrying patches that originate in better-teams' vendored copy.
  https://github.com/YacineSahli/teams-core
- **egui / eframe / egui_extras** (MIT or Apache-2.0) — the UI toolkit.
- **egui Theme / omarchy integration** via the fastframe stack (MIT).
- **tokio, reqwest, serde, jiff, rusqlite (SQLCipher), notify-rust,
  cpal/ALSA** and other crates: each under its own license (MIT or
  Apache-2.0 unless noted). The full dependency graph with licenses is
  in `Cargo.lock` and `cargo about` output.

## Bundled assets

- **Inter** — SIL Open Font License 1.1 (`assets/fonts/Inter-LICENSE.txt`).
- **Noto Color Emoji** — SIL Open Font License 1.1, bundled via
  fastframe-emoji (`assets/fonts/NotoEmoji-LICENSE.txt`).
- **Lucide icons** — ISC License (derived from Feather, MIT). A copy
  ships with the fastframe icon set.
- **Omarchy theme templates** (`contrib/omarchy/`) — fastframe stack, MIT.

## Notice for ost

The original `ost` library is Copyright (c) eisbaw and contributors,
licensed under the MIT license; its LICENSE file is preserved in the
teams-core repository this project builds on.
