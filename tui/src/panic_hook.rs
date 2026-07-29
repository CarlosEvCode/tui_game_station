use crossterm::{
    execute,
    terminal::{disable_raw_mode, LeaveAlternateScreen},
};
use std::io::stdout;
use std::panic;

pub fn init_panic_hook() {
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen);
        eprintln!("\n=== TUI GAME STATION CRASHED ===");
        eprintln!("Restoring terminal state...");
        original_hook(panic_info);
    }));
}
