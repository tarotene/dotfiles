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
# bin/* is wrapped, and share/applications/*.desktop is rewritten file-by-file
# rather than via a `cp -r`'d whole tree, because the two entry points differ
# per package (some ship `Exec=<name>` resolved through PATH — alacritty,
# zoom, warp-terminal — others hardcode an absolute store path — Chrome,
# Slack — and the rewrite is needed for the latter) *and* because a bulk
# `cp -r ${pkg}/share "$out/share"; chmod -R u+w "$out/share"` was bisected
# (adding chromium as a consumer, home/modules/line.nix) to intermittently
# fail deep in the build sandbox — "Operation not permitted" chmod'ing
# share/applications/chromium-browser.desktop, a file the same builder just
# created moments earlier, not reproducible outside the sandbox on an
# identical copy. It reproduces only when copying the *entire* share/ tree
# (chromium's alone is tens of thousands of files: locales, multi-resolution
# icon themes, native-messaging manifests); a single-file cp+chmod of the
# very same source file never failed, however many times repeated. The exact
# sandbox-internal mechanism was not pinned down (a nix-daemon store
# optimisation pass racing the in-progress build is the leading suspect,
# given the failure's sensitivity to how many files are touched and its
# absence outside the sandbox), so this works around it rather than
# explaining it: only the handful of *.desktop files that actually need
# patching are copied+chmod'd (proven safe per-file), everything else in
# share/ is symlinked straight to the original, matching the top-level
# loop's existing treatment of every non-bin/non-share entry below.
#
# Caveat: the result is a plain derivation, so passthru and .override are
# lost. Apply any .override to the package *before* handing it to nixGLWrap.
{ pkgs }:
pkg:
pkgs.runCommand "${pkg.name}-nixgl" { nativeBuildInputs = [ pkgs.makeWrapper ]; } ''
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
    mkdir -p "$out/share"

    # Symlink everything in share/ except applications/ — see header for
    # why this avoids a bulk `cp -r` of the whole (potentially huge) tree.
    for entry in ${pkg}/share/*; do
      name="$(basename "$entry")"
      case "$name" in
        applications) ;;
        *) ln -s "$entry" "$out/share/$name" ;;
      esac
    done

    if [ -d ${pkg}/share/applications ]; then
      mkdir -p "$out/share/applications"
      for entry in ${pkg}/share/applications/*; do
        name="$(basename "$entry")"
        case "$name" in
          # Only *.desktop files need Exec=/TryExec= repointed at the
          # wrappers, so only they get materialized (copied, then
          # chmod'd+patched individually — proven reliable per-file even
          # when the same operation over the whole tree was not).
          *.desktop)
            cp "$entry" "$out/share/applications/$name"
            chmod u+w "$out/share/applications/$name"
            substituteInPlace "$out/share/applications/$name" --replace-quiet "${pkg}/bin/" "$out/bin/"
            ;;
          *) ln -s "$entry" "$out/share/applications/$name" ;;
        esac
      done
    fi
  fi
''
