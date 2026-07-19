# Setup Guide

Bring a Pop!_OS host up to the declarative home-manager environment. For the
full per-host migration procedure (existing machines, rollback, greenfield
details) see [`docs/cutover-runbook.md`](docs/cutover-runbook.md).

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
[`docs/cutover-runbook.md`](docs/cutover-runbook.md): install system packages,
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

## Secrets (SOPS)

Secrets are runtime-decrypted in the interactive shell (never at activation).
Set up the host-local SOPS config and add secrets with:

```bash
./scripts/setup-sops-secrets.sh init                            # host-local ~/.sops/.sops.yaml
./scripts/setup-sops-secrets.sh add-secret GITHUB_ACCESS_TOKEN  # add a secret
./scripts/setup-sops-secrets.sh validate                        # verify
```

Secrets load automatically in new shells (or `reload_sops_secrets`). With the
YubiKey absent the shell still starts cleanly (silent failure preserved). See
[ADR-0003](docs/adr/0003-secrets-and-identity.md).

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
