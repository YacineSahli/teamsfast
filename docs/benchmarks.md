# Benchmarks: memory and startup

Paired Linux runs comparing **TeamsFast** with **teams-for-linux** (the
Electron-based Microsoft Teams client). Medians of four paired runs on
one Linux laptop, measured 19 October 2026 with
[`scripts/bench.sh`](../scripts/bench.sh).

## Results

TeamsFast used **153 MB** of idle RAM, compared with **394 MB** for
teams-for-linux — **2.6× less** — while showing the signed-in chat list
against their sign-in page (see caveat below). First-window paint was a
tie at **320 ms** for both.

| Measurement | TeamsFast (release) | teams-for-linux 2.26.0 |
| --- | ---: | ---: |
| Idle RAM, PSS | **153.1 MB** / 146.1 MiB | **394.0 MB** / 375.9 MiB |
| First window paint | **320 ms** | **320 ms** |
| Processes | 1 | 6 |

## The four runs

Order alternated within each pair. Each RAM figure is the median of
five samples taken 25 s after first paint.

| Run | TeamsFast RAM, MiB PSS | t4l RAM, MiB PSS | TeamsFast paint | t4l paint |
| --- | ---: | ---: | ---: | ---: |
| 1 | 145.7 | 375.0 | 321 ms | 318 ms |
| 2 | 146.2 | 376.4 | 321 ms | 326 ms |
| 3 | 145.4 | 374.0 | 318 ms | 317 ms |
| 4 | 146.2 | 376.4 | 318 ms | 322 ms |

Every sample is in
[`measurements.json`](assets/benchmarks/2026-10-19/measurements.json).

## How we measured

**Machine and software.** Intel i5-1240P, 16 GiB RAM, Fedora Linux 44
(kernel 7.2.9-201). TeamsFast was the local release build
(`cargo build --release`), signed in to a real work account with cached
history. teams-for-linux 2.26.0 ran from the release AppImage (extracted
once, so launches are steady-state), with a fresh `HOME`, `--disable-gpu
--no-sandbox` (the runnable configuration under a headless X server).
Both apps ran alone on a fresh 1280×800 Xvfb display per run.

**First paint.** A monotonic millisecond timer started immediately
before process creation; a 10 fps grayscale capture of the display
reported the first frame with a lit pixel in the central region. The
same detector measured both apps. This is *window paint*, not *content
ready*: TeamsFast's first frame already carries its cached chat list,
while the Electron window paints before its sign-in page renders.

**Idle RAM.** Sum of `Pss` from `/proc/PID/smaps_rollup` over the whole
process tree (PSS gives each process its proportional share of shared
pages, so a sum avoids double-counting). TeamsFast is one process;
teams-for-linux's total covers its main, GPU, renderer, utility and
zygote processes (five children beside the root).

## What the comparison covers — and what it does not

- **teams-for-linux sat at its sign-in page.** Its account login is
  interactive, so its numbers are pre-authentication; a signed-in
  session adds renderer workload and will raise its figure. TeamsFast's
  number is a fully signed-in session with cached chats. We consider
  the comparison fair *for that direction* — their post-login number
  can only grow — and will publish a signed-in pair once a login has
  been completed once in the harness.
- **One machine, one session each.** Local history, cache sizes and
  workload change memory use; these runs cover exactly what is stated.
- **TeamsFast's window rendered 1100×760** (winit caps window size under
  Xvfb's zero-millimetre display), teams-for-linux's 1280×800. A smaller
  surface slightly favours TeamsFast; the delta is small next to the
  measured gap.
- `--disable-gpu` removes teams-for-linux's GPU process work. Under
  Xvfb there is no real GPU either way.
- Resident system RAM only: no VRAM, compositor or X server accounting.

The exported data contains only counts, timings, and memory
measurements — no messages, contact names, credentials, or screenshots.

## Reproduce

```sh
cargo build --release
mkdir -p /tmp/tfl-bench && cd /tmp/tfl-bench
curl -sLO https://github.com/IsmaelMartinez/teams-for-linux/releases/download/v2.26.0/teams-for-linux-2.26.0.AppImage
chmod +x teams-for-linux-2.26.0.AppImage && ./teams-for-linux-2.26.0.AppImage --appimage-extract
mv squashfs-root /tmp/tfl-bench/ 2>/dev/null || true
cd - && scripts/bench.sh 4
```
