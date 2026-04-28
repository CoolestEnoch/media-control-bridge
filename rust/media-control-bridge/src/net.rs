use anyhow::{anyhow, Context, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

use crate::protocol::{MediaCommand, PlaybackState, WireMessage};

pub async fn send_command(
    connect: &str,
    token: Option<&str>,
    command: MediaCommand,
) -> Result<Option<PlaybackState>> {
    let mut stream = TcpStream::connect(connect)
        .await
        .with_context(|| format!("failed to connect to {connect}"))?;

    let msg = WireMessage::Command {
        v: 1,
        token: token.map(ToOwned::to_owned),
        command,
    };
    let mut line = serde_json::to_string(&msg)?;
    line.push('\n');
    stream.write_all(line.as_bytes()).await?;
    stream.flush().await?;

    let mut reader = BufReader::new(stream);
    let mut reply = String::new();
    let n = reader.read_line(&mut reply).await?;
    if n == 0 {
        return Err(anyhow!("server closed connection without reply"));
    }

    match serde_json::from_str::<WireMessage>(&reply)? {
        WireMessage::Ack { ok, message, state, .. } => {
            if ok {
                Ok(state)
            } else {
                Err(anyhow!(message))
            }
        }
        other => Err(anyhow!("unexpected reply: {other:?}")),
    }
}
