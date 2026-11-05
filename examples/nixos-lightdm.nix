# Import this module from configuration.nix after compiling HelloWM.
# The binary should be installed as ~/.local/bin/hellowm for the logged-in user.
{ pkgs, ... }:
let
  hellowmRunner = pkgs.writeShellScriptBin "hellowm" ''
    exec "$HOME/.local/bin/hellowm" "$@"
  '';

  hellowmSession = pkgs.runCommand "hellowm-session" {
    providedSessions = [ "hellowm" ];
  } ''
    mkdir -p "$out/share/xsessions"
    cp ${../share/xsessions/hellowm.desktop} "$out/share/xsessions/hellowm.desktop"
  '';
in
{
  services.displayManager.sessionPackages = [ hellowmSession ];
  environment.systemPackages = [ hellowmRunner ];
}
