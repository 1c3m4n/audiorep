#[cfg(target_os = "macos")]
use objc2_core_audio::{
    AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectID,
    AudioObjectPropertyAddress, kAudioHardwarePropertyProcessObjectList,
    kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject,
    kAudioProcessPropertyIsRunningOutput, kAudioProcessPropertyPID,
};
#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::path::Path;
#[cfg(target_os = "macos")]
use std::process::Command;
#[cfg(target_os = "linux")]
use std::process::Command;

use crate::audio_info::{AudioDevice, AudioInfo, PlaybackSource, StreamState};
use crate::error::{AudioError, Result};

#[cfg(target_os = "linux")]
const PROC_ASOUND: &str = "/proc/asound";

pub struct ProcParser;

impl ProcParser {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(not(target_os = "linux"))]
impl ProcParser {
    pub fn parse_audio_info(&self) -> Result<AudioInfo> {
        #[cfg(target_os = "macos")]
        {
            let devices = Self::read_macos_devices()?;
            if devices.is_empty() {
                return Err(AudioError::NoDevices);
            }

            Ok(AudioInfo { devices })
        }

        #[cfg(not(target_os = "macos"))]
        {
            Ok(AudioInfo {
                devices: Vec::new(),
            })
        }
    }
}

#[cfg(target_os = "macos")]
impl ProcParser {
    fn read_macos_devices() -> Result<Vec<AudioDevice>> {
        let output = Command::new("system_profiler")
            .args(["SPAudioDataType"])
            .output()?;
        if !output.status.success() {
            return Err(AudioError::NoDevices);
        }

        let text = String::from_utf8_lossy(&output.stdout);
        let output_volume = Self::read_macos_output_volume();
        let mut devices = Self::parse_macos_devices(&text, output_volume);
        let sources = Self::read_macos_playback_sources();
        for device in devices.iter_mut() {
            if device.is_playback {
                device.sources = sources.clone();
            }
        }
        Ok(devices)
    }

    fn read_macos_playback_sources() -> Vec<PlaybackSource> {
        Self::read_macos_playback_sources_coreaudio()
    }

    fn read_macos_playback_sources_coreaudio() -> Vec<PlaybackSource> {
        let process_ids = Self::get_coreaudio_process_ids();
        if process_ids.is_empty() {
            return Vec::new();
        }

        let mut sources = Vec::new();
        for pid in process_ids {
            if let Some(name) = Self::get_process_name_from_pid(pid)
                && !sources.iter().any(|s: &PlaybackSource| s.name == name)
            {
                sources.push(PlaybackSource {
                    name,
                    sample_rate: None,
                });
            }
        }

        sources
    }

    fn get_coreaudio_process_ids() -> Vec<u32> {
        let property = AudioObjectPropertyAddress {
            mSelector: kAudioHardwarePropertyProcessObjectList,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        };

        let mut size: u32 = 0;
        let size_status = unsafe {
            AudioObjectGetPropertyDataSize(
                kAudioObjectSystemObject as u32,
                std::ptr::NonNull::from(&property),
                0,
                std::ptr::null(),
                std::ptr::NonNull::from(&mut size),
            )
        };

        if size_status != 0 || size == 0 {
            return Vec::new();
        }

        let count = size as usize / std::mem::size_of::<AudioObjectID>();
        let mut ids = vec![0u32; count];
        let mut data_size = size;
        let data_status = unsafe {
            AudioObjectGetPropertyData(
                kAudioObjectSystemObject as u32,
                std::ptr::NonNull::from(&property),
                0,
                std::ptr::null(),
                std::ptr::NonNull::from(&mut data_size),
                std::ptr::NonNull::new(ids.as_mut_ptr()).unwrap().cast(),
            )
        };

        if data_status != 0 {
            return Vec::new();
        }

        ids.into_iter()
            .filter(|id| Self::process_is_running_output(*id))
            .filter_map(|id| Self::process_pid(id))
            .collect()
    }

    fn process_is_running_output(id: u32) -> bool {
        let property = AudioObjectPropertyAddress {
            mSelector: kAudioProcessPropertyIsRunningOutput,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        };
        let mut value: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        let status = unsafe {
            AudioObjectGetPropertyData(
                id,
                std::ptr::NonNull::from(&property),
                0,
                std::ptr::null(),
                std::ptr::NonNull::from(&mut size),
                std::ptr::NonNull::from(&mut value).cast(),
            )
        };
        status == 0 && value != 0
    }

    fn process_pid(id: u32) -> Option<u32> {
        let property = AudioObjectPropertyAddress {
            mSelector: kAudioProcessPropertyPID,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        };
        let mut value: i32 = 0;
        let mut size = std::mem::size_of::<i32>() as u32;
        let status = unsafe {
            AudioObjectGetPropertyData(
                id,
                std::ptr::NonNull::from(&property),
                0,
                std::ptr::null(),
                std::ptr::NonNull::from(&mut size),
                std::ptr::NonNull::from(&mut value).cast(),
            )
        };
        if status == 0 && value > 0 {
            Some(value as u32)
        } else {
            None
        }
    }

    fn get_process_name_from_pid(pid: u32) -> Option<String> {
        // Don't include audiorep itself as a source
        let self_pid = std::process::id();
        if pid == self_pid {
            return None;
        }

        let mut buf = vec![0i8; 4096];
        let result = unsafe {
            libc::proc_pidpath(
                pid as i32,
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len() as u32,
            )
        };
        if result > 0 {
            let path = unsafe {
                std::ffi::CStr::from_ptr(buf.as_ptr())
                    .to_string_lossy()
                    .to_string()
            };
            if !path.is_empty() {
                let name = std::path::Path::new(&path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_string())?;
                // Filter out system processes
                if Self::is_system_process(&name) {
                    return None;
                }
                return Some(name);
            }
        }

        // Fallback to ps
        let output = Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "comm="])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if name.is_empty() {
            return None;
        }
        let name = std::path::Path::new(&name)
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())?;
        if Self::is_system_process(&name) {
            return None;
        }
        Some(name)
    }

    fn is_system_process(name: &str) -> bool {
        let system_processes = [
            "audiorep",
            "audioaccessoryd",
            "com.apple.audio.SandboxHelper",
            "heard",
            "kernel_task",
            "launchd",
            "logd",
            "mds",
            "mds_stores",
            "notifyd",
            "opencode",
            "syslogd",
            "trustd",
            "zsh",
            "bash",
            "sh",
            "fish",
            "tmux",
            "screen",
        ];
        system_processes.contains(&name)
            || name.starts_with("com.apple.")
            || name.starts_with("audio")
    }

    fn read_macos_output_volume() -> Option<u8> {
        // Skip volume reading on macOS to avoid osascript terminal flicker.
        // Multi-output devices don't have a single volume control anyway.
        None
    }

    fn parse_macos_devices(text: &str, output_volume: Option<u8>) -> Vec<AudioDevice> {
        let mut devices = Vec::new();
        let mut current_name: Option<String> = None;
        let mut is_output = false;
        let mut is_default_output = false;
        let mut sample_rate = None;
        let mut channels = None;

        for line in text.lines() {
            let trimmed = line.trim();

            if trimmed.is_empty() {
                continue;
            }

            let indent = line.chars().take_while(|ch| ch.is_whitespace()).count();
            if indent == 8 && trimmed.ends_with(':') && !trimmed.contains("(") {
                if is_output {
                    Self::push_macos_device(
                        &mut devices,
                        current_name.take(),
                        is_default_output,
                        output_volume,
                        sample_rate.take(),
                        channels.take(),
                    );
                }

                current_name = Some(trimmed.trim_end_matches(':').to_string());
                is_output = false;
                is_default_output = false;
                sample_rate = None;
                channels = None;
                continue;
            }

            if indent < 10 {
                continue;
            }

            if let Some(value) = trimmed.strip_prefix("Output Channels:") {
                channels = value.trim().parse::<u32>().ok();
                is_output = true;
            } else if let Some(value) = trimmed.strip_prefix("Current SampleRate:") {
                sample_rate = value.trim().parse::<u32>().ok();
            } else if trimmed == "Default Output Device: Yes" {
                is_output = true;
                is_default_output = true;
            } else if trimmed == "Output Source: Default" {
                is_output = true;
            }
        }

        if is_output {
            Self::push_macos_device(
                &mut devices,
                current_name,
                is_default_output,
                output_volume,
                sample_rate,
                channels,
            );
        }

        devices
    }

    fn push_macos_device(
        devices: &mut Vec<AudioDevice>,
        name: Option<String>,
        is_default_output: bool,
        output_volume: Option<u8>,
        sample_rate: Option<u32>,
        channels: Option<u32>,
    ) {
        let Some(card_name) = name else {
            return;
        };

        let next_id = devices.len() as u32;
        devices.push(AudioDevice {
            card_id: next_id,
            card_name,
            pcm_id: 0,
            sub_id: 0,
            is_playback: true,
            state: if is_default_output {
                StreamState::Running
            } else {
                StreamState::Stopped
            },
            sample_rate,
            channels,
            sources: Vec::new(),
            volume: if is_default_output {
                output_volume.into_iter().collect()
            } else {
                Vec::new()
            },
        });
    }
}

#[cfg(target_os = "linux")]
impl ProcParser {
    pub fn parse_audio_info(&self) -> Result<AudioInfo> {
        let path = Path::new(PROC_ASOUND);
        if !path.exists() {
            return Err(AudioError::NoDevices);
        }

        let mut devices = Vec::new();

        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            if name_str.starts_with("card") {
                // Skip files like "cards" (not directories)
                let entry_path = entry.path();
                if !entry_path.is_dir() {
                    continue;
                }
                if let Some(card_id) = Self::extract_card_id(&name_str) {
                    let card_name = Self::read_card_name(card_id)?;
                    let pcm_devices = Self::read_pcm_devices(card_id, &card_name)?;
                    devices.extend(pcm_devices);
                }
            }
        }

        if devices.is_empty() {
            return Err(AudioError::NoDevices);
        }

        Self::attach_playback_sources(&mut devices);

        Ok(AudioInfo { devices })
    }

    fn extract_card_id(name: &str) -> Option<u32> {
        name.strip_prefix("card")?.parse().ok()
    }

    fn read_card_name(card_id: u32) -> Result<String> {
        let path = format!("{}/card{}/id", PROC_ASOUND, card_id);
        match fs::read_to_string(&path) {
            Ok(id) => Ok(id.trim().to_string()),
            Err(e) if e.raw_os_error() == Some(6) => {
                // ENXIO - device not accessible
                Ok(format!("card{}", card_id))
            }
            Err(e) => Err(e.into()),
        }
    }

    fn read_pcm_devices(card_id: u32, card_name: &str) -> Result<Vec<AudioDevice>> {
        let mut devices = Vec::new();
        let card_path = format!("{}/card{}", PROC_ASOUND, card_id);
        let card_dir = Path::new(&card_path);

        if !card_dir.exists() {
            return Ok(devices);
        }

        for entry in fs::read_dir(card_dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            if entry.path().is_dir()
                && let Some(pcm_id) = Self::extract_pcm_id(&name_str)
            {
                let sub_devices = Self::read_sub_devices(card_id, pcm_id, &name_str, card_name)?;
                devices.extend(sub_devices);
            }
        }

        Ok(devices)
    }

    fn extract_pcm_id(name: &str) -> Option<u32> {
        // PCM names are like "pcm0p" or "pcm3p" (playback) / "pcm0c" (capture)
        // Extract the number part
        name.strip_prefix("pcm")?
            .trim_end_matches(|c: char| c.is_alphabetic())
            .parse()
            .ok()
    }

    fn read_sub_devices(
        card_id: u32,
        pcm_id: u32,
        pcm_name: &str,
        card_name: &str,
    ) -> Result<Vec<AudioDevice>> {
        let mut devices = Vec::new();
        let pcm_path = format!("{}/card{}/{}", PROC_ASOUND, card_id, pcm_name);
        let pcm_dir = Path::new(&pcm_path);

        if !pcm_dir.exists() {
            return Ok(devices);
        }

        for entry in fs::read_dir(pcm_dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            if entry.path().is_dir()
                && let Some(sub_id) = Self::extract_sub_id(&name_str)
            {
                let status = Self::read_status(card_id, pcm_name, sub_id)?;
                let hw_params = Self::read_hw_params(card_id, pcm_name, sub_id)?;

                devices.push(AudioDevice {
                    card_id,
                    card_name: card_name.to_string(),
                    pcm_id,
                    sub_id,
                    is_playback: Self::is_playback_pcm(pcm_name),
                    state: status.state,
                    sample_rate: hw_params.sample_rate,
                    channels: hw_params.channels,
                    sources: Vec::new(),
                    volume: Vec::new(),
                });
            }
        }

        Ok(devices)
    }

    fn extract_sub_id(name: &str) -> Option<u32> {
        name.strip_prefix("sub")?.parse().ok()
    }

    fn is_playback_pcm(name: &str) -> bool {
        name.ends_with('p')
    }

    fn read_status(card_id: u32, pcm_name: &str, sub_id: u32) -> Result<StatusInfo> {
        let path = format!(
            "{}/card{}/{}/sub{}/status",
            PROC_ASOUND, card_id, pcm_name, sub_id
        );
        match fs::read_to_string(&path) {
            Ok(content) => Self::parse_status(&content),
            Err(e) if e.raw_os_error() == Some(6) => {
                // ENXIO - device not configured, stream is closed
                Ok(StatusInfo {
                    state: StreamState::Stopped,
                })
            }
            Err(e) => Err(e.into()),
        }
    }

    fn parse_status(content: &str) -> Result<StatusInfo> {
        let trimmed = content.trim();

        // Handle "closed" status
        if trimmed == "closed" {
            return Ok(StatusInfo {
                state: StreamState::Stopped,
            });
        }

        let mut state = StreamState::Stopped;

        for line in content.lines() {
            if line.starts_with("state:") {
                let state_str = line.split(':').nth(1).unwrap_or("").trim();
                state = StreamState::from_str(state_str);
            }
        }

        Ok(StatusInfo { state })
    }

    fn read_hw_params(card_id: u32, pcm_name: &str, sub_id: u32) -> Result<HwParamsInfo> {
        let path = format!(
            "{}/card{}/{}/sub{}/hw_params",
            PROC_ASOUND, card_id, pcm_name, sub_id
        );
        match fs::read_to_string(&path) {
            Ok(content) => Self::parse_hw_params(&content),
            Err(e) if e.raw_os_error() == Some(6) => {
                // ENXIO - device not configured, stream is closed
                Ok(HwParamsInfo {
                    sample_rate: None,
                    channels: None,
                })
            }
            Err(e) => Err(e.into()),
        }
    }

    fn parse_hw_params(content: &str) -> Result<HwParamsInfo> {
        let trimmed = content.trim();

        // Handle "closed" hw_params
        if trimmed == "closed" {
            return Ok(HwParamsInfo {
                sample_rate: None,
                channels: None,
            });
        }

        let mut sample_rate = None;
        let mut channels = None;

        for line in content.lines() {
            if line.starts_with("rate:") {
                let rate_str = line.split(':').nth(1).unwrap_or("").trim();
                sample_rate = rate_str.split(' ').next().and_then(|s| s.parse().ok());
            } else if line.starts_with("channels:") {
                let ch_str = line.split(':').nth(1).unwrap_or("").trim();
                channels = ch_str.parse().ok();
            }
        }

        Ok(HwParamsInfo {
            sample_rate,
            channels,
        })
    }

    fn attach_playback_sources(devices: &mut [AudioDevice]) {
        let sink_cards = match Self::read_sink_cards() {
            Ok(sink_cards) => sink_cards,
            Err(_) => return,
        };
        let sink_inputs = match Self::read_sink_inputs() {
            Ok(sink_inputs) => sink_inputs,
            Err(_) => return,
        };

        for device in devices.iter_mut().filter(|device| device.is_playback) {
            device.sources = sink_inputs
                .iter()
                .filter(|input| sink_cards.get(&input.sink_index) == Some(&device.card_id))
                .map(|input| PlaybackSource {
                    name: input.name.clone(),
                    sample_rate: input.sample_rate,
                })
                .collect();
        }
    }

    fn read_sink_cards() -> std::result::Result<std::collections::HashMap<u32, u32>, ()> {
        let output = Self::run_pactl(&["list", "sinks"]).map_err(|_| ())?;
        let mut sink_cards = std::collections::HashMap::new();
        let mut current_sink = None;

        for line in output.lines() {
            let trimmed = line.trim();

            if let Some(index) = trimmed.strip_prefix("Sink #") {
                current_sink = index.parse::<u32>().ok();
                continue;
            }

            if let Some(card) = trimmed.strip_prefix("api.alsa.pcm.card = ")
                && let (Some(sink_index), Some(card_id)) =
                    (current_sink, card.trim_matches('"').parse::<u32>().ok())
            {
                sink_cards.insert(sink_index, card_id);
            }
        }

        Ok(sink_cards)
    }

    fn read_sink_inputs() -> std::result::Result<Vec<SinkInputInfo>, ()> {
        let output = Self::run_pactl(&["list", "sink-inputs"]).map_err(|_| ())?;
        Ok(Self::parse_sink_inputs(&output))
    }

    fn run_pactl(args: &[&str]) -> std::result::Result<String, ()> {
        let output = Command::new("pactl").args(args).output().map_err(|_| ())?;
        if !output.status.success() {
            return Err(());
        }

        String::from_utf8(output.stdout).map_err(|_| ())
    }

    fn parse_sink_inputs(content: &str) -> Vec<SinkInputInfo> {
        let mut inputs = Vec::new();
        let mut current: Option<SinkInputInfo> = None;

        for line in content.lines() {
            let trimmed = line.trim();

            if trimmed.is_empty() {
                continue;
            }

            if trimmed.starts_with("Sink Input #") {
                if let Some(input) = current.take() {
                    inputs.push(input);
                }
                current = Some(SinkInputInfo::default());
                continue;
            }

            let Some(input) = current.as_mut() else {
                continue;
            };

            if let Some(sink) = trimmed.strip_prefix("Sink: ") {
                input.sink_index = sink.parse().unwrap_or(0);
            } else if let Some(spec) = trimmed.strip_prefix("Sample Specification: ") {
                input.sample_rate = Self::parse_sample_rate(spec);
            } else if let Some(name) = trimmed.strip_prefix("media.name = ") {
                input.media_name = Some(name.trim_matches('"').to_string());
            } else if let Some(name) = trimmed.strip_prefix("application.name = ") {
                input.app_name = Some(name.trim_matches('"').to_string());
            } else if let Some(name) = trimmed.strip_prefix("node.name = ") {
                input.node_name = Some(name.trim_matches('"').to_string());
            } else if let Some(name) = trimmed.strip_prefix("application.process.binary = ") {
                input.binary_name = Some(name.trim_matches('"').to_string());
            }
        }

        if let Some(input) = current.take() {
            inputs.push(input);
        }

        inputs
            .into_iter()
            .filter(|input| input.sink_index != 0)
            .map(|mut input| {
                input.name = input.display_name();
                input
            })
            .collect()
    }

    fn parse_sample_rate(spec: &str) -> Option<u32> {
        spec.split_whitespace()
            .find(|part| part.ends_with("Hz"))
            .and_then(|part| part.trim_end_matches("Hz").parse().ok())
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct StatusInfo {
    state: StreamState,
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct HwParamsInfo {
    sample_rate: Option<u32>,
    channels: Option<u32>,
}

#[cfg(target_os = "linux")]
#[derive(Debug, Default)]
struct SinkInputInfo {
    sink_index: u32,
    sample_rate: Option<u32>,
    media_name: Option<String>,
    app_name: Option<String>,
    node_name: Option<String>,
    binary_name: Option<String>,
    name: String,
}

#[cfg(target_os = "linux")]
impl SinkInputInfo {
    fn display_name(&self) -> String {
        let media_name = self
            .media_name
            .as_deref()
            .filter(|name| !Self::is_generic_name(name));
        let app_name = self
            .app_name
            .as_deref()
            .or(self.node_name.as_deref())
            .or(self.binary_name.as_deref());

        match (app_name, media_name) {
            (Some(app), Some(media)) if !Self::same_name(app, media) => {
                format!("{app}: {media}")
            }
            (_, Some(media)) => media.to_string(),
            (Some(app), None) => app.to_string(),
            (None, None) => self
                .media_name
                .as_deref()
                .unwrap_or("Unknown source")
                .to_string(),
        }
    }

    fn is_generic_name(name: &str) -> bool {
        matches!(
            name.trim().to_ascii_lowercase().as_str(),
            "playback" | "audio stream"
        )
    }

    fn same_name(left: &str, right: &str) -> bool {
        left.trim().eq_ignore_ascii_case(right.trim())
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn test_extract_card_id() {
        assert_eq!(ProcParser::extract_card_id("card0"), Some(0));
        assert_eq!(ProcParser::extract_card_id("card1"), Some(1));
        assert_eq!(ProcParser::extract_card_id("card10"), Some(10));
        assert_eq!(ProcParser::extract_card_id("notacard"), None);
    }

    #[test]
    fn test_extract_pcm_id() {
        assert_eq!(ProcParser::extract_pcm_id("pcm0p"), Some(0));
        assert_eq!(ProcParser::extract_pcm_id("pcm3p"), Some(3));
        assert_eq!(ProcParser::extract_pcm_id("pcm10p"), Some(10));
        assert_eq!(ProcParser::extract_pcm_id("pcm0c"), Some(0));
        assert_eq!(ProcParser::extract_pcm_id("notapcm"), None);
    }

    #[test]
    fn test_is_playback_pcm() {
        assert!(ProcParser::is_playback_pcm("pcm0p"));
        assert!(!ProcParser::is_playback_pcm("pcm0c"));
    }

    #[test]
    fn test_parse_sink_inputs() {
        let content = "Sink Input #83\nSink: 61\nSample Specification: float32le 2ch 48000Hz\nProperties:\n\tmedia.name = \"Firefox\"\n\tapplication.name = \"Firefox\"\n\n";
        let inputs = ProcParser::parse_sink_inputs(content);
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].sink_index, 61);
        assert_eq!(inputs[0].sample_rate, Some(48000));
        assert_eq!(inputs[0].name, "Firefox");
    }

    #[test]
    fn test_parse_sink_inputs_prefers_app_name_over_playback() {
        let content = "Sink Input #83\nSink: 61\nSample Specification: float32le 2ch 48000Hz\nProperties:\n\tapplication.name = \"Chromium\"\n\tapplication.process.binary = \"chromium\"\n\tmedia.name = \"Playback\"\n\tnode.name = \"Chromium\"\n\n";
        let inputs = ProcParser::parse_sink_inputs(content);
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].name, "Chromium");
    }

    #[test]
    fn test_parse_sink_inputs_combines_app_and_media_name() {
        let content = "Sink Input #83\nSink: 61\nSample Specification: float32le 2ch 48000Hz\nProperties:\n\tapplication.name = \"Firefox\"\n\tmedia.name = \"YouTube\"\n\n";
        let inputs = ProcParser::parse_sink_inputs(content);
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].name, "Firefox: YouTube");
    }

    #[test]
    fn test_parse_status_running() {
        let content = "state: RUNNING\nowner_pid: 1234\n";
        let info = ProcParser::parse_status(content).unwrap();
        assert!(matches!(info.state, StreamState::Running));
    }

    #[test]
    fn test_parse_status_paused() {
        let content = "state: PAUSED\n";
        let info = ProcParser::parse_status(content).unwrap();
        assert!(matches!(info.state, StreamState::Paused));
    }

    #[test]
    fn test_parse_hw_params() {
        let content = "access: MMAP_INTERLEAVED\nformat: S16_LE\nsubformat: STD\nchannels: 2\nrate: 44100 (44100/1)\n";
        let info = ProcParser::parse_hw_params(content).unwrap();
        assert_eq!(info.sample_rate, Some(44100));
        assert_eq!(info.channels, Some(2));
    }

    #[test]
    fn test_stream_state_from_str() {
        assert!(matches!(
            StreamState::from_str("RUNNING"),
            StreamState::Running
        ));
        assert!(matches!(
            StreamState::from_str("PAUSED"),
            StreamState::Paused
        ));
        assert!(matches!(
            StreamState::from_str("STOPPED"),
            StreamState::Stopped
        ));
        assert!(matches!(
            StreamState::from_str("UNKNOWN"),
            StreamState::Unknown(_)
        ));
    }
}

#[cfg(all(test, target_os = "macos"))]
mod macos_tests {
    use super::*;

    #[test]
    fn test_parse_macos_devices_marks_default_output_running() {
        let content = r#"
Audio:

    Devices:

        MacBook Pro Microphone:

          Default Input Device: Yes
          Input Channels: 1
          Manufacturer: Apple Inc.
          Current SampleRate: 48000
          Transport: Built-in
          Input Source: MacBook Pro Microphone

        MacBook Pro Speakers:

          Default Output Device: Yes
          Default System Output Device: Yes
          Manufacturer: Apple Inc.
          Output Channels: 2
          Current SampleRate: 96000
          Transport: Built-in
          Output Source: MacBook Pro Speakers

        Microsoft Teams Audio:

          Input Channels: 1
          Manufacturer: Microsoft Corp.
          Output Channels: 1
          Current SampleRate: 48000
          Transport: Virtual
          Input Source: Microsoft Teams Audio Device
          Output Source: Microsoft Teams Audio Device
"#;

        let devices = ProcParser::parse_macos_devices(content, Some(69));
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].card_name, "MacBook Pro Speakers");
        assert!(matches!(devices[0].state, StreamState::Running));
        assert_eq!(devices[0].sample_rate, Some(96000));
        assert_eq!(devices[0].channels, Some(2));
        assert_eq!(devices[0].volume, vec![69]);

        assert_eq!(devices[1].card_name, "Microsoft Teams Audio");
        assert!(matches!(devices[1].state, StreamState::Stopped));
        assert_eq!(devices[1].sample_rate, Some(48000));
        assert_eq!(devices[1].channels, Some(1));
        assert!(devices[1].volume.is_empty());
    }

    #[test]
    fn test_read_macos_playback_sources_parses_lsof_output() {
        let lsof_output = r#"p1234
cSpotify
n/dev/audio
p5678
cFirefox
n/dev/audio
p9999
ckernel_task
n/dev/null
"#;

        let mut sources = Vec::new();
        let mut current_pid = None;
        let mut current_name = None;

        for line in lsof_output.lines() {
            if let Some(pid_str) = line.strip_prefix('p') {
                current_pid = pid_str.parse::<u32>().ok();
            } else if let Some(name) = line.strip_prefix('c') {
                current_name = Some(name.to_string());
            } else if line.starts_with('n') && line.contains("audio") {
                if let (Some(_pid), Some(ref name)) = (current_pid, current_name) {
                    if !sources.iter().any(|s: &PlaybackSource| s.name == *name) {
                        sources.push(PlaybackSource {
                            name: name.clone(),
                            sample_rate: None,
                        });
                    }
                }
                current_pid = None;
                current_name = None;
            }
        }

        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].name, "Spotify");
        assert_eq!(sources[1].name, "Firefox");
    }
}
