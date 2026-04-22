#[cfg(target_os = "linux")]
use std::io::Read;
#[cfg(target_os = "linux")]
use std::process::{Command, Stdio};
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "linux")]
use std::sync::{Arc, Mutex};
#[cfg(target_os = "linux")]
use std::thread;

#[cfg(target_os = "linux")]
use rustfft::{FftPlanner, num_complex::Complex32};

const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: usize = 2;
const FFT_SIZE: usize = 2048;
const BAR_COUNT: usize = 32;
const UPDATE_STEP: usize = 512;
const MIN_FREQUENCY: f32 = 30.0;
const DEFAULT_SENSITIVITY: usize = 100;
const DEFAULT_DECAY: usize = 2;
const MAX_SENSITIVITY: usize = 250;
const MAX_DECAY: usize = 12;

#[derive(Debug, Clone)]
pub struct SpectrumSnapshot {
    pub bins: Vec<u64>,
    pub peaks: Vec<u64>,
    pub source_name: Option<String>,
    pub message: String,
    pub active: bool,
    pub sensitivity: usize,
    pub decay: usize,
}

impl SpectrumSnapshot {
    fn starting() -> Self {
        Self {
            bins: vec![0; BAR_COUNT],
            peaks: vec![0; BAR_COUNT],
            source_name: None,
            message: "Starting spectrum capture...".to_string(),
            active: false,
            sensitivity: DEFAULT_SENSITIVITY,
            decay: DEFAULT_DECAY,
        }
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct SpectrumSettings {
    sensitivity: usize,
    decay: usize,
}

pub struct SpectrumMonitor {
    #[cfg(target_os = "linux")]
    snapshot: Arc<Mutex<SpectrumSnapshot>>,
    #[cfg(target_os = "linux")]
    stop: Arc<AtomicBool>,
    #[cfg(target_os = "linux")]
    child_pid: Arc<Mutex<Option<u32>>>,
    #[cfg(target_os = "linux")]
    settings: Arc<Mutex<SpectrumSettings>>,
}

impl SpectrumMonitor {
    #[cfg(not(target_os = "linux"))]
    pub fn new() -> Self {
        Self {}
    }

    #[cfg(target_os = "linux")]
    pub fn new() -> Self {
        let snapshot = Arc::new(Mutex::new(SpectrumSnapshot::starting()));
        let stop = Arc::new(AtomicBool::new(false));
        let child_pid = Arc::new(Mutex::new(None));
        let settings = Arc::new(Mutex::new(SpectrumSettings {
            sensitivity: DEFAULT_SENSITIVITY,
            decay: DEFAULT_DECAY,
        }));

        let worker_snapshot = Arc::clone(&snapshot);
        let worker_stop = Arc::clone(&stop);
        let worker_child_pid = Arc::clone(&child_pid);
        let worker_settings = Arc::clone(&settings);

        thread::spawn(move || {
            run_capture(
                worker_snapshot,
                worker_stop,
                worker_child_pid,
                worker_settings,
            );
        });

        Self {
            snapshot,
            stop,
            child_pid,
            settings,
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub fn snapshot(&self) -> SpectrumSnapshot {
        SpectrumSnapshot {
            bins: vec![0; BAR_COUNT],
            peaks: vec![0; BAR_COUNT],
            source_name: None,
            message: "Spectrum capture is not supported on this platform yet".to_string(),
            active: false,
            sensitivity: DEFAULT_SENSITIVITY,
            decay: DEFAULT_DECAY,
        }
    }

    #[cfg(target_os = "linux")]
    pub fn snapshot(&self) -> SpectrumSnapshot {
        self.snapshot
            .lock()
            .map(|snapshot| snapshot.clone())
            .unwrap_or_else(|_| SpectrumSnapshot {
                bins: vec![0; BAR_COUNT],
                peaks: vec![0; BAR_COUNT],
                source_name: None,
                message: "Spectrum state unavailable".to_string(),
                active: false,
                sensitivity: DEFAULT_SENSITIVITY,
                decay: DEFAULT_DECAY,
            })
    }

    #[cfg(not(target_os = "linux"))]
    pub fn adjust_sensitivity(&self, _delta: isize) {}

    #[cfg(target_os = "linux")]
    pub fn adjust_sensitivity(&self, delta: isize) {
        self.update_settings(|settings| {
            settings.sensitivity = clamp_setting(settings.sensitivity, delta, 25, MAX_SENSITIVITY);
        });
    }

    #[cfg(not(target_os = "linux"))]
    pub fn adjust_decay(&self, _delta: isize) {}

    #[cfg(target_os = "linux")]
    pub fn adjust_decay(&self, delta: isize) {
        self.update_settings(|settings| {
            settings.decay = clamp_setting(settings.decay, delta, 1, MAX_DECAY);
        });
    }

    #[cfg(target_os = "linux")]
    fn update_settings(&self, update: impl FnOnce(&mut SpectrumSettings)) {
        if let Ok(mut settings) = self.settings.lock() {
            update(&mut settings);
            if let Ok(mut snapshot) = self.snapshot.lock() {
                snapshot.sensitivity = settings.sensitivity;
                snapshot.decay = settings.decay;
            }
        }
    }
}

#[cfg(target_os = "linux")]
impl Drop for SpectrumMonitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);

        if let Ok(mut child_pid) = self.child_pid.lock() {
            if let Some(pid) = child_pid.take() {
                let _ = Command::new("kill")
                    .args(["-TERM", &pid.to_string()])
                    .status();
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn run_capture(
    snapshot: Arc<Mutex<SpectrumSnapshot>>,
    stop: Arc<AtomicBool>,
    child_pid: Arc<Mutex<Option<u32>>>,
    settings: Arc<Mutex<SpectrumSettings>>,
) {
    let monitor_source = match resolve_monitor_source() {
        Ok(source) => source,
        Err(error) => {
            set_error(&snapshot, error);
            return;
        }
    };

    let mut child = match Command::new("parec")
        .args([
            "--device",
            &monitor_source,
            "--format=float32le",
            "--rate=48000",
            "--channels=2",
            "--raw",
            "--latency-msec=20",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            set_error(&snapshot, format!("Failed to start parec: {error}"));
            return;
        }
    };

    if let Ok(mut pid_slot) = child_pid.lock() {
        *pid_slot = Some(child.id());
    }

    let Some(mut stdout) = child.stdout.take() else {
        set_error(&snapshot, "parec did not provide stdout".to_string());
        let _ = child.kill();
        let _ = child.wait();
        return;
    };

    if let Ok(mut state) = snapshot.lock() {
        state.source_name = Some(monitor_source.clone());
        state.message = format!("Capturing monitor source {monitor_source}");
    }

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    let mut raw_buffer = vec![0_u8; 16_384];
    let mut sample_window = Vec::with_capacity(FFT_SIZE);
    let mut smoothed_bins = vec![0_u64; BAR_COUNT];
    let mut peak_bins = vec![0_u64; BAR_COUNT];
    let mut pending_samples = 0;

    while !stop.load(Ordering::Relaxed) {
        let bytes_read = match stdout.read(&mut raw_buffer) {
            Ok(bytes_read) => bytes_read,
            Err(error) => {
                set_error(&snapshot, format!("Failed reading monitor audio: {error}"));
                break;
            }
        };

        if bytes_read == 0 {
            set_error(&snapshot, "Monitor audio stream ended".to_string());
            break;
        }

        let mono_samples = decode_float32le_stereo_to_mono(&raw_buffer[..bytes_read]);
        pending_samples += mono_samples.len();
        sample_window.extend(mono_samples);

        if sample_window.len() > FFT_SIZE {
            let overflow = sample_window.len() - FFT_SIZE;
            sample_window.drain(0..overflow);
        }

        if sample_window.len() == FFT_SIZE && pending_samples >= UPDATE_STEP {
            pending_samples = 0;
            let (sensitivity, decay) = read_settings(&settings);
            let bins = compute_spectrum_bins(&sample_window, &fft, BAR_COUNT, sensitivity);
            smooth_bins(&mut smoothed_bins, &bins);
            update_peak_hold(&mut peak_bins, &smoothed_bins, decay);
            update_snapshot(
                &snapshot,
                &monitor_source,
                &smoothed_bins,
                &peak_bins,
                sensitivity,
                decay,
            );
        }
    }

    let _ = child.kill();
    let _ = child.wait();

    if let Ok(mut pid_slot) = child_pid.lock() {
        *pid_slot = None;
    }
}

#[cfg(target_os = "linux")]
fn resolve_monitor_source() -> Result<String, String> {
    let pactl_info = run_command("pactl", &["info"])?;
    let sources = run_command("pactl", &["list", "short", "sources"])?;
    let source_names = parse_source_names(&sources);

    if let Some(default_sink) = pactl_info
        .lines()
        .find_map(|line| line.strip_prefix("Default Sink: "))
    {
        let monitor_source = format!("{default_sink}.monitor");
        if source_names.iter().any(|name| name == &monitor_source) {
            return Ok(monitor_source);
        }
    }

    source_names
        .into_iter()
        .find(|name| name.ends_with(".monitor"))
        .ok_or_else(|| "No monitor source found for the default sink".to_string())
}

#[cfg(target_os = "linux")]
fn run_command(command: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(command)
        .args(args)
        .output()
        .map_err(|error| format!("Failed to run {command}: {error}"))?;

    if !output.status.success() {
        return Err(format!("{command} exited with status {}", output.status));
    }

    String::from_utf8(output.stdout)
        .map_err(|error| format!("Invalid UTF-8 from {command}: {error}"))
}

#[cfg(target_os = "linux")]
fn parse_source_names(sources: &str) -> Vec<String> {
    sources
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .map(ToString::to_string)
        .collect()
}

#[cfg(target_os = "linux")]
fn decode_float32le_stereo_to_mono(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(CHANNELS * 4)
        .map(|frame| {
            let left = f32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]);
            let right = f32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
            (left + right) * 0.5
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn compute_spectrum_bins(
    samples: &[f32],
    fft: &Arc<dyn rustfft::Fft<f32>>,
    bin_count: usize,
    sensitivity: usize,
) -> Vec<u64> {
    let mut buffer: Vec<Complex32> = samples
        .iter()
        .enumerate()
        .map(|(index, sample)| {
            let window = hann_window(index, samples.len());
            Complex32::new(sample * window, 0.0)
        })
        .collect();

    fft.process(&mut buffer);

    let magnitudes: Vec<f32> = buffer[..buffer.len() / 2]
        .iter()
        .map(|value| value.norm())
        .collect();

    let mut bins = Vec::with_capacity(bin_count);
    let nyquist = SAMPLE_RATE as f32 / 2.0;

    for bucket in 0..bin_count {
        let start_hz = band_edge_frequency(bucket, bin_count, nyquist);
        let end_hz = band_edge_frequency(bucket + 1, bin_count, nyquist);
        let start = frequency_to_index(start_hz, magnitudes.len());
        let end = frequency_to_index(end_hz, magnitudes.len()).max(start + 1);
        let avg = if start < end {
            let end = end.min(magnitudes.len());
            let sum: f32 = magnitudes[start..end].iter().sum();
            sum / (end - start) as f32
        } else {
            0.0
        };

        bins.push(avg);
    }

    let peak = bins.iter().copied().fold(0.0_f32, f32::max).max(1e-6);
    bins.into_iter()
        .map(|value| {
            (((value / peak).sqrt() * sensitivity as f32).round()).clamp(0.0, 100.0) as u64
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn band_edge_frequency(bucket: usize, bin_count: usize, nyquist: f32) -> f32 {
    let ratio = bucket as f32 / bin_count as f32;
    let span = (nyquist / MIN_FREQUENCY).ln();
    MIN_FREQUENCY * (span * ratio).exp()
}

#[cfg(target_os = "linux")]
fn frequency_to_index(frequency: f32, len: usize) -> usize {
    let nyquist = SAMPLE_RATE as f32 / 2.0;
    ((frequency / nyquist) * len as f32)
        .floor()
        .clamp(0.0, len.saturating_sub(1) as f32) as usize
}

#[cfg(target_os = "linux")]
fn hann_window(index: usize, size: usize) -> f32 {
    if size <= 1 {
        return 1.0;
    }

    let phase = (2.0 * std::f32::consts::PI * index as f32) / (size - 1) as f32;
    0.5 * (1.0 - phase.cos())
}

#[cfg(target_os = "linux")]
fn smooth_bins(current: &mut [u64], next: &[u64]) {
    for (current, next) in current.iter_mut().zip(next.iter().copied()) {
        *current = ((*current as f32 * 0.65) + (next as f32 * 0.35)).round() as u64;
    }
}

#[cfg(target_os = "linux")]
fn update_peak_hold(peaks: &mut [u64], bins: &[u64], decay: usize) {
    for (peak, bin) in peaks.iter_mut().zip(bins.iter().copied()) {
        *peak = if bin >= *peak {
            bin
        } else {
            peak.saturating_sub(decay as u64)
        };
    }
}

pub fn spectrum_label_positions(bin_count: usize) -> Vec<(usize, &'static str)> {
    if bin_count == 0 {
        return Vec::new();
    }

    let last = bin_count.saturating_sub(1);
    vec![
        (0, "30"),
        (
            (((bin_count as f32) * 0.28).round() as usize).min(last),
            "125",
        ),
        (
            (((bin_count as f32) * 0.48).round() as usize).min(last),
            "500",
        ),
        (
            (((bin_count as f32) * 0.68).round() as usize).min(last),
            "2k",
        ),
        (
            (((bin_count as f32) * 0.84).round() as usize).min(last),
            "8k",
        ),
        (last, "24k"),
    ]
}

#[cfg(target_os = "linux")]
fn update_snapshot(
    snapshot: &Arc<Mutex<SpectrumSnapshot>>,
    source_name: &str,
    bins: &[u64],
    peaks: &[u64],
    sensitivity: usize,
    decay: usize,
) {
    if let Ok(mut state) = snapshot.lock() {
        state.bins = bins.to_vec();
        state.peaks = peaks.to_vec();
        state.source_name = Some(source_name.to_string());
        state.message = format!("{} Hz monitor capture", SAMPLE_RATE);
        state.active = true;
        state.sensitivity = sensitivity;
        state.decay = decay;
    }
}

#[cfg(target_os = "linux")]
fn set_error(snapshot: &Arc<Mutex<SpectrumSnapshot>>, message: String) {
    if let Ok(mut state) = snapshot.lock() {
        state.message = message;
        state.active = false;
        state.bins.fill(0);
        state.peaks.fill(0);
    }
}

fn read_settings(settings: &Arc<Mutex<SpectrumSettings>>) -> (usize, usize) {
    settings
        .lock()
        .map(|settings| (settings.sensitivity, settings.decay))
        .unwrap_or((DEFAULT_SENSITIVITY, DEFAULT_DECAY))
}

fn clamp_setting(current: usize, delta: isize, min: usize, max: usize) -> usize {
    current.saturating_add_signed(delta).clamp(min, max)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn test_parse_source_names() {
        let sources = "61\talsa_output.test.monitor\tPipeWire\ts24le 2ch 48000Hz\tRUNNING\n64\talsa_input.test\tPipeWire\ts16le 2ch 32000Hz\tSUSPENDED\n";
        assert_eq!(
            parse_source_names(sources),
            vec![
                "alsa_output.test.monitor".to_string(),
                "alsa_input.test".to_string()
            ]
        );
    }

    #[test]
    fn test_decode_float32le_stereo_to_mono() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1.0_f32.to_le_bytes());
        bytes.extend_from_slice(&(-1.0_f32).to_le_bytes());
        bytes.extend_from_slice(&0.25_f32.to_le_bytes());
        bytes.extend_from_slice(&0.75_f32.to_le_bytes());

        assert_eq!(decode_float32le_stereo_to_mono(&bytes), vec![0.0, 0.5]);
    }

    #[test]
    fn test_compute_spectrum_bins_output_size() {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_SIZE);
        let samples: Vec<f32> = (0..FFT_SIZE)
            .map(|index| (index as f32 * 2.0 * std::f32::consts::PI / 32.0).sin())
            .collect();

        let bins = compute_spectrum_bins(&samples, &fft, 16, DEFAULT_SENSITIVITY);
        assert_eq!(bins.len(), 16);
        assert!(bins.iter().any(|value| *value > 0));
    }

    #[test]
    fn test_spectrum_label_positions() {
        let labels = spectrum_label_positions(32);
        assert_eq!(labels.first(), Some(&(0, "30")));
        assert_eq!(labels.last(), Some(&(31, "24k")));
    }

    #[test]
    fn test_clamp_setting() {
        assert_eq!(clamp_setting(100, 10, 25, 250), 110);
        assert_eq!(clamp_setting(25, -10, 25, 250), 25);
        assert_eq!(clamp_setting(250, 10, 25, 250), 250);
    }
}
