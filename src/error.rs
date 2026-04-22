use std::io;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum AudioError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("No audio devices found")]
    NoDevices,
}

pub type Result<T> = std::result::Result<T, AudioError>;
