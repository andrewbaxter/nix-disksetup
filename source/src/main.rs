use {
    aargvark::{
        traits_impls::AargvarkJson,
        vark,
        Aargvark,
    },
    config::Config,
    loga::{
        ea,
        fatal,
        Log,
        ResultContext,
    },
    path_absolutize::Absolutize,
    std::{
        fs::create_dir_all,
        path::PathBuf,
    },
};

pub mod blockdev;
pub mod fs_ext4;
pub mod fs_bcachefs;
pub mod config;
pub mod key;
pub mod util;

#[derive(Aargvark)]
struct Args {
    config: AargvarkJson<Config>,
}

fn main1() -> Result<(), loga::Error> {
    let args = vark::<Args>();
    let config = args.config.value;
    let log = Log::new_root(if config.debug.is_some() {
        loga::DEBUG
    } else {
        loga::INFO
    });
    let mount_path =
        config
            .mountpoint
            .clone()
            .unwrap_or_else(|| PathBuf::from("/mnt/persistent"))
            .absolutize()
            .context("Couldn't make mountpoint absolute")?
            .into_owned();
    match config.fs.as_ref().unwrap_or(&config::FilesystemMode::Bcachefs {}) {
        config::FilesystemMode::Ext4 {} => fs_ext4::main(&log, &config, &mount_path)?,
        config::FilesystemMode::Bcachefs {} => fs_bcachefs::main(&log, &config, &mount_path)?,
    }

    // Ensure subdirectories in mountpoint
    for path in config.ensure_dirs.unwrap_or_default() {
        create_dir_all(
            &mount_path.join(&path),
        ).stack_context_with(&log, "Failed to create mount point subidr", ea!(subdir = path.to_string_lossy()))?;
    }
    return Ok(());
}

fn main() {
    match main1() {
        Ok(_) => { },
        Err(e) => {
            fatal(e);
        },
    }
}
