use std::io;
use std::time::{Duration, Instant};

use crossterm::{
    ExecutableCommand,
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::{Backend, CrosstermBackend},
};
use tracing::warn;

use crate::error::Result;
use crate::proc_parser::ProcParser;
use crate::spectrum::SpectrumMonitor;
use crate::visualizer::Visualizer;

pub struct Ui {
    parser: ProcParser,
    spectrum: SpectrumMonitor,
    visualizer: Visualizer,
    selected_index: usize,
    show_hidden: bool,
    last_refresh: Instant,
    refresh_interval: Duration,
}

impl Ui {
    pub fn new() -> Self {
        Self {
            parser: ProcParser::new(),
            spectrum: SpectrumMonitor::new(),
            visualizer: Visualizer::new(),
            selected_index: 0,
            show_hidden: false,
            last_refresh: Instant::now(),
            refresh_interval: Duration::from_millis(500),
        }
    }

    pub fn run(&mut self) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        stdout.execute(EnterAlternateScreen)?;

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let result = self.run_app(&mut terminal);

        disable_raw_mode()?;
        terminal.backend_mut().execute(LeaveAlternateScreen)?;

        result
    }

    fn run_app<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        let mut audio_info = self.parser.parse_audio_info()?;

        loop {
            if self.should_refresh() {
                match self.parser.parse_audio_info() {
                    Ok(info) => {
                        audio_info = info;
                    }
                    Err(e) => {
                        warn!("Failed to parse audio info: {}", e);
                    }
                }
            }

            self.clamp_selection(&audio_info);
            let spectrum = self.spectrum.snapshot();

            terminal.draw(|f| {
                self.visualizer.render(
                    f,
                    &audio_info,
                    &spectrum,
                    self.selected_index,
                    self.show_hidden,
                );
            })?;

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        let visible_len = audio_info.visible_devices(self.show_hidden).len();

                        match key.code {
                            KeyCode::Char('q') | KeyCode::Char('Q') => break,
                            KeyCode::Up => {
                                if self.selected_index > 0 {
                                    self.selected_index -= 1;
                                }
                            }
                            KeyCode::Down => {
                                if self.selected_index < visible_len.saturating_sub(1) {
                                    self.selected_index += 1;
                                }
                            }
                            KeyCode::Char('h') | KeyCode::Char('H') => {
                                self.show_hidden = !self.show_hidden;
                                self.clamp_selection(&audio_info);
                            }
                            KeyCode::Char('-') | KeyCode::Char('_') => {
                                self.spectrum.adjust_sensitivity(-10);
                            }
                            KeyCode::Char('+') | KeyCode::Char('=') => {
                                self.spectrum.adjust_sensitivity(10);
                            }
                            KeyCode::Char('[') => {
                                self.spectrum.adjust_decay(-1);
                            }
                            KeyCode::Char(']') => {
                                self.spectrum.adjust_decay(1);
                            }
                            KeyCode::Char('r') | KeyCode::Char('R') => {
                                self.last_refresh = Instant::now() - self.refresh_interval;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn should_refresh(&self) -> bool {
        self.last_refresh.elapsed() >= self.refresh_interval
    }

    fn clamp_selection(&mut self, audio_info: &crate::audio_info::AudioInfo) {
        let visible_len = audio_info.visible_devices(self.show_hidden).len();
        self.selected_index = self.selected_index.min(visible_len.saturating_sub(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ui_new() {
        let ui = Ui::new();
        assert_eq!(ui.selected_index, 0);
        assert!(!ui.show_hidden);
        assert_eq!(ui.refresh_interval, Duration::from_millis(500));
    }
}
