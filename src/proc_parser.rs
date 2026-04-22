use std::fs;
use std::path::Path;

use crate::audio_info::{AudioDevice, AudioInfo, StreamState};
use crate::error::{AudioError, Result};

const PROC_ASOUND: &str = "/proc/asound";

pub struct ProcParser;

impl ProcParser {
    pub fn new() -> Self {
        Self
    }

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

            if entry.path().is_dir() {
                if let Some(pcm_id) = Self::extract_pcm_id(&name_str) {
                    let sub_devices =
                        Self::read_sub_devices(card_id, pcm_id, &name_str, card_name)?;
                    devices.extend(sub_devices);
                }
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

            if entry.path().is_dir() {
                if let Some(sub_id) = Self::extract_sub_id(&name_str) {
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
                        volume: Vec::new(),
                    });
                }
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
}

#[derive(Debug)]
struct StatusInfo {
    state: StreamState,
}

#[derive(Debug)]
struct HwParamsInfo {
    sample_rate: Option<u32>,
    channels: Option<u32>,
}

#[cfg(test)]
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
