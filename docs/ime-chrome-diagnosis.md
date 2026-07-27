# Ctrl+Space silently fails — investigation record

Status: **cause located, upstream fix needed.** A measurement log, not a
decision. Two separate defects were found; several plausible-looking hypotheses
were refuted along the way and are recorded so nobody re-treads them.

Tracked in [#14](https://github.com/tarotene/dotfiles/issues/14).

## Symptom

On `company-pop-new` (Pop!_OS 24.04, COSMIC/Wayland), `Ctrl+Space` sometimes does
nothing: the keypress is swallowed, the input method does not change, and latin
text goes into a Japanese field. Pressing again often does nothing either.

It presents as Chrome-only but is not — Slack Desktop is also a native Wayland
client and was hit at the same time. Slack normally lives as a tab inside Chrome
here, so the desktop app was rarely open to be noticed.

## Cause: fcitx5 consumes the trigger key without switching

Measured with fcitx5 under `--verbose 'default=5,key_trace=5'`. Healthy press:

```
16:56:00.156828 KeyEvent: Key(Control+space states=4) Release:0
16:56:00.156892 Instance::deactivateInputMethod
16:56:00.157197 Activate: [Last]: [Activating]:keyboard-us
16:56:00.157264 Input method switched
16:56:00.157661 KeyEvent handling time: 0ms result:1
```

Failing press — the key is consumed and nothing else happens:

```
16:28:15.043187 KeyEvent: Key(Control+space states=4) Release:0
16:28:15.043215 KeyEvent handling time: 0ms result:1
```

Over one session: **102 presses, 86 switched, 16 failed (16%)**, all 16 consumed
(`result:1`). The failures come in bursts — the user pressing repeatedly because
nothing happened.

The switch is **deferred, not lost**. At 16:24:56 it landed 1.4 s later, attached
to a focus event rather than to the keypress:

```
16:24:56.745  KeyEvent: Key(Control+space) ... result:1        <- consumed, no switch
16:24:58.157  Instance::deactivateInputMethod event_type=4100  <- focus event
16:24:58.157  Activate: [Last]: [Activating]:mozc              <- switch appears here
```

Consistent with fcitx5 applying the toggle to an input context that is not the
one actually focused at that instant.

**The Wayland side was healthy at the moment of failure**, which is what rules
out the compositor for this defect. Tracing fcitx5 itself with `WAYLAND_DEBUG=1`:

```
16:28:15  zwp_input_method_keyboard_grab_v2#21.key(176, 12201550, 57, 1)  <- space with ctrl held
16:28:16  zwp_input_method_v2#17.deactivate()                             <- only afterwards
16:28:16  -> zwp_input_method_keyboard_grab_v2#21.release()
```

The grab was live and delivering keys. fcitx5 also conforms to the protocol
throughout: every `activate` answered with `grab_keyboard`, every `deactivate`
with `release`, and `commit(serial)` always matching the `done` count.

## Second, separate defect: cosmic-comp sends a duplicate `enter`

Real, reproducible on demand, and benign in isolation. When any client creates a
`zwp_text_input_v3`, the compositor sends `enter` to an already-entered client
with no intervening `leave` — which text-input-v3 forbids.

Minimal reproducer, both programs first-party (`timtest/timwin.c` is a focusable
text-input client, `timtest/main.c` a surfaceless one):

```
16:53:50  timwin  <- enter                    (clicked, holds focus)
16:53:53  timtest creates a text_input        (no surface — never focusable)
16:53:53  timwin  !! enter while already entered (missing leave)
```

Chrome and `timwin` both recover by re-sending `enable`, and a probe that
provoked the violation while Chrome was composing showed preedit continuing
normally. Screen lock/unlock triggers it too, presumably because the lock screen
is itself a text-input client.

## Not faults

- fcitx5 reverting to `keyboard-us` when focus moves is `ActiveByDefault=False`
  plus `ShareInputState=No` behaving as configured; keys then passing through
  with `result:0` is correct.
- nix GTK3/GTK4 do lack the apt `im-fcitx5.so`, so a nix GTK app under X11 would
  fall back to XIM — but no affected app is an X11 client, so it never applied.
- Chrome 149 is already a native Wayland client and already binds
  `zwp_text_input_manager_v3`. `--ozone-platform-hint=auto`,
  `--enable-wayland-ime` and `--wayland-text-input-version=3` are no-ops here:
  two builds traced under `WAYLAND_DEBUG`, one with all four flags and one with
  none, produce identical text-input traffic.

## Refuted hypotheses

Each of these looked convincing and was wrong. The common failure mode was
reading a truncated tail or a hand-picked window instead of aggregating over the
full log.

| Hypothesis | How it died |
|---|---|
| It's the nix/apt split — nix Chrome can't load the GTK fcitx immodule | true but irrelevant: Chrome is not an X11 client |
| Chrome runs under XWayland and is stuck on flaky XIM | `_NET_CLIENT_LIST` is empty; Chrome is native Wayland |
| Adding wayland-ime flags will put it on text-input-v3 | already there; protocol traces identical with and without |
| A serial desync between client `commit`s and compositor `done` | direct count: 105 commits vs `done(106)` — a one-event lag, permitted. The large apparent offsets came from a monitor attached mid-stream |
| cosmic-comp leaves the IM deactivated while a focused client has text-input enabled | reading the complete trace: Chrome had sent `disable` and received `leave` first |
| Cross-client interference: another client's `disable` kills the focused client's IME | surfaceless-client experiment produces no `activate`/`deactivate` traffic at all |
| The duplicate `enter` is what breaks input | provoked it on demand while Chrome was composing; preedit continued |

## Instrumentation

Everything lives in the session scratchpad under `ime-diag/`; it is not part of
the repo. Rebuild as follows.

Both halves of the protocol must be traced — tracing only the client is what
made this look like a Chrome bug for hours:

```bash
# input-method half
WAYLAND_DEBUG=1 /usr/bin/fcitx5 -r -D --verbose 'default=5,key_trace=5' 2>&1 | ...
# client half
WAYLAND_DEBUG=1 google-chrome-stable 2>&1 | ...
```

Two details that are easy to get wrong: filter the per-frame flood at write time
(otherwise a multi-hour trace is tens of GB rather than a few MB), and prefix
wall-clock timestamps — libwayland's own `[ 906672.653]` stamp is on a clock that
matches nothing else on the system.

The detector that actually works operates at millisecond resolution on adjacent
log lines: for each `Key(Control+space ... Release:0)`, look ahead ~250 ms for
`Input method switched`. Per-second bucketing cannot see this failure, because
several switches can occur inside one second and the user's repeat presses land
in the same second as the failure.

Key synthesis is **not** available for automating the check: `wtype` exits 0 but
its keys never reach fcitx5, because the compositor excludes virtual-keyboard
input from the input-method grab (otherwise fcitx5's own key forwarding would
loop). A real keystroke is unavoidable.

### `key_trace` captures the login password — redact before it hits disk

`--verbose key_trace=5` logs **every** keystroke, and fcitx5 keeps its keyboard
grab while the COSMIC lock screen is up. An unfiltered trace therefore records
the password typed at the lock screen, in clear, including the `keycode:` field
(which identifies the physical key just as well as the key name does). This
happened during the investigation and had to be scrubbed after the fact.

Pipe the trace through a filter that keeps only the keys the detector needs and
redacts everything else, so plaintext never reaches disk:

```
allow='Control\+space|Zenkaku_Hankaku|Hangul|Super\+space|Control_L|Shift_L|Return|Escape|BackSpace|Tab'
... | perl -ne 'if (/KeyEvent: Key\(/) {
        my $keep = /KeyEvent: Key\((?:'"$allow"')[^)]*\)/;
        s{Key\((?!(?:'"$allow"'))[^)]*\)}{Key(<redacted>)}g;
        s{rawKey: .*origKey: [^)]*\)}{rawKey: <redacted> origKey: <redacted>};
        s{keycode: \d+}{keycode: <redacted>} unless $keep;
      } print'
```

The hotkey detector only needs `Control+space` and the adjacent
`Input method switched` / `result:` lines, so nothing of value is lost.

## Fixed along the way

fcitx5 was starting **twice**. A hand-placed
`~/.config/autostart/org.fcitx.Fcitx5.desktop` predating the home-manager
migration and the home-managed `fcitx5.desktop` both became
`app-*@autostart.service` units execing `/usr/bin/fcitx5`; they raced at login
and the loser died with `Unable to request dbus name`. The winner was
nondeterministic, and on the observed boot it was the *unmanaged* one.
`home.activation.quarantineStrayFcitx5Autostart` in `home/modules/desktop.nix`
now renames the stray file aside. Unrelated to this symptom; fixed because the
declared and running state disagreed.

## The failure is bursty, not a per-press dice roll

This corrects the earlier reading of the same data. The aggregate ratio (16 of
112 presses) invited a per-press probability model, and that model is wrong. A
later recurrence, fully traced:

```
17:28:26     logind Session.Lock
17:28:30.83  Ctrl+Space -> consumed, no switch
17:28:31.94  Ctrl+Space -> consumed, no switch
17:28:32.14  Ctrl+Space -> consumed, no switch
17:28:32.27  Ctrl+Space -> consumed, no switch
17:28:32.39  Ctrl+Space -> consumed, no switch
17:28:32.78  Ctrl+Space -> consumed, no switch      <- 6 of 6 failed
17:28:35.52  Deactivate -> Activate  (a focus event)
17:28:41.41  Ctrl+Space -> switched to mozc          <- works again
```

Six consecutive presses, 100% failure, then recovery after an unrelated focus
event. So this is a **transient state lasting seconds** during which every press
fails — not an independent chance per press.

Two practical consequences:

- **A manual escape exists**: move focus away and back (click another window,
  click back). The state clears on a focus event. That is what 17:28:35 shows.
- **`ShareInputState=All` does not help this path.** It reduces how often you
  *need* to press, which lowers ambient exposure, but during a burst you must
  press and every press fails. In the traced recurrence it could not have helped
  at all: the user had deliberately switched to `keyboard-us` at 17:28:25 before
  locking, so there was no active state to carry across.

Lock-adjacency, on the evidence available: of the two locks with logind data, one
(17:28) was followed immediately by a six-press burst, the other (16:49) by no
failures at all. Bursts also occur mid-session with no lock nearby. So the lock
is a strong aggravator, not the only trigger.

## Mitigation 1: restart fcitx5 on session lock

`systemd.user.services.fcitx5-lock-recover` in `home/modules/desktop.nix` watches
logind for `Session.Lock` and restarts fcitx5, so the daemon is fresh by the time
the screen comes back. It restarts the `app-fcitx5@autostart.service` unit rather
than exec'ing fcitx5, to keep the daemon owned by its own unit; it falls back to
`fcitx5 -r -d` when that unit is inactive (a hand-started fcitx5, e.g. while
debugging).

The lock is the only observable moment — COSMIC emits `Session.Lock` but never
`Unlock` and never calls `SetLockedHint`, so an unlock-triggered restart is not
implementable here.

**Unverified assumption**: that restarting fcitx5 *while the screen is locked*
leaves it able to grab the input method after the unlock. It has not been
observed failing, but it has not been proven either. If Japanese input is dead
immediately after every unlock from now on, this service is the first thing to
suspect — `systemctl --user stop fcitx5-lock-recover` to rule it out.

## Mitigation 2: press the trigger key less often

Reduces ambient exposure only, for the reason above.

`ShareInputState=No` (fcitx5's default) gives every application its own
active/inactive state, so with `ActiveByDefault=False` the input method drops
back to `keyboard-us` on every focus change and `Ctrl+Space` has to be pressed
again. **`ShareInputState=All`** shares one state across applications: press once,
and moving between Chrome, the terminal and Slack keeps it. Fewer presses, so
fewer encounters with the 14% failure.

`ActiveByDefault` stays `False` deliberately. `True` would remove nearly all
remaining presses, but every text field — terminals, the omnibox, password
prompts — would start in Japanese mode.

Deployed as `config/fcitx5/config` plus `home.activation.seedFcitx5Config` in
`home/modules/desktop.nix`. Seed-if-absent, not a store symlink, for the same
reason as the profile: fcitx5 rewrites the file at runtime. An existing machine
therefore needs the one-line edit by hand (`ShareInputState=All`, then
`fcitx5 -r`); new machines get it from the seed.

### Measuring whether it helped

Re-run the millisecond detector over a later session and compare presses per hour
and the failure ratio. The pre-change baseline is 112 presses, 96 switched, 16
failed (14%).
