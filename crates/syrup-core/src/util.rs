//! Small helpers every crate needs: timestamps, hashes, text normalisation.

use std::time::{SystemTime, UNIX_EPOCH};

/// Now, as ISO 8601 UTC (`2026-09-29T01:25:00Z`), without a date library.
pub fn now_iso() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    iso_from_unix(secs)
}

pub fn iso_from_unix(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Seconds since the Unix epoch for an ISO 8601 UTC date or date-time (`2026-09-29` or `2026-09-29T01:25:00Z`).
pub fn unix_from_iso(iso: &str) -> Option<u64> {
    let date = iso.get(0..10)?;
    let mut parts = date.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    let d: u32 = parts.next()?.parse().ok()?;
    let days = days_from_civil(y, m, d);
    let mut secs = days * 86_400;
    if let Some(time) = iso.get(11..19) {
        let t: Vec<i64> = time.split(':').filter_map(|p| p.parse().ok()).collect();
        if t.len() == 3 {
            secs += t[0] * 3600 + t[1] * 60 + t[2];
        }
    }
    u64::try_from(secs).ok()
}

pub fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// Howard Hinnant's civil-date algorithms.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// FNV-1a, 64 bits: a stable hash for ids (not for security).
pub fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// Bits that differ between two 64-bit perceptual hashes.
pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// Lower case, letters and digits only, single spaces: for comparing names.
pub fn normalize_words(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
            space = false;
        } else if !space && !out.is_empty() {
            out.push(' ');
            space = true;
        }
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_round_trip() {
        assert_eq!(iso_from_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_from_unix(1_790_631_500), "2026-09-28T21:38:20Z");
        assert_eq!(unix_from_iso("2026-09-28T21:38:20Z"), Some(1_790_631_500));
        assert_eq!(unix_from_iso("2024-02-29"), Some(1_709_164_800));
    }

    #[test]
    fn words_normalise() {
        assert_eq!(normalize_words("  MapleStory - Zakum's Altar!! "), "maplestory zakum s altar");
        assert_eq!(hamming(0b1011, 0b0010), 2);
        assert_ne!(fnv64(b"a"), fnv64(b"b"));
    }
}
