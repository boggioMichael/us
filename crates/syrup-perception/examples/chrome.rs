//! Finds the game in a screenshot of the whole screen (without the taskbar):
//! `cargo run -p syrup-perception --example chrome -- <screenshot.png>`

fn main() {
    let path = std::env::args().nth(1).expect("a screenshot");
    let img = image::open(&path).expect("an image").to_rgba8();
    let engine = syrup_perception::best_engine();
    match syrup_perception::chrome::game_area(&img, &*engine) {
        Some(r) => println!("the game is {}x{} at ({}, {}); below it is the taskbar", r.w, r.h, r.x, r.y),
        None => println!("no taskbar found ({} OCR)", engine.name()),
    }
}
