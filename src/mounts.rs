use std::char;
use std::ffi::OsString;
use std::io::{Error, ErrorKind, Read, Result};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

/// A mount entry which contains information regarding how and where a device
/// is mounted.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct MountInfo {
    /// The device which is mounted.
    pub source: PathBuf,
    /// Where the device is mounted.
    pub dest:   PathBuf,
    /// The type of the mounted file system.
    pub fstype: String,
    /// Options specified for this file system.
    pub options: Vec<String>,
    /// Defines if the file system should be dumped.
    pub dump: i32,
    /// Defines if the file system should be checked, and in what order.
    pub pass: i32,
}

/// A list of parsed mount entries from `/proc/mounts`.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct MountList(pub Vec<MountInfo>);

impl MountList {
    fn parse_value(value: &str) -> Result<OsString> {
        let mut ret = Vec::new();

        let mut bytes = value.bytes();
        while let Some(b) = bytes.next() {
            match b {
                b'\\' => {
                    let mut code = 0;
                    for _i in 0..3 {
                        if let Some(b) = bytes.next() {
                            code *= 8;
                            code += u32::from_str_radix(&(b as char).to_string(), 8)
                                .map_err(|err| Error::new(ErrorKind::Other, err))?;
                        } else {
                            return Err(Error::new(ErrorKind::Other, "truncated octal code"));
                        }
                    }
                    ret.push(code as u8);
                }
                _ => {
                    ret.push(b);
                }
            }
        }

        Ok(OsString::from_vec(ret))
    }

    fn parse_line(line: &str) -> Result<MountInfo> {
        let mut parts = line.split(' ');

        let source = parts
            .next()
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, "missing source"))?;
        let dest = parts
            .next()
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, "missing dest"))?;
        let fstype = parts
            .next()
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, "missing type"))?;
        let options = parts
            .next()
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, "missing options"))?;
        let dump = parts
            .next()
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, "missing dump"))?
            .parse::<i32>()
            .map_err(|_| Error::new(ErrorKind::InvalidData, "dump value is not a number"))?;
        let pass = parts
            .next()
            .ok_or_else(|| Error::new(ErrorKind::InvalidData, "missing pass"))?
            .parse::<i32>()
            .map_err(|_| Error::new(ErrorKind::InvalidData, "pass value is not a number"))?;

        Ok(MountInfo {
            source: PathBuf::from(Self::parse_value(source)?),
            dest:   PathBuf::from(Self::parse_value(dest)?),
            fstype: fstype.to_owned(),
            options: options.split(',').map(String::from).collect(),
            dump,
            pass
        })
    }

    /// Parse mounts given from an iterator of mount entry lines.
    pub fn parse_from<'a, I: Iterator<Item = &'a str>>(lines: I) -> Result<MountList> {
        lines.map(Self::parse_line)
            .collect::<Result<Vec<MountInfo>>>()
            .map(MountList)
    }

    /// Read a new list of mounts into memory from `/proc/mounts`.
    pub fn new() -> Result<MountList> {
        let file = ::open("/proc/mounts")
            .and_then(|mut file| {
                let length = file.metadata().ok().map_or(0, |x| x.len() as usize);
                let mut string = String::with_capacity(length);
                file.read_to_string(&mut string).map(|_| string)
            })?;

        Self::parse_from(file.lines())
    }

    /// Find the first mount which which has the `path` destination.
    pub fn get_mount_by_dest<P: AsRef<Path>>(&self, path: P) -> Option<&MountInfo> {
        self.0
            .iter()
            .find(|mount| mount.dest == path.as_ref())
    }

    /// Find the first mount hich has the source `path`.
    pub fn get_mount_by_source<P: AsRef<Path>>(&self, path: P) -> Option<&MountInfo> {
        self.0
            .iter()
            .find(|mount| mount.source == path.as_ref())
    }

    /// Iterate through each source that starts with the given `path`.
    pub fn source_starts_with<'a>(&'a self, path: &'a Path) -> Box<Iterator<Item = &MountInfo> + 'a> {
        self.starts_with(path.as_os_str().as_bytes(), |m| &m.source)
    }

    /// Iterate through each destination that starts with the given `path`.
    pub fn destination_starts_with<'a>(&'a self, path: &'a Path) -> Box<Iterator<Item = &MountInfo> + 'a> {
        self.starts_with(path.as_os_str().as_bytes(), |m| &m.dest)
    }

    fn starts_with<'a, F: Fn(&'a MountInfo) -> &'a Path + 'a>(
        &'a self,
        path: &'a [u8],
        func: F
    ) -> Box<Iterator<Item = &MountInfo> + 'a> {
        let iterator = self.0
            .iter()
            .filter(move |mount| {
                let input = func(mount).as_os_str().as_bytes();
                input.len() >= path.len() && &input[..path.len()] == path
            });

        Box::new(iterator)
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use super::*;

    const SAMPLE: &str = r#"sysfs /sys sysfs rw,nosuid,nodev,noexec,relatime 0 0
proc /proc proc rw,nosuid,nodev,noexec,relatime 0 0
udev /dev devtmpfs rw,nosuid,relatime,size=16420480k,nr_inodes=4105120,mode=755 0 0
tmpfs /run tmpfs rw,nosuid,noexec,relatime,size=3291052k,mode=755 0 0
/dev/sda2 / ext4 rw,noatime,errors=remount-ro,data=ordered 0 0
fusectl /sys/fs/fuse/connections fusectl rw,relatime 0 0
/dev/sda1 /boot/efi vfat rw,relatime,fmask=0077,dmask=0077,codepage=437,iocharset=iso8859-1,shortname=mixed,errors=remount-ro 0 0
/dev/sda6 /mnt/data ext4 rw,noatime,data=ordered 0 0"#;

    #[test]
    fn mounts() {
        let mounts = MountList::parse_from(SAMPLE.lines()).unwrap();

        assert_eq!(
            mounts.get_mount_by_source(Path::new("/dev/sda1")).unwrap(),
            &MountInfo {
                source: PathBuf::from("/dev/sda1"),
                dest: PathBuf::from("/boot/efi"),
                fstype: "vfat".into(),
                options: vec![
                    "rw".into(),
                    "relatime".into(),
                    "fmask=0077".into(),
                    "dmask=0077".into(),
                    "codepage=437".into(),
                    "iocharset=iso8859-1".into(),
                    "shortname=mixed".into(),
                    "errors=remount-ro".into(),
                ],
                dump: 0,
                pass: 0,
            }
        );

        let path = &Path::new("/");
        assert_eq!(
            mounts.destination_starts_with(path).map(|m| m.dest.clone()).collect::<Vec<_>>(),
            {
                let mut vec: Vec<PathBuf> = Vec::new();
                vec.push("/sys".into());
                vec.push("/proc".into());
                vec.push("/dev".into());
                vec.push("/run".into());
                vec.push("/".into());
                vec.push("/sys/fs/fuse/connections".into());
                vec.push("/boot/efi".into());
                vec.push("/mnt/data".into());
                vec
            }
        );
    }
}
