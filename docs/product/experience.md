# What playing with Syrup is like

Syrup is a companion, not an assistant: it notices, learns and occasionally
says something short. This page describes what the player sees and hears,
and why, from the first minute with a new game to the tenth session.

## The first session with a game

1. **"Don't know this one yet. Learning it with you."** Syrup watches the
   window in front (or the one named with `--window` / `--exe`). A catalogued
   game is named at once ("MapleStory? New to me. Learning it."). Any other
   game is "new": it gets a profile keyed by its executable, and Syrup says
   "Not sure yet" instead of guessing.
2. **It finds the interface.** Within seconds, whatever stays still while the
   game moves becomes interface: bars, numbers, panels, a minimap. The
   devtools page shows each element as it's found.
3. **It says what it learned, once, when you're not busy.** "That bar at the
   bottom left is health. I think." The "I think" goes away as the evidence
   grows (a label reading "HP", a fall during a fight, an empty bar at the
   death screen). If it's wrong, you say so (👎, or name the element on the
   devtools page) and the correction outranks every inference from then on.
4. **It looks the game up** in the background (research is on by default for
   live play). If it finds the game, its genres pick Syrup's hat (a knight
   helmet for a fantasy RPG, say), its current version is noted, and facts
   arrive with their sources. None of this ever blocks the game loop.
5. **It coaches, rarely.** Warnings come first ("Careful. Health at 22%.",
   "3 shots left.", "Clock's running out. Decide."), then tips from what it
   looked up ("Mossy King: weak to fire. (wiki)"), then remarks on patterns
   ("Third time here. Watch its pattern before you commit.").
6. **At the end: "How it went."** The session in three lines (time, deaths,
   what it learned, a highlight, a habit to work on), saved with the session.

## The second session

"Dungeon 3D again. I remember this one." The game is recognised from what
Syrup learned about it: the executable, the window title, the look of its
interface. Syrup starts from the stored profile, so the interface is known,
concepts have their names from the first seconds, and old facts are marked
stale if the version changed. The player model carries over: skills Syrup is
sure about, habits, muted topics.

## The overlay

| mode | shows | for |
|---|---|---|
| hidden | nothing (Syrup still watches, learns and remembers) | playing undisturbed |
| minimal | Syrup's head and one short line | most play |
| normal | the card: the line, where it comes from ("I saw it · 90%", "looked up", "players say", "patch notes"), and feedback | the default |
| analysis | the card, plus what Syrup sees: the game and how sure, the scene, each concept and its value, speed | understanding or correcting Syrup |
| post-game | the summary | the end of a session |

Clicking Syrup's head moves to the next mode (minimal → normal → analysis →
hidden); `--mode` or the devtools page brings a hidden Syrup back. The
overlay sits at the top right of the game window. It never takes the focus,
clicks pass through to the game everywhere except on the card, and it keeps
itself out of screen captures so Syrup never reads its own card. Everything
is painted in Maplesyrup's colours: ink `#57351F`, cream `#FFF4DD`, amber
`#D99A43`, honey `#FFE5B8`.

## Feedback

Every line Syrup shows can be answered with one click:

| | means | changes |
|---|---|---|
| ✓ useful | more like this | this kind of advice and this topic weigh more for this game |
| ✕ wrong | this was wrong | this kind weighs less; if the line was about a concept ("health at 20%"), Syrup doubts that concept |
| ? explain | why? | Syrup says what the line rests on: the evidence, or the fact and its source |
| 🔇 stop | never say this again | the topic is muted for this game |
| research | look it up | the topic is researched now |
| ignore | not now | a small nudge down |

Feedback is kept in the player model, so it lasts beyond the session.

## Interruptions

Most moments produce no advice. When Syrup has candidates, the interruption
policy decides:

- Critical lines (health about to run out) always go through, with 3 seconds
  between them.
- Anything else waits for at least 8 seconds of quiet after the last line,
  at most 4 lines a minute, and 45 seconds before the same topic comes back.
- When the screen is busy (a fight), only important lines worth enough get
  through. The rest wait in a short queue for a calmer moment, and expire if
  it doesn't come in time.
- In menus nothing actionable is said; over cutscenes and dialogue only
  critical lines are.
- A line's value is its urgency × its confidence × how this player rated
  this kind of line and this topic before.

Every held-back line is logged with its reason ("the player is busy",
"said 12s ago") on the devtools page and in the session timeline.

## Spoilers

`--spoilers` sets how much Syrup may tell from what it looked up:

| level | Syrup says |
|---|---|
| none | only what it saw for itself; no looked-up facts |
| hints_only | nudges, never answers, nothing about what's ahead |
| normal (default) | facts, but nothing that gives away story or surprises |
| full_information | everything it knows |

Each fact carries a spoiler level, decided when it was researched: words
like "final boss", "ending", "betray" or "is actually" make a major spoiler,
and "boss", "secret" or "unlock" a mild one.

## Syrup's voice

Short sentences, curious, a little playful, never a lecture. "Wait. That
attack repeats every 4 seconds." Not "Based on my comprehensive analysis".
On Windows Syrup can speak its lines through the system voice (`--no-voice`
turns it off). A new line cuts off the last.

## Syrup

The Maplesyrup dog, drawn from Maplesyrup's companion art, always with its
**S** medallion. Its hat comes from the game: the plugin's choice, the
catalog's, or the genres research found. Its face follows what it's doing:
thinking when unsure, excited on meeting a game, warning, researching (a
magnifier), proud at the end, surprised, confused. It blinks and bobs a little
while speaking; `--reduced-motion` stills it.

## Privacy, memory and forgetting

What Syrup learns stays on the computer, in plain JSON. Frames are kept only
with `--record`. `syrup profiles` shows what it knows; `syrup forget <game>`,
`--player`, `--recordings`, `--sessions` or `--everything` removes it.
