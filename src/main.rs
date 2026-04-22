use std::env;

use crate::error::Result;
use crate::ui::Ui;

mod audio_info;
mod error;
mod proc_parser;
mod spectrum;
mod ui;
mod visualizer;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const NAME: &str = env!("CARGO_PKG_NAME");

fn main() {
    tracing_subscriber::fmt::init();

    let args: Vec<String> = env::args().collect();
    if args.len() > 1 {
        match args[1].as_str() {
            "--help" | "-h" => {
                print_help();
                return;
            }
            "--version" | "-V" => {
                println!("{NAME} {VERSION}");
                return;
            }
            _ => {}
        }
    }

    if !is_tty() {
        eprintln!("Error: {NAME} requires an interactive terminal (TTY).");
        std::process::exit(1);
    }

    if let Err(e) = run_app() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn print_help() {
    println!("{NAME} {VERSION}");
    println!();
    println!("Terminal-based audio pipeline visualizer.");
    println!();
    println!("USAGE:");
    println!("    {NAME} [OPTIONS]");
    println!();
    println!("OPTIONS:");
    println!("    -h, --help       Print help information");
    println!("    -V, --version    Print version information");
    println!();
    println!("KEYBOARD SHORTCUTS:");
    println!("    q                Quit");
    println!("    ↑/↓              Navigate devices");
    println!("    h                Toggle hidden/stopped devices");
    println!("    +/-              Adjust spectrum sensitivity");
    println!("    [ ]              Adjust spectrum decay");
    println!("    j/k              Adjust output sample rate");
}

#[cfg(unix)]
fn is_tty() -> bool {
    unsafe { libc::isatty(libc::STDIN_FILENO) != 0 }
}

#[cfg(not(unix))]
fn is_tty() -> bool {
    true
}

fn run_app() -> Result<()> {
    let mut ui = Ui::new();
    ui.run()
}
