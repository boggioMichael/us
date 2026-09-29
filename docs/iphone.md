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
   window title to recognise the game by). The line under the name says
   whether the brain is reachable.
2. Tap the round button, then **Start Broadcast**. A red mark at the top of
   the screen shows while Syrup is watching.
3. Go play. Syrup talks when there's something worth saying, mostly nothing.
4. To stop: tap the red mark, then **Stop**. Syrup says how the session went.

The broadcast can also be started from Control Center (press and hold
Screen Recording, pick Syrup) without opening the app; Syrup's lines then
come as notifications.

While the broadcast is on, Syrup sees the whole screen, notifications
included. Each frame goes to the brain, is analysed, and is dropped; nothing
is kept unless the brain was started with `--record`. On a phone connection,
a frame every half second is roughly 150–300 MB an hour; Wi-Fi is better.

## Setting it up

Once, in this order. Everything a build can do by itself, it does: it
registers the app's IDs with Apple, and makes you a TestFlight tester who
gets every build.

### 1. The brain, on a Windows PC

1. **Smart App Control** blocks programs it doesn't know, Syrup included.
   Since the April 2026 update it can be turned off and on again without
   reinstalling Windows: Windows Security → App & browser control → Smart
   App Control settings.
2. **[Tailscale](https://tailscale.com/download)** (free): install it and
   sign in. It gives the PC an address the phone reaches from anywhere.
3. **Syrup**: download `syrup-windows.zip` from the
   [releases](https://github.com/boggioMichael/us/releases) (*Syrup for
   Windows*), unzip it, and double-click **Install Syrup.cmd**. It:
   - copies Syrup to `%LOCALAPPDATA%\Syrup` and starts it, without a window,
     now and whenever you sign in to Windows (no administrator rights);
   - asks Tailscale for the door to the phone (`tailscale funnel --bg 8080`;
     the first time, open the link it shows and allow Funnel);
   - shows the address (`https://….ts.net`), copies it, and opens the
     GitHub page where it goes (step 3).

   **Uninstall Syrup.cmd** stops it and takes it out of the Startup folder;
   what Syrup learned stays in `%APPDATA%\SyrupUniversal`. **Start
   Syrup.cmd** runs it in a window instead, to see what it's doing.

Without a token, the brain belongs to the first phone that talks to it and
refuses every other one. For a new phone, run `syrup.exe serve --pair` once.

**Hosted instead** (always on, no PC): the `Dockerfile` at the top of the
repository builds the brain with Tesseract for text. Set `SYRUP_TOKEN` to a
long random word (the phone sends it with every request), and keep `/data`
on a disk if what Syrup learns should survive a restart.

### 2. Apple

1. Join the [Apple Developer Program](https://developer.apple.com/programs/enroll/).
2. **An upload key.** [App Store Connect](https://appstoreconnect.apple.com)
   → Users and Access → Integrations → App Store Connect API (the first
   time: Request Access) → Team Keys → +. Name it GitHub, give it **Admin**
   access, Generate, and download the `.p8` file (only once possible). Note
   the Key ID, and the Issuer ID above the table.
3. **The app.** Apps → + → New App: iOS, a name that's free on the App Store
   (e.g. *Syrup Game Coach*), English, bundle ID `com.boggiomichael.syrup`,
   SKU `syrup`. (The only thing Apple won't let a build do. If the bundle ID
   isn't in the list yet, run the build once first: it registers it.)

The Team ID is on [developer.apple.com/account](https://developer.apple.com/account),
under Membership details.

### 3. GitHub

[New repository secret](https://github.com/boggioMichael/us/settings/secrets/actions/new),
once for each:

| secret | value |
|---|---|
| `ASC_KEY_ID` | the key's Key ID |
| `ASC_ISSUER_ID` | the Issuer ID |
| `ASC_KEY_P8` | the whole `.p8` file, opened in Notepad |
| `APPLE_TEAM_ID` | the Team ID |
| `SYRUP_SERVER_URL` | the address Install Syrup showed (it's on the clipboard) |
| `SYRUP_TOKEN` | only for a hosted brain with a token |

From then on, every push builds the app and uploads it to TestFlight
(`.github/workflows/ios.yml`); a build can also be started by hand from the
Actions tab. A new server address means building again.

### 4. The iPhone

Apple emails a TestFlight invitation. Install **TestFlight** from the App
Store, open the invitation, and install Syrup. New builds show up there by
themselves.

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
