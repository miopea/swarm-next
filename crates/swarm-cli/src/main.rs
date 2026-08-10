use std::{env, process::ExitCode};

use swarm_cli::{CliError, execute, format_status, parse_command};
use swarm_terminal::{HostClient, default_terminal_socket_path};

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(error.exit_code())
        }
    }
}

async fn run() -> Result<(), CliError> {
    let command = parse_command(env::args_os().skip(1))?;
    let socket =
        env::var_os("SWARM_TERMINAL_SOCKET").map_or_else(default_terminal_socket_path, Into::into);
    let status = execute(&HostClient::new(socket), command).await?;
    println!(
        "{}",
        format_status(&status).map_err(swarm_terminal::IpcError::from)?
    );
    Ok(())
}
