use anyhow::{anyhow, Result};
use tokio::sync::mpsc;

use crate::net::send_command;
use crate::protocol::MediaCommand;

#[cfg(target_os = "linux")]
pub async fn run_mpris_client(connect: String, token: Option<String>, name: String) -> Result<()> {
    use mpris_server::Player;

    let bus_name = format!("rs.mediabridge.{}", sanitize_bus_part(&name));
    let player = Player::builder(&bus_name)
        .can_play(true)
        .can_pause(true)
        .can_go_next(true)
        .can_go_previous(true)
        .can_control(true)
        .identity(name.clone())
        .build()
        .await?;

    let (tx, mut rx) = mpsc::unbounded_channel::<MediaCommand>();

    {
        let tx = tx.clone();
        player.connect_play_pause(move |_| {
            let _ = tx.send(MediaCommand::PlayPause);
        });
    }
    {
        let tx = tx.clone();
        player.connect_play(move |_| {
            let _ = tx.send(MediaCommand::Play);
        });
    }
    {
        let tx = tx.clone();
        player.connect_pause(move |_| {
            let _ = tx.send(MediaCommand::Pause);
        });
    }
    {
        let tx = tx.clone();
        player.connect_stop(move |_| {
            let _ = tx.send(MediaCommand::Stop);
        });
    }
    {
        let tx = tx.clone();
        player.connect_next(move |_| {
            let _ = tx.send(MediaCommand::Next);
        });
    }
    {
        let tx = tx;
        player.connect_previous(move |_| {
            let _ = tx.send(MediaCommand::Previous);
        });
    }

    eprintln!("MPRIS client registered as org.mpris.MediaPlayer2.{bus_name}");
    eprintln!("Connected controls will be sent to {connect}");

    let run_fut = player.run();
    tokio::pin!(run_fut);

    loop {
        tokio::select! {
            result = &mut run_fut => {
                result?;
                return Ok(());
            }
            maybe_cmd = rx.recv() => {
                match maybe_cmd {
                    Some(cmd) => {
                        if let Err(err) = send_command(&connect, token.as_deref(), cmd).await {
                            eprintln!("send failed: {err:#}");
                        }
                    }
                    None => return Ok(()),
                }
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub async fn run_mpris_client(_connect: String, _token: Option<String>, _name: String) -> Result<()> {
    Err(anyhow!("mpris-client is only implemented on Linux"))
}

fn sanitize_bus_part(input: &str) -> String {
    let mut s = String::new();
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            s.push(ch);
        } else {
            s.push('_');
        }
    }
    if s.is_empty() || s.chars().next().unwrap().is_ascii_digit() {
        s.insert(0, '_');
    }
    s
}
