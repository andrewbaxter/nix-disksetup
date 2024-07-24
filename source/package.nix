{ pkgs }: pkgs.callPackage
  ({ lib
   , rustPlatform
   , pkg-config
   , rustc
   , cargo
   , makeWrapper
   }:
  rustPlatform.buildRustPackage rec {
    pname = "volumesetup";
    version = "0.0.0";
    cargoLock = {
      lockFile = ./Cargo.lock;
    };
    src = ./.;
    buildFeatures = [
      "smartcard"
    ];
    buildInputs = [
      nettle
      pcsclite
    ];
    nativeBuildInputs = [
      pkg-config
      cargo
      rustc
      rustPlatform.bindgenHook
    ];
    postFixup =
      let
        path = lib.makeBinPath [
          pkgs.systemd
          pkgs.e2fsprogs
          pkgs.cryptsetup
          pkgs.util-linux
        ];
      in
      ''
        wrapProgram $out/bin/volumesetup --prefix PATH : ${path}
      '';
  })
{ }
