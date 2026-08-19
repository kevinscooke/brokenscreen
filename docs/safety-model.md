# Safety model

Broken Screen intentionally controls a failure-prone hardware boundary with an undocumented operating-system function. Safety rules are therefore product requirements, not implementation details.

## Invariants

1. A confirmed physical external display must be active and online before protection begins.
2. Virtual and unknown displays never authorize the initial internal-display disconnect.
3. Losing all usable external displays forces restoration.
4. Disabling automation or quitting restores the managed display.
5. Only one internal display is managed at a time.
6. A timed manual test always schedules restoration.

## Recovery layers

- **App-scoped configuration:** lets WindowServer unwind state when the app terminates.
- **Independent watchdog:** observes the parent process and performs an explicit session-scoped restore after a crash.
- **Reconciliation loop:** reevaluates topology and lid state once per second.
- **Shutdown handler:** restores automation and test state during normal exit.
- **Fail-closed classification:** unknown devices do not count as physical safety displays.

## Trust boundaries

The frontend is informational. Rust rechecks live display state before changing it. Stored preferences express user intent but never bypass topology checks. Display metadata supplied by macOS is trusted only when it explicitly identifies a physical device.

## Known limitations

- The configuration symbol is private and may change in future macOS releases.
- Hardware damage can be electrical; software cannot make a damaged cable or hinge safe.
- DisplayLink, docks, virtual displays, sleep/wake timing, and future Apple hardware require broader field testing.
- The current release targets Apple silicon and is not suitable for the Mac App Store.
