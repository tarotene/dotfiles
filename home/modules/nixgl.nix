# Shared nixGL wrapper (#13 / ADR-0006). Extracted out of desktop.nix (#9) so
# both desktop.nix's system-wide Linux GUI apps and identity-scoped modules
# (personal.nix's cloud-AI tools, which must not land on company hosts —
# see that module's warp-terminal entry) can wrap a package without
# duplicating this derivation-building logic.
#
# nix-built GL applications look for their driver under /run/opengl-driver,
# a NixOS-only path that does not exist on these Pop!_OS hosts, so EGL never
# initializes: alacritty exits before opening a window, and Chrome/Slack/Zoom
# survive by silently falling back to software rendering
# (`--use-gl=disabled`). Pointing the loader variables at the *system* mesa
# does not fix it — measured: /usr/share/glvnd/egl_vendor.d/50_mesa.json
# names a bare "libEGL_mesa.so.0", which a nix process cannot resolve, and
# making it resolvable would mean loading glibc-2.39-linked drivers into a
# glibc-2.42 process. nix's own mesa in the closure is therefore not a
# preference, it is the only option.
#
# Wrap only the packages that actually need it (per-package, not mapped over
# a whole list) — a blanket wrap would silently pull a 1.1 GiB GL closure
# onto the next GUI package that has nothing to do with GL.
#
# bin/* is wrapped *and* share/applications/*.desktop is rewritten, because
# the two entry points differ per package: some ship `Exec=<name>` and
# resolve through PATH (alacritty, zoom, warp-terminal), others hardcode an
# absolute store path (Chrome, Slack) — without the rewrite the latter would
# keep launching unwrapped from a desktop launcher while working fine from a
# shell.
#
# Caveat: the result is a plain derivation, so passthru and .override are
# lost. Apply any .override to the package *before* handing it to nixGLWrap.
{ pkgs }:
pkg:
pkgs.runCommand "${pkg.name}-nixgl"
  {
    nativeBuildInputs = [ pkgs.makeWrapper ];
    inherit (pkg) meta;
  }
  ''
    mkdir -p "$out/bin"

    # Everything except bin/ and share/ can stay a symlink to the original.
    for entry in ${pkg}/*; do
      name="$(basename "$entry")"
      case "$name" in
        bin | share) ;;
        *) ln -s "$entry" "$out/$name" ;;
      esac
    done

    for binary in ${pkg}/bin/*; do
      makeWrapper ${pkgs.nixgl.nixGLIntel}/bin/nixGLIntel \
        "$out/bin/$(basename "$binary")" \
        --add-flags "$binary"
    done

    if [ -d ${pkg}/share ]; then
      cp -r ${pkg}/share "$out/share"
      chmod -R u+w "$out/share"

      # Repoint absolute Exec=/TryExec= at the wrappers. A plain prefix
      # substitution covers both, and any other absolute reference into
      # bin/ that a desktop entry may carry.
      for desktop in "$out"/share/applications/*.desktop; do
        [ -e "$desktop" ] || continue
        substituteInPlace "$desktop" --replace-quiet "${pkg}/bin/" "$out/bin/"
      done
    fi
  ''
