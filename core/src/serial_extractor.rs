use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

pub struct SerialExtractor;

impl SerialExtractor {
    /// Extract unique game serial/gamecode from ROM/ISO files.
    pub fn extract_serial<P: AsRef<Path>>(path: P, platform_slug: &str) -> Option<String> {
        let path_ref = path.as_ref();
        let mut file = File::open(path_ref).ok()?;

        match platform_slug {
            "ds" | "nds" => Self::extract_nds(&mut file),
            "gba" => Self::extract_gba(&mut file),
            "gamecube" | "wii" => Self::extract_gc_wii(&mut file),
            "ps1" | "ps2" | "psp" => Self::extract_playstation(&mut file),
            _ => None,
        }
    }

    /// Extract Nintendo DS GameCode at offset 0x0C (4 bytes ASCII).
    fn extract_nds(file: &mut File) -> Option<String> {
        file.seek(SeekFrom::Start(0x0C)).ok()?;
        let mut buf = [0u8; 4];
        file.read_exact(&mut buf).ok()?;

        if buf.iter().all(|&b| b >= 32 && b <= 126) {
            let code = String::from_utf8_lossy(&buf).trim().to_uppercase();
            if !code.is_empty() {
                return Some(code);
            }
        }
        None
    }

    /// Extract Game Boy Advance GameCode at offset 0xAC (4 bytes ASCII).
    fn extract_gba(file: &mut File) -> Option<String> {
        file.seek(SeekFrom::Start(0xAC)).ok()?;
        let mut buf = [0u8; 4];
        file.read_exact(&mut buf).ok()?;

        if buf.iter().all(|&b| b >= 32 && b <= 126) {
            let code = String::from_utf8_lossy(&buf).trim().to_uppercase();
            if !code.is_empty() {
                return Some(code);
            }
        }
        None
    }

    /// Extract GameCube / Wii GameCode at offset 0x00 (6 bytes ASCII).
    fn extract_gc_wii(file: &mut File) -> Option<String> {
        file.seek(SeekFrom::Start(0x00)).ok()?;
        let mut buf = [0u8; 6];
        file.read_exact(&mut buf).ok()?;

        if buf.iter().all(|&b| b >= 32 && b <= 126) {
            let code = String::from_utf8_lossy(&buf).trim().to_uppercase();
            if !code.is_empty() {
                return Some(code);
            }
        }
        None
    }

    /// Extract PlayStation (PS1, PS2, PSP) Disc Serial (e.g. SCUS-94424, SLUS-00782).
    fn extract_playstation(file: &mut File) -> Option<String> {
        file.seek(SeekFrom::Start(0)).ok()?;
        let mut buffer = vec![0u8; 30 * 1024 * 1024]; // Read up to 30MB
        let bytes_read = file.read(&mut buffer).ok()?;
        buffer.truncate(bytes_read);

        let prefixes: [&[u8]; 15] = [
            b"SLUS", b"SCUS", b"SLES", b"SCES", b"SLPS", b"SCPS", b"SLED", b"SCED",
            b"ULUS", b"ULEU", b"ULJS", b"UCUS", b"UCES", b"NPUG", b"NPEH",
        ];

        for prefix in prefixes {
            let mut pos = 0;
            while pos + 4 <= buffer.len() {
                if let Some(found) = buffer[pos..].windows(4).position(|w| w.eq_ignore_ascii_case(prefix)) {
                    let abs_pos = pos + found;
                    let slice = &buffer[abs_pos..abs_pos + 20.min(buffer.len() - abs_pos)];
                    let text = String::from_utf8_lossy(slice);

                    let mut num_str = String::new();
                    for ch in text.chars().skip(4) {
                        if ch.is_ascii_digit() {
                            num_str.push(ch);
                        } else if ch == '_' || ch == '-' || ch == '.' || ch == ' ' {
                            continue;
                        } else if !num_str.is_empty() {
                            break;
                        }
                    }

                    if num_str.len() >= 4 && num_str.len() <= 6 {
                        let pref_str = String::from_utf8_lossy(prefix).to_uppercase();
                        return Some(format!("{}-{}", pref_str, num_str));
                    }

                    pos = abs_pos + 4;
                } else {
                    break;
                }
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_extract_nds_serial() {
        let temp_dir = std::env::temp_dir();
        let path = temp_dir.join("test_nds_header.nds");
        {
            let mut file = File::create(&path).unwrap();
            let mut data = vec![0u8; 0x20];
            data[0x0C..0x10].copy_from_slice(b"AZEE");
            file.write_all(&data).unwrap();
        }

        let serial = SerialExtractor::extract_serial(&path, "nds");
        let _ = std::fs::remove_file(&path);
        assert_eq!(serial, Some("AZEE".to_string()));
    }
}
