use anyhow::{anyhow, Result};

use crate::protocol::{MediaCommand, PlaybackState};

#[cfg(target_os = "windows")]
pub async fn control_current_session(command: MediaCommand) -> Result<Option<PlaybackState>> {
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSessionManager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus,
    };

    let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?.get()?;
    let session = manager.GetCurrentSession()?;

    match command {
        MediaCommand::Play => {
            session.TryPlayAsync()?.get()?;
            Ok(None)
        }
        MediaCommand::Pause => {
            session.TryPauseAsync()?.get()?;
            Ok(None)
        }
        MediaCommand::PlayPause => {
            session.TryTogglePlayPauseAsync()?.get()?;
            Ok(None)
        }
        MediaCommand::Stop => {
            session.TryStopAsync()?.get()?;
            Ok(None)
        }
        MediaCommand::Next => {
            session.TrySkipNextAsync()?.get()?;
            Ok(None)
        }
        MediaCommand::Previous => {
            session.TrySkipPreviousAsync()?.get()?;
            Ok(None)
        }
        MediaCommand::Status => {
            let info = session.GetPlaybackInfo()?;
            let playback = match info.PlaybackStatus()? {
                GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing => Some("Playing".to_string()),
                GlobalSystemMediaTransportControlsSessionPlaybackStatus::Paused => Some("Paused".to_string()),
                GlobalSystemMediaTransportControlsSessionPlaybackStatus::Stopped => Some("Stopped".to_string()),
                GlobalSystemMediaTransportControlsSessionPlaybackStatus::Closed => Some("Closed".to_string()),
                GlobalSystemMediaTransportControlsSessionPlaybackStatus::Changing => Some("Changing".to_string()),
                _ => None,
            };

            let props = session.TryGetMediaPropertiesAsync()?.get().ok();
            let title = props.as_ref().and_then(|p| p.Title().ok()).map(|s| s.to_string());
            let artist = props.as_ref().and_then(|p| p.Artist().ok()).map(|s| s.to_string());
            let album = props.as_ref().and_then(|p| p.AlbumTitle().ok()).map(|s| s.to_string());

            Ok(Some(PlaybackState { playback, title, artist, album }))
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub async fn control_current_session(_command: MediaCommand) -> Result<Option<PlaybackState>> {
    Err(anyhow!("Windows SMTC/GSMTC target is only available on Windows"))
}

#[cfg(target_os = "windows")]
pub async fn run_smtc_client(connect: String, token: Option<String>, name: String) -> Result<()> {
    use tokio::sync::mpsc;
    use windows::core::HSTRING;
    use windows::Foundation::TypedEventHandler;
    use windows::Media::{
        MediaPlaybackStatus, MediaPlaybackType, SystemMediaTransportControlsButton,
        SystemMediaTransportControlsButtonPressedEventArgs,
    };
    use windows::Media::Playback::MediaPlayer;

    let player = MediaPlayer::new()?;
    player.CommandManager()?.SetIsEnabled(false)?;
    let smtc = player.SystemMediaTransportControls()?;
    smtc.SetIsEnabled(true)?;
    smtc.SetIsPlayEnabled(true)?;
    smtc.SetIsPauseEnabled(true)?;
    smtc.SetIsNextEnabled(true)?;
    smtc.SetIsPreviousEnabled(true)?;
    smtc.SetIsStopEnabled(true)?;
    smtc.SetPlaybackStatus(MediaPlaybackStatus::Playing)?;

    let updater = smtc.DisplayUpdater()?;
    updater.SetType(MediaPlaybackType::Music)?;
    let music = updater.MusicProperties()?;
    music.SetTitle(&HSTRING::from(name.clone()))?;
    music.SetArtist(&HSTRING::from("media-control-bridge"))?;
    updater.Update()?;

    let (tx, mut rx) = mpsc::unbounded_channel::<MediaCommand>();
    let tx2 = tx.clone();
    let _registration = smtc.ButtonPressed(&TypedEventHandler::<_, SystemMediaTransportControlsButtonPressedEventArgs>::new(
        move |_sender, args| {
            if let Some(args) = args {
                let cmd = match args.Button()? {
                    SystemMediaTransportControlsButton::Play => Some(MediaCommand::Play),
                    SystemMediaTransportControlsButton::Pause => Some(MediaCommand::Pause),
                    SystemMediaTransportControlsButton::Stop => Some(MediaCommand::Stop),
                    SystemMediaTransportControlsButton::Next => Some(MediaCommand::Next),
                    SystemMediaTransportControlsButton::Previous => Some(MediaCommand::Previous),
                    _ => Some(MediaCommand::PlayPause),
                };
                if let Some(cmd) = cmd {
                    let _ = tx2.send(cmd);
                }
            }
            Ok(())
        },
    ))?;

    eprintln!("Windows SMTC client registered as '{name}'");
    eprintln!("Connected controls will be sent to {connect}");

    while let Some(cmd) = rx.recv().await {
        if let Err(err) = crate::net::send_command(&connect, token.as_deref(), cmd).await {
            eprintln!("send failed: {err:#}");
        }
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub async fn run_smtc_client(_connect: String, _token: Option<String>, _name: String) -> Result<()> {
    Err(anyhow!("smtc-client is only implemented on Windows"))
}
