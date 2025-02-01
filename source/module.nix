{ config, pkgs, lib, ... }:
{
  options = {
    volumesetup = {
      enable = lib.mkOption {
        description = "Enable the volumesetup service to run at boot. See volumesetup documentation for default values for various parameters.";
        default = false;
        type = lib.types.bool;
      };
      puteron = lib.mkOption {
        description = "Don't define the systemd service and instead define a puteron task named `volumesetup`. To start the task you should either override `default_on` or add it as a strong upstream of another task that will be on.";
        default = false;
        type = lib.types.bool;
      };
      config = lib.mkOption {
        description = "Config for volumesetup. This is directly serialized as JSON, so see the JSON config documentation. This is validated during the build for basic sanity checks.";
        type = lib.types.attrset;
      };
    };
  };
  config =
    let
      pkg = (import ./package.nix) { pkgs = pkgs; };
      volumesetupConfig = pkgs.writeTextFile {
        name = "volumesetup-config";
        text = config.volumesetup.config;
        checkPhase = ''
          ${pkg}/bin/volumesetup demon run $out --validate
        '';
      };
    in
    {
      systemd.services = lib.mkIf (cfg.enable && !cfg.puteron) {
        volumesetup = {
          wantedBy = [ "local-fs.target" ];
          after = [
            "local-fs.target"
            # For pcscd
            "sockets.target"
          ];
          serviceConfig.Type = "oneshot";
          serviceConfig.RemainAfterExit = "yes";
          startLimitIntervalSec = 0;
          serviceConfig.Restart = "on-failure";
          serviceConfig.RestartSec = 60;
          script = "${pkg}/bin/volumesetup ${volumesetupConfig}";
        };
      };
      puteron.notifySystemd.["systemd-local-fs-target"] = true;
      puteron.notifySystemd.["sockets.target"] = true;
      puteron.task = lib.mkIf (cfg.enable && cfg.puteron) {
        volumesetup.short = {
          upstream = {
            "systemd-local-fs-target" = "weak";
            # For pcscd
            "systemd-sockets-target" = "weak";
          };
          command = [ "${pkg}/bin/volumesetup" "${volumesetupConfig}" ];
        };
      };
    };
}
