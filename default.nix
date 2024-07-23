{ config, pkgs, lib, ... }:
{
  options = {
    volumesetup = {
      enable = mkOption {
        type = types.bool;
        default = false;
        description = "Enable the volumesetup service to run at boot.";
      };
      mountpoint = mkOption {
        type = types.str;
        default = "/mnt/persistent";
        description = "Where to mount the volume";
      };
      encryption = mkOption {
        type = types.enum [ "none" "password" "smartcard" ];
        default = "none";
        description = "Encrypt the disk, and where to obtain the key. See the other `encryption` options for per-mode options.";
      };
      encryptionSmartcardKeyfile = mkOption {
        type = types.str;
        description = "Path to encrypted key file";
      };
      encryptionSmartcardPinMode = mkOption {
        types = types.enum [ "factory-default" "text" "numpad" ];
        description = "How to get the PIN for the smartcard.";
      };
      ensureDirs = mkOption {
        type = with types; listOf str;
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
          serviceConfig.Type = "simple";
          startLimitIntervalSec = 0;
          serviceConfig.Restart = "on-failure";
          serviceConfig.RestartSec = 60;
          script =
            let
              pkg = import ./volumesetup.nix;
              cmdline = lib.concatStringsSep " "
                (
                  [ "${pkg}/bin/volumesetup" ] ++
                  {
                    "none" = [ ];
                    "password" = [ "--encryption password" ];
                    "smartcard" = [ "--encryption smartcard ${cfg.encryptionSmartcardKeyfile} ${cfg.encryptionSmartcardPinMode}" ];
                  }.${cfg.encryption} ++
                  [ "--mountpoint ${cfg.mountpoint}" ] ++
                  (lib.option ((builtins.length cfg.ensureDirs) > 0) [ "--ensure-dirs" ] ++ cfg.ensureDirs) ++
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
