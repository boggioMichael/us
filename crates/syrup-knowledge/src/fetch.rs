//! Fetching pages for research: `curl` for real, recorded responses in
//! tests, nothing at all when research is off.

use std::path::{Path, PathBuf};
use std::process::Command;

use syrup_core::util::{fnv64, unix_now};

pub trait Fetcher: Send + Sync {
    fn get(&self, url: &str) -> Result<String, String>;
}

/// Research switched off: every request refused, nothing leaves the computer.
pub struct Offline;

impl Fetcher for Offline {
    fn get(&self, url: &str) -> Result<String, String> {
        Err(format!("research is off (would have fetched {url})"))
    }
}

/// The `curl` on the PATH (built into Windows 10 and later, macOS and most Linux).
pub struct Curl {
    pub timeout_s: u32,
}

impl Default for Curl {
    fn default() -> Self {
        Curl { timeout_s: 12 }
    }
}

pub const USER_AGENT: &str = "SyrupUniversal/0.1 (game coach; +https://github.com/boggioMichael/us)";

impl Fetcher for Curl {
    fn get(&self, url: &str) -> Result<String, String> {
        let out = Command::new("curl")
            .args([
                "-sSL",
                "--compressed",
                "--max-time",
                &self.timeout_s.to_string(),
                "-A",
                USER_AGENT,
                "-H",
                "Accept: application/json, text/html;q=0.9",
            ])
            .arg(url)
            .output()
            .map_err(|e| format!("curl is not available: {e}"))?;
        if !out.status.success() {
            return Err(format!("{url}: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

/// The file name a URL's recorded response is kept under.
pub fn fixture_name(url: &str) -> String {
    let short: String = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect();
    let short: String = short.chars().take(80).collect();
    format!("{short}-{:08x}.txt", fnv64(url.as_bytes()) as u32)
}

/// Recorded responses from a folder (for tests and offline demos). Missing
/// ones are refused and listed, so a test says which URL it needed.
pub struct Fixtures {
    pub dir: PathBuf,
    pub missing: std::sync::Mutex<Vec<String>>,
}

impl Fixtures {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Fixtures { dir: dir.into(), missing: Default::default() }
    }
}

impl Fetcher for Fixtures {
    fn get(&self, url: &str) -> Result<String, String> {
        let path = self.dir.join(fixture_name(url));
        std::fs::read_to_string(&path).map_err(|_| {
            if let Ok(mut m) = self.missing.lock() {
                m.push(format!("{url} -> {}", path.display()));
            }
            format!("no recorded response for {url}")
        })
    }
}

/// Keeps responses on disk for a while, so the same question does not hit
/// the network twice in a week.
pub struct Cached<F: Fetcher> {
    pub inner: F,
    pub dir: PathBuf,
    pub max_age_s: u64,
}

impl<F: Fetcher> Cached<F> {
    pub fn new(inner: F, dir: &Path) -> Self {
        Cached { inner, dir: dir.to_path_buf(), max_age_s: 7 * 24 * 3600 }
    }
}

impl<F: Fetcher> Fetcher for Cached<F> {
    fn get(&self, url: &str) -> Result<String, String> {
        let path = self.dir.join(fixture_name(url));
        if let Ok(meta) = std::fs::metadata(&path)
            && let Ok(modified) = meta.modified()
            && let Ok(age) = modified.elapsed()
            && age.as_secs() < self.max_age_s
            && let Ok(body) = std::fs::read_to_string(&path)
        {
            return Ok(body);
        }
        let body = self.inner.get(url)?;
        let _ = std::fs::create_dir_all(&self.dir);
        let _ = std::fs::write(&path, &body);
        let _ = unix_now();
        Ok(body)
    }
}

/// Percent-encodes a query parameter.
pub fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_encoding() {
        assert_eq!(urlencode("Mossy King's crown"), "Mossy%20King%27s%20crown");
        let a = fixture_name("https://en.wikipedia.org/api/rest_v1/page/summary/Sky_Meadow");
        assert!(a.starts_with("en_wikipedia_org_api_rest_v1_page_summary_sky_meadow-"));
        assert_ne!(a, fixture_name("https://en.wikipedia.org/api/rest_v1/page/summary/Sky_Meadow2"));
    }

    #[test]
    fn fixtures_and_cache() {
        let dir = tempfile::tempdir().unwrap();
        let url = "https://example.org/a?b=1";
        std::fs::write(dir.path().join(fixture_name(url)), "hello").unwrap();
        let f = Fixtures::new(dir.path());
        assert_eq!(f.get(url).unwrap(), "hello");
        assert!(f.get("https://example.org/missing").is_err());
        assert_eq!(f.missing.lock().unwrap().len(), 1);
        let cache = tempfile::tempdir().unwrap();
        let c = Cached::new(Fixtures::new(dir.path()), cache.path());
        assert_eq!(c.get(url).unwrap(), "hello");
        std::fs::remove_file(dir.path().join(fixture_name(url))).unwrap();
        assert_eq!(c.get(url).unwrap(), "hello", "served from the cache");
        assert!(Offline.get(url).is_err());
    }
}
