# System-manager declaration for the two Pop!_OS machines. Activation is separate
# from home-manager; apt still owns the scheduler daemon and kernel integration.
{
  nixpkgs.hostPlatform = "x86_64-linux";
  system-manager.allowAnyDistro = true;
  system-manager.linkCurrentSystem = false;
  services.userborn.enable = false;
  security.enableWrappers = false;
  nix.enable = false;

  environment.etc."system76-scheduler/process-scheduler/desktop-priority.kdl" = {
    source = ../config/system76-scheduler/desktop-priority.kdl;
    replaceExisting = true;
  };
}
