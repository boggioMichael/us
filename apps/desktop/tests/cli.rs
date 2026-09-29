//! The `syrup` program, run as a player would run it.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn syrup(args: &[&str]) -> Output {
    let out = Command::new(env!("CARGO_BIN_EXE_syrup")).args(args).output().expect("syrup runs");
    assert!(
        out.status.success(),
        "syrup {args:?} failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/syrup-knowledge/fixtures")
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn simulate_learns_a_game_and_the_other_commands_see_it() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    let pictures = dir.path().join("overlay");
    // A game session, as JSON.
    let out = syrup(&[
        "simulate",
        "scroller",
        "--seconds",
        "30",
        "--ocr",
        "none",
        "--data-dir",
        s(&data),
        "--overlay-dir",
        s(&pictures),
        "--json",
        "--fixtures",
        s(&fixtures()),
    ]);
    let summary: serde_json::Value = serde_json::from_slice(&out.stdout).expect("a JSON summary");
    assert_eq!(summary["title"], "Sky Meadow Online");
    assert!(summary["analysed_frames"].as_u64().unwrap() >= 20, "{summary}");
    assert!(std::fs::read_dir(&pictures).unwrap().count() >= 2, "overlay pictures");
    // What was learned is listed, and can be shown in detail.
    let out = syrup(&["profiles", "--data-dir", s(&data)]);
    assert!(text(&out).contains("Sky Meadow Online"), "{}", text(&out));
    let out = syrup(&["profiles", "Sky Meadow Online", "--data-dir", s(&data)]);
    let profile: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(!profile["known_ui_elements"].as_array().unwrap().is_empty());
    // Research from recorded pages, remembered with the game.
    let out = syrup(&[
        "research",
        "Sky Meadow Online",
        "--topic",
        "Mossy King",
        "--fixtures",
        s(&fixtures()),
        "--data-dir",
        s(&data),
    ]);
    let t = text(&out);
    assert!(t.contains("Mossy King") && t.contains("https://"), "{t}");
    // Forgetting it.
    let out = syrup(&["forget", "Sky Meadow Online", "--data-dir", s(&data)]);
    assert!(text(&out).contains("Forgot"), "{}", text(&out));
    let out = syrup(&["profiles", "--data-dir", s(&data)]);
    assert!(text(&out).contains("not learned any game"), "{}", text(&out));
}

#[test]
fn a_folder_of_screenshots_replays() {
    let dir = tempfile::tempdir().unwrap();
    let frames = dir.path().join("frames");
    std::fs::create_dir_all(&frames).unwrap();
    // Twelve seconds of the dungeon, a frame every half second, named by time.
    let mut game = syrup_testgames::Session::of(syrup_testgames::GameKind::Dungeon, 2, 2.0, None);
    for i in 0..24u64 {
        let (img, _) = game.step_image();
        img.save(frames.join(format!("{}ms.png", i * 500))).unwrap();
    }
    let data = dir.path().join("data");
    let out = syrup(&["replay", s(&frames), "--ocr", "none", "--data-dir", s(&data), "--json", "--explain"]);
    let summary: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(summary["analysed_frames"].as_u64().unwrap() >= 10, "{summary}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("sees:"), "--explain prints what Syrup sees");
}

#[test]
fn syrup_draws_itself() {
    let dir = tempfile::tempdir().unwrap();
    syrup(&["avatar", "--out", s(dir.path())]);
    for name in ["hats.png", "face-neutral.png", "face-warning.png", "overlay-normal.png", "overlay-postgame.png"] {
        let img = image::open(dir.path().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(img.width() > 50, "{name}");
    }
}

#[test]
fn mistakes_are_explained() {
    let out = Command::new(env!("CARGO_BIN_EXE_syrup")).args(["simulate", "tetris"]).output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no test game called \"tetris\""));
    let out =
        Command::new(env!("CARGO_BIN_EXE_syrup")).args(["simulate", "cards", "--spoilers", "some"]).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stderr).contains("no spoiler level"));
}
