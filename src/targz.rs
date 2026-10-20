//! Minimal `.tar.gz` reader for the Linux update archive: gzip around plain ustar entries, inflated
//! with `miniz_oxide` (already used by `zip.rs`). Only regular files are kept.

use std::collections::HashMap;

const GZIP_MAGIC: [u8; 3] = [0x1f, 0x8b, 8];
const GZIP_HEADER: usize = 10;
// Gzip header flags (RFC 1952).
const FHCRC: u8 = 2;
const FEXTRA: u8 = 4;
const FNAME: u8 = 8;
const FCOMMENT: u8 = 16;
const BLOCK: usize = 512;

/// File name (lowercased, without directories) -> contents. The whole unpacked tar is capped at
/// `max_total` bytes.
pub fn untar_gz(data: &[u8], max_total: usize) -> Result<HashMap<String, Vec<u8>>, String> {
    let tar = gunzip(data, max_total)?;
    let bad = || "The update archive is damaged".to_string();
    let mut files = HashMap::new();
    let mut at = 0;
    while at + BLOCK <= tar.len() {
        let header = &tar[at..at + BLOCK];
        if header.iter().all(|&b| b == 0) {
            break;
        }
        let name = field(&header[0..100]);
        let size = usize::from_str_radix(field(&header[124..136]).trim(), 8).map_err(|_| bad())?;
        let body = at + BLOCK;
        let content = tar.get(body..body + size).ok_or_else(bad)?;
        if matches!(header[156], b'0' | 0) {
            let base = name.rsplit('/').next().unwrap_or(name).to_lowercase();
            files.insert(base, content.to_vec());
        }
        at = body + size.div_ceil(BLOCK) * BLOCK;
    }
    Ok(files)
}

/// A NUL-padded tar header field as text.
fn field(bytes: &[u8]) -> &str {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    std::str::from_utf8(&bytes[..end]).unwrap_or_default()
}

fn gunzip(data: &[u8], max: usize) -> Result<Vec<u8>, String> {
    let bad = || "The update archive isn't gzip".to_string();
    if data.get(..3) != Some(&GZIP_MAGIC[..]) {
        return Err(bad());
    }
    let flags = *data.get(3).ok_or_else(bad)?;
    let mut at = GZIP_HEADER;
    if flags & FEXTRA != 0 {
        let len = u16::from_le_bytes([*data.get(at).ok_or_else(bad)?, *data.get(at + 1).ok_or_else(bad)?]) as usize;
        at += 2 + len;
    }
    for flag in [FNAME, FCOMMENT] {
        if flags & flag != 0 {
            at += data.get(at..).ok_or_else(bad)?.iter().position(|&b| b == 0).ok_or_else(bad)? + 1;
        }
    }
    if flags & FHCRC != 0 {
        at += 2;
    }
    miniz_oxide::inflate::decompress_to_vec_with_limit(data.get(at..).ok_or_else(bad)?, max).map_err(|e| format!("The update archive is damaged: {e:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(name: &str, size: usize, kind: u8) -> Vec<u8> {
        let mut h = vec![0u8; BLOCK];
        h[..name.len()].copy_from_slice(name.as_bytes());
        let size = format!("{size:011o}");
        h[124..135].copy_from_slice(size.as_bytes());
        h[156] = kind;
        h
    }

    fn targz(entries: &[(&str, &[u8], u8)], gzip_flags: u8, extra: &[u8]) -> Vec<u8> {
        let mut tar = Vec::new();
        for (name, body, kind) in entries {
            tar.extend(header(name, body.len(), *kind));
            tar.extend(*body);
            tar.resize(tar.len().div_ceil(BLOCK) * BLOCK, 0);
        }
        tar.extend([0u8; BLOCK * 2]);
        let mut gz = vec![0x1f, 0x8b, 8, gzip_flags, 0, 0, 0, 0, 0, 3];
        gz.extend(extra);
        gz.extend(miniz_oxide::deflate::compress_to_vec(&tar, 6));
        gz.extend([0u8; 8]); // CRC32 + size trailer, not checked
        gz
    }

    #[test]
    fn reads_regular_files_and_skips_the_rest() {
        let exe = vec![7u8; 1500];
        let gz = targz(&[("SlimSpot-1.0.0/", b"", b'5'), ("SlimSpot-1.0.0/slimspot", &exe, b'0'), ("SlimSpot-1.0.0/LICENSE", b"MIT", 0)], 0, &[]);
        let files = untar_gz(&gz, 1 << 20).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files["slimspot"], exe);
        assert_eq!(files["license"], b"MIT");
    }

    #[test]
    fn skips_the_gzip_file_name() {
        let gz = targz(&[("a", b"x", b'0')], FNAME, b"archive.tar\0");
        assert_eq!(untar_gz(&gz, 1 << 20).unwrap()["a"], b"x");
    }

    #[test]
    fn rejects_other_data_and_oversized_archives() {
        assert!(untar_gz(b"PK\x03\x04", 1 << 20).is_err());
        let gz = targz(&[("big", &vec![0u8; 10_000], b'0')], 0, &[]);
        assert!(untar_gz(&gz, 1000).is_err());
    }
}
