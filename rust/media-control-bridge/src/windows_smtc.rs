use anyhow::{anyhow, Context, Result};

use crate::protocol::{MediaCommand, PlaybackState};

#[cfg(target_os = "windows")]
pub async fn control_current_session(command: MediaCommand) -> Result<Option<PlaybackState>> {
    // First try the proper Windows GSMTC remote-control API. Some Windows environments
    // (old builds, non-interactive sessions, stripped/server images, or broken WinRT
    // registrations) can fail with 0x80040154 / REGDB_E_CLASSNOTREG. For media-control
    // commands, fall back to synthetic multimedia keys so the controlled endpoint still
    // works for the common "music is playing here" case.
    match control_current_session_gsmtc(command.clone()).await {
        Ok(state) => Ok(state),
        Err(err) if can_fallback_to_media_key(&command) => {
            eprintln!(
                "warning: Windows GSMTC control failed ({err:#}); falling back to multimedia key"
            );
            send_windows_media_key(command).context("Windows multimedia-key fallback failed")?;
            Ok(None)
        }
        Err(err) => Err(err),
    }
}

#[cfg(target_os = "windows")]
async fn control_current_session_gsmtc(command: MediaCommand) -> Result<Option<PlaybackState>> {
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

#[cfg(target_os = "windows")]
fn can_fallback_to_media_key(command: &MediaCommand) -> bool {
    matches!(
        command,
        MediaCommand::Play
            | MediaCommand::Pause
            | MediaCommand::PlayPause
            | MediaCommand::Stop
            | MediaCommand::Next
            | MediaCommand::Previous
    )
}

#[cfg(target_os = "windows")]
fn send_windows_media_key(command: MediaCommand) -> Result<()> {
    // Use a tiny PowerShell P/Invoke shim instead of the WinRT media-control API. This
    // avoids the 0x80040154 class-registration failure path and behaves like pressing
    // the hardware media keys on the keyboard. VK codes:
    //   0xB0 = VK_MEDIA_NEXT_TRACK
    //   0xB1 = VK_MEDIA_PREV_TRACK
    //   0xB2 = VK_MEDIA_STOP
    //   0xB3 = VK_MEDIA_PLAY_PAUSE
    // There is no reliable global discrete Play-only/Pause-only multimedia key, so both
    // Play and Pause fall back to the Play/Pause toggle when GSMTC is unavailable.
    let vk: u16 = match command {
        MediaCommand::Play | MediaCommand::Pause | MediaCommand::PlayPause => 0xB3,
        MediaCommand::Stop => 0xB2,
        MediaCommand::Next => 0xB0,
        MediaCommand::Previous => 0xB1,
        MediaCommand::Status => return Err(anyhow!("status cannot be read via multimedia-key fallback")),
    };

    let ps = format!(
        r#"
$ErrorActionPreference = 'Stop'
Add-Type -Namespace Mcb -Name Native -MemberDefinition @'
[System.Runtime.InteropServices.DllImport("user32.dll")]
public static extern void keybd_event(byte bVk, byte bScan, int dwFlags, int dwExtraInfo);
'@
[Mcb.Native]::keybd_event([byte]{vk}, [byte]0, 0, 0)
[Mcb.Native]::keybd_event([byte]{vk}, [byte]0, 2, 0)
"#
    );

    let status = std::process::Command::new("powershell.exe")
        .arg("-NoProfile")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-Command")
        .arg(ps)
        .status()
        .context("failed to launch powershell.exe for multimedia-key fallback")?;

    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("powershell multimedia-key fallback exited with {status}"))
    }
}

#[cfg(not(target_os = "windows"))]
pub async fn control_current_session(_command: MediaCommand) -> Result<Option<PlaybackState>> {
    Err(anyhow::anyhow!("Windows SMTC/GSMTC target is only available on Windows"))
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
            let Some(args) = args.as_ref() else {
                return Ok(());
            };

            let cmd = match args.Button()? {
                SystemMediaTransportControlsButton::Play => MediaCommand::Play,
                SystemMediaTransportControlsButton::Pause => MediaCommand::Pause,
                SystemMediaTransportControlsButton::Stop => MediaCommand::Stop,
                SystemMediaTransportControlsButton::Next => MediaCommand::Next,
                SystemMediaTransportControlsButton::Previous => MediaCommand::Previous,
                _ => MediaCommand::PlayPause,
            };
            let _ = tx2.send(cmd);
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
    Err(anyhow::anyhow!("smtc-client is only implemented on Windows"))
}
