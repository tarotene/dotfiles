# A periodic job as one declaration: a systemd user service + timer on Linux, a
# launchd agent on darwin (ADR-0018). `timerPair { … }` returns a module; a
# module that needs a job writes `imports = [ (timerPair { … }) ];`.
#
# What it shares is the *form* every such pair repeated by hand: a oneshot
# service with the job's PATH, a timer that starts exactly that service, a
# `timers.target` install, and a launchd agent whose PATH is the same list
# (launchd *replaces* PATH, systemd's `Environment=` sets it, so both get the
# list spelled out). Everything that differs stays a parameter: the
# descriptions, the program and arguments, the PATH list, the schedule, the
# launchd side of the schedule, extra [Unit]/[Service] keys.
#
# It deliberately does not write `dotfiles.pipeline.timerNodes.<name>`. That
# one line is the pointer from the registration to the pipeline-map node, and
# the check that it exists is home/modules/pipeline.nix's assertion; a helper
# that wrote the pointer would make the pointer vanish from the call site and
# turn the assertion into something the helper satisfies by construction
# (ADR-872 Amendment 2026-10-27: a helper as the enforcement point was rejected
# because a raw `systemd.user.timers.*` bypasses it; the form-sharing helper is
# outside that ruling, and a raw definition is still caught by the assertion).
#
# `schedule` has two closed shapes, so a dead declaration cannot be written:
#   { calendar; persistent ? false; randomizedDelay ? null; }   OnCalendar=
#   { afterBoot; every; accuracy ? null; randomizedDelay ? null; } OnBootSec= + OnUnitActiveSec=
# `Persistent=` only does anything for a calendar timer (systemd.timer(5);
# the #442 lesson: a timer with only OnBootSec=/OnUnitActiveSec= and
# `Persistent = true` re-fired on every login), so only the calendar shape
# accepts `persistent`. Any other key fails the evaluation.
{ lib, pkgs }:
{
  # Unit name: `<name>.service`, `<name>.timer`, and the launchd agent's name.
  name,
  description,
  timerDescription,
  # The service's program (systemd ExecStart=) and its arguments.
  exec,
  args ? [ ],
  # The launchd agent's program, when it differs from the service's (a service that runs a
  # deployed link in ~/.local/bin while the agent runs the store path).
  launchdExec ? exec,
  # PATH of the job: a `lib.makeBinPath` string. `launchdPath` when darwin needs another list
  # (darwin has no util-linux, and a script that shells out to grep needs gnugrep there).
  path,
  launchdPath ? path,
  # False leaves the whole job out (a job gated on an option).
  enable ? true,
  # Extra keys of the service's [Unit] and [Service] (X-SwitchMethod, TimeoutStartSec, …).
  unit ? { },
  service ? { },
  schedule,
  # launchd's schedule and its extra config: StartInterval / StartCalendarInterval,
  # RunAtLoad, StandardOutPath, …
  launchd,
}:
let
  calendarKeys = [
    "calendar"
    "persistent"
    "randomizedDelay"
  ];
  elapsedKeys = [
    "afterBoot"
    "every"
    "accuracy"
    "randomizedDelay"
  ];
  isCalendar = schedule ? calendar;
  allowed = if isCalendar then calendarKeys else elapsedKeys;
  unknown = lib.filter (k: !(lib.elem k allowed)) (lib.attrNames schedule);
  optionalKey = key: value: lib.optionalAttrs (value != null) { ${key} = value; };

  timerKeys =
    if isCalendar then
      {
        OnCalendar = schedule.calendar;
      }
      // lib.optionalAttrs (schedule.persistent or false) { Persistent = true; }
      // optionalKey "RandomizedDelaySec" (schedule.randomizedDelay or null)
    else
      {
        OnBootSec = schedule.afterBoot;
        OnUnitActiveSec = schedule.every;
      }
      // optionalKey "AccuracySec" (schedule.accuracy or null)
      // optionalKey "RandomizedDelaySec" (schedule.randomizedDelay or null);
in
assert lib.assertMsg (
  isCalendar || (schedule ? afterBoot && schedule ? every)
) "timerPair ${name}: schedule is { calendar; … } or { afterBoot; every; … }";
assert lib.assertMsg (unknown == [ ])
  "timerPair ${name}: schedule key(s) ${lib.concatStringsSep ", " unknown} not allowed for this shape (allowed: ${lib.concatStringsSep ", " allowed}; `persistent` is only for a calendar timer, #442)";
{
  config = lib.mkMerge [
    (lib.mkIf (enable && pkgs.stdenv.isLinux) {
      systemd.user.services.${name} = {
        Unit = {
          Description = description;
        }
        // unit;
        Service = {
          Type = "oneshot";
          ExecStart = lib.concatStringsSep " " ([ exec ] ++ map lib.escapeShellArg args);
          Environment = "PATH=${path}";
        }
        // service;
      };
      systemd.user.timers.${name} = {
        Unit.Description = timerDescription;
        Timer = timerKeys // {
          Unit = "${name}.service";
        };
        Install.WantedBy = [ "timers.target" ];
      };
    })
    # home-manager's launchd module asserts `agentPlists != {} -> isDarwin`, so the agent must not
    # be declared at all on Linux; the mkIf is required, not just documentation.
    (lib.mkIf (enable && pkgs.stdenv.isDarwin) {
      launchd.agents.${name} = {
        enable = true;
        config = {
          ProgramArguments = [ launchdExec ] ++ args;
          EnvironmentVariables.PATH = launchdPath;
        }
        // launchd;
      };
    })
  ];
}
