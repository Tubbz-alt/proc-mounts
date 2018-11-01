use std::char;
use std::ffi::OsString;
use std::io::{Error, ErrorKind, Read, Result};
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// A swap entry, which defines an active swap.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct SwapInfo {
    /// The path where the swap originates from.
    pub source:   PathBuf,
    /// The kind of swap, such as `partition` or `file`.
    pub kind:     OsString,
    /// The size of the swap partition.
    pub size:     usize,
    /// Whether the swap is used or not.
    pub used:     usize,
    /// The priority of a swap, which indicates the order of usage.
    pub priority: isize,
}

/// A list of parsed swap entries from `/proc/swaps`.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct SwapList(pub Vec<SwapInfo>);

impl SwapList {
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

    fn parse_line(line: &str) -> Result<SwapInfo> {
        let mut parts = line.split_whitespace();

        fn parse<F: FromStr>(string: &OsString) -> Result<F> {
            let string = string.to_str().ok_or_else(|| Error::new(
                ErrorKind::InvalidData,
                "/proc/swaps contains non-UTF8 entry"
            ))?;

            string.parse::<F>().map_err(|_| Error::new(
                ErrorKind::InvalidData,
                "/proc/swaps contains invalid data"
            ))
        }

        macro_rules! next_value {
            ($err:expr) => {{
                parts.next()
                    .ok_or_else(|| Error::new(ErrorKind::Other, $err))
                    .and_then(|val| Self::parse_value(val))
            }}
        }

        Ok(SwapInfo {
            source:   PathBuf::from(next_value!("Missing source")?),
            kind:     next_value!("Missing kind")?,
            size:     parse::<usize>(&next_value!("Missing size")?)?,
            used:     parse::<usize>(&next_value!("Missing used")?)?,
            priority: parse::<isize>(&next_value!("Missing priority")?)?,
        })
    }

    pub fn parse_from<'a, I: Iterator<Item = &'a str>>(lines: I) -> Result<SwapList> {
        lines.map(Self::parse_line)
            .collect::<Result<Vec<SwapInfo>>>()
            .map(SwapList)
    }

    pub fn new() -> Result<SwapList> {
        let file = ::open("/proc/swaps")
            .and_then(|mut file| {
                let length = file.metadata().ok().map_or(0, |x| x.len() as usize);
                let mut string = String::with_capacity(length);
                file.read_to_string(&mut string).map(|_| string)
            })?;

        Self::parse_from(file.lines().skip(1))
    }

    /// Returns true if the given path is a entry in the swap list.
    pub fn get_swapped(&self, path: &Path) -> bool {
        self.0.iter().any(|mount| mount.source == path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::ffi::OsString;

    const SAMPLE: &str = r#"Filename				Type		Size	Used	Priority
/dev/sda5                               partition	8388600	0	-2"#;

    #[test]
    fn swaps() {
        let swaps = SwapList::parse_from(SAMPLE.lines().skip(1)).unwrap();
        assert_eq!(
            swaps,
            SwapList(vec![
                SwapInfo {
                    source: PathBuf::from("/dev/sda5"),
                    kind: OsString::from("partition"),
                    size: 8_388_600,
                    used: 0,
                    priority: -2
                }
            ])
        );

        assert!(swaps.get_swapped(Path::new("/dev/sda5")));
        assert!(!swaps.get_swapped(Path::new("/dev/sda1")));
    }
}
