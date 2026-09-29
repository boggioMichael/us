# Evaluation

What was measured, how, and what it showed, as of the end of the MVP
(September 2026). Everything here can be rerun: the synthetic games with
`cargo test -p syrup-e2e -- --nocapture` or `syrup simulate <game> --truth`,
a recording with `syrup replay <file> --explain`.

## Three synthetic games, against their ground truth

Each game reports the truth of every frame (where each element is, its
value, which screen is up, what happened), and Syrup is scored against it.
The three-game end-to-end test plays each game from nothing in one data
folder (70 s of the dungeon, 75 s of the side-scroller, 60 s of the card
game, all at 4 fps), then plays each again for 12 s. CI runs it on every
push and prints these numbers:

| | Linux (Tesseract) | macOS (Tesseract) | Windows (its own OCR) | no OCR |
|---|---|---|---|---|
| **dungeon** scene right | 99% | 99% | 99% | 96% |
| health known · mean error | 97% · 0.003 | 97% · 0.003 | 59% · 0.006 | 59% · 0.004 |
| **side-scroller** scene right | 91% | 91% | 91% | 88% |
| deaths seen / happened | 1 / 1 | 1 / 1 | 1 / 1 | 0 / 1 |
| level-ups seen / happened | 1 / 1 | 1 / 1 | 1 / 1 | 0 / 1 |
| false victories | 0 | 0 | 0 | 0 |
| health known · error | 98% · 0.026 | 88% · 0.021 | 91% · 0.038 | 88% · 0.014 |
| mana known · error | 91% · 0.038 | 91% · 0.041 | 91% · 0.078 | 24% · 0.059 |
| experience known · error | 98% · 0.014 | 98% · 0.014 | 98% · 0.000 | 91% · 0.049 |
| boss health known · error | 84% · 0.004 | 84% · 0.004 | 84% · 0.004 | 84% · 0.004 |
| **card game** scene right | 75% | 72% | 77% | 68% |
| defeats seen / happened | 1 / 1 | 1 / 1 | 1 / 1 | 0 / 1 |
| timer known · error | 62% · 0.302 | 62% · 0.302 | 100% · (read as seconds) | 33% · 0.447 |

"Known" is the share of analysed frames, while the element was on screen,
in which Syrup had the concept; "error" is the mean distance between its
value and the true one, as fractions (0.02 = two percentage points).

The second session of each game passes on every system. Syrup names the
game at once from what it learned ("Dungeon 3D again. I remember this
one."), the profile shows two sessions, and the concepts are known from the
start.

What the numbers say:

- **Reading text matters most for naming things.** Without OCR, health is
  learned from colour and behaviour (a red bar that falls in fights), which
  takes longer. The side-scroller's death dialog and level-up banner are
  words, so without OCR they're missed.
- **Windows' OCR reads lines well but not a lone two-letter label.** In CI
  it read "AMMO 24" and "YOU DIED", but nothing from an isolated "HP". So the
  dungeon's health is learned as without OCR. It reads the card game's
  "TIME 0:07", which is why the timer is known all the time there.
- **The card game is the weakest.** Its still table between moves looks like
  a menu until the interface has been seen long enough, and its timer bar's
  frame and track are nearly the same dark colour as the table.

## A real game: a MapleStory recording

A 3 min 14 s screen recording of a Chaos Zakum solo (level 230, 1152×720, 30
fps, recorded with the Windows taskbar visible), replayed locally with

```
syrup replay chaos-zakum-solo.mp4 --title MapleStory --fps 4 --explain
```

The recording is the player's own and isn't committed. Neither is any
frame of it.

**Taskbar.** Found on the first frame by the date it shows, and cut: Syrup
watched the 1152×674 above it.

**Recognition.** MapleStory, from the window title, at once; the MapleStory
plugin took over (its hat, its knowledge, its bar positions).

**Bars, on 48 frames of the fight (12 s at 4 fps).** On screen: HP full
(71867 / 71867), MP 99.7%, EXP 26.9%.

| | before the fixes | after |
|---|---|---|
| HP: frames read · mean error · worst | 35 of 48 · 0.191 · 0.394 | 35 of 48 · 0.000 · 0.000 |
| MP | 48 of 48 · 0.080 · 0.100 | 48 of 48 · 0.003 · 0.003 |
| EXP | 48 of 48 · 0.490 · 0.635 | 48 of 48 · 0.006 · 0.007 |

Before, the lava next to the full HP bar was taken for its empty part, a
strip of interface beside MP was taken for more track, and the numbers
printed on the EXP bar's track cut it short. See `bars.rs`: a coloured
"track" must be the fill's own hue dimmed, text on a track is bridged,
and a bar ends at its frame.

**The session.**

| | first replay | last replay |
|---|---|---|
| what Syrup said | "New objective: atv Quest\| Morass.", "Almost dead. Back off now." (twice), "Down. Shake it off." (a death that did not happen), "Stamina almost gone." | only its greeting |
| deaths counted (really: 0) | 1 | 0 |
| concepts | mana, currency, health, energy, stamina, score, experience | experience, mana, health, boss health, currency |
| boss health | not found | 94% → 0% over the fight, following the boss's bar |
| experience | read at 43–54% from a bar cut short (the screen shows 26.9%) | 26% throughout |
| post-game | "3 min, 1 death. … Habit to work on: fights at low health (3 times)." | "3 min, 0 deaths. I learned: experience, mana, health, boss_health, currency." |

What fixed the false lines:

- a fade to black is a transition, not a death
- nothing is read from a black screen, so bars don't "empty" when it goes dark
- two-letter labels need repeating before they count ("ST" and "EN" were OCR
  noise from the quick-slot keys)
- the plugin now says MapleStory has no stamina, energy, ammo, score or
  rounds
- a panel title in capitals isn't an objective
- low-resource warnings need a concept Syrup is fairly sure of

**Still weak on this recording.**

- **Health.** The HP bar was found in 35 of 48 frames, and it wasn't kept as
  an interface element: effects crossing it keep its stillness low, and the
  white numbers printed on it sometimes break the solid-fill test. So
  "health" went to red look-alikes (a quick-slot key's flickering
  background) at 57–61% confidence. Syrup said nothing about health, because
  it wasn't sure, but the value it shows on the devtools page and in
  analysis mode is wrong. This is the next thing to fix: bars printed with
  text, and interface under constant effects.
- **Hundreds of short-lived regions** (about 950 in 3 minutes) from damage
  numbers and skill effects. They're harmless to what Syrup says but noisy
  on the devtools page, and they cost OCR reads.
- **OCR noise in objectives** ("Final Mission", "Find whe…") from the quest
  helper panel.
- **Speed.** With Tesseract run once per read, a replay analyses about 1.2
  frames a second in this container (683 frames in 9 minutes). Live on
  Windows, text is read off the loop by Windows' own OCR, which is much
  faster (below).

## Live on Windows, in CI

On every push a Windows runner opens the dungeon test game in a real window
(named `dungeon3d.exe`, as a real game's executable would be) and runs
`syrup live --window "Dungeon 3D" --seconds 50` with the overlay window up and
Windows' OCR:

- 173 frames analysed in 50 s (the sampler analyses 2–10 frames a second,
  depending on how busy the screen is)
- learned ammo, health and currency
- said "Don't know this one yet. Learning it with you.", then "Heads up.
  Health at 25%." when the dungeon's ambush hit, then the post-game summary
- the executable and the interface were stored in the profile; overlay
  pictures were written
- a second session (`--exe dungeon3d.exe`, another seed) opened with "Dungeon
  3D again. I remember this one."

The four combinations of overlay window and OCR run first on their own.
That diagnostic, with `SYRUP_TRACE`, found a bug that could only show on
Windows: the window grabber's cleanup recursed without end.

## Research against the real web, in CI

A CI job looks up Hollow Knight, MapleStory and Balatro, and Hornet in Hollow
Knight, on the live web: 46 facts, each with its source. It's
informational (the web changes); the research pipeline itself is tested
against recorded pages (`crates/syrup-knowledge/fixtures`, a fictional game).

## Speed

Measured with the release build in this (container, no GPU) environment,
60 s of each synthetic game at 4 fps, frames analysed as a replay (every
sampled frame, however long it takes):

| | no OCR | Tesseract (a process per read) |
|---|---|---|
| dungeon (213 frames analysed) | 4.3 s | 67 s |
| side-scroller (120) | 5.8 s | 74 s |
| card game (120) | 7.5 s | 38 s |

Without OCR, a frame costs about 20–30 ms, including drawing the synthetic
game. Tesseract dominates when it's used. Live, OCR runs on its
own thread, and the sampler backs off when an analysis runs over its 60 ms
budget, so the game loop never waits.

## Not measured

- A person playing a real game live on their own PC, and what they think of
  Syrup's timing, voice and overlay.
- Long sessions (hours), and many sessions of one game.
- Genres beyond the three synthetic games and MapleStory: shooters,
  racing, strategy, fighting games, games whose HUD fades out.
- Research quality at scale (only the CI sample above).
