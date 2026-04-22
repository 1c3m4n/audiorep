#[derive(Debug, Clone, PartialEq)]
pub enum StreamState {
    Running,
    Paused,
    Stopped,
    Unknown(String),
}

impl StreamState {
    pub fn from_str(s: &str) -> Self {
        match s.trim() {
            "RUNNING" => StreamState::Running,
            "PAUSED" => StreamState::Paused,
            "STOPPED" => StreamState::Stopped,
            other => StreamState::Unknown(other.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioDevice {
    pub card_id: u32,
    pub card_name: String,
    pub pcm_id: u32,
    pub sub_id: u32,
    pub is_playback: bool,
    pub state: StreamState,
    pub sample_rate: Option<u32>,
    pub channels: Option<u32>,
    pub volume: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AudioInfo {
    pub devices: Vec<AudioDevice>,
}

impl AudioInfo {
    pub fn visible_devices(&self, show_hidden: bool) -> Vec<&AudioDevice> {
        self.devices
            .iter()
            .filter(|device| device.is_playback)
            .filter(|device| show_hidden || !matches!(device.state, StreamState::Stopped))
            .collect()
    }

    pub fn active_device(&self, show_hidden: bool) -> Option<&AudioDevice> {
        self.visible_devices(show_hidden)
            .into_iter()
            .find(|d| matches!(d.state, StreamState::Running))
    }
}
