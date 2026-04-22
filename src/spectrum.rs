#[cfg(target_os = "linux")]
use std::io::Read;
#[cfg(target_os = "linux")]
use std::process::{Command, Stdio};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::sync::{Arc, Mutex};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::thread;

#[cfg(target_os = "macos")]
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
#[cfg(target_os = "macos")]
use cpal::{FromSample, Sample, SampleFormat, SizedSample, Stream};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use rustfft::{FftPlanner, num_complex::Complex32};

#[cfg(target_os = "linux")]
const SAMPLE_RATE: u32 = 48_000;
#[cfg(target_os = "linux")]
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Debug)]
struct SpectrumSettings {
    sensitivity: usize,
    decay: usize,
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
struct CaptureState {
    sample_window: Vec<f32>,
    smoothed_bins: Vec<u64>,
    peak_bins: Vec<u64>,
    pending_samples: usize,
}

pub struct SpectrumMonitor {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    snapshot: Arc<Mutex<SpectrumSnapshot>>,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    stop: Arc<AtomicBool>,
    #[cfg(target_os = "linux")]
    child_pid: Arc<Mutex<Option<u32>>>,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    settings: Arc<Mutex<SpectrumSettings>>,
}

impl SpectrumMonitor {
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
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

    #[cfg(target_os = "macos")]
    pub fn new() -> Self {
        let snapshot = Arc::new(Mutex::new(SpectrumSnapshot::starting()));
        let stop = Arc::new(AtomicBool::new(false));
        let settings = Arc::new(Mutex::new(SpectrumSettings {
            sensitivity: DEFAULT_SENSITIVITY,
            decay: DEFAULT_DECAY,
        }));

        let worker_snapshot = Arc::clone(&snapshot);
        let worker_stop = Arc::clone(&stop);
        let worker_settings = Arc::clone(&settings);

        thread::spawn(move || {
            run_capture_macos(worker_snapshot, worker_stop, worker_settings);
        });

        Self {
            snapshot,
            stop,
            settings,
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
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

    #[cfg(any(target_os = "linux", target_os = "macos"))]
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

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    pub fn adjust_sensitivity(&self, _delta: isize) {}

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub fn adjust_sensitivity(&self, delta: isize) {
        self.update_settings(|settings| {
            settings.sensitivity = clamp_setting(settings.sensitivity, delta, 25, MAX_SENSITIVITY);
        });
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    pub fn adjust_decay(&self, _delta: isize) {}

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    pub fn adjust_decay(&self, delta: isize) {
        self.update_settings(|settings| {
            settings.decay = clamp_setting(settings.decay, delta, 1, MAX_DECAY);
        });
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl Drop for SpectrumMonitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);

        #[cfg(target_os = "linux")]
        if let Ok(mut child_pid) = self.child_pid.lock() {
            if let Some(pid) = child_pid.take() {
                let _ = Command::new("kill")
                    .args(["-TERM", &pid.to_string()])
                    .status();
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn run_capture_macos(
    snapshot: Arc<Mutex<SpectrumSnapshot>>,
    stop: Arc<AtomicBool>,
    settings: Arc<Mutex<SpectrumSettings>>,
) {
    use cpal::traits::{DeviceTrait, HostTrait};

    let host = cpal::default_host();

    // Try to find BlackHole or similar virtual audio device
    let device = host.devices().ok().and_then(|mut devices| {
        devices.find(|d| {
            d.name()
                .map(|name| {
                    let name_lower = name.to_lowercase();
                    name_lower.contains("blackhole")
                        || name_lower.contains("loopback")
                        || name_lower.contains("virtual")
                        || name_lower.contains("soundflower")
                })
                .unwrap_or(false)
        })
    });

    let Some(device) = device else {
        set_error(
            &snapshot,
            "Spectrum: Install BlackHole (brew install blackhole-2ch) and set it as output"
                .to_string(),
        );
        return;
    };

    let source_name = device
        .name()
        .unwrap_or_else(|_| "Virtual Audio Device".to_string());

    // Try to use the device as an input (for virtual devices this captures the loopback)
    let supported_config = match device.default_input_config() {
        Ok(config) => config,
        Err(error) => {
            set_error(
                &snapshot,
                format!("Failed to read virtual device config: {error}"),
            );
            return;
        }
    };

    let sample_format = supported_config.sample_format();
    let config: cpal::StreamConfig = supported_config.clone().into();
    let sample_rate = config.sample_rate;
    let channels = usize::from(config.channels.max(1));

    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    let capture_state = Arc::new(Mutex::new(CaptureState {
        sample_window: Vec::with_capacity(FFT_SIZE),
        smoothed_bins: vec![0; BAR_COUNT],
        peak_bins: vec![0; BAR_COUNT],
        pending_samples: 0,
    }));

    let stream_result = match sample_format {
        SampleFormat::I8 => build_macos_stream::<i8>(
            &device,
            &config,
            channels,
            sample_rate,
            &source_name,
            &snapshot,
            &settings,
            &capture_state,
            &fft,
        ),
        SampleFormat::I16 => build_macos_stream::<i16>(
            &device,
            &config,
            channels,
            sample_rate,
            &source_name,
            &snapshot,
            &settings,
            &capture_state,
            &fft,
        ),
        SampleFormat::I24 => build_macos_stream::<cpal::I24>(
            &device,
            &config,
            channels,
            sample_rate,
            &source_name,
            &snapshot,
            &settings,
            &capture_state,
            &fft,
        ),
        SampleFormat::I32 => build_macos_stream::<i32>(
            &device,
            &config,
            channels,
            sample_rate,
            &source_name,
            &snapshot,
            &settings,
            &capture_state,
            &fft,
        ),
        SampleFormat::I64 => build_macos_stream::<i64>(
            &device,
            &config,
            channels,
            sample_rate,
            &source_name,
            &snapshot,
            &settings,
            &capture_state,
            &fft,
        ),
        SampleFormat::U8 => build_macos_stream::<u8>(
            &device,
            &config,
            channels,
            sample_rate,
            &source_name,
            &snapshot,
            &settings,
            &capture_state,
            &fft,
        ),
        SampleFormat::U16 => build_macos_stream::<u16>(
            &device,
            &config,
            channels,
            sample_rate,
            &source_name,
            &snapshot,
            &settings,
            &capture_state,
            &fft,
        ),
        SampleFormat::U24 => build_macos_stream::<cpal::U24>(
            &device,
            &config,
            channels,
            sample_rate,
            &source_name,
            &snapshot,
            &settings,
            &capture_state,
            &fft,
        ),
        SampleFormat::U32 => build_macos_stream::<u32>(
            &device,
            &config,
            channels,
            sample_rate,
            &source_name,
            &snapshot,
            &settings,
            &capture_state,
            &fft,
        ),
        SampleFormat::U64 => build_macos_stream::<u64>(
            &device,
            &config,
            channels,
            sample_rate,
            &source_name,
            &snapshot,
            &settings,
            &capture_state,
            &fft,
        ),
        SampleFormat::F32 => build_macos_stream::<f32>(
            &device,
            &config,
            channels,
            sample_rate,
            &source_name,
            &snapshot,
            &settings,
            &capture_state,
            &fft,
        ),
        SampleFormat::F64 => build_macos_stream::<f64>(
            &device,
            &config,
            channels,
            sample_rate,
            &source_name,
            &snapshot,
            &settings,
            &capture_state,
            &fft,
        ),
        SampleFormat::DsdU8 | SampleFormat::DsdU16 | SampleFormat::DsdU32 => Err(format!(
            "Unsupported sample format for spectrum capture: {sample_format:?}"
        )),
        _ => Err(format!(
            "Unsupported sample format for spectrum capture: {sample_format:?}"
        )),
    };

    let stream = match stream_result {
        Ok(stream) => stream,
        Err(error) => {
            set_error(&snapshot, error);
            return;
        }
    };

    if let Ok(mut state) = snapshot.lock() {
        state.source_name = Some(source_name.clone());
        state.message = format!("Capturing {} at {} Hz", source_name, sample_rate);
        state.active = true;
    }

    if let Err(error) = stream.play() {
        set_error(
            &snapshot,
            format!("Failed to start virtual device stream: {error}"),
        );
        return;
    }

    while !stop.load(Ordering::Relaxed) {
        thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[cfg(target_os = "macos")]
struct MacOSIOProcContext {
    snapshot: Arc<Mutex<SpectrumSnapshot>>,
    settings: Arc<Mutex<SpectrumSettings>>,
    capture_state: Arc<Mutex<CaptureState>>,
    fft: Arc<dyn rustfft::Fft<f32>>,
    source_name: String,
    channels: usize,
    sample_rate: u32,
}

#[cfg(target_os = "macos")]
unsafe extern "C-unwind" fn macos_ioproc_callback(
    _in_device: u32,
    _in_now: std::ptr::NonNull<objc2_core_audio_types::AudioTimeStamp>,
    _in_input_data: std::ptr::NonNull<objc2_core_audio_types::AudioBufferList>,
    _in_input_time: std::ptr::NonNull<objc2_core_audio_types::AudioTimeStamp>,
    out_output_data: std::ptr::NonNull<objc2_core_audio_types::AudioBufferList>,
    _in_output_time: std::ptr::NonNull<objc2_core_audio_types::AudioTimeStamp>,
    in_client_data: *mut libc::c_void,
) -> i32 {
    if in_client_data.is_null() {
        return 0;
    }

    let context = unsafe { &*(in_client_data as *const MacOSIOProcContext) };

    // Process the output data (which contains the audio being played)
    let buffer_list = unsafe { &*out_output_data.as_ptr() };

    if buffer_list.mNumberBuffers > 0 {
        let buffer = unsafe { &*buffer_list.mBuffers.as_ptr() };
        let sample_count = buffer.mDataByteSize as usize / std::mem::size_of::<f32>();
        let samples =
            unsafe { std::slice::from_raw_parts(buffer.mData as *const f32, sample_count) };

        process_macos_samples(
            samples,
            context.channels,
            context.sample_rate,
            &context.source_name,
            &context.snapshot,
            &context.settings,
            &context.capture_state,
            &context.fft,
        );
    }

    0
}

#[cfg(target_os = "macos")]
fn process_macos_samples(
    samples: &[f32],
    channels: usize,
    sample_rate: u32,
    source_name: &str,
    snapshot: &Arc<Mutex<SpectrumSnapshot>>,
    settings: &Arc<Mutex<SpectrumSettings>>,
    capture_state: &Arc<Mutex<CaptureState>>,
    fft: &Arc<dyn rustfft::Fft<f32>>,
) {
    let Ok(mut capture) = capture_state.lock() else {
        return;
    };

    let frame_channels = channels.max(1);
    for frame in samples.chunks_exact(frame_channels) {
        let sum: f32 = frame.iter().sum();
        capture.sample_window.push(sum / frame_channels as f32);
        capture.pending_samples += 1;
    }

    if capture.sample_window.len() > FFT_SIZE {
        let overflow = capture.sample_window.len() - FFT_SIZE;
        capture.sample_window.drain(0..overflow);
    }

    if capture.sample_window.len() == FFT_SIZE && capture.pending_samples >= UPDATE_STEP {
        capture.pending_samples = 0;
        let (sensitivity, decay) = read_settings(settings);
        let bins = compute_spectrum_bins(
            &capture.sample_window,
            fft,
            BAR_COUNT,
            sensitivity,
            sample_rate,
        );
        smooth_bins(&mut capture.smoothed_bins, &bins);
        let smoothed_bins = capture.smoothed_bins.clone();
        update_peak_hold(&mut capture.peak_bins, &smoothed_bins, decay);
        update_snapshot(
            snapshot,
            source_name,
            &smoothed_bins,
            &capture.peak_bins,
            sensitivity,
            decay,
            sample_rate,
        );
    }
}

#[cfg(target_os = "macos")]
#[allow(clippy::too_many_arguments)]
fn build_macos_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    sample_rate: u32,
    source_name: &str,
    snapshot: &Arc<Mutex<SpectrumSnapshot>>,
    settings: &Arc<Mutex<SpectrumSettings>>,
    capture_state: &Arc<Mutex<CaptureState>>,
    fft: &Arc<dyn rustfft::Fft<f32>>,
) -> std::result::Result<Stream, String>
where
    T: SizedSample + Copy + Send + 'static,
    f32: FromSample<T>,
{
    let source_name = source_name.to_string();
    let snapshot_for_data = Arc::clone(snapshot);
    let settings = Arc::clone(settings);
    let capture_state = Arc::clone(capture_state);
    let fft = Arc::clone(fft);
    let snapshot_for_error = Arc::clone(snapshot);

    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                process_macos_input(
                    data,
                    channels,
                    sample_rate,
                    &source_name,
                    &snapshot_for_data,
                    &settings,
                    &capture_state,
                    &fft,
                );
            },
            move |error| {
                set_error(
                    &snapshot_for_error,
                    format!("macOS spectrum stream error: {error}"),
                );
            },
            None,
        )
        .map_err(|error| format!("Failed to build macOS spectrum stream: {error}"))
}

#[cfg(target_os = "macos")]
fn process_macos_input<T>(
    data: &[T],
    channels: usize,
    sample_rate: u32,
    source_name: &str,
    snapshot: &Arc<Mutex<SpectrumSnapshot>>,
    settings: &Arc<Mutex<SpectrumSettings>>,
    capture_state: &Arc<Mutex<CaptureState>>,
    fft: &Arc<dyn rustfft::Fft<f32>>,
) where
    T: SizedSample + Copy,
    f32: FromSample<T>,
{
    let Ok(mut capture) = capture_state.lock() else {
        return;
    };

    let frame_channels = channels.max(1);
    for frame in data.chunks_exact(frame_channels) {
        let sum: f32 = frame.iter().copied().map(f32::from_sample).sum();
        capture.sample_window.push(sum / frame_channels as f32);
        capture.pending_samples += 1;
    }

    if capture.sample_window.len() > FFT_SIZE {
        let overflow = capture.sample_window.len() - FFT_SIZE;
        capture.sample_window.drain(0..overflow);
    }

    if capture.sample_window.len() == FFT_SIZE && capture.pending_samples >= UPDATE_STEP {
        capture.pending_samples = 0;
        let (sensitivity, decay) = read_settings(settings);
        let bins = compute_spectrum_bins(
            &capture.sample_window,
            fft,
            BAR_COUNT,
            sensitivity,
            sample_rate,
        );
        smooth_bins(&mut capture.smoothed_bins, &bins);
        let smoothed_bins = capture.smoothed_bins.clone();
        update_peak_hold(&mut capture.peak_bins, &smoothed_bins, decay);
        update_snapshot(
            snapshot,
            source_name,
            &smoothed_bins,
            &capture.peak_bins,
            sensitivity,
            decay,
            sample_rate,
        );
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
            let bins =
                compute_spectrum_bins(&sample_window, &fft, BAR_COUNT, sensitivity, SAMPLE_RATE);
            smooth_bins(&mut smoothed_bins, &bins);
            update_peak_hold(&mut peak_bins, &smoothed_bins, decay);
            update_snapshot(
                &snapshot,
                &monitor_source,
                &smoothed_bins,
                &peak_bins,
                sensitivity,
                decay,
                SAMPLE_RATE,
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn compute_spectrum_bins(
    samples: &[f32],
    fft: &Arc<dyn rustfft::Fft<f32>>,
    bin_count: usize,
    sensitivity: usize,
    sample_rate: u32,
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
    let nyquist = sample_rate as f32 / 2.0;

    for bucket in 0..bin_count {
        let start_hz = band_edge_frequency(bucket, bin_count, nyquist);
        let end_hz = band_edge_frequency(bucket + 1, bin_count, nyquist);
        let start = frequency_to_index(start_hz, magnitudes.len(), sample_rate);
        let end = frequency_to_index(end_hz, magnitudes.len(), sample_rate).max(start + 1);
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn band_edge_frequency(bucket: usize, bin_count: usize, nyquist: f32) -> f32 {
    let ratio = bucket as f32 / bin_count as f32;
    let span = (nyquist / MIN_FREQUENCY).ln();
    MIN_FREQUENCY * (span * ratio).exp()
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn frequency_to_index(frequency: f32, len: usize, sample_rate: u32) -> usize {
    let nyquist = sample_rate as f32 / 2.0;
    ((frequency / nyquist) * len as f32)
        .floor()
        .clamp(0.0, len.saturating_sub(1) as f32) as usize
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn hann_window(index: usize, size: usize) -> f32 {
    if size <= 1 {
        return 1.0;
    }

    let phase = (2.0 * std::f32::consts::PI * index as f32) / (size - 1) as f32;
    0.5 * (1.0 - phase.cos())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn smooth_bins(current: &mut [u64], next: &[u64]) {
    for (current, next) in current.iter_mut().zip(next.iter().copied()) {
        *current = ((*current as f32 * 0.65) + (next as f32 * 0.35)).round() as u64;
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
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

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn update_snapshot(
    snapshot: &Arc<Mutex<SpectrumSnapshot>>,
    source_name: &str,
    bins: &[u64],
    peaks: &[u64],
    sensitivity: usize,
    decay: usize,
    sample_rate: u32,
) {
    if let Ok(mut state) = snapshot.lock() {
        state.bins = bins.to_vec();
        state.peaks = peaks.to_vec();
        state.source_name = Some(source_name.to_string());
        state.message = format!("{} Hz capture", sample_rate);
        state.active = true;
        state.sensitivity = sensitivity;
        state.decay = decay;
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn set_error(snapshot: &Arc<Mutex<SpectrumSnapshot>>, message: String) {
    if let Ok(mut state) = snapshot.lock() {
        state.message = message;
        state.active = false;
        state.bins.fill(0);
        state.peaks.fill(0);
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
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

        let bins = compute_spectrum_bins(&samples, &fft, 16, DEFAULT_SENSITIVITY, SAMPLE_RATE);
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
