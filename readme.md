# Volumesetup

This is a small program to automatically set up attached storage on cloud or baremetal hosts at boot.

It looks for an existing disk matching a well-known UUID and either unlocks and mounts it or formats and mounts the most suitable unused (not mounted) physical storage it finds.

It has two filesystems:

- `ext4` - The largest disk is selected and formatted

- `bcachefs` - All unused disks are added, if new disks are found at boot they will be added, and missing/failed disks will be removed

It has three encryption methods:

- No encryption - provision an unencrypted disk

- Shared image encryption - credentials are directly encrypt/decrypt the drive, and the image itself contains no encrypted data

  Credentials can be provided programmatically (file/stdin) or interactively (systemd-ask-password)

- Private image encryption - the image contains an encrypted key used to encrypt/decrypt the drive

  At the moment, credentials must be provided interactively (via gpg smartcard, via NFC or USB).

  Additional encrypted data can be included in the image which will be decrypted at unlock.

## Installation

### Nix

Either have it run automatically like:

```nix
imports = [
    ./volumesetup/source/module.nix
];
config = {
    volumesetup = {
        enable = true;
    };
};
```

Or import the package and call it yourself:

```nix
let volumesetup = (import ./volumesetup/source/package.nix { pkgs = pkgs; }); in "${volumesetup}/bin/volumesetup ..."
```

The above minimal config will format the largest unused disk as ext4, unencrypted.

Here's another example with smartcard encryption:

```nix
imports = [
    ./volumesetup/source/module.nix
];
config = {
    volumesetup = {
        enable = true;
        debug = true;
        encryption = {
            private_image = {
                key_path = "${./smartcard_keyfile}";
                key_mode = {
                    smartcard = {
                        pin = "factory_default";
                    };
                };
            };
        };
    };
};
```

### Other systems

Clone the repo and build it with `cargo build`. For smartcard support you need to enable the feature `smartcard`.

### Smartcard

Make sure the system has `pcscd` running and the correct `pcsc` drivers for your smartcard reader. You can test the reader with `opgpcard list`. `pscs_scan` and `pcsc-spy` may also help.

Any number of smartcards can be used to unlock the volume. To use smartcard you need to prepare a keyfile encrypted with all keys you want to allow to unlock it:

1. Get the ASCII Armored public keys (`person1.pubkey`, `person2.pubkey`, etc) for all keys you want to be able to unlock with

2. Create a disk key

   ```
   pwgen --symbols --secure 100 1 > diskkey.plaintext
   ```

   Make sure to back this up in case you want to replace keys in the future.

3. Encrypt the disk key, using `sq` or another GPG client

   ```
   sq encrypt -o diskkey --recipient-file person1.pubkey --recipient-file person2.pubkey ... diskkey.plaintext
   ```

4. Place the file in the system image and run `volumesetup --encryption private-image --key /path/to/diskkey --key-mode smartcard text` at boot.

#### Nix

You can also do it as part of your system build, if you're building a disk image on a secured build host, with something like this:

```nix
{
    imports = [ ./path/to/volumesetup/source/module.nix ];
    config = {
        volumesetup.enable = true;
        volumesetup.encryption.private_image{
            key_path =
                let key = derivation {
                    name = "volumesetup-key";
                    system = builtins.currentSystem;
                    builder = "${pkgs.bash}/bin/bash";
                    args = [(pkgs.writeText "volumesetup-key-script" ''
                        ${pkgs.sequoia-sq}/bin/sq encrypt \
                            --recipient-file ${./person1.pubkey} \
                            --recipient-file ${./person2.pubkey} \
                            --output $out \
                            ${./diskkey.plaintext} \
                            ;
                    '')];
                }; in "${key}";
            key_mode = "smartcard";
        };
    };
}
```

### Private-image additional decryption

When using private-image mode an additional file can be decrypted by the key. This file could contain credentials or other non-dynamic private data. The file is PGP passphrase-encrypted (symmetric).

1. Create the file you want to place in the image, like `decrypt.txt`

2. Encrypt it with `rage --encrypt --passphrase -o decrypt.age decrypt.txt < diskkey.plaintext`

3. Pass an additional `--decrypt decrypt.age` argument to `volumesetup`

#### Nix

You can also do it with Nix, if you've already configured private-image mode:

```nix
{
    imports = [ ./path/to/volumesetup/source/module.nix ];
    config = {
        volumesetup.encryption.private_image.decrypt =
            let key = derivation {
                name = "volumesetup-decrypt-encrypt";
                system = builtins.currentSystem;
                builder = "${pkgs.bash}/bin/bash";
                args = [(pkgs.writeText "volumesetup-decrypt-encrypt-script" ''
                    ${pkgs.sequoia-sq}/bin/sq encrypt \
                        --password-file ${./diskkey.plaintext} \
                        --output $out \
                        ${decrypt} \
                        ;
                '')];
            }; in "${key}";
    };
}
```
