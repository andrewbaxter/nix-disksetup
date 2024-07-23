# Volumesetup

This is a small program to automatically set up attached storage.

Important information in bullet point form:

- Sets up the single largest unmounted physical (disk or usb) volume it detects

- Pays existing data no mind. It won't wipe mounted disks, like the root disk, but all other disks are fair game

- Cloud or bare metal

- Unencrypted or encrypted

- Encrypted with a password or via a GPG smartcard, like a Yubikey

- Encrypted with a GPG smartcard via NFC or USB

## Help

```
$ volumesetup -h
Usage: /mnt/scratch/cargo_target/debug/volumesetup [ ...OPT]

    [--encrypted ENCRYPTION-MODE]  The encryption key, if the volume should be
                                   encrypted. Otherwise unencrypted.
    [--mountpoint <PATH>]          The mount point of the volume.  Defaults to
                                   `/mnt/persistent`.
    [--create-dirs <PATH>[ ...]]   Ensure these directories (and parents)
                                   relative to the mountdir once it's mounted.

ENCRYPTION-MODE: none | file | password | smartcard

    none                 Disk is unencrypted.
    file <PATH> | -      The contents of a text (utf8) file are used as the
                         password.
    password             `systemd-ask-password` will be used to query the
                         password. The volume will be initialized/unlocked with
                         the password.
    smartcard smartcard  A GPG smartcard is used to decrypt a key file which is
                         then used to initialize/unlock the volume. A prompt
                         will be written to all system terminals. If your NFC
                         reader has a light, the light will come on when it
                         wants to unlock the key.

smartcard: KEY-PATH PIN

    KEY-PATH: <PATH>  The location of the key to use to initialize/unlock the
                      volume. The key file should be an encrypted utf-8 string.
                      Start and end whitespace will be stripped.
    PIN: PIN-MODE     How to get the PIN.

PIN-MODE: factory-default | numpad | text

    factory-default   Use the default PIN (`123456`)
    numpad            Use a numeric PIN entry, with a scrambled keypad prompt.
                      Press the numpad keys that correspond positionally to the
                      numbers displayed in the prompt. This accepts presses
                      from the blocks (starting from the top left, left to
                      right, top to bottom): `789456123` `uiojklm,.` or
                      `wersdfxcv`.
    text              Request an alphanumeric PIN.
```

Run `volumesetup --help` for possibly more up to date details.

## Installation

If you're building a system with Nix, add `default.nix` to `modules = [];`. See `default.nix` for parameters. You can either use the parameters to have a unit created automatically or call it yourself via `${pkgs.volumesetup}/bin/volumesetup`.

Make sure the system has `pcscd` running and the correct `pcsc` drivers for your smartcard reader. You can test the reader with `opgpcard list`. `pscs_scan` and `pcsc-spy` may also help.

Otherwise this is a normal Rust program you can clone and build with `cargo build`. For smartcard support you need to enable the feature `smartcard`.

This is intended to be used at boot time. It uses `systemd-ask-password` to ask for PINs and passwords and writes to all `tty` and `pty` devices to indicate when to touch the smartcard.

## Encrypting a key for smartcard decryption

You can set up volume to be unlockable with any number of keys.

1. Get the ASCII Armored public keys (`person1.pubkey`, `person2.pubkey`, etc) for all keys you want to be able to unlock with

2. Create a disk key

   ```
   pwgen --symbols --secure 100 1 > disk.plaintext
   ```

   Make sure to back this up in case you want to replace keys in the future.

3. Encrypt the disk key, using `sq` or another GPG client

   ```
   sq encrypt -o disk.key --recipient-file person1.pubkey --recipient-file person2.pubkey ... disk.plaintext
   ```

4. Place the file in the system image and run `volumesetup --encrypted smartcard /path/to/disk.key text` at boot.

### Doing it in Nix

You can also do it as part of your system build, if you're building a disk image on a secured build host, something like this:

```nix
{
    modules = [ ./path/to/volumesetup/default.nix ];
    config = {
        volumesetup.enable = true;
        volumesetup.encryption = "smartcard";
        volumesetup.encryptionSmartcardKeyfile =
            let key = derivation {
                name = "volumesetup-key";
                builder = "${pkgs.bash}/bin/bash";
                args = [(pkgs.writeText "volumesetup-key-script" ''
                    ${pkgs.sequoia-sq}/bin/sq \
                        -o $out \
                        --recipient-file ${./person1.pubkey} \
                        --recipient-file ${./person2.pubkey} \
                        ${./disk.plaintext} \
                        ;
                '')];
            }; in "${key}";
    };
}
```
