# Ctrl+Space silently fails — investigation record

Status: **cause located; the upstream fix is identified and released; verification
on this host pending.** A measurement log, not a decision. Two separate defects
were found; several plausible-looking hypotheses were refuted along the way and
are recorded so nobody re-treads them.

Tracked in [#14](https://github.com/tarotene/dotfiles/issues/14).

> **The blind spot, in one line.** This investigation never asked what version of
> fcitx5 it was measuring. It was apt's **5.1.7** — tagged 2024-01-16, capped
> there by noble/universe, and **just over two years older than the upstream fix**.
> See "The version axis" below; read it before spending any more time on the
> traces.

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

Over one session: **112 presses, 96 switched, 16 failed (14%)**, all 16 consumed
(`result:1`). The failures come in bursts — the user pressing repeatedly because
nothing happened.

(An earlier version of this line, and the code block in
[#14](https://github.com/tarotene/dotfiles/issues/14), said 102/86/16 — that was
the same session counted while it was still running. The 16 failures are the same
16; only the successes kept accumulating. **112/96/16 is the baseline**, and it is
the figure "Measuring whether it helped" compares against. See also the note under
Instrumentation: the current detector splits one of those 16 into a `DEFERRED`
bucket.)

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

## The version axis — the question nobody asked

Everything above was measured against apt's **fcitx5 5.1.7-1build3**, upstream tag
`5.1.7` of **2024-01-16**. Pop!_OS 24.04 inherits noble/universe, where that is
both the installed and the candidate version — `apt-cache policy fcitx5` shows no
other: there is no newer fcitx5 available through apt at all. Upstream was at
5.1.21 (2026-06-26) by the time this was written, fourteen releases later.

**The defect is fixed upstream**, by
[`c2c757f0e3d4`](https://github.com/fcitx/fcitx5/commit/c2c757f0e3d4) —
"Force focus in on wayland keyevent." (2026-02-12), whose commit message ends
`Fix #1503`:

```diff
--- a/src/frontend/waylandim/waylandimserverv2.cpp
+++ b/src/frontend/waylandim/waylandimserverv2.cpp
-    if (!realFocus()) {
-        focusInWrapper();
-    }
+    focusInWrapper();
```

The commit also adds the same unconditional `focusInWrapper()` to
`waylandimserver.cpp`, the input-method-v1 frontend. The hunk above is the one
that matters here: this host's Chrome and Slack are on `wayland_v2`.

The commit message explains the mechanism:

> keyevent is triggered by grab, so if there's a grab, there should be a focus.
> When a non-ti client alt tab to another client, the focus might be steal away
> temporarily. We need ensure key event can take back the focus just like other
> frontend.

That is the same conclusion this document reached at "Consistent with fcitx5
applying the toggle to an input context that is not the one actually focused at
that instant" — arrived at independently, from the traces.

First release containing it, by `git` tag containment:

| tag | date | contains `c2c757f0e3d4` |
|---|---|---|
| 5.1.16 | 2025-10-26 | no |
| 5.1.17 | 2025-12-22 | no |
| **5.1.18** | **2026-03-17** | **yes** |
| 5.1.19 – 5.1.21 | 2026-03-18 … 2026-06-26 | yes |

### Upstream fcitx/fcitx5#1503 is a near-duplicate

Its reproducer: focus **Firefox's address bar**, **Alt+Tab** to Alacritty, press
`Ctrl+Space` — nothing happens. Its workarounds: press Super twice, or open and
close the launcher, or switch via the panel icon instead of Alt+Tab. That is the
same "move focus away and back" escape recorded below, on KDE rather than COSMIC —
so **the defect is not COSMIC-specific**, and the compositor was never a candidate
for Defect 1. Note also that switching via the panel icon does *not* reproduce it:
a **keyboard** window switch is part of the trigger, not a mouse one.

### Why this host is a good place to hit it

`c2c757f0e3d4` is about a key arriving through a live grab while fcitx5's manager
considers some *other* input context focused. This host runs a **mixed-frontend
session**, which manufactures exactly that situation:

| client | provenance | fcitx5 frontend |
|---|---|---|
| ghostty (terminal), Firefox | apt + `GTK_IM_MODULE=fcitx` → `im-fcitx5.so` | `dbus` |
| **Chrome, Slack Desktop** | nix, native Wayland | **`wayland_v2`** |

Verified from `/proc/<pid>/maps` (both apt clients map `im-fcitx5.so`) and from
`org.fcitx.Fcitx.Controller1.DebugInfo`, which lists four `frontend:dbus` input
contexts alongside a single `frontend:wayland_v2` one.

This is what explains the symptom's shape: **Chrome and Slack are the only
`wayland_v2` clients here**, so they are the only ones that can be the victim.
Everything else talks to fcitx5 over D-Bus. "It presents as Chrome-only" was not a
coincidence and not a Chrome bug — it is the set of clients on the affected code
path.

### What this does not yet establish

That 5.1.18+ actually fixes it *here*. Commit archaeology is not measurement, and
this document has already retracted three conclusions that were reached by
plausible reasoning from real evidence (see "Refuted hypotheses"). The verification
is a three-arm comparison — apt 5.1.7, nix 5.1.16 (pre-fix), nix 5.1.19 (post-fix)
— under the keyboard-switch provocation above, scored per *trial* rather than per
press for the reason given in "The failure is bursty". Results will be recorded
here.

## Second, separate defect: cosmic-comp sends a duplicate `enter`

**Status: unreported upstream, deliberately deferred.** Not withdrawn and not
refuted — it is real and it is benign in isolation, so it lost to other work. The
evidence is kept here so a later decision to report it does not start from zero.

Real, reproducible on demand, and benign in isolation. When any client creates a
`zwp_text_input_v3`, the compositor sends `enter` to an already-entered client
with no intervening `leave` — which text-input-v3 forbids.

Minimal reproducer, both programs first-party (`timtest/timwin.c` was a focusable
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

**What reporting this would cost now.** Both `timtest` programs are gone — they
lived in the scratchpad described under Instrumentation and were never committed.
They would have to be rewritten, and this time committed. Without them the report
is a trace excerpt plus a citation of the protocol text, which is likely to be
answered with a request for a reproducer.

Do not fold this into any fcitx5 report: the hypothesis that this duplicate
`enter` is what breaks input was **tested and refuted** (see "Refuted
hypotheses"), and offering a refuted cause is a good way to have the rest of a
report ignored.

## Not faults

- fcitx5 reverting to `keyboard-us` when focus moves is `ActiveByDefault=False`
  plus `ShareInputState=No` behaving as configured; keys then passing through
  with `result:0` is correct. (`ShareInputState` is now `All` — see the mitigation
  section — so this describes the state at the time of the measurements above,
  not the current configuration.)
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

Every entry in that table is a hypothesis that was *formed and then tested*. The
costlier mistake was of a different kind: a question never asked at all. Six of
those seven rows are about **where** the defect lived — which client, which
protocol, which process — and none of them asked **when**: what version was
running, and whether anyone upstream had already fixed it. Checking that costs one
`apt-cache policy` and one look at the upstream log, and it would have come first
in the right order. See "The version axis".

## Instrumentation

The redaction filter and the detector are **`scripts/fcitx5-key-trace.pl`**, in
this repo, covered by `--selftest` in CI.

They did not used to be. The first round of this investigation kept them in a
session scratchpad under `ime-diag/`, along with the `timtest/` reproducers; the
scratchpad went away and took every one of them with it, which left every number
below unreproducible and the Defect 2 reproducer unrecoverable. That is why the
tool is committed, and why its `--selftest` fixtures are the log excerpts quoted
in this document: if the detector and this document ever disagree about what those
traces mean, CI fails.

Both halves of the protocol must be traced — tracing only the client is what
made this look like a Chrome bug for hours:

```bash
# input-method half (stop the unit first; see "What to do when it happens")
WAYLAND_DEBUG=1 /usr/bin/fcitx5 -D --verbose 'default=5,key_trace=5' 2>&1 \
  | perl scripts/fcitx5-key-trace.pl --redact --stamp >> trace.log
# client half
WAYLAND_DEBUG=1 google-chrome-stable 2>&1 | ...
```

`-r` is deliberately **not** in that command. Stop
`app-fcitx5@autostart.service` first, so there is nothing to replace; `-r` is the
flag that produced the dead-unit state described below, and `-D` (do not
daemonize) is already the default.

Then:

```bash
perl scripts/fcitx5-key-trace.pl --report < trace.log
perl scripts/fcitx5-key-trace.pl --report --trials trials.tsv < trace.log
```

Two details that are easy to get wrong, both handled by `--redact --stamp`:
filter the per-frame flood at write time (otherwise a multi-hour trace is tens of
GB rather than a few MB), and prefix wall-clock timestamps — libwayland's own
`[ 906672.653]` stamp is on a clock that matches nothing else on the system.

The detector operates at millisecond resolution on adjacent log lines: for each
`Key(Control+space ... Release:0)`, look ahead ~250 ms for `Input method
switched`. Per-second bucketing cannot see this failure, because several switches
can occur inside one second and the user's repeat presses land in the same second
as the failure.

It sorts each press into three buckets, not two:

| bucket | meaning |
|---|---|
| `SWITCHED` | the input method changed within the window |
| `FAILED` | nothing happened; `consumed` records `result:1` vs `result:0` |
| `DEFERRED` | the switch arrived late, attached to a focus event |

`DEFERRED` exists because of the 16:24:56 case below: counting it as a success
and counting it as a failure are both wrong. Its boundary is a judgement call —
distinguishing "the toggle was applied late" from "an unrelated focus event
cleared the state and a later press did the work" is not possible from the log
alone, since both look like a focus event followed by an `Activate`. The tool uses
a latency bound (`--defer-ms`, default 2000), which puts the 1.4 s case in
`DEFERRED` and the 2.7 s recovery of the six-press burst in `FAILED` — the way
this document reads both. Two selftest fixtures pin exactly that boundary.

One consequence worth stating plainly: **under this detector the baseline session
reads 15 failed + 1 deferred, not 16 failed.** The "16" quoted throughout predates
the distinction. The burst grouping is unaffected (5, 7, 2, 1, 1), and that
grouping is itself a selftest fixture built from the failure timestamps recorded
in [#14](https://github.com/tarotene/dotfiles/issues/14).

Failures are grouped into **bursts** — consecutive failures no more than
`--gap-ms` apart — because a burst is one latched episode observed repeatedly,
not N independent samples. See "The failure is bursty" below for why that
distinction decides how the numbers may be read.

Key synthesis is **not** available for automating the check: `wtype` exits 0 but
its keys never reach fcitx5, because the compositor excludes virtual-keyboard
input from the input-method grab (otherwise fcitx5's own key forwarding would
loop). A real keystroke is unavoidable.

### Running a different fcitx5 version alongside apt's

Needed to test the version axis, and the one place with a real trap. apt caps
fcitx5 at 5.1.7, so a newer build has to come from nixpkgs:

```bash
nix build --no-link --print-out-paths --impure --expr '
  let p = import (builtins.getFlake "github:NixOS/nixpkgs/<rev>") {
            system = "x86_64-linux"; config.allowUnfree = true; };
  in p.qt6Packages.fcitx5-with-addons.override {
       addons = [ p.fcitx5-mozc ]; withConfigtool = false; }'
```

`withConfigtool = false` drops the configtool's KDE Qt6 closure: 25 paths / 44 MiB
instead of 84 / 101 MiB, and only the `symlinkJoin` is built locally.

**Stop `app-fcitx5@autostart.service` first** and confirm `pgrep -xc fcitx5` is 0.
Two fcitx5 processes cannot both hold the D-Bus name, and the loser leaves the
unit dead — see "Withdrawn, retryable under conditions".

Three environment variables matter, and getting the second one wrong is a silent
failure rather than a loud one:

| variable | why |
|---|---|
| `FCITX_DATA_DIRS=$W/share/fcitx5` | **the trap.** Addon *descriptors* (`addon/*.conf`) are a `pkgdatadir`-type path, so they fall back to every `XDG_DATA_DIRS` entry — which puts `/usr/share/fcitx5/addon` *ahead* of the nix one. `AddonManager::load()` keeps the first match per filename, and `checkDependencies` passes `core:5.1.7 <= 5.1.19`, so apt's 5.1.7 descriptors are used against the newer core **without any error**. Setting this replaces that list; the core's own `share/fcitx5` is still appended automatically because `"pkgdatadir"` starts with `pkg`. Leave `XDG_DATA_DIRS` alone — classicui needs `/usr/share/icons`. |
| `XDG_CONFIG_HOME=$LAB/xdgconfig` | the **only** way to isolate mozc. `mozc_server` has no `MOZC_*` profile override; it reads `XDG_CONFIG_HOME`/`HOME`, and `fcitx5-mozc.so` spawns it so it inherits this. Without it, mozc 2.30 rewrites `~/.config/mozc`, whose `LRUStorage` files (`.history.db`, `segment.db`, …) are **silently discarded and recreated** on a version mismatch — learned-conversion loss, and there is no seed for them anywhere in this repo. Take a tar backup regardless. |
| `FCITX_ADDON_DIRS` (unset it) | the `.so` path. `fcitx5-with-addons` sets it with `--prefix`, not `--set`, so `env -u FCITX_ADDON_DIRS` guarantees exactly one directory. It happens to *replace* the compiled-in default rather than prepending (`"addondir"` does not start with `pkg`), so apt's `.so` files cannot be dlopen'd either way — but do not rely on the ambient variable staying empty. |

Verify what is actually executing, from `/proc`, not from the log:

```bash
P=$(pgrep -x fcitx5)
readlink /proc/$P/exe
grep -cE '/usr/lib/x86_64-linux-gnu/(fcitx5/|libFcitx5)' /proc/$P/maps   # must be 0
grep -E 'Loaded addon (waylandim|wayland|classicui|mozc)|Could not load addon' trace.log | sort -u
```

The apt daemon maps 19 `.so` under `/usr/lib/x86_64-linux-gnu/fcitx5/` plus
`libFcitx5{Core,Config,Utils}.so.5.1.7`; under a correctly fenced nix arm that
count is 0. The store path in `/proc/$P/exe` is the version proof.

### `key_trace` captures the login password — redact before it hits disk

`--verbose key_trace=5` logs **every** keystroke, and fcitx5 keeps its keyboard
grab while the COSMIC lock screen is up. An unfiltered trace therefore records
the password typed at the lock screen, in clear. This happened during the
investigation and had to be scrubbed after the fact.

**Use `scripts/fcitx5-key-trace.pl --redact`.** Do not hand-roll the filter — an
earlier version of this section did, and it was incomplete in two ways that are
easy to miss.

There are **two** channels carrying key material, not one:

1. fcitx5's own `KeyEvent: Key(<keysym>)` lines, plus the adjacent `rawKey:`,
   `origKey:` and `keycode:` fields. The `keycode:` field identifies the physical
   key just as precisely as the keysym does.
2. **`WAYLAND_DEBUG`'s `.key(serial, time, keycode, state)` — field 3 is a raw
   evdev keycode.** The excerpt at the top of this document
   (`…grab_v2#21.key(176, 12201550, 57, 1)`, where `57` is `KEY_SPACE`) is
   exactly the shape a password character takes here. **The earlier filter did
   not touch this channel at all**, so any trace taken with `WAYLAND_DEBUG=1` —
   which is the whole point of tracing both halves — still recorded the password
   as a keycode sequence.

Two further properties the earlier filter did not have:

- It rewrote non-allowlisted keysyms to `Key(<redacted>)`, keeping one line per
  keystroke. That **leaks the length** of whatever was typed. The tool drops the
  line entirely instead.
- It passed through anything it failed to match. The tool **fails closed**: a line
  that looks like it carries key material but cannot be decomposed is dropped.

The allow-list is also tighter than the one printed here before: `Shift_L` is out
(it reveals which positions were capitalised) and so is `BackSpace` (it reveals
corrections, hence length). `Return` stays — a password cannot contain it, and it
marks the moment a lock screen was dismissed, which is what correlates a trace
with a lock cycle.

By default the tool redacts **every** `.key()` keycode, including the trigger's.
`--keep-trigger-keycodes` keeps `29`/`97`/`57` (both Controls and space) for the
one case that needs them: a short, deliberate capture showing that the grab was
live and delivering at the instant of a failure — the excerpt an upstream report
wants. Residual exposure when that flag is used: a secret containing a space
would have those positions visible.

Nothing of value is lost for the detector either way: it works entirely off
`Control+space` and the adjacent `Input method switched` / `result:` lines.

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

## Withdrawn, retryable under conditions: restarting fcitx5 on session lock

> **Motivation largely removed.** This workaround exists to dodge the trigger-key
> failure, and that failure is fixed upstream in 5.1.18 (see "The version axis").
> If the three-arm verification confirms it, there is nothing left for this to
> work around and it should not be retried. Its one open assumption — that
> restarting fcitx5 while the screen is locked leaves it able to grab the input
> method after the unlock — stays **unproven either way**, so if some future
> reason to restart fcitx5 on lock appears, the preconditions and design changes
> below still apply.

Implemented, shipped, and taken back out. Recorded because the reasoning looked
sound, and because the first write-up of *why* it failed was itself wrong.

The idea: watch logind for `Session.Lock` and restart fcitx5, so the daemon is
fresh by the time the screen comes back. The lock is the only observable moment —
COSMIC emits `Session.Lock` but never `Unlock` and never calls `SetLockedHint`.

### What actually happened

After a reboot, Japanese input stopped recovering from lock/unlock at all, and
the autostart unit was found dead:

```
11:02:03  app-fcitx5@autostart.service: ExecStart=/usr/bin/fcitx5
          "Failed to create addon: dbus Unable to request dbus name.
           Is there another fcitx already running?"
          -> unit inactive (dead)
11:02:22  fcitx5 -r -d running with PPID 1, outside any fcitx5 unit
```

The detached `fcitx5 -r -d` was **started by hand**, not by the service — its
cgroup was a terminal scope, not the watcher's. (The first version of this
section blamed the service's fallback branch for it. That was a misattribution:
the cgroup contradicted the claim and was not followed up.)

The real sequence:

1. fcitx5 was restarted by hand with `fcitx5 -r -d` — `-r` replaces the running
   instance, so the unit's `ExecStart` process exits and the unit goes inactive;
   `-d` daemonizes, so the survivor sits outside any fcitx5 unit.
2. On the next lock the service ran
   `systemctl --user restart app-fcitx5@autostart.service`.
3. The unit started `/usr/bin/fcitx5`, could not take the D-Bus name because the
   hand-started instance held it, and exited — leaving the unit dead.

With the unit dead there is nothing for the next lock to restart, so a failure
that used to clear itself within seconds became one that persists until fcitx5 is
restarted manually. **That** is the reason to keep it out: the workaround made the
failure worse in a state that is easy to reach.

### Why it stays out for now

- **Its central assumption is still unproven**: that restarting fcitx5 while the
  screen is locked leaves it able to grab the input method after the unlock.
  Nothing observed either confirms or refutes it.
- **It cannot coexist with hand-started fcitx5 instances**, which are exactly what
  gets used while debugging this bug.
- **It was only ever exercised on one branch.** Before the reboot the autostart
  unit was inactive (fcitx5 had been hand-started for tracing), so every
  observation came from the fallback path. The unit-restart path — the one that
  runs in normal operation — first executed after the reboot. A change whose
  behaviour depends on how fcitx5 was started has to be tested across a reboot.

### Conditions for retrying it

Not a dead end, but it may only be retried from a **clean state**, verified
first, and the retry must answer the open assumption rather than assume it.

Preconditions, all four checked immediately before the test:

```bash
# 1. exactly one fcitx5, and it belongs to its unit
pgrep -xc fcitx5                                    # expect 1
cat /proc/"$(pgrep -x fcitx5)"/cgroup                # expect app-fcitx5@autostart.service
# 2. the unit is the owner, not a leftover
systemctl --user is-active app-fcitx5@autostart.service   # expect active
# 3. no hand-started instance anywhere
ps -eo args | grep -c '[f]citx5 -r'                  # expect 0
# 4. session freshly rebooted, so the unit path is the one under test
uptime -p
```

The test itself must confirm the assumption directly, not by feel: restart fcitx5
**while the screen is locked**, then after unlocking check that its Wayland side
came up and that the input method is reachable.

```bash
journalctl --user -u app-fcitx5@autostart.service -b \
  | grep -E 'Loaded addon (waylandim|mozc)|classicui for wayland|Unable to request'
```

If `waylandim` and `classicui for wayland` are absent, or `Unable to request dbus
name` appears, the assumption is refuted and the approach is finished.

Design changes any retry should carry:

- **No direct-exec fallback.** Restart the unit or do nothing. The fallback is
  what makes a race with a hand-started instance possible in the first place.
- **Refuse to act when the state is not clean** — if `pgrep -xc fcitx5` is not 1,
  or the running fcitx5 is outside the unit's cgroup, log and skip rather than
  restart into a name conflict.

### Recovery, if this state is reached again

```bash
systemctl --user stop fcitx5-lock-recover 2>/dev/null   # if it still exists
pkill -x fcitx5
systemctl --user start app-fcitx5@autostart.service
journalctl --user -u app-fcitx5@autostart.service -b \
  | grep -E 'Loaded addon (waylandim|mozc)|classicui for wayland'
```

## What to do when it happens

- **Move focus away and back** (click another window, click back). The transient
  state clears on a focus event — that is what the 17:28:35 recovery shows.
- If it persists, restart fcitx5 **through its unit**:

  ```bash
  systemctl --user restart app-fcitx5@autostart.service
  ```

- **Do not use `fcitx5 -r -d`.** `-r` replaces the running instance, so the unit's
  `ExecStart` process exits and the unit goes inactive; `-d` daemonizes, so the
  survivor ends up outside any fcitx5 unit with PPID 1. The result works, but
  fcitx5 is no longer owned by systemd, and anything that later restarts the unit
  hits a D-Bus name conflict and leaves the unit dead. That is how the state
  described above was reached.

## The one mitigation that stayed: press the trigger key less often

Reduces ambient exposure only, for the reason above. It is the only lever left
after the lock-triggered restart was withdrawn.

`ShareInputState=No` (fcitx5's default) gives every application its own
active/inactive state, so with `ActiveByDefault=False` the input method drops
back to `keyboard-us` on every focus change and `Ctrl+Space` has to be pressed
again. **`ShareInputState=All`** shares one state across applications: press once,
and moving between Chrome, the terminal and Slack keeps it. Fewer presses means
fewer encounters — it does not make any individual press more reliable.

`ActiveByDefault` stays `False` deliberately. `True` would remove nearly all
remaining presses, but every text field — terminals, the omnibox, password
prompts — would start in Japanese mode.

Deployed as `config/fcitx5/config` plus `home.activation.seedFcitx5Config` in
`home/modules/desktop.nix`. Seed-if-absent, not a store symlink, for the same
reason as the profile: fcitx5 rewrites the file at runtime. An existing machine
therefore needs the one-line edit by hand (`ShareInputState=All`, then
`systemctl --user restart app-fcitx5@autostart.service` — **not** `fcitx5 -r`, for
the reason in "What to do when it happens"); new machines get it from the seed.

It stays even if the version fix lands. Sharing the input-method state across
applications is a reasonable setting on its own terms — one press instead of one
per focus change — independently of whether any individual press is reliable.

### Measuring whether it helped

Re-run the detector over a later session and compare presses per hour and the
failure ratio:

```bash
perl scripts/fcitx5-key-trace.pl --report < trace.log
```

The pre-change baseline is 112 presses, 96 switched, 16 failed (14%); under the
current detector that reads 15 failed + 1 deferred (see Instrumentation).

Note what this measurement can and cannot show. `ShareInputState=All` changes
*presses per hour*, not the per-press failure rate, so the ratio is the comparable
number and it should be roughly unchanged. A drop in the ratio would be evidence
about something else — most likely the version — and should not be attributed to
this setting.
