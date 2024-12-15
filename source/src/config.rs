use {
    serde::Deserialize,
    std::path::PathBuf,
};

pub(crate) const OUTER_UUID: &'static str = "3d02cfd4-968a-4fe4-a2a0-fe84614485f6";
pub(crate) const INNER_UUID: &'static str = "0afee777-4fca-45c6-9bed-64bf3091536b";

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum SharedImageKeyMode {
    /// The contents of a text (utf8) file are used as the password.
    File(PathBuf),
    /// `systemd-ask-password` will be used to query the password. The volume will be
    /// initialized/unlocked with the password.
    Password,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) struct SharedImageArgs {
    /// How to unlock the volume
    pub(crate) key_mode: SharedImageKeyMode,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum PinMode {
    /// Use the default PIN (`123456`)
    FactoryDefault,
    /// Use a numeric PIN entry, with a scrambled keypad prompt. Press the numpad keys
    /// that correspond positionally to the numbers displayed in the prompt.
    ///
    /// This accepts presses from the blocks (starting from the top left, left to
    /// right, top to bottom): `789456123` `uiojklm,.` or `wersdfxcv`.
    Numpad,
    /// Request an alphanumeric PIN.
    Text,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum PrivateImageKeyMode {
    /// A GPG smartcard is used to decrypt a key file which is then used to
    /// initialize/unlock the volume. A prompt will be written to all system terminals.
    /// If your NFC reader has a light, the light will come on when it wants to unlock
    /// the key.
    #[cfg(feature = "smartcard")]
    Smartcard {
        /// How to get the PIN.
        pin: PinMode,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) struct PrivateImageArgs {
    /// The location of the key to use to initialize/unlock the volume.
    ///
    /// The key file should be an encrypted utf-8 string. Start and end whitespace will
    /// be stripped.
    pub(crate) key_path: PathBuf,
    /// How to unlock the key file
    pub(crate) key_mode: PrivateImageKeyMode,
    /// Additional data to decrypt. The decrypted data will be written to
    /// `/run/volumesetup_decrypted`.
    pub(crate) decrypt: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum EncryptionMode {
    /// Disk is unencrypted.
    None,
    /// A password is used directly to encrypt the disk
    SharedImage(SharedImageArgs),
    /// A password in an encrypted file stored in the image is used to encrypt the disk
    PrivateImage(PrivateImageArgs),
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum FilesystemMode {
    /// The largest unused disk will be used and formatted ext4.
    Ext4,
    /// All unused disks will be added to the pool
    Bcachefs,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) struct Config {
    pub(crate) debug: Option<()>,
    /// Override the default UUID.
    pub(crate) uuid: Option<String>,
    /// How encryption should be handled.  Defaults to unencrypted.
    pub(crate) encryption: Option<EncryptionMode>,
    /// Filesystem to use, how to turn disks into filesystems.
    pub(crate) fs: Option<FilesystemMode>,
    /// The mount point of the volume.  Defaults to `/mnt/persistent`.
    pub(crate) mountpoint: Option<PathBuf>,
    /// Ensure these directories (and parents) relative to the mountdir once it's
    /// mounted.
    pub(crate) ensure_dirs: Option<Vec<PathBuf>>,
}
