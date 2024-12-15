{ config, pkgs, lib, ... }:
{
  options = {
    volumesetup =
      let
        taggedAttrUnion = spec: libtypes.addCheck (lib.types.submodule spec) (v: builtins.length (lib.attrsToList v) == 1);
        taggedAttrEnumUnion = enumSpec: attrsSpec: lib.types.oneOf [
          (lib.types.enum enumSpec)
          (lib.types.addCheck (lib.types.submodule spec) (v: builtins.length (lib.attrsToList v) == 1))
        ];
      in
      {
        enable = lib.mkOption {
          description = "Enable the volumesetup service to run at boot. See volumesetup documentation for default values for various parameters.";
          default = false;
          type = lib.types.bool;
        };
        debug = lib.mkOption {
          default = false;
          type = lib.types.bool;
        };
        uuid = lib.mkOption {
          default = null;
          type = lib.types.nullOr lib.types.str;
        };
        encryption = lib.mkOption {
          default = null;
          type = lib.types.nullOr taggedAttrEnumUnion [ "none" ] {
            shared_image = lib.mkOption {
              default = null;
              type = lib.types.nullOr lib.types.submodule {
                key_mode = lib.mkOption {
                  type = taggedAttrUnion {
                    file = lib.mkOption {
                      default = null;
                      type = lib.types.nullOr taggedEnumAttrUnion [ "password" ] {
                        file = lib.mkOption {
                          default = null;
                          type = lib.types.nullOr lib.types.string;
                        };
                      };
                    };

                  };
                };
              };
            };
            private_image = lib.mkOption {
              default = null;
              type = lib.types.nullOr lib.types.submodule {
                key_path = lib.mkOption {
                  type = lib.types.str;
                };
                key_mode = lib.mkOption {
                  type = taggedAttrUnion {
                    smartcard = lib.mkOption {
                      type = lib.types.submodule {
                        pin = lib.types.enum [ "factory_default" "numpad" "text" ];
                      };
                    };
                  };
                };
                decrypt = lib.mkOption {
                  default = null;
                  type = lib.types.nullOr lib.types.str;
                };
              };
            };
          };
        };
        fs = lib.mkOption {
          default = null;
          type = lib.types.enum [ "ext4" "bcachefs" ];
        };
        mountpoint = lib.mkOption {
          type = lib.types.nullOr lib.types.str;
          default = null;
        };
        ensure_dirs = lib.mkOption {
          type = lib.types.nullOr (lib.types.listOf lib.types.str);
          default = null;
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
          script =
            let
              pkg = (import ./package.nix) { pkgs = pkgs; };
              config = pkgs.writeText "volumesetup-config" (builtins.toJSON (pkgs.lib.filterAttrsRecursive (name: value: value != null) cfg));
            in
            "${pkg}/bin/volumesetup ${config}";
        };
      };
    };
}
