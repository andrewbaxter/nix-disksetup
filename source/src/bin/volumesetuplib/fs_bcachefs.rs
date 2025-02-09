use {
    super::blockdev::LsblkDevice,
    crate::{
        blockdev::find_unused,
        config::{
            Config,
            OUTER_UUID,
        },
        key::{
            get_private_image_key,
            get_shared_image_key,
        },
        util::SimpleCommandExt,
    },
    loga::{
        ea,
        DebugDisplay,
        ErrContext,
        Log,
        ResultContext,
    },
    std::{
        collections::HashSet,
        fs::{
            read_dir,
            read_link,
        },
        os::unix::ffi::OsStrExt,
        path::PathBuf,
        process::Command,
    },
};

fn mount(log: &Log, uuid: &str, mount_path: &PathBuf, key: Option<&String>) -> Result<(), loga::Error> {
    let mut c = Command::new("bcachefs");
    c.arg("mount");
    c.arg("-o").arg("degraded,fsck,fix_errors");
    c.arg(format!("UUID={}", uuid)).arg(mount_path);
    if let Some(key) = key {
        c.arg("--key_location=stdin");
        log.log(loga::DEBUG, format!("Running {:?}", c));
        c.simple().run_stdin(key.as_bytes()).context("Error mounting bcachefs")?;
    } else {
        c.arg("--key_location=fail");
        log.log(loga::DEBUG, format!("Running {:?}", c));
        c.simple().run().context("Error mounting bcachefs")?;
    }
    return Ok(());
}

pub(crate) fn main(
    log: &Log,
    blocks: Vec<LsblkDevice>,
    config: &Config,
    mount_path: &PathBuf,
) -> Result<(), loga::Error> {
    match main1(log, blocks, config, mount_path) {
        Ok(_) => {
            return Ok(());
        },
        Err(e) => {
            let mut c = Command::new("umount");
            c.arg("--lazy");
            c.arg(mount_path);
            if let Err(e) = c.simple().run() {
                eprintln!("Warning: failed to unmount [{}] as cleanup after error: {}", mount_path.dbg_str(), e);
            }
            return Err(e);
        },
    }
}

pub(crate) fn main1(
    log: &Log,
    blocks: Vec<LsblkDevice>,
    config: &Config,
    mount_path: &PathBuf,
) -> Result<(), loga::Error> {
    let uuid = config.uuid.as_ref().map(|x| x.as_str()).unwrap_or(OUTER_UUID);
    let mut c = Command::new("bcachefs");
    c.arg("show-super").arg(format!("/dev/disk/by-uuid/{}", uuid));
    log.log(loga::DEBUG, format!("Running {:?}", c));
    if let Ok(_) = c.simple().run_stdout() {
        log.log(loga::INFO, format!("Filesystem found with UUID {}, mounting", uuid));

        // # Mount - can't add/remove until that's done
        let key;
        match config.encryption.as_ref().unwrap_or(&crate::config::EncryptionMode::None {}) {
            crate::config::EncryptionMode::None {} => {
                key = None;
            },
            crate::config::EncryptionMode::DirectKey(enc_args) => {
                key = Some(get_shared_image_key(&enc_args.key_mode, true)?);
            },
            crate::config::EncryptionMode::IndirectKey(enc_args) => {
                key = Some(get_private_image_key(&log, &enc_args.key_path, &enc_args.key_mode)?);
            },
        }
        mount(log, &uuid, &mount_path, key.as_ref())?;

        // # Check current state
        let mut missing = vec![];
        let mut used_extra = 
            // Unused detection uses lsblk mountpoints, but bcachefs devices don't have
            // mountpoints in lsblk - so exclude those separately
            HashSet::new();
        let mut last_index = 0;
        for d in read_dir(format!("/sys/fs/bcachefs/{}", uuid)).context("Error reading bcachefs sys dir")? {
            let d = match d {
                Ok(d) => d,
                Err(e) => {
                    log.log_err(loga::WARN, e.context("Error reading sysfs directory entry"));
                    continue;
                },
            };
            let name = match d.file_name().to_str().map(|x| x.to_string()) {
                Some(n) => n,
                None => {
                    log.log_with(
                        loga::WARN,
                        "Error reading sysfs directory entry name as utf-8",
                        ea!(name = String::from_utf8_lossy(d.file_name().as_bytes())),
                    );
                    continue;
                },
            };
            let Some(index) = name.strip_prefix("dev-") else {
                continue;
            };
            let index = match usize::from_str_radix(index, 10) {
                Ok(i) => i,
                Err(e) => {
                    log.log_err(
                        loga::WARN,
                        e.context_with(
                            "Error parsing device index from sysfs tree",
                            ea!(name = String::from_utf8_lossy(d.file_name().as_bytes())),
                        ),
                    );
                    continue;
                },
            };
            last_index = last_index.max(index);
            if d.path().join("block").exists() {
                used_extra.insert(
                    read_link(d.path().join("block"))
                        .context_with("Error reading bcachefs dev link", ea!(path = d.path().dbg_str()))?
                        .file_name()
                        .expect("Bcachefs dev link doesn't link to file")
                        .to_os_string(),
                );
            } else {
                missing.push(index);
            }
        }

        // # Add fresh devices
        let unused = find_unused(blocks)?;
        let mut added = false;
        for b in unused {
            if used_extra.contains(b.path.file_name().unwrap()) {
                continue;
            }
            log.log(loga::INFO, format!("Adding new device [{}] to pool", b.path.dbg_str()));
            let hdd = b.rota.unwrap_or(true);
            let mut c = Command::new("bcachefs");
            last_index += 1;
            c.arg("device").arg("add").arg("--label").arg(format!("{}.d{}", match hdd {
                true => "hdd",
                false => "ssd",
            }, last_index)).arg(&mount_path).arg(b.path);
            log.log(loga::DEBUG, format!("Running {:?}", c));
            c.simple().run().context("Error adding new device")?;
            added = true;
        }

        // # Remove dead/missing devices
        for index in missing {
            log.log(loga::INFO, format!("Removing lost device [{}] from pool", index));
            let mut c = Command::new("bcachefs");
            c.arg("device").arg("remove").arg(index.to_string()).arg(mount_path);
            log.log(loga::DEBUG, format!("Running {:?}", c));
            c.simple().run().context("Error removing failed/missing device")?;
        }

        // # Replicate data with few replicas after disks were lost
        if added {
            log.log(loga::INFO, format!("Triggering rereplicate"));
            let mut c = Command::new("bcachefs");
            log.log(loga::DEBUG, format!("Running {:?}", c));
            c.arg("data").arg("rereplicate").arg(mount_path);
            c.simple().run()?;
        }
    } else {
        log.log(loga::INFO, format!("No filesystem found with UUID {} (show-super failed), creating", uuid));

        // # New array
        let key;
        {
            let mut c = Command::new("bcachefs");
            c
                .arg("format")
                .arg(format!("--uuid={}", uuid))
                .arg("--force")
                .arg("--replicas=2")
                .arg("--metadata_replicas_required=2")
                .arg("--data_replicas_required=2")
                .arg("--compression=zstd");
            match config.encryption.as_ref().unwrap_or(&crate::config::EncryptionMode::None {}) {
                crate::config::EncryptionMode::None {} => {
                    key = None;
                },
                crate::config::EncryptionMode::DirectKey(enc_args) => {
                    key = Some(get_shared_image_key(&enc_args.key_mode, true)?);
                    c.arg("--encrypted");
                },
                crate::config::EncryptionMode::IndirectKey(enc_args) => {
                    key = Some(get_private_image_key(&log, &enc_args.key_path, &enc_args.key_mode)?);
                    c.arg("--encrypted");
                },
            }
            let mut label_id = 0;
            let mut has_hdd = false;
            let mut has_ssd = false;
            let unused = find_unused(blocks)?;
            if unused.len() < 2 {
                return Err(
                    loga::err(
                        "No existing volume found, and insufficient unused block devices to create new volume with replicas=2",
                    ),
                );
            }
            for b in unused {
                log.log(loga::INFO, format!("With volume [{}]", b.path.dbg_str()));
                if b.rota.unwrap_or(true) {
                    c.arg(format!("--label=hdd.d{}", label_id)).arg(b.path);
                    label_id += 1;
                    has_hdd = true;
                } else {
                    c.arg(format!("--label=ssd.d{}", label_id)).arg(b.path);
                    label_id += 1;
                    has_ssd = true;
                }
            }
            if has_ssd {
                c.arg("--promote_target=ssd").arg("--foreground_target=ssd");
            }
            if has_hdd {
                c.arg("--background_target=hdd");
            }
            log.log(loga::DEBUG, format!("Running {:?}", c));
            if let Some(key) = &key {
                c.simple().run_stdin(key.as_bytes()).context("Error formatting bcachefs")?;
            } else {
                c.simple().run().context("Error formatting bcachefs")?;
            }
        }
        log.log(loga::INFO, format!("Mounting filesystem"));
        mount(log, &uuid, &mount_path, key.as_ref())?;
    }
    return Ok(());
}
