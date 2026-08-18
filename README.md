# Broken Screen for Mac

> Broken screen doesn't mean broken dreams.

Broken Screen for Mac is a focused utility for MacBooks with damaged, unwanted, or permanently dark internal displays. Turn it on once and macOS stops treating the built-in panel as usable desktop space whenever an external monitor is connected—even when the lid opens.

No lost windows. No wandering pointer. No complicated display-control dashboard.

## What it does

- Keeps the built-in display disconnected while an external display is active.
- Reapplies the setting when the lid opens or the Mac wakes.
- Restores the built-in display if the last external display disappears.
- Restores normal behavior when Broken Screen is turned off or quits.
- Remembers whether Broken Screen was enabled.
- Lives in the menu bar and hides its window instead of quitting when closed.
- Can launch quietly at login.
- Runs an independent watchdog that restores the internal display if the app crashes.
- Runs locally without an account, analytics, or cloud service.

## Who it is for

- MacBooks with broken display panels, flex cables, or hinges.
- Desk setups where the internal screen should stay out of the desktop layout.
- People who want one obvious switch instead of a full display-management suite.

## Current status

Broken Screen is an early macOS prototype. It has been tested on Apple silicon running macOS Tahoe 26 with directly connected external displays. Broader hardware, dock, DisplayLink, sleep/wake, and failure-recovery testing is still required before a public release.

**Known limitation:** virtual displays are not filtered from physical monitors yet. Connecting or disconnecting a virtual display while Broken Screen is enabled can trigger an unwanted display reconfiguration or temporarily blank another display. Turn Broken Screen off before changing virtual-display connections until physical-display filtering is implemented.

The app uses an undocumented macOS display-configuration function because Apple does not provide a public API for soft-disconnecting the built-in display. That makes Broken Screen unsuitable for the Mac App Store and means future macOS releases may require compatibility updates.

Broken Screen cannot make damaged display wiring electrically safe. If moving the hinge causes heat, smells, shutdowns, or other electrical symptoms, stop using the hinge and have the Mac serviced.

## Development

Prerequisites:

- macOS
- Apple Command Line Tools
- Rust stable
- Node.js and npm

```sh
npm install
npm run tauri dev
```

Production checks:

```sh
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
```

## How it works

The Tauri frontend provides the small control surface. The Rust backend uses public Core Graphics calls to identify online, active, and built-in displays. When Broken Screen is enabled and a usable external display exists, it applies an app-scoped display configuration through the private `CGSConfigureDisplayEnabled`/`SLSConfigureDisplayEnabled` function available in macOS.

Safety rules are part of the product, not optional extras:

1. Do not disconnect the internal display without an active external display.
2. Restore the internal display when external displays disappear.
3. Restore it when automation is disabled or the app exits.
4. Keep switching app-scoped so WindowServer can unwind it when the process ends.
5. Start an independent recovery process before disconnecting the internal display.

The launch-at-login setting points to the installed app. During development, leave it off unless you intentionally want the debug build to start when you sign in.

## Roadmap

- Signed and notarized distribution
- Physical-display filtering for AirPlay, Sidecar, and virtual displays
- Event-driven hot-plug and wake handling
- Automated tests across Apple silicon models, docks, and macOS versions
- Accessible onboarding and recovery instructions

## Brand

Broken Screen for Mac is made by **Search & Be Found**.

## License

No public license has been selected yet. All rights are reserved until a license is added.
