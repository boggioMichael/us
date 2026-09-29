# Syrup on an iPhone

An iPhone can't run Syrup next to a game, and no app can look at another
app's screen, except through a **screen broadcast** the player starts. So the
phone app has two halves, and the brain stays on a server:

| part | where | what it does |
|---|---|---|
| **eyes** | the app's broadcast extension (`apps/ios/SyrupEyes`) | while the player broadcasts, sends the brain a frame every half second or so, scaled down and compressed as the brain asks |
| **mouth** | the app (`apps/ios/Syrup`) | keeps asking the brain what to say, and says it, from the background too (an open audio session, the way voice assistants do it); the game's sound is turned down while Syrup speaks |
| **brain** | `syrup serve` on a computer or a server | everything else: seeing, recognising the game, learning it, remembering, researching, deciding what's worth saying. The same [runtime](architecture/overview.md) as on a PC, one per phone ([protocol](../crates/syrup-runtime/src/server.rs)) |

If the app isn't running to speak (iOS closed it), the eyes show Syrup's
lines as notifications instead.

## Playing

1. Open Syrup. Optionally type the game's name (it helps: a phone has no
   window title to recognise the game by).
2. Tap the round button, then **Start Broadcast**. A red pill at the top of
   the screen shows while Syrup is watching.
3. Go play. Syrup talks when there's something worth saying, mostly nothing.
4. To stop: tap the red pill, then **Stop**. Syrup says how the session went.

While the broadcast is on, Syrup sees the whole screen, notifications
included. Each frame goes to the brain, is analysed, and is dropped; nothing
is kept unless the brain was started with `--record`. On a phone connection,
a frame every half second is roughly 150–300 MB an hour; Wi-Fi is better.

## Setting it up

### 1. Apple

The app reaches an iPhone only through Apple: **TestFlight** first (Apple's
app for installing apps before they're public; no review for your own
team), the **App Store** later (after Apple's review).

1. Join the [Apple Developer Program](https://developer.apple.com/programs/enroll/).
2. **An upload key.** App Store Connect → Users and Access → Integrations →
   App Store Connect API (the first time: Request Access) → Team Keys → +.
   Name it GitHub, give it **Admin** access, Generate, and download the
   `.p8` file (it can only be downloaded once). Note the Key ID, and the
   Issuer ID shown above the table.
3. **The app's ID.** [developer.apple.com/account](https://developer.apple.com/account)
   → Identifiers → + → App IDs → App. Description `Syrup`, Bundle ID
   (Explicit) `com.boggiomichael.syrup`. The broadcast's ID
   (`….syrup.eyes`) is registered on its own at the first upload.
4. **The app in App Store Connect.** Apps → + → New App: iOS, a name that's
   free on the App Store (e.g. *Syrup Game Coach*), English, the bundle ID
   above, SKU `syrup`.

The Team ID is on [developer.apple.com/account](https://developer.apple.com/account)
under Membership details.

### 2. The brain

**On a Windows PC** (free; it has to be on while you play):

1. Smart App Control blocks programs it doesn't know, Syrup included.
   Since the April 2026 update it can be turned off and on again without
   reinstalling Windows: Windows Security → App & browser control → Smart
   App Control settings.
2. Download `syrup-windows.zip` from the
   [releases](https://github.com/boggioMichael/us/releases) (*Syrup for
   Windows*), unzip it anywhere, and double-click **Start Syrup.cmd**. It
   listens on this computer only (port 8080), reads text with Windows' own
   OCR, and keeps what it learns in `%APPDATA%\SyrupUniversal`.
3. Give it an address the phone can reach from anywhere, with
   [Tailscale](https://tailscale.com/download) (free): install it, sign in,
   and in PowerShell run `tailscale funnel --bg 8080` (the first time it
   shows a link to allow Funnel). The address it prints, ending in
   `.ts.net`, is the server's address. It stays the same.

Without a token, the brain belongs to the first phone that talks to it and
refuses every other one. For a new phone, run `Start Syrup.cmd --pair` once
from a terminal.

**Hosted** (always on): the `Dockerfile` at the top of the repository builds
the brain with Tesseract for text. Set `SYRUP_TOKEN` to a long random word
(the phone sends it with every request); keep `/data` on a disk if what
Syrup learns should survive a restart. `syrup serve --help` lists the rest.

### 3. GitHub

In the repository's Settings → Secrets and variables → Actions → New
repository secret:

| secret | value |
|---|---|
| `ASC_KEY_ID` | the key's Key ID |
| `ASC_ISSUER_ID` | the Issuer ID |
| `ASC_KEY_P8` | the whole `.p8` file, opened in a text editor |
| `APPLE_TEAM_ID` | the Team ID |
| `SYRUP_SERVER_URL` | the brain's address, e.g. `https://my-pc.tail1234.ts.net` |
| `SYRUP_TOKEN` | only for a hosted brain with a token |

From then on, every push builds the app and uploads it to TestFlight
(`.github/workflows/ios.yml`); a run can also be started by hand from the
Actions tab. Changing the server's address means building again.

### 4. TestFlight

In App Store Connect → the app → TestFlight, add yourself to an internal
testing group. Install **TestFlight** from the App Store on the iPhone,
accept the invitation, and install Syrup.

## How it's checked

- The brain's side is tested like the rest of Syrup: a simulated phone
  sends a synthetic game's frames as JPEGs over HTTP and asks what to say,
  and must hear every line, in order, then the summary
  (`crates/syrup-runtime/src/server.rs`). CI also builds the Docker image
  and talks to it.
- CI compiles the app with Xcode 26 on every push.

Not checked, because it needs a real iPhone: the broadcast's frames
arriving the right way up in landscape games, speaking from the background
for a whole session, the notification fallback, and how Syrup does on real
phone games, whose interfaces (touch buttons, portrait screens) it hasn't
seen before.
