//! The function we want to "hot reload" in this POC.
//!
//! Without a running sender (the `tuft run` watcher we haven't written
//! yet), editing this file and rebuilding only takes effect when you
//! restart `subsecond-poc`. The sender is the missing half that turns
//! `subsecond::call` from "pass-through" into "actual hot reload".

pub fn render(tick: u64) {
    // Edit this line, save, rerun `cargo run -p subsecond-poc`.
    // Once we have the sender side, no restart will be needed.
    println!("[app] tick {tick}: hello from subsecond-poc render()");
}
