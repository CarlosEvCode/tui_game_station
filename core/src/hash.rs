use anyhow::{Context, Result};
use crc32fast::Hasher as Crc32Hasher;
use md5::Context as Md5Context;
use sha1::Digest;
use sha1::Sha1;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct RomHashes {
    pub crc32: String,
    pub md5: String,
    pub sha1: String,
    pub file_size: u64,
}

pub struct HashCalculator;

impl HashCalculator {
    /// Compute CRC32, MD5, SHA1 and file size in a single sequential pass.
    pub fn calculate_hashes<P: AsRef<Path>>(path: P) -> Result<RomHashes> {
        let file = File::open(path.as_ref())
            .with_context(|| format!("Failed to open file for hashing: {:?}", path.as_ref()))?;
        let metadata = file.metadata()?;
        let file_size = metadata.len();

        let mut reader = BufReader::with_capacity(128 * 1024, file); // 128 KB buffer
        let mut buffer = [0u8; 64 * 1024]; // 64 KB chunk

        let mut crc_hasher = Crc32Hasher::new();
        let mut md5_hasher = Md5Context::new();
        let mut sha1_hasher = Sha1::new();

        loop {
            let bytes_read = reader.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            let chunk = &buffer[..bytes_read];
            crc_hasher.update(chunk);
            md5_hasher.consume(chunk);
            sha1_hasher.update(chunk);
        }

        let crc32_val = crc_hasher.finalize();
        let crc32_str = format!("{:08X}", crc32_val);
        let md5_str = format!("{:x}", md5_hasher.compute());
        let sha1_str = format!("{:x}", sha1_hasher.finalize());

        Ok(RomHashes {
            crc32: crc32_str,
            md5: md5_str,
            sha1: sha1_str,
            file_size,
        })
    }
}
