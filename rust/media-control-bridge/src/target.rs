use anyhow::{anyhow, Context, Result};
use tokio::process::Command as TokioCommand;

use crate::protocol::{MediaCommand, PlaybackState};

#[derive(Debug, Clone)]
pub enum TargetKind {
    Mpris { player: Option<String> },
    Cmd(CommandMap),
    Smtc,
}

#[derive(Debug, Clone, Default)]
pub struct CommandMap {
    pub play: Option<String>,
    pub pause: Option<String>,
    pub play_pause: Option<String>,
    pub stop: Option<String>,
    pub next: Option<String>,
    pub previous: Option<String>,
    pub status: Option<String>,
}

impl TargetKind {
    pub async fn handle(&self, command: MediaCommand) -> Result<Option<PlaybackState>> {
        match self {
            Self::Mpris { player } => handle_linux_mpris(player.as_deref(), command).await,
            Self::Cmd(map) => handle_cmd(map, command).await,
            Self::Smtc => handle_smtc(command).await,
        }
    }
}

async fn run_shell(script: &str) -> Result<String> {
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = TokioCommand::new("cmd.exe");
        c.arg("/C").arg(script);
        c
    };

    #[cfg(not(target_os = "windows"))]
    let mut cmd = {
        let mut c = TokioCommand::new("sh");
        c.arg("-c").arg(script);
        c
    };

    let out = cmd.output().await.with_context(|| format!("running shell command: {script}"))?;
    if !out.status.success() {
        return Err(anyhow!(
            "command failed: {script}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

async fn handle_cmd(map: &CommandMap, command: MediaCommand) -> Result<Option<PlaybackState>> {
    let script = match command {
        MediaCommand::Play => map.play.as_deref(),
        MediaCommand::Pause => map.pause.as_deref(),
        MediaCommand::PlayPause => map.play_pause.as_deref(),
        MediaCommand::Stop => map.stop.as_deref(),
        MediaCommand::Next => map.next.as_deref(),
        MediaCommand::Previous => map.previous.as_deref(),
        MediaCommand::Status => map.status.as_deref(),
    }
    .ok_or_else(|| anyhow!("no command mapping configured for {command:?}"))?;

    let output = run_shell(script).await?;
    if matches!(command, MediaCommand::Status) {
        Ok(Some(PlaybackState {
            playback: if output.is_empty() { None } else { Some(output) },
            ..Default::default()
        }))
    } else {
        Ok(None)
    }
}

async fn run_playerctl(player: Option<&str>, args: &[&str]) -> Result<String> {
    let mut cmd = TokioCommand::new("playerctl");
    if let Some(player) = player {
        cmd.arg("-p").arg(player);
    }
    cmd.args(args);
    let out = cmd.output().await.context("failed to run playerctl; install playerctl or use --target cmd")?;
    if !out.status.success() {
        return Err(anyhow!(
            "playerctl failed: stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

async fn handle_linux_mpris(player: Option<&str>, command: MediaCommand) -> Result<Option<PlaybackState>> {
    #[cfg(target_os = "linux")]
    {
        match command {
            MediaCommand::Play => {
                run_playerctl(player, &["play"]).await?;
                Ok(None)
            }
            MediaCommand::Pause => {
                run_playerctl(player, &["pause"]).await?;
                Ok(None)
            }
            MediaCommand::PlayPause => {
                run_playerctl(player, &["play-pause"]).await?;
                Ok(None)
            }
            MediaCommand::Stop => {
                run_playerctl(player, &["stop"]).await?;
                Ok(None)
            }
            MediaCommand::Next => {
                run_playerctl(player, &["next"]).await?;
                Ok(None)
            }
            MediaCommand::Previous => {
                run_playerctl(player, &["previous"]).await?;
                Ok(None)
            }
            MediaCommand::Status => {
                let playback = run_playerctl(player, &["status"]).await.ok();
                let title = run_playerctl(player, &["metadata", "xesam:title"]).await.ok();
                let artist = run_playerctl(player, &["metadata", "xesam:artist"]).await.ok();
                let album = run_playerctl(player, &["metadata", "xesam:album"]).await.ok();
                Ok(Some(PlaybackState { playback, title, artist, album }))
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (player, command);
        Err(anyhow!("--target mpris is only implemented on Linux"))
    }
}

async fn handle_smtc(command: MediaCommand) -> Result<Option<PlaybackState>> {
    #[cfg(target_os = "windows")]
    {
        crate::windows_smtc::control_current_session(command).await
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = command;
        Err(anyhow!("--target smtc is only implemented on Windows"))
    }
}
