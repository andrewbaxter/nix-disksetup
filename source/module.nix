{ config, pkgs, lib, ... }:
{
  options = {
    volumesetup = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = false;
        description = "Enable the volumesetup service to run at boot. See volumesetup documentation for default values for various parameters.";
      };
      uuid = lib.mkOption {
        type = lib.types.str;
        default = "";
        description = "Override the default UUID.";
      };
      mountpoint = lib.mkOption {
        type = lib.types.str;
        default = "/mnt/persistent";
        description = "Where to mount the volume";
      };
      encryption = lib.mkOption {
        type = lib.types.enum [ "none" "password" "smartcard" ];
        default = "none";
        description = "Encrypt the disk, and where to obtain the key. See the other `encryption` options for per-mode options.";
      };
      encryptionSmartcardKeyfile = lib.mkOption {
        type = lib.types.str;
        description = "Path to encrypted key file";
      };
      encryptionSmartcardPinMode = lib.mkOption {
        type = lib.types.enum [ "factory-default" "text" "numpad" ];
        description = "How to get the PIN for the smartcard.";
      };
      ensureDirs = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [ ];
        description = "A list of paths of directories to create. These may be absolute or relative to the mountpoint.";
      };
    };
  };
  config =
    let
      cfg = config.volumesetup;
    in
    {
      systemd.services = lib.mkIf cfg.enable {
        volumesetup = {
          wantedBy = [ "multi-user.target" ];
          serviceConfig.Type = "oneshot";
          serviceConfig.RemainAfterExit = "yes";
          startLimitIntervalSec = 0;
          serviceConfig.Restart = "on-failure";
          serviceConfig.RestartSec = 60;
          script =
            let
              pkg = (import ./package.nix) { pkgs = pkgs; };
              cmdline = lib.concatStringsSep " "
                (
                  [ "${pkg}/bin/volumesetup" ] ++
                  (lib.lists.optionals (cfg.uuid != "") [ "--uuid ${cfg.uuid}" ]) ++
                  {
                    "none" = [ ];
                    "password" = [ "--encryption password" ];
                    "smartcard" = [ "--encryption smartcard ${cfg.encryptionSmartcardKeyfile} ${cfg.encryptionSmartcardPinMode}" ];
                  }.${cfg.encryption} ++
                  [ "--mountpoint ${cfg.mountpoint}" ] ++
                  (lib.lists.optionals (cfg.ensureDirs != [ ]) [ "--ensure-dirs" ] ++ cfg.ensureDirs) ++
                  [ ]
                );
            in
            ''
              set -xeu
              exec ${cmdline}
            '';
        };
      };
    };
}
