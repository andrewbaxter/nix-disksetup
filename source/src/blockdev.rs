use {
    crate::util::SimpleCommandExt,
    loga::ResultContext,
    serde::Deserialize,
    std::{
        cmp::Reverse,
        collections::HashSet,
        process::Command,
    },
};

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct LsblkRoot {
    blockdevices: Vec<LsblkDevice>,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct LsblkDevice {
    /// path to the device node
    pub(crate) path: String,
    /// size of the device in bytes.
    pub(crate) size: i64,
    /// de-duplicated chain of subsystems
    pub(crate) subsystems: String,
    /// all locations where device is mounted
    pub(crate) mountpoints: Vec<Option<String>>,
    /// device type
    #[serde(rename = "type")]
    pub(crate) type_: String,
    /// filesystem UUID. Not always a standard uuid, can be 8 characters.
    pub(crate) uuid: Option<String>,
    #[serde(default)]
    pub(crate) children: Vec<LsblkDevice>,
    /// Rotational - true = hdd, missing = maybe raid, assume rotational
    pub(crate) rota: Option<bool>,
}

pub(crate) fn lsblk() -> Result<Vec<LsblkDevice>, loga::Error> {
    return Ok(
        Command::new("lsblk")
            .arg("--bytes")
            .arg("--json")
            .arg("--output-all")
            .arg("--tree")
            .simple()
            .run_json_out::<LsblkRoot>()
            .context("Error looking up block devices for disk setup")?
            .blockdevices,
    );
}

pub(crate) fn find_unused(blocks: Vec<LsblkDevice>) -> Result<Vec<LsblkDevice>, loga::Error> {
    let mut out = vec![];
    for candidate in blocks {
        let subsystems = candidate.subsystems.split(":").collect::<HashSet<&str>>();

        // Only consider physical disks
        if candidate.type_ != "disk" || subsystems.contains("usb") {
            continue;
        }

        // Skip in-use devices (recursive)
        fn in_use(candidate: &LsblkDevice) -> bool {
            if candidate.mountpoints.iter().filter(|p| p.is_some()).count() > 0 {
                return true;
            }
            for child in &candidate.children {
                if in_use(child) {
                    return true;
                }
            }
            return false;
        }

        if in_use(&candidate) {
            continue;
        }

        // Maybe keep as candidate
        out.push((candidate.size, candidate));
    }
    out.sort_by_cached_key(|v| Reverse(v.0));
    return Ok(out.into_iter().map(|x| x.1).collect());
}
