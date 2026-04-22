#[cfg(target_os = "linux")]
use std::fs;
use std::io;
#[cfg(target_os = "macos")]
use std::mem;
#[cfg(target_os = "linux")]
use std::process::Command;
#[cfg(target_os = "macos")]
use std::ptr::{NonNull, null};
#[cfg(target_os = "linux")]
use std::thread;
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
use cpal::traits::{DeviceTrait, HostTrait};
use crossterm::{
    ExecutableCommand,
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
#[cfg(target_os = "macos")]
use objc2_core_audio::{
    AudioObjectGetPropertyData, AudioObjectID, AudioObjectPropertyAddress,
    AudioObjectSetPropertyData, kAudioDevicePropertyNominalSampleRate,
    kAudioHardwarePropertyDefaultOutputDevice, kAudioObjectPropertyElementMain,
    kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
};
use ratatui::{
    Terminal,
    backend::{Backend, CrosstermBackend},
};
#[cfg(target_os = "macos")]
use std::process::Command;
use tracing::warn;

use crate::error::Result;
use crate::proc_parser::ProcParser;
use crate::spectrum::SpectrumMonitor;
use crate::visualizer::Visualizer;

#[derive(Debug, Clone, PartialEq)]
pub struct OutputRateInfo {
    pub current_rate: u32,
    pub selected_rate: Option<u32>,
}

pub struct Ui {
    parser: ProcParser,
    spectrum: SpectrumMonitor,
    visualizer: Visualizer,
    selected_index: usize,
    show_hidden: bool,
    rate_status: Option<(String, Instant)>,
    diagnostic_status: Option<(String, Instant)>,
    last_refresh: Instant,
    refresh_interval: Duration,
    #[cfg(target_os = "macos")]
    cached_audio_info: Option<crate::audio_info::AudioInfo>,
    #[cfg(target_os = "macos")]
    last_seen_rate: Option<u32>,
}

impl Ui {
    pub fn new() -> Self {
        Self {
            parser: ProcParser::new(),
            spectrum: SpectrumMonitor::new(),
            visualizer: Visualizer::new(),
            selected_index: 0,
            show_hidden: false,
            rate_status: None,
            diagnostic_status: None,
            last_refresh: Instant::now(),
            refresh_interval: Duration::from_millis(500),
            #[cfg(target_os = "macos")]
            cached_audio_info: None,
            #[cfg(target_os = "macos")]
            last_seen_rate: None,
        }
    }

    pub fn run(&mut self) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        stdout.execute(EnterAlternateScreen)?;

        // Set terminal title and keep it fixed
        print!("\x1b]0;audiorep\x07");
        io::Write::flush(&mut stdout)?;

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let result = self.run_app(&mut terminal);

        disable_raw_mode()?;
        terminal.backend_mut().execute(LeaveAlternateScreen)?;

        result
    }

    fn run_app<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        #[cfg(target_os = "macos")]
        {
            // On macOS, parse once and cache to avoid repeated system_profiler calls
            if self.cached_audio_info.is_none() {
                match self.parser.parse_audio_info() {
                    Ok(info) => self.cached_audio_info = Some(info),
                    Err(error) => {
                        let message = format!("audio probe failed: {}", error);
                        warn!("{}", message);
                        self.set_diagnostic_status(message);
                    }
                }
            }
        }

        let mut audio_info = match self.parser.parse_audio_info() {
            Ok(info) => info,
            Err(error) => {
                let message = format!("audio probe failed: {}", error);
                warn!("{}", message);
                self.set_diagnostic_status(message);
                crate::audio_info::AudioInfo {
                    devices: Vec::new(),
                }
            }
        };

        loop {
            let has_input = event::poll(Duration::from_millis(16))?;

            #[cfg(not(target_os = "macos"))]
            if self.should_refresh() {
                match self.parser.parse_audio_info() {
                    Ok(info) => {
                        audio_info = info;
                        self.diagnostic_status = None;
                    }
                    Err(e) => {
                        let message = format!("audio probe failed: {}", e);
                        warn!("{}", message);
                        self.set_diagnostic_status(message);
                    }
                }
            }

            #[cfg(target_os = "macos")]
            {
                // On macOS, use cached info to avoid system_profiler stutter
                if let Some(ref cached) = self.cached_audio_info {
                    audio_info = cached.clone();
                }
            }

            self.clamp_selection(&audio_info);
            self.clear_expired_statuses();
            let spectrum = self.spectrum.snapshot();
            let rate_info = self.current_rate_info();
            let footer_rate_label = self.footer_rate_label();
            let status_message = self.status_message().or_else(|| {
                if !spectrum.active && !spectrum.message.is_empty() {
                    Some(spectrum.message.clone())
                } else {
                    None
                }
            });
            terminal.draw(|f| {
                self.visualizer.render(
                    f,
                    &audio_info,
                    &spectrum,
                    rate_info.as_ref(),
                    &footer_rate_label,
                    status_message.as_deref(),
                    self.selected_index,
                    self.show_hidden,
                );
            })?;

            if has_input
                && let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                let visible_len = audio_info.visible_devices(self.show_hidden).len();

                match key.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') => break,
                    KeyCode::Up if self.selected_index > 0 => {
                        self.selected_index -= 1;
                    }
                    KeyCode::Down if self.selected_index < visible_len.saturating_sub(1) => {
                        self.selected_index += 1;
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
                    KeyCode::Char('j') | KeyCode::Char('J') => {
                        self.adjust_output_rate(-1);
                    }
                    KeyCode::Char('k') | KeyCode::Char('K') => {
                        self.adjust_output_rate(1);
                    }
                    _ => {}
                }
            }
        }

        Ok(())
    }

    fn should_refresh(&mut self) -> bool {
        if self.last_refresh.elapsed() >= self.refresh_interval {
            self.last_refresh = Instant::now();
            true
        } else {
            false
        }
    }

    fn clamp_selection(&mut self, audio_info: &crate::audio_info::AudioInfo) {
        let visible_len = audio_info.visible_devices(self.show_hidden).len();
        self.selected_index = self.selected_index.min(visible_len.saturating_sub(1));
    }

    fn current_rate_info(&self) -> Option<OutputRateInfo> {
        #[cfg(target_os = "linux")]
        {
            Self::read_pipewire_rates()
                .ok()
                .map(
                    |(current_rate, forced_rate, _allowed_rates)| OutputRateInfo {
                        current_rate,
                        selected_rate: (forced_rate != 0).then_some(forced_rate),
                    },
                )
        }

        #[cfg(target_os = "macos")]
        {
            Self::read_macos_rates()
                .ok()
                .map(|(current_rate, _supported_rates)| OutputRateInfo {
                    current_rate,
                    selected_rate: Some(current_rate),
                })
        }

        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        Self::read_pipewire_rates()
            .ok()
            .map(
                |(current_rate, forced_rate, _allowed_rates)| OutputRateInfo {
                    current_rate,
                    selected_rate: (forced_rate != 0).then_some(forced_rate),
                },
            )
    }

    #[cfg(target_os = "linux")]
    fn footer_rate_label(&self) -> String {
        "j/k: rate".to_string()
    }

    #[cfg(target_os = "macos")]
    fn footer_rate_label(&self) -> String {
        let rates = Self::read_macos_rates()
            .map(|(_, rates)| rates)
            .unwrap_or_default();
        if rates.is_empty() {
            "j/k: rate".to_string()
        } else {
            let rates_str = rates
                .iter()
                .map(|r| format!("{}", r / 1000))
                .collect::<Vec<_>>()
                .join(", ");
            format!("j/k: rate [{}k]", rates_str)
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn footer_rate_label(&self) -> String {
        "rate: unsupported".to_string()
    }

    #[cfg(target_os = "linux")]
    fn adjust_output_rate(&mut self, direction: isize) {
        let Ok((current_rate, forced_rate, allowed_rates)) = Self::read_pipewire_rates() else {
            return;
        };
        let supported_rates =
            Self::read_supported_output_rates().unwrap_or_else(|| allowed_rates.clone());
        let candidate_rates = Self::filter_supported_rates(&allowed_rates, &supported_rates);
        let anchor_rate = if forced_rate == 0 {
            current_rate
        } else {
            forced_rate
        };
        let Some(target_rate) = Self::step_rate(anchor_rate, &candidate_rates, direction) else {
            return;
        };

        let _ = Command::new("pw-metadata")
            .args([
                "-n",
                "settings",
                "0",
                "clock.force-rate",
                &target_rate.to_string(),
            ])
            .output();

        self.reset_default_sink();
        let message = format!("forcing {} Hz, resetting sink", target_rate);
        self.rate_status = Some((message, Instant::now()));
    }

    #[cfg(target_os = "macos")]
    fn adjust_output_rate(&mut self, direction: isize) {
        let Ok((current_rate, supported_rates)) = Self::read_macos_rates() else {
            self.rate_status = Some((
                "failed to read macOS output rate".to_string(),
                Instant::now(),
            ));
            return;
        };
        let Some(target_rate) = Self::step_rate(current_rate, &supported_rates, direction) else {
            return;
        };

        match Self::set_macos_output_rate(target_rate) {
            Ok(()) => {
                let message = format!("set default output to {} Hz", target_rate);
                self.rate_status = Some((message, Instant::now()));
            }
            Err(error) => {
                self.rate_status = Some((error, Instant::now()));
            }
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn adjust_output_rate(&mut self, _direction: isize) {
        self.rate_status = Some((
            "rate control is not supported on this platform".to_string(),
            Instant::now(),
        ));
    }

    fn set_diagnostic_status(&mut self, message: String) {
        self.diagnostic_status = Some((message, Instant::now()));
    }

    fn status_message(&self) -> Option<String> {
        self.rate_status
            .as_ref()
            .map(|(message, _)| message.clone())
            .or_else(|| {
                self.diagnostic_status
                    .as_ref()
                    .map(|(message, _)| message.clone())
            })
    }

    fn clear_expired_statuses(&mut self) {
        if self
            .rate_status
            .as_ref()
            .is_some_and(|(_, timestamp)| timestamp.elapsed() > Duration::from_secs(2))
        {
            self.rate_status = None;
        }

        if self
            .diagnostic_status
            .as_ref()
            .is_some_and(|(_, timestamp)| timestamp.elapsed() > Duration::from_secs(4))
        {
            self.diagnostic_status = None;
        }
    }

    #[cfg(target_os = "linux")]
    fn reset_default_sink(&self) {
        let _ = Command::new("pactl")
            .args(["suspend-sink", "@DEFAULT_SINK@", "1"])
            .output();
        thread::sleep(Duration::from_millis(200));
        let _ = Command::new("pactl")
            .args(["suspend-sink", "@DEFAULT_SINK@", "0"])
            .output();
    }

    #[cfg(target_os = "linux")]
    fn read_pipewire_rates() -> std::result::Result<(u32, u32, Vec<u32>), ()> {
        let output = Command::new("pw-metadata")
            .args(["-n", "settings"])
            .output()
            .map_err(|_| ())?;
        if !output.status.success() {
            return Err(());
        }

        let text = String::from_utf8(output.stdout).map_err(|_| ())?;
        let current_rate = text
            .lines()
            .find_map(|line| line.split("key:'clock.rate' value:'").nth(1))
            .and_then(|part| part.split('\'').next())
            .and_then(|part| part.parse::<u32>().ok())
            .ok_or(())?;

        let forced_rate = text
            .lines()
            .find_map(|line| line.split("key:'clock.force-rate' value:'").nth(1))
            .and_then(|part| part.split('\'').next())
            .and_then(|part| part.parse::<u32>().ok())
            .unwrap_or(0);

        let allowed_rates = text
            .lines()
            .find_map(|line| line.split("key:'clock.allowed-rates' value:'").nth(1))
            .and_then(|part| part.split('\'').next())
            .map(Self::parse_allowed_rates)
            .filter(|rates| !rates.is_empty())
            .unwrap_or_else(|| vec![current_rate]);

        Ok((current_rate, forced_rate, allowed_rates))
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn read_pipewire_rates() -> std::result::Result<(u32, u32, Vec<u32>), ()> {
        Err(())
    }

    #[cfg(target_os = "macos")]
    fn read_macos_rates() -> std::result::Result<(u32, Vec<u32>), ()> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or(())?;
        let current_rate = device
            .default_output_config()
            .map_err(|_| ())?
            .sample_rate();
        let ranges = device
            .supported_output_configs()
            .map_err(|_| ())?
            .map(|config| (config.min_sample_rate(), config.max_sample_rate()))
            .collect::<Vec<_>>();
        let supported_rates = Self::expand_supported_rates(&ranges, current_rate);
        Ok((current_rate, supported_rates))
    }

    #[cfg(target_os = "macos")]
    fn set_macos_output_rate(target_rate: u32) -> std::result::Result<(), String> {
        let device_id = Self::default_macos_output_device_id()
            .ok_or_else(|| "failed to find default macOS output device".to_string())?;
        let property = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyNominalSampleRate,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        };
        let rate = target_rate as f64;
        let size = mem::size_of::<f64>() as u32;
        let status = unsafe {
            AudioObjectSetPropertyData(
                device_id,
                NonNull::from(&property),
                0,
                null(),
                size,
                NonNull::from(&rate).cast(),
            )
        };

        if status == 0 {
            Ok(())
        } else {
            Err(format!(
                "failed to set macOS output rate to {} Hz (OSStatus {})",
                target_rate, status
            ))
        }
    }

    #[cfg(target_os = "macos")]
    fn default_macos_output_device_id() -> Option<AudioObjectID> {
        let property = AudioObjectPropertyAddress {
            mSelector: kAudioHardwarePropertyDefaultOutputDevice,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        };
        let mut device_id: AudioObjectID = 0;
        let mut size = mem::size_of::<AudioObjectID>() as u32;
        let status = unsafe {
            AudioObjectGetPropertyData(
                kAudioObjectSystemObject as u32,
                NonNull::from(&property),
                0,
                null(),
                NonNull::from(&mut size),
                NonNull::from(&mut device_id).cast(),
            )
        };

        (status == 0 && device_id != 0).then_some(device_id)
    }

    #[cfg(any(target_os = "linux", test))]
    fn parse_allowed_rates(raw: &str) -> Vec<u32> {
        raw.trim_matches(|ch| ch == '[' || ch == ']')
            .split_whitespace()
            .filter_map(|part| part.trim_end_matches(',').parse::<u32>().ok())
            .collect()
    }

    fn step_rate(current_rate: u32, allowed_rates: &[u32], direction: isize) -> Option<u32> {
        if allowed_rates.is_empty() || direction == 0 {
            return None;
        }

        let current_index = allowed_rates
            .iter()
            .position(|rate| *rate == current_rate)?;
        let next_index = current_index.saturating_add_signed(direction);
        allowed_rates.get(next_index).copied()
    }

    #[cfg(any(target_os = "linux", test))]
    fn filter_supported_rates(allowed_rates: &[u32], supported_rates: &[u32]) -> Vec<u32> {
        let filtered: Vec<u32> = allowed_rates
            .iter()
            .copied()
            .filter(|rate| supported_rates.contains(rate))
            .collect();

        if filtered.is_empty() {
            allowed_rates.to_vec()
        } else {
            filtered
        }
    }

    #[cfg(target_os = "linux")]
    fn read_supported_output_rates() -> Option<Vec<u32>> {
        let card_id = Self::read_default_sink_card_id()?;
        let stream_path = format!("/proc/asound/card{}/stream0", card_id);
        let content = fs::read_to_string(stream_path).ok()?;
        let mut rates = Vec::new();

        for line in content.lines() {
            let trimmed = line.trim();
            if let Some(raw_rates) = trimmed.strip_prefix("Rates: ") {
                for rate in raw_rates.split(',') {
                    if let Ok(rate) = rate.trim().parse::<u32>()
                        && !rates.contains(&rate)
                    {
                        rates.push(rate);
                    }
                }
            }
        }

        if rates.is_empty() {
            None
        } else {
            rates.sort_unstable();
            Some(rates)
        }
    }

    #[cfg(target_os = "linux")]
    fn read_default_sink_card_id() -> Option<u32> {
        let output = Command::new("pactl")
            .args(["list", "sinks"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }

        let text = String::from_utf8(output.stdout).ok()?;
        text.lines().find_map(|line| {
            line.trim()
                .strip_prefix("api.alsa.pcm.card = ")
                .and_then(|value| value.trim_matches('"').parse::<u32>().ok())
        })
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
        assert!(ui.diagnostic_status.is_none());
        assert_eq!(ui.refresh_interval, Duration::from_millis(500));
    }

    #[test]
    fn test_parse_allowed_rates() {
        assert_eq!(
            Ui::parse_allowed_rates("[ 44100 48000 96000 ]"),
            vec![44100, 48000, 96000]
        );
        assert_eq!(
            Ui::parse_allowed_rates("[ 44100, 48000, 96000 ]"),
            vec![44100, 48000, 96000]
        );
    }

    #[test]
    fn test_step_rate() {
        let rates = vec![44100, 48000, 96000];
        assert_eq!(Ui::step_rate(48000, &rates, -1), Some(44100));
        assert_eq!(Ui::step_rate(48000, &rates, 1), Some(96000));
        assert_eq!(Ui::step_rate(44100, &rates, -1), Some(44100));
        assert_eq!(Ui::step_rate(96000, &rates, 1), None);
    }

    #[test]
    fn test_filter_supported_rates() {
        let rates = Ui::filter_supported_rates(&[44100, 48000, 88200, 96000], &[48000, 96000]);
        assert_eq!(rates, vec![48000, 96000]);
    }
}
