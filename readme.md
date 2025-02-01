# Volumesetup

This is a small program to automatically set up attached storage on cloud or baremetal hosts at boot.

It looks for an existing disk matching a well-known UUID and either unlocks and mounts it or formats and mounts the most suitable unused (not mounted) physical storage it finds.

It has supports filesystems:

- `ext4` - The largest disk is selected and formatted

- `bcachefs` - All unused disks are added, if new disks are found at boot they will be added, and missing/failed disks will be removed

And three encryption modes:

- No encryption - provision an unencrypted disk

- Direct key encryption - credentials are directly used to encrypt/decrypt the drive

  Credentials can be provided programmatically (file/stdin) or interactively (systemd-ask-password).

- Indirect key encryption - the administrator installs an encrypted (via gpg) key on the system, and volumesetup decrypts this to format/mount the volume.

  A typical use case is if you have multiple users with GPG keys - you'd generate a volume key and encrypt it with each user's key as a recipient so that any one of them can unlock the volume.

  At the moment, credentials to unlock the volume must be provided interactively (via gpg smartcard, via NFC or USB).

  Additional encrypted data can be included in the image which will be decrypted at unlock (see the section on additional decryption).

## Installation

### Nix

You can start with this snippet:

```nix
{
    imports = [
        ./volumesetup/source/module.nix
    ];
    config = {
        volumesetup = {
            enable = true;
        };
    };
}
```

The above minimal config will format the largest unused disk as `ext4`, unencrypted.

You can also import just the package and define your own service:

```nix
let volumesetup = (import ./volumesetup/source/package.nix { pkgs = pkgs; }); in "${volumesetup}/bin/volumesetup ..."
```

Here's another example with smartcard encryption:

```nix
{
    config = {
        volumesetup = {
            enable = true;
            debug = true;
            encryption = {
                indirect_key = {
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
}
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
        volumesetup.encryption.indirect_key =
            import https://raw.githubusercontent.com/andrewbaxter/volumesetup/refs/heads/master/source/lib.nix {
            pkgs = pkgs;
            lib = lib;
        }.encryptKeyfile {
            name = "volumesetup_keyfile";
            keyPath = ./diskkey.plaintext;
            publicKeyPaths = [ ./person1.pubkey ./person2.pubkey ];
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
       "indirect_key": {
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

### Indirect-key additional decryption

When using indirect-key mode an additional file can be automatically decrypted by the key. This file could contain credentials or other non-dynamic private data. The file is PGP passphrase-encrypted (symmetric).

#### Nix

If you've already configured indirect-key mode, you can generate the data to decryption payload like this:

```nix
{
    imports = [ ./path/to/volumesetup/source/module.nix ];
    config = {
        volumesetup.config.encryption.indirect_key.decrypt =
        import https://raw.githubusercontent.com/andrewbaxter/volumesetup/refs/heads/master/source/lib.nix {
            pkgs = pkgs;
            lib = lib;
        }.encryptData {
            name = "volumesetup_decrypt";
            keyPath = ./diskkey.plaintext;
            plaintext = decrypt;
        };
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
       "indirect_key": {
         "decrypt": "path/to/decrypt.gpg"
       }
     }
   }
   ```

   to your volumesetup config.
