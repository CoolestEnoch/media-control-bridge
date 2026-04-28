use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tracing::{error, info};

use crate::protocol::WireMessage;
use crate::target::TargetKind;

pub async fn serve(listen: &str, token: Option<String>, target: TargetKind) -> Result<()> {
    let listener = TcpListener::bind(listen)
        .await
        .with_context(|| format!("failed to listen on {listen}"))?;
    info!("listening on {listen}");

    let token = Arc::new(token);
    let target = Arc::new(target);

    loop {
        let (stream, peer) = listener.accept().await?;
        let token = Arc::clone(&token);
        let target = Arc::clone(&target);
        tokio::spawn(async move {
            if let Err(err) = handle_conn(stream, token, target).await {
                error!("connection {peer} failed: {err:#}");
            }
        });
    }
}

async fn handle_conn(stream: TcpStream, token: Arc<Option<String>>, target: Arc<TargetKind>) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await? {
        let reply = match serde_json::from_str::<WireMessage>(&line) {
            Ok(msg) => {
                if !authorized(token.as_deref(), msg.token()) {
                    WireMessage::err("unauthorized")
                } else {
                    match msg {
                        WireMessage::Command { command, .. } => match target.handle(command).await {
                            Ok(state) => WireMessage::ok("ok", state),
                            Err(err) => WireMessage::err(format!("{err:#}")),
                        },
                        WireMessage::Hello { .. } => WireMessage::ok("hello", None),
                        WireMessage::Ack { .. } => WireMessage::err("client sent unexpected ack"),
                    }
                }
            }
            Err(err) => WireMessage::err(format!("invalid json: {err}")),
        };

        let mut out = serde_json::to_string(&reply)?;
        out.push('\n');
        writer.write_all(out.as_bytes()).await?;
        writer.flush().await?;
    }
    Ok(())
}

fn authorized(expected: Option<&str>, got: Option<&str>) -> bool {
    match expected {
        None => true,
        Some(expected) => got == Some(expected),
    }
}
