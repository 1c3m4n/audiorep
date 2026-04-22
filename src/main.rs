use crate::error::Result;
use crate::ui::Ui;

mod audio_info;
mod error;
mod proc_parser;
mod spectrum;
mod ui;
mod visualizer;

fn main() {
    tracing_subscriber::fmt::init();

    if let Err(e) = run_app() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn run_app() -> Result<()> {
    let mut ui = Ui::new();
    ui.run()
}
