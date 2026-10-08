//! Minimal zip reader (stored and deflate entries) for the update archive.

use std::collections::HashMap;

// Zip record signatures and the largest end-of-central-directory search window (22 + 65535 comment).
const EOCD_SIG: u32 = 0x0605_4b50;
const CENTRAL_SIG: u32 = 0x0201_4b50;
const LOCAL_SIG: u32 = 0x0403_4b50;
const EOCD_SEARCH: usize = 22 + u16::MAX as usize;
const DEFLATE: u16 = 8;

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

/// Entries by lowercased file name, folders dropped; entries over `max_entry` bytes are skipped.
pub fn unzip(zip: &[u8], max_entry: usize) -> Result<HashMap<String, Vec<u8>>, String> {
    let bad = || "Not a zip file".to_string();
    let start = zip.len().saturating_sub(EOCD_SEARCH);
    let eocd = (start..zip.len().saturating_sub(21)).rev().find(|&i| u32_at(zip, i) == Some(EOCD_SIG)).ok_or_else(bad)?;
    let count = u16_at(zip, eocd + 10).ok_or_else(bad)? as usize;
    let mut at = u32_at(zip, eocd + 16).ok_or_else(bad)? as usize;
    let mut files = HashMap::new();
    for _ in 0..count {
        if u32_at(zip, at) != Some(CENTRAL_SIG) {
            return Err(bad());
        }
        let method = u16_at(zip, at + 10).ok_or_else(bad)?;
        let size = u32_at(zip, at + 20).ok_or_else(bad)? as usize;
        let (name_len, extra, comment) = (u16_at(zip, at + 28), u16_at(zip, at + 30), u16_at(zip, at + 32));
        let (name_len, extra, comment) = (name_len.ok_or_else(bad)? as usize, extra.ok_or_else(bad)? as usize, comment.ok_or_else(bad)? as usize);
        let local = u32_at(zip, at + 42).ok_or_else(bad)? as usize;
        let name = String::from_utf8_lossy(zip.get(at + 46..at + 46 + name_len).ok_or_else(bad)?).to_lowercase();
        at += 46 + name_len + extra + comment;
        let base = name.rsplit(['/', '\\']).next().unwrap_or_default().to_string();
        if base.is_empty() || u32_at(zip, local) != Some(LOCAL_SIG) {
            continue;
        }
        let data_at = local + 30 + u16_at(zip, local + 26).ok_or_else(bad)? as usize + u16_at(zip, local + 28).ok_or_else(bad)? as usize;
        let raw = zip.get(data_at..data_at + size).ok_or_else(bad)?;
        let data = match method {
            0 => raw.to_vec(),
            DEFLATE => match miniz_oxide::inflate::decompress_to_vec_with_limit(raw, max_entry) {
                Ok(d) => d,
                Err(e) => {
                    log::warn!("zip entry {name}: inflate failed: {e:?}");
                    continue;
                }
            },
            other => {
                log::warn!("zip entry {name}: unsupported compression {other}");
                continue;
            }
        };
        files.insert(base, data);
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stored (method 0) zip with one entry, as written by any zip tool.
    fn stored_zip(name: &str, data: &[u8]) -> Vec<u8> {
        let mut z = Vec::new();
        let local = [&LOCAL_SIG.to_le_bytes()[..], &[0; 22], &(name.len() as u16).to_le_bytes(), &[0, 0], name.as_bytes(), data].concat();
        z.extend_from_slice(&local);
        let central_at = z.len() as u32;
        let size = (data.len() as u32).to_le_bytes();
        let central = [&CENTRAL_SIG.to_le_bytes()[..], &[0; 16], &size, &size, &(name.len() as u16).to_le_bytes(), &[0; 12], &0u32.to_le_bytes(), name.as_bytes()].concat();
        z.extend_from_slice(&central);
        let eocd = [&EOCD_SIG.to_le_bytes()[..], &[0; 6], &1u16.to_le_bytes(), &(central.len() as u32).to_le_bytes(), &central_at.to_le_bytes(), &[0, 0]].concat();
        z.extend_from_slice(&eocd);
        z
    }

    #[test]
    fn unzips_nested_names_case_insensitively() {
        let files = unzip(&stored_zip("Dir/SlimSpot.EXE", b"MZ"), 1 << 20).unwrap();
        assert_eq!(files["slimspot.exe"], b"MZ");
    }
}
