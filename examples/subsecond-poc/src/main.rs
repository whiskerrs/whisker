//! Standalone POC for the subsecond crate. See `Cargo.toml` for what
//! this verifies and what it deliberately does *not* verify.
//!
//! Usage:
//!   cargo run -p subsecond-poc
//!   # then press Enter to invoke render(); Ctrl-C to quit.

use std::io::{self, BufRead, Write};
use std::sync::Arc;

mod app;

fn main() {
    println!("subsecond-poc — minimal API sanity check\n");

    // 1. The jump table starts empty: no patch applied yet.
    let jt = unsafe { subsecond::get_jump_table() };
    println!(
        "  subsecond::get_jump_table() on boot ─ {}",
        if jt.is_some() { "Some(_)" } else { "None" },
    );

    // 2. A patch handler is registered for free; we'll never fire it in
    //    this POC (no sender exists), but the call has to compile and
    //    not blow up at registration time.
    subsecond::register_handler(Arc::new(|| {
        println!("[subsecond] (would-fire) patch applied callback");
    }));
    println!("  register_handler  ─ ok");

    // 3. `subsecond::call(|| f())` should be observably equivalent to
    //    `f()` while no patch is in play. We loop on stdin so the
    //    behaviour is easy to eyeball.
    println!("\nPress Enter to invoke app::render() (Ctrl-C to quit):");
    let stdin = io::stdin();
    let mut tick: u64 = 0;
    loop {
        print!("> ");
        io::stdout().flush().ok();
        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => break, // EOF
            Ok(_) => {}
            Err(_) => break,
        }
        tick += 1;
        subsecond::call(|| app::render(tick));
    }
}
