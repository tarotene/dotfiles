# Setup Guide

Bring a Pop!_OS host up to the declarative home-manager environment. For the
full per-host migration procedure (existing machines, rollback, greenfield
details) see [`cutover-runbook.md`](cutover-runbook.md). For a macOS host
(darwin, ADR-0018), see [`setup-macos.md`](setup-macos.md) instead — the
system layer, IME, and terminal wrapping all differ from what follows here.

## Prerequisites

- Pop!_OS 24.04 LTS
- Internet connection
- Your YubiKey (for identity: git signing, SSH, secret decryption)

## Greenfield host (fresh install)

One command installs Nix, the system-layer apt packages, and runs
home-manager:

```bash
curl -fsSL https://raw.githubusercontent.com/tarotene/dotfiles/main/bootstrap.sh | bash
```

Or clone first and run locally (add `--dry-run` to preview):

```bash
git clone https://github.com/tarotene/dotfiles.git ~/dotfiles
cd ~/dotfiles
./bootstrap.sh
```

`bootstrap.sh`:

1. Installs Nix via the Determinate Systems installer (multi-user default).
2. Installs the system-layer apt packages (`scripts/install-packages.sh`).
3. Runs `home-manager switch --flake .#"$(hostname)"`. The hostname must match a
   key in `homeConfigurations` in `flake.nix` (e.g. `personal-pop`,
   `company-pop-old`, `company-pop-new`).
4. Registers the Nix-provided zsh in `/etc/shells` (idempotent).

## Existing host

To migrate a machine that already has the old procedural dotfiles, follow the
**Existing host cutover** procedure in
[`cutover-runbook.md`](cutover-runbook.md): install system packages,
run `home-manager switch -b backup` (backs up any file collisions), switch your
login shell, verify, then clean up the `.bak` files.

## Manual steps (identity — hardware-bound)

These cannot be declarative because identity is rooted in the YubiKey
(ADR-0003):

```bash
# 1. Insert the YubiKey, then bind the card:
gpg --card-status
gpg --import keys/*.pub        # if not already imported at activation
gpg --edit-key <KEYID>         # trust → 5 (ultimate) → quit

# 2. Set the per-host git signing key (in home/hosts/<hostname>.nix), then:
home-manager switch --flake .#"$(hostname)"

# 3. Switch your login shell to the Nix-provided zsh:
NIX_ZSH="$HOME/.nix-profile/bin/zsh"
grep -qxF "$NIX_ZSH" /etc/shells || echo "$NIX_ZSH" | sudo tee -a /etc/shells >/dev/null
chsh -s "$NIX_ZSH"             # log out and back in to take effect
```

## Manual step: esa MCP token (host-local, personal identity only)

The esa.io MCP server (`@esaio/esa-mcp-server`, registered via
`home/modules/esa.nix`) reads its token from `~/.config/esa/token.gpg`,
encrypted to the personal identity's master key — GnuPG resolves this to
whichever [E] subkey is currently valid (ADR-0003 Amendment 4,
[ADR-0022](adr/0022-esa-mcp-host-local-gpg-secret.md)). The file is
host-local and never enters git. Personal hosts only (`personal-pop`,
`altair`) — company hosts do not import this module. The token only needs
issuing once: the same encrypted `.gpg` blob can be copied verbatim to
every personal host — no per-host re-encryption needed.

**Card-free decryption (recommended, #252)**: run
`scripts/gpg-subkey generate --key <personal-fingerprint> --usage encrypt`
once per host to cut a per-machine on-disk [E] subkey (mirrors the existing
on-disk [S] subkey this repo already uses for signing). GnuPG then prefers
this newest on-disk [E] over the YubiKey's card-backed one automatically —
no `--recipient` change needed on the encryption side. Without this step,
decryption still works but requires the YubiKey inserted every time
`gpg-agent`'s cache is cold (once per login, same shape as signing before
this step existed). The card's original [E] is never revoked by this —
it stays available as a fallback.

**On esa.io** — confirmed against esa-mcp-server's README and esa's PAT v2
docs (2026-09-19). esa's own token-creation screen is screenshot-only in
its docs (no exact URL or field-label text is published), so the menu path
below is quoted verbatim from <https://docs.esa.io/posts/559> but the
literal URL/field names are unverified — if a step below doesn't match
what's on screen, the scope/name values are still the ones to use:

1. Navigate: **SETTINGS > その他 > パーソナルアクセストークン・OAuth >
   新しい PAT v2 を作成**.
2. Name/description field (if the form has one): `dotfiles-esa-mcp`.
3. Scopes — select exactly esa-mcp-server's documented minimum (least
   privilege over the blanket `read write`):
   `read:post` `write:post` `read:category` `read:tag` `read:attachment`
   `read:team` `read:member` `admin:comment`.
4. Expiry: esa's docs don't describe this field either way. If one is
   offered, "no expiry" avoids a silent renewal-day breakage; otherwise
   note the date so a future rotation isn't a surprise.
5. Copy the token value — esa will not show it again after this screen.

**Locally** — paste this whole block; it prompts once for the token and
never echoes, logs, or leaves it in shell history:

```bash
mkdir -p ~/.config/esa
read -rs ESA_TOKEN     # paste the token, press Enter (not echoed)
printf '%s' "$ESA_TOKEN" | gpg --encrypt --no-throw-keyids \
  --recipient 1DCDC49510DCC9BF58C89751B7D596E9AA6F36E8 \
  --output ~/.config/esa/token.gpg
unset ESA_TOKEN
gpg --quiet --decrypt ~/.config/esa/token.gpg | wc -c   # round-trip: prints byte length, not the token
```

`--no-throw-keyids` is required here: `home/modules/gpg.nix`'s
`throw-keyids = true` applies to every `gpg --encrypt` on this host, so
without this flag the recipient key ID is stripped and decryption falls
back to trying every card-backed secret key in the keyring in turn — one
"insert card" GUI prompt per card owned. See
[`docs/claude/esa-mcp.md`](claude/esa-mcp.md) for the full incident.

Rotating an existing token: revoke the old PAT v2 on esa.io first (same
menu as step 1), then re-run the block above to overwrite
`~/.config/esa/token.gpg`.

Full design + troubleshooting: [`docs/claude/esa-mcp.md`](claude/esa-mcp.md).

## Company host: CrowdStrike Falcon Sensor

Falcon Sensor is a root-owned system service and is installed separately from
home-manager and the ordinary apt package list. On `company-pop-new`, follow
the dedicated [Falcon Sensor runbook](docs/falcon-sensor.md) after bootstrap.
The company-provided `.deb` and CID must remain outside Git.

## Verify

```bash
zsh --version && starship --version && sheldon --version
git config user.name && git config user.signingkey
gpg --card-status               # requires YubiKey inserted
git log --show-signature -1
which bat ripgrep fd nvim claude
```

## Customization

- **Packages / config**: edit the relevant module under `home/modules/` (or a
  literal file under `config/`), then `home-manager switch`.
- **System-layer apt packages**: edit `packages/declarative/apt-packages.txt`,
  then `./scripts/install-packages.sh`.
- **Zsh modules**: edit `config/zsh/modules/`; changes apply on the next
  `home-manager switch` (they are deployed via `xdg.configFile`).

## Rollback

```bash
home-manager generations
home-manager switch --flake .#"$(hostname)" --rollback
```
