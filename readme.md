# Volumesetup

This is a small program to automatically set up attached storage on cloud or baremetal hosts at boot.

It looks for an existing disk matching a well-known UUID and either unlocks and mounts it or formats and mounts the most suitable unused (not mounted) physical storage it finds.

It has supports filesystems:

- `ext4` - The largest disk is selected and formatted

- `bcachefs` - All unused disks are added, if new disks are found at boot they will be added, and missing/failed disks will be removed

And three encryption modes:

- No encryption - provision an unencrypted disk

- Shared image encryption - credentials are directly encrypt/decrypt the drive, and the image itself contains no encrypted data

  Credentials can be provided programmatically (file/stdin) or interactively (systemd-ask-password)

- Private image encryption - the image contains an encrypted key used to encrypt/decrypt the drive

  At the moment, credentials must be provided interactively (via gpg smartcard, via NFC or USB).

  Additional encrypted data can be included in the image which will be decrypted at unlock.

## Installation

### Nix

You can start with this snippet:

```nix
imports = [
    ./volumesetup/source/module.nix
];
config = {
    volumesetup = {
        enable = true;
    };
    # Required for bcachefs to identify fs member disks
    services.udev.extraRules = ''
        ENV{ID_FS_USAGE}=="filesystem|other|crypto", ENV{ID_FS_UUID_SUB_ENC}=="?*", SYMLINK+="disk/by-uuid-sub/$env{ID_FS_UUID_SUB_ENC}"
    '';
};
```

The above minimal config will format the largest unused disk as `ext4`, unencrypted.

You can also import just the package and define your own service:

```nix
let volumesetup = (import ./volumesetup/source/package.nix { pkgs = pkgs; }); in "${volumesetup}/bin/volumesetup ..."
```

Here's another example with smartcard encryption:

```nix
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

Clone the repo and build it with `cargo build`.

- For smartcard support you need to enable the feature `smartcard`.

- For bcachefs you'll need to add the above rule (in the Nix section) for `/dev/disk/by-uuid-sub`

### Smartcard

Make sure the system has `pcscd` running and the correct `pcsc` drivers for your smartcard reader. You can test the reader with `opgpcard list`. `pscs_scan` and `pcsc-spy` may also help.

Any number of smartcards can be used to unlock the volume. To use a smartcard you need to prepare a keyfile encrypted with all keys you want to allow to unlock it.

#### Nix

First get the ASCII Armored public keys (`person1.pubkey`, `person2.pubkey`, etc) for all keys you want to be able to unlock with.

Then add config like this:

```nix
{
    imports = [ ./path/to/volumesetup/source/module.nix ];
    config = {
        volumesetup.enable = true;
        volumesetup.encryption.private_image = {
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

#### Manually

On other systems or if doing this manually,

1. Get the ASCII Armored public keys (`person1.pubkey`, `person2.pubkey`, etc) for all keys you want to be able to unlock with.

2. Create a disk key

   ```
   pwgen --symbols --secure 100 1 > diskkey.plaintext
   ```

   Make sure to back this up in case you want to replace keys in the future.

3. Encrypt the disk key, using `sq` or another GPG client

   ```
   sq encrypt -o /path/to/diskkey --recipient-file person1.pubkey --recipient-file person2.pubkey ... diskkey.plaintext
   ```

4. Prepare the volumesetup config with

   ```json
   {
     "encryption": {
       "private_image": {
         "key_path": "/path/to/diskkey",
         "key_mode": {
           "smartcard": {
             "pin": "text"
           }
         }
       }
     }
   }
   ```

5. Place the file in the system image and run `volumesetup /path/to/config.json` at boot.

### Private-image additional decryption

When using private-image mode an additional file can be automatically decrypted by the key. This file could contain credentials or other non-dynamic private data. The file is PGP passphrase-encrypted (symmetric).

#### Nix

If you've already configured private-image mode, just add something like this:

```nix
{
    imports = [ ./path/to/volumesetup/source/module.nix ];
    config = {
        volumesetup.encryption.private_image.decrypt =
            let
                decrypt = "my data...";
                key = derivation {
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
                };
            in "${key}";
    };
}
```

#### Manually

On other systems or if doing this manually,

1. Create the file you want encrypt and place in the image, like `decrypt.txt`.

2. Encrypt it with `sq encrypt --password-file diskkey.plaintext --output decrypt.gpg decrypt.txt`

3. Add

   ```json
   {
     "encryption": {
       "private_image": {
         "decrypt": "path/to/decrypt.gpg"
       }
     }
   }
   ```

   to your volumesetup config.
