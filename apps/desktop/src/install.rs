//! `syrup install` and `syrup uninstall`: Syrup's brain for the phone app, on
//! this Windows PC, running whenever you're signed in, and reachable by your
//! phone through Tailscale Funnel.
//!
//! Nothing here needs administrator rights. The program is copied to
//! `%LOCALAPPDATA%\Syrup`, and a small launcher in your Startup folder starts
//! it without a window. What Syrup learns stays where it always is
//! (`%APPDATA%\SyrupUniversal`), and `uninstall` leaves it there.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const PORT: u16 = 8080;
const LAUNCHER: &str = "Syrup.vbs";
const SECRETS: &str = "https://github.com/boggioMichael/us/settings/secrets/actions/new";

/// Starts `syrup.exe serve` without a window. Plain ASCII, so it works
/// whatever the Windows user is called: the folder is found when it runs.
const VBS: &str = r#"' Starts Syrup's brain for the phone app, without a window.
' Put here by "syrup install"; "syrup uninstall" removes it.
Set shell = CreateObject("WScript.Shell")
dir = shell.ExpandEnvironmentStrings("%LOCALAPPDATA%") & "\Syrup"
shell.CurrentDirectory = dir
shell.Run """" & dir & "\syrup.exe"" serve --bind 127.0.0.1 --port 8080", 0, False
"#;

fn folder(var: &str) -> Result<PathBuf, String> {
    std::env::var_os(var).map(PathBuf::from).ok_or_else(|| format!("{var} is not set"))
}

fn startup_folder() -> Result<PathBuf, String> {
    Ok(folder("APPDATA")?.join(r"Microsoft\Windows\Start Menu\Programs\Startup"))
}

fn windows_only(what: &str) -> Result<(), String> {
    if cfg!(windows) {
        Ok(())
    } else {
        Err(format!("`syrup {what}` sets Syrup up on a Windows PC. Elsewhere, run `syrup serve` (docs/iphone.md)."))
    }
}

/// Stops every other running `syrup.exe` (an older copy, still serving).
fn stop_others() {
    let _ = Command::new("taskkill")
        .args(["/F", "/IM", "syrup.exe", "/FI", &format!("PID ne {}", std::process::id())])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn same_file(a: &Path, b: &Path) -> bool {
    matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
}

/// Whether Syrup answers on this computer.
fn answering() -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], PORT));
    let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_millis(500)) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    if s.write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\n\r\n").is_err() {
        return false;
    }
    let mut answer = String::new();
    let _ = s.read_to_string(&mut answer);
    answer.starts_with("HTTP/1.1 200")
}

fn tailscale() -> Option<PathBuf> {
    let works = |p: &Path| {
        Command::new(p).arg("version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
    };
    let on_path = PathBuf::from("tailscale");
    if works(&on_path) {
        return Some(on_path);
    }
    let installed = folder("ProgramFiles").ok()?.join(r"Tailscale\tailscale.exe");
    works(&installed).then_some(installed)
}

/// The address Funnel gives this computer (`https://….ts.net`), if it is on.
fn funnel_address(ts: &Path) -> Option<String> {
    let out = Command::new(ts).args(["funnel", "status"]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    first_https(&text)
}

fn first_https(text: &str) -> Option<String> {
    let at = text.find("https://")?;
    let url: String = text[at..].chars().take_while(|c| !c.is_whitespace() && !matches!(c, ')' | '"' | '\'')).collect();
    let url = url.trim_end_matches(['/', ',', '.']).to_string();
    url.contains(".ts.net").then_some(url)
}

fn to_clipboard(text: &str) -> bool {
    let Ok(mut child) = Command::new("clip").stdin(Stdio::piped()).spawn() else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    child.wait().is_ok_and(|s| s.success())
}

fn open_in_browser(url: &str) {
    let _ = Command::new("cmd").args(["/C", "start", "", url]).status();
}

pub fn install() -> Result<(), String> {
    windows_only("install")?;
    let home = folder("LOCALAPPDATA")?.join("Syrup");
    std::fs::create_dir_all(&home).map_err(|e| format!("could not make {}: {e}", home.display()))?;
    let me = std::env::current_exe().map_err(|e| format!("where am I? {e}"))?;
    let target = home.join("syrup.exe");
    println!("Installing Syrup's brain for the phone app…");
    stop_others();
    if !same_file(&me, &target) {
        // The copy that was running may take a moment to let go of the file.
        let mut copied = Err(std::io::Error::other("not tried"));
        for _ in 0..40 {
            copied = std::fs::copy(&me, &target);
            if copied.is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        copied.map_err(|e| format!("could not copy Syrup to {}: {e}", target.display()))?;
    }
    let vbs = VBS.replace('\n', "\r\n");
    let launcher = home.join(LAUNCHER);
    std::fs::write(&launcher, &vbs).map_err(|e| format!("could not write {}: {e}", launcher.display()))?;
    let startup = startup_folder()?;
    std::fs::create_dir_all(&startup).map_err(|e| format!("could not open the Startup folder: {e}"))?;
    std::fs::write(startup.join(LAUNCHER), &vbs)
        .map_err(|e| format!("could not add Syrup to the Startup folder: {e}"))?;
    Command::new("wscript.exe").arg(&launcher).status().map_err(|e| format!("could not start Syrup: {e}"))?;
    let mut up = false;
    for _ in 0..30 {
        if answering() {
            up = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    if !up {
        return Err("Syrup didn't start. If Windows said it blocked it, Smart App Control is on (docs/iphone.md). \
                    Double-click Start Syrup.cmd to see what's wrong."
            .into());
    }
    println!();
    println!("  ✓ Syrup's brain is running, and starts by itself whenever you sign in to Windows.");
    println!();
    let Some(ts) = tailscale() else {
        println!("  Next: your phone needs a way to reach this computer.");
        println!("  Install Tailscale from https://tailscale.com/download, sign in, then run Install Syrup again.");
        open_in_browser("https://tailscale.com/download/windows");
        return Ok(());
    };
    if funnel_address(&ts).is_none() {
        println!("  Opening a door for your phone (Tailscale Funnel). If a link appears, open it and allow Funnel.");
        println!();
        let _ = Command::new(&ts).args(["funnel", "--bg", &PORT.to_string()]).status();
    }
    match funnel_address(&ts) {
        Some(url) => {
            println!();
            println!("  ✓ Your phone reaches Syrup at:");
            println!();
            println!("      {url}");
            println!();
            if to_clipboard(&url) {
                println!("  It's copied. In the GitHub page that opens, name the secret SYRUP_SERVER_URL,");
                println!("  paste the address as its value, and press Add secret.");
                open_in_browser(SECRETS);
            } else {
                println!("  Put it in GitHub as the secret SYRUP_SERVER_URL: {SECRETS}");
            }
            Ok(())
        }
        None => Err("Tailscale didn't give this computer an address yet. Make sure you're signed in to Tailscale \
                     and that Funnel is allowed (the link it showed), then run Install Syrup again."
            .into()),
    }
}

pub fn uninstall() -> Result<(), String> {
    windows_only("uninstall")?;
    stop_others();
    let launcher = startup_folder()?.join(LAUNCHER);
    if launcher.exists() {
        std::fs::remove_file(&launcher).map_err(|e| format!("could not remove {}: {e}", launcher.display()))?;
    }
    println!("Syrup's brain is stopped, and won't start with Windows any more.");
    println!("What it learned is still in %APPDATA%\\SyrupUniversal (`syrup forget --everything` deletes it).");
    println!("To close the door Tailscale opened for your phone: tailscale funnel reset");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_funnel_address_is_found_in_what_tailscale_prints() {
        let status = "# Funnel on:\n#     - https://desktop-7e0609g.tail1234.ts.net\n\nhttps://desktop-7e0609g.tail1234.ts.net (Funnel on)\n|-- / proxy http://127.0.0.1:8080\n";
        assert_eq!(first_https(status).as_deref(), Some("https://desktop-7e0609g.tail1234.ts.net"));
        assert_eq!(first_https("No serve config"), None);
        assert_eq!(first_https("see https://example.com/help"), None);
    }

    #[test]
    fn the_launcher_is_plain_ascii_and_starts_the_server_hidden() {
        assert!(VBS.is_ascii());
        assert!(VBS.contains(r#"serve --bind 127.0.0.1 --port 8080", 0, False"#));
        assert!(VBS.contains(r#"ExpandEnvironmentStrings("%LOCALAPPDATA%")"#));
    }
}
