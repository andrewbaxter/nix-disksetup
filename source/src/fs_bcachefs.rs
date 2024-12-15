use {
    crate::{
        blockdev::{
            find_unused,
            lsblk,
        },
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
        Log,
        ResultContext,
    },
    std::{
        path::PathBuf,
        process::Command,
    },
    structre::structre,
};

fn mount(uuid: &str, mount_path: &PathBuf, key: Option<&String>) -> Result<(), loga::Error> {
    let mut c = Command::new("bcachefs");
    c.arg("mount");
    c.arg("--key-location=fail");
    c.arg("-o").arg("degraded,fsck,fix_errors");
    c.arg(format!("UUID={}", uuid)).arg(mount_path);
    if let Some(key) = key {
        c.simple().run_stdin(key.as_bytes()).context("Error mounting bcachefs")?;
    } else {
        c.simple().run().context("Error mounting bcachefs")?;
    }
    return Ok(());
}

pub(crate) fn main(log: &Log, config: &Config, mount_path: &PathBuf) -> Result<(), loga::Error> {
    let uuid = config.uuid.as_ref().map(|x| x.as_str()).unwrap_or(OUTER_UUID);
    if let Ok(info) =
        Command::new("bcachefs")
            .arg("show-super")
            .arg(format!("/dev/disk/by-uuid/{}", uuid))
            .simple()
            .run_stdout() {
        let info = String::from_utf8(info).context("Bcachefs show-super output is invalid utf-8")?;

        #[derive(Default, Debug)]
        struct Device {
            bcachefs_id: Option<usize>,
            uuid: Option<String>,
        }

        let mut superblock_devices = vec![];
        let mut last_index = 0;
        for line in info.lines() {
            let line = line.trim();
            if line.len() == 0 {
                continue;
            }
            let Some((k, v)) = line.split_once(":") else {
                log.log_with(loga::DEBUG, "Non kv line in bcachefs show-super output", ea!(line = line));
                continue;
            };
            let v = v.trim();
            match k {
                "Device" => {
                    superblock_devices.push(Device::default());
                },
                "  Label" => {
                    #[structre("d(?<label_index>\\d+) \\((?<id>\\d+)\\)")]
                    struct Label {
                        label_index: usize,
                        id: usize,
                    }

                    let label =
                        Label::try_from(v).map_err(loga::err).context("Bcachefs has disk with invalid label")?;
                    superblock_devices.last_mut().unwrap().bcachefs_id = Some(label.id);
                    if label.label_index > last_index {
                        last_index = label.label_index;
                    }
                },
                "  UUID" => {
                    superblock_devices.last_mut().unwrap().uuid = Some(v.to_string());
                },
                _ => { },
            }
        }

        // Mount
        let key;
        match config.encryption.as_ref().unwrap_or(&crate::config::EncryptionMode::None {}) {
            crate::config::EncryptionMode::None {} => {
                key = None;
            },
            crate::config::EncryptionMode::SharedImage(enc_args) => {
                key = Some(get_shared_image_key(&enc_args.key_mode, true)?);
            },
            crate::config::EncryptionMode::PrivateImage(enc_args) => {
                key = Some(get_private_image_key(&log, &enc_args.key_path, &enc_args.key_mode)?);
            },
        }
        mount(&uuid, &mount_path, key.as_ref())?;

        // Remove failed/missing devices
        let mut missing = false;
        for (i, b) in superblock_devices.iter().enumerate() {
            let Some(uuid) = &b.uuid else {
                log.log(loga::WARN, format!("UUID missing for superblock device #{} ({:?})", i, b));
                continue;
            };
            let Some(bcachefs_id) = &b.bcachefs_id else {
                log.log(loga::WARN, format!("Bcachefs ID missing for superblock device #{} ({:?})", i, b));
                continue;
            };
            if !PathBuf::from(format!("/dev/disk/by-uuid-sub/{}", uuid)).exists() {
                let mut c = Command::new("bcachefs");
                c.arg("device").arg("remove").arg(bcachefs_id.to_string());
                c.simple().run().context("Error removing failed/missing device")?;
                missing = true;
            }
        }
        if missing {
            Command::new("bcachefs").arg("data").arg("rereplicate").simple().run()?;
        }

        // Add fresh devices
        let blocks = lsblk()?;
        let unused = find_unused(blocks)?;
        for b in unused {
            let hdd = b.rota.unwrap_or(true);
            let mut c = Command::new("bcachefs");
            last_index += 1;
            c.arg("device").arg("add").arg("--label").arg(format!("{}.d{}", match hdd {
                true => "hdd",
                false => "ssd",
            }, last_index)).arg(&mount_path).arg(b.path);
            c.simple().run().context("Error adding new device")?;
        }
    } else {
        // New array
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
                crate::config::EncryptionMode::SharedImage(enc_args) => {
                    key = Some(get_shared_image_key(&enc_args.key_mode, true)?);
                    c.arg("--encrypted");
                },
                crate::config::EncryptionMode::PrivateImage(enc_args) => {
                    key = Some(get_private_image_key(&log, &enc_args.key_path, &enc_args.key_mode)?);
                    c.arg("--encrypted");
                },
            }
            let mut label_id = 0;
            let mut has_hdd = false;
            let mut has_ssd = false;
            for b in find_unused(lsblk()?)? {
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
            if let Some(key) = &key {
                c.simple().run_stdin(key.as_bytes()).context("Error formatting bcachefs")?;
            } else {
                c.simple().run().context("Error formatting bcachefs")?;
            }
        }
        mount(&uuid, &mount_path, key.as_ref())?;
    }
    return Ok(());
}
