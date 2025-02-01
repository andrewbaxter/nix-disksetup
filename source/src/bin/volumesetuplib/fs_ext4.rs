use {
    crate::{
        blockdev::{
            find_unused,
            lsblk,
        },
        config::{
            Config,
            EncryptionMode,
            INNER_UUID,
            OUTER_UUID,
        },
        key::{
            get_private_image_key,
            get_shared_image_key,
        },
        util::{
            from_utf8,
            SimpleCommandExt,
        },
    },
    flowcontrol::{
        shed,
        superif,
        ta_return,
    },
    loga::{
        ea,
        DebugDisplay,
        Log,
        ResultContext,
    },
    sequoia_openpgp::{
        crypto::Password,
        parse::{
            stream::DecryptorBuilder,
            Parse,
        },
        policy::StandardPolicy,
    },
    std::{
        fs::{
            File,
            OpenOptions,
        },
        io::copy,
        os::unix::fs::OpenOptionsExt,
        path::{
            Path,
            PathBuf,
        },
        process::Command,
        thread::sleep,
        time::Duration,
    },
};

pub(crate) fn main(log: &Log, config: &Config, mount_path: &PathBuf) -> Result<(), loga::Error> {
    let outer_uuid = config.uuid.as_ref().map(|x| x.as_str()).unwrap_or(OUTER_UUID);
    let outer_uuid_dev_path = PathBuf::from(format!("/dev/disk/by-uuid/{}", &outer_uuid));
    let inner_uuid_dev_path = PathBuf::from(format!("/dev/disk/by-uuid/{}", &INNER_UUID));

    // Mounting - helper methods
    let format = |dev_path: &Path, uuid: &str| -> Result<PathBuf, loga::Error> {
        log.log_with(loga::INFO, "Creating filesystem", ea!(dev = dev_path.dbg_str()));
        Command::new("mkfs.ext4")
            .arg("-F")
            .arg(dev_path)
            .arg("-U")
            .arg(uuid)
            .simple()
            .run()
            .context("Error formatting persistent volume")?;
        let fs_dev_path = PathBuf::from(format!("/dev/disk/by-uuid/{}", uuid));
        for _ in 0 .. 30 {
            if fs_dev_path.exists() {
                return Ok(fs_dev_path);
            }
            sleep(Duration::from_secs(1));
        }
        return Err(
            loga::err_with(
                "Even after formatting disk ext4, it never appeared in `by-uuid`. Try wiping the disk to remove misleading headers or doing a health check.",
                ea!(dev = dev_path.to_string_lossy(), path = fs_dev_path.to_string_lossy()),
            ),
        );
    };
    let ensure_mounted = |fs_dev_path: &Path| {
        ta_return!((), loga::Error);
        let systemd_mount_name =
            from_utf8(
                Command::new("systemd-escape")
                    .arg("--path")
                    .arg("--suffix=mount")
                    .arg(&mount_path)
                    .simple()
                    .run_stdout()
                    .context("Error determining systemd mount name")?,
            ).context("Systemd mount name via systemd-escape is not valid utf-8")?;
        let raw_active_state =
            from_utf8(
                Command::new("systemctl")
                    .arg("show")
                    .arg("--property=ActiveState")
                    .arg(systemd_mount_name.trim())
                    .simple()
                    .run_stdout()
                    .context("Error checking mount unit active state")?,
            ).context("Mount unit active state isn't valid utf-8")?;
        let Some((key, value)) = raw_active_state.trim().split_once("=") else {
            return Err(
                loga::err_with(
                    "Unable to parse mount unit active state",
                    ea!(unit = systemd_mount_name, raw_active_state = raw_active_state),
                ),
            );
        };
        if key != "ActiveState" {
            return Err(
                loga::err_with(
                    "Active state output has unexpected KV data",
                    ea!(unit = systemd_mount_name, raw_active_state = raw_active_state),
                ),
            );
        }
        if value != "active" {
            log.log_with(
                loga::INFO,
                "Mounting filesystem",
                ea!(dev = fs_dev_path.dbg_str(), mountpoint = mount_path.dbg_str(), unit_state = value),
            );
            Command::new("systemd-mount")
                .arg("--options=noatime")
                .arg("--collect")
                .arg(fs_dev_path)
                .arg(&mount_path)
                .simple()
                .run()
                .context("Failed to mount persistent disk")?;
        }
        return Ok(());
    };
    let ensure_map_luks = |key: &str| -> Result<PathBuf, loga::Error> {
        let mapper_name = "persistent";
        let mapper_dev_path = PathBuf::from(format!("/dev/mapper/{}", mapper_name));
        if mapper_dev_path.exists() {
            return Ok(mapper_dev_path);
        }
        log.log_with(loga::INFO, "Unlocking LUKS device", ea!(dev = outer_uuid_dev_path.dbg_str()));
        Command::new("cryptsetup")
            .arg("open")
            .arg("--key-file=-")
            .arg(&outer_uuid_dev_path)
            .arg(mapper_name)
            .simple()
            .run_stdin(key.as_bytes())
            .context("Error opening existing encrypted volume")?;
        return Ok(mapper_dev_path);
    };
    let decrypt_extra = |key: &str, data_path: &Option<PathBuf>| -> Result<(), loga::Error> {
        if let Some(data_path) = data_path {
            let log = log.fork(ea!(path = data_path.dbg_str()));
            let decrypted_path = "/run/volumesetup_decrypted";

            struct Helper {
                key: Password,
            }

            impl sequoia_openpgp::parse::stream::DecryptionHelper for Helper {
                fn decrypt<
                    D,
                >(
                    &mut self,
                    _pkesks: &[sequoia_openpgp::packet::PKESK],
                    skesks: &[sequoia_openpgp::packet::SKESK],
                    _sym_algo: Option<sequoia_openpgp::types::SymmetricAlgorithm>,
                    mut decrypt: D,
                ) -> sequoia_openpgp::Result<Option<sequoia_openpgp::Fingerprint>>
                where
                    D:
                        FnMut(
                            sequoia_openpgp::types::SymmetricAlgorithm,
                            &sequoia_openpgp::crypto::SessionKey,
                        ) -> bool {
                    'next_skesk: for skesk in skesks {
                        let Ok((algo, sk)) = skesk.decrypt(&self.key) else {
                            continue 'next_skesk;
                        };
                        if !decrypt(algo, &sk) {
                            continue 'next_skesk;
                        }
                        return Ok(None);
                    }
                    Ok(None)
                }
            }

            impl sequoia_openpgp::parse::stream::VerificationHelper for Helper {
                fn get_certs(
                    &mut self,
                    _ids: &[sequoia_openpgp::KeyHandle],
                ) -> sequoia_openpgp::Result<Vec<sequoia_openpgp::Cert>> {
                    Ok(Vec::new())
                }

                fn check(
                    &mut self,
                    _structure: sequoia_openpgp::parse::stream::MessageStructure,
                ) -> sequoia_openpgp::Result<()> {
                    Ok(())
                }
            }

            copy(
                &mut DecryptorBuilder::from_reader(
                    File::open(
                        &data_path,
                    ).context_with("Error opening additional file to decrypt", ea!(path = data_path.dbg_str()))?,
                )
                    .map_err(
                        |e| loga::err(
                            e.to_string(),
                        ).context_with(
                            "Error creating decryptor builder from file to decrypt",
                            ea!(path = data_path.dbg_str()),
                        ),
                    )?
                    .with_policy(&StandardPolicy::new(), None, Helper { key: Password::from(key) })
                    .map_err(
                        |e| loga::err(
                            e.to_string(),
                        ).context_with("Decryption failed", ea!(path = data_path.dbg_str())),
                    )?,
                &mut OpenOptions::new()
                    .mode(0o600)
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(decrypted_path)
                    .context_with(
                        "Error opening additional file to decrypt destination path",
                        ea!(path = decrypted_path),
                    )?,
            ).stack_context_with(&log, "Error writing file", ea!(dest_path = decrypted_path))?;
        }
        return Ok(());
    };
    let blocks = lsblk()?;

    // Ensure mount
    superif!({
        // Does the volume already exist?
        for candidate in &blocks {
            let uuid = candidate.uuid.as_ref().map(|u| u.as_str());
            if uuid == Some(&outer_uuid) {
                log.log_with(loga::INFO, "Found persistent disk", ea!(disk = candidate.path));
                break 'exists_outer candidate;
            }
            log.log_with(
                loga::DEBUG,
                "UUID mismatch",
                ea!(disk = candidate.path, found = candidate.uuid.dbg_str(), want = outer_uuid),
            );
        }

        // Find existing volume, or candidate disk to format
        let unused = find_unused(blocks)?;
        for candidate in &unused {
            log.log_with(loga::INFO, "Found unused disk", ea!(disk = candidate.path, size = candidate.size));
        }
        let best_candidate = unused.into_iter().next();

        // Didn't find existing volume, so format the best candidate volume
        let candidate = best_candidate.context("Couldn't find persistent disk or a suitable candidate for formatting")?;
        log.log_with(
            loga::INFO,
            "Couldn't find persistent disk, formatting best attached candidate disk",
            ea!(disk = candidate.path),
        );
        let setup_encrypted = |key: &str| -> Result<(), loga::Error> {
            log.log_with(loga::INFO, "Initializing LUKS device", ea!(dev = candidate.path.dbg_str()));
            Command::new("cryptsetup")
                .arg("luksFormat")
                .arg("--type=luks2")
                .arg("--key-file=-")
                .arg(&candidate.path)
                .simple()
                .run_stdin(key.as_bytes())
                .context("Error encypting new volume on persistent disk")?;
            Command::new("cryptsetup")
                .arg("luksUUID")
                .arg("--uuid")
                .arg(&outer_uuid)
                .arg(&candidate.path)
                .simple()
                .run()
                .context("Error setting UUID on newly encrypted volume on persistent disk")?;
            shed!{
                'exists_outer1 _;
                for _ in 0 .. 30 {
                    if outer_uuid_dev_path.exists() {
                        break 'exists_outer1;
                    }
                    sleep(Duration::from_secs(1));
                }
                return Err(
                    loga::err_with(
                        "LUKS source disk with UUID never appeared",
                        ea!(path = outer_uuid_dev_path.dbg_str()),
                    ),
                );
            }
            let luks_dev_path = ensure_map_luks(&key).context("Error mapping new LUKS volume")?;
            let fs_dev_path = format(&luks_dev_path, INNER_UUID)?;
            ensure_mounted(&fs_dev_path)?;
            return Ok(());
        };
        match config.encryption.as_ref().unwrap_or(&EncryptionMode::None {}) {
            EncryptionMode::None {} => {
                let fs_dev_path = format(&PathBuf::from(&candidate.path), &outer_uuid)?;
                ensure_mounted(&fs_dev_path)?;
            },
            EncryptionMode::SharedImage(enc_args) => {
                let key = get_shared_image_key(&enc_args.key_mode, true)?;
                setup_encrypted(&key)?;
            },
            EncryptionMode::PrivateImage(enc_args) => {
                let key = get_private_image_key(&log, &enc_args.key_path, &enc_args.key_mode)?;
                setup_encrypted(&key)?;
                decrypt_extra(&key, &enc_args.decrypt)?;
            },
        }
    } candidate = 'exists_outer {
        // Found existing volume, just mount it
        let mount_encrypted = |key: &str| -> Result<(), loga::Error> {
            let luks_dev_path = ensure_map_luks(key)?;
            let fs_dev_path = shed!{
                'exists_inner1 _;
                for _ in 0 .. 30 {
                    if inner_uuid_dev_path.exists() {
                        break 'exists_inner1 inner_uuid_dev_path;
                    }
                    sleep(Duration::from_secs(1));
                }
                log.log_with(
                    loga::INFO,
                    "Filesystem with UUID never appeared; assuming formatting never completed.",
                    ea!(dev = inner_uuid_dev_path.dbg_str()),
                );
                break 'exists_inner1 format(&luks_dev_path, INNER_UUID)?;
            };
            ensure_mounted(&fs_dev_path)?;
            return Ok(());
        };
        match config.encryption.as_ref().unwrap_or(&EncryptionMode::None {}) {
            EncryptionMode::None {} => {
                let fs_dev_path = PathBuf::from(&candidate.path);
                ensure_mounted(&fs_dev_path)?;
            },
            EncryptionMode::SharedImage(enc_args) => {
                let key = get_shared_image_key(&enc_args.key_mode, false)?;
                mount_encrypted(&key)?;
            },
            EncryptionMode::PrivateImage(enc_args) => {
                let key = get_private_image_key(&log, &enc_args.key_path, &enc_args.key_mode)?;
                mount_encrypted(&key)?;
                decrypt_extra(&key, &enc_args.decrypt)?;
            },
        }
    });

    // Done
    return Ok(());
}
