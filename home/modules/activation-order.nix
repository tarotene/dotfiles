# An activation step that orders itself after (or before) a name that no longer exists.
#
# home-manager's activation DAG drops an `entryAfter` / `entryBefore` target that is not a
# defined step, silently: no error, no warning, and the ordering the author wrote is gone.
# That is how a step that must run after another one (a registrar that rewrites a file the
# other also rewrites, #61) loses its guarantee the day the other step is renamed or removed.
# Every target is checked here, so a stale name fails the evaluation instead.
{ config, lib, ... }:
let
  steps = config.home.activation;
  names = lib.attrNames steps;
  # `after` and `before` are lists of step names (the built-in steps — writeBoundary,
  # linkGeneration, … — are entries of the same set).
  dangling = lib.concatMap (
    name:
    let
      step = steps.${name};
    in
    map (target: "${name} → ${target}") (
      lib.filter (t: !(lib.elem t names)) ((step.after or [ ]) ++ (step.before or [ ]))
    )
  ) names;
in
{
  assertions = [
    {
      assertion = dangling == [ ];
      message = "home.activation: ordered after/before a step that does not exist (home-manager would drop it silently): ${lib.concatStringsSep ", " dangling}";
    }
  ];
}
