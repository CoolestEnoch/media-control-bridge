use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MediaCommand {
    Play,
    Pause,
    PlayPause,
    Stop,
    Next,
    Previous,
    Status,
}

impl std::str::FromStr for MediaCommand {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().replace('-', "_").as_str() {
            "play" => Ok(Self::Play),
            "pause" => Ok(Self::Pause),
            "play_pause" | "playpause" | "toggle" => Ok(Self::PlayPause),
            "stop" => Ok(Self::Stop),
            "next" => Ok(Self::Next),
            "previous" | "prev" => Ok(Self::Previous),
            "status" => Ok(Self::Status),
            other => Err(format!("unknown command: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlaybackState {
    pub playback: Option<String>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireMessage {
    Hello {
        v: u8,
        token: Option<String>,
        name: Option<String>,
    },
    Command {
        v: u8,
        token: Option<String>,
        command: MediaCommand,
    },
    Ack {
        v: u8,
        ok: bool,
        message: String,
        state: Option<PlaybackState>,
    },
}

impl WireMessage {
    pub fn ok(message: impl Into<String>, state: Option<PlaybackState>) -> Self {
        Self::Ack {
            v: 1,
            ok: true,
            message: message.into(),
            state,
        }
    }

    pub fn err(message: impl Into<String>) -> Self {
        Self::Ack {
            v: 1,
            ok: false,
            message: message.into(),
            state: None,
        }
    }

    pub fn token(&self) -> Option<&str> {
        match self {
            WireMessage::Hello { token, .. } | WireMessage::Command { token, .. } => {
                token.as_deref()
            }
            WireMessage::Ack { .. } => None,
        }
    }
}
