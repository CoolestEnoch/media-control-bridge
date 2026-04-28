mod linux_mpris_client;
mod net;
mod protocol;
mod server;
mod target;
mod windows_smtc;

use anyhow::{anyhow, Result};
use clap::{CommandFactory, Parser, ValueEnum};
use protocol::MediaCommand;
use target::{CommandMap, TargetKind};

#[derive(Debug, Parser)]
#[command(name = "media-control-bridge")]
#[command(about = "TCP protocol bridge between Linux MPRIS and Windows SMTC media controls")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, clap::Subcommand)]
enum Commands {
    /// Run the controlled endpoint. This listens on TCP and controls the local player.
    Serve {
        /// Listen address, e.g. 127.0.0.1:17777 or 0.0.0.0:17777
        #[arg(long, default_value = "127.0.0.1:17777")]
        listen: String,

        /// Optional shared secret. Recommended when listening on anything except 127.0.0.1.
        #[arg(long)]
        token: Option<String>,

        /// Local control adapter.
        #[arg(long, value_enum)]
        target: TargetArg,

        /// Linux MPRIS player name for playerctl -p. Omit for playerctl's default player.
        #[arg(long)]
        player: Option<String>,

        /// Shell command for play.
        #[arg(long)]
        cmd_play: Option<String>,

        /// Shell command for pause.
        #[arg(long)]
        cmd_pause: Option<String>,

        /// Shell command for play/pause toggle.
        #[arg(long)]
        cmd_play_pause: Option<String>,

        /// Shell command for stop.
        #[arg(long)]
        cmd_stop: Option<String>,

        /// Shell command for next.
        #[arg(long)]
        cmd_next: Option<String>,

        /// Shell command for previous.
        #[arg(long)]
        cmd_previous: Option<String>,

        /// Shell command for status.
        #[arg(long)]
        cmd_status: Option<String>,
    },

    /// Linux controller endpoint: expose a local MPRIS player that forwards commands over TCP.
    MprisClient {
        /// Controlled endpoint address, e.g. 192.168.1.20:17777
        #[arg(long)]
        connect: String,

        /// Optional shared secret.
        #[arg(long)]
        token: Option<String>,

        /// Local player name shown by MPRIS-capable desktops.
        #[arg(long, default_value = "RemoteMusic")]
        name: String,
    },

    /// Windows controller endpoint: expose a local SMTC player that forwards commands over TCP.
    SmtcClient {
        /// Controlled endpoint address, e.g. 192.168.1.20:17777
        #[arg(long)]
        connect: String,

        /// Optional shared secret.
        #[arg(long)]
        token: Option<String>,

        /// Local title shown in Windows media controls.
        #[arg(long, default_value = "RemoteMusic")]
        name: String,
    },

    /// Send one command and exit. Useful for testing.
    Send {
        #[arg(long)]
        connect: String,

        #[arg(long)]
        token: Option<String>,

        #[arg(long)]
        command: MediaCommand,
    },
}

#[derive(Debug, Clone, ValueEnum)]
enum TargetArg {
    /// Linux MPRIS via playerctl.
    Mpris,
    /// Windows GSMTC current session.
    Smtc,
    /// Arbitrary shell commands.
    Cmd,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    if std::env::args_os().len() == 1 {
        eprintln!("warning: no subcommand was supplied; printing help. Use `serve`, `mpris-client`, `smtc-client`, or `send`.\n");
        let mut cmd = Cli::command();
        cmd.print_help()?;
        eprintln!();
        std::process::exit(2);
    }

    let cli = Cli::parse();

    match cli.command {
        Commands::Serve {
            listen,
            token,
            target,
            player,
            cmd_play,
            cmd_pause,
            cmd_play_pause,
            cmd_stop,
            cmd_next,
            cmd_previous,
            cmd_status,
        } => {
            let target = match target {
                TargetArg::Mpris => TargetKind::Mpris { player },
                TargetArg::Smtc => TargetKind::Smtc,
                TargetArg::Cmd => {
                    let map = CommandMap {
                        play: cmd_play,
                        pause: cmd_pause,
                        play_pause: cmd_play_pause,
                        stop: cmd_stop,
                        next: cmd_next,
                        previous: cmd_previous,
                        status: cmd_status,
                    };
                    if map.play.is_none()
                        && map.pause.is_none()
                        && map.play_pause.is_none()
                        && map.stop.is_none()
                        && map.next.is_none()
                        && map.previous.is_none()
                        && map.status.is_none()
                    {
                        return Err(anyhow!("--target cmd requires at least one --cmd-* mapping"));
                    }
                    TargetKind::Cmd(map)
                }
            };
            server::serve(&listen, token, target).await
        }
        Commands::MprisClient { connect, token, name } => {
            linux_mpris_client::run_mpris_client(connect, token, name).await
        }
        Commands::SmtcClient { connect, token, name } => {
            windows_smtc::run_smtc_client(connect, token, name).await
        }
        Commands::Send { connect, token, command } => {
            let state = net::send_command(&connect, token.as_deref(), command).await?;
            if let Some(state) = state {
                println!("{}", serde_json::to_string_pretty(&state)?);
            } else {
                println!("ok");
            }
            Ok(())
        }
    }
}
