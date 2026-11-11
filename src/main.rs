//! Entry point: runs the server, or one of the host user's commands.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use riftview_api::cli;

/// Log level used when the `RUST_LOG` environment variable is not set.
const DEFAULT_LOG_FILTER: &str = "info,tower_http=info";

#[derive(Parser)]
#[command(name = "riftview-api", version, about = "RiftView host service")]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the server (the default when no command is given).
    Serve,
    /// Pair a new device: shows a six-digit code, then asks you to approve.
    Pair,
    /// Show the server's addresses and certificate pin.
    Info,
    /// List paired devices.
    Devices,
    /// Revoke a device so it can never connect again.
    Revoke { device_id: String },
    /// Manage shared folders.
    #[command(subcommand)]
    Share(ShareCommand),
}

#[derive(Subcommand)]
enum ShareCommand {
    /// List shared folders.
    List,
    /// Share a folder.
    Add {
        /// Name devices will see.
        name: String,
        /// Full path of the folder on this computer.
        path: PathBuf,
        /// Allow devices with write permissions to change files here.
        #[arg(long)]
        writable: bool,
    },
    /// Stop sharing a folder (the folder itself is not touched).
    Remove { share_id: String },
}

fn main() -> ExitCode {
    let args = Args::parse();

    let result = match args.command.unwrap_or(Command::Serve) {
        Command::Serve => return serve(),
        Command::Pair => cli::pair(),
        Command::Info => cli::info(),
        Command::Devices => cli::list_devices(),
        Command::Revoke { device_id } => cli::revoke_device(&device_id),
        Command::Share(ShareCommand::List) => cli::list_shares(),
        Command::Share(ShareCommand::Add {
            name,
            path,
            writable,
        }) => cli::add_share(&name, &path, writable),
        Command::Share(ShareCommand::Remove { share_id }) => cli::remove_share(&share_id),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn serve() -> ExitCode {
    set_up_logging();

    let runtime = tokio::runtime::Runtime::new().expect("failed to start async runtime");
    match runtime.block_on(riftview_api::server::run()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!("fatal: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn set_up_logging() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER));

    tracing_subscriber::fmt().with_env_filter(filter).init();
}
