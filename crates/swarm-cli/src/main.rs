use std::{env, process::ExitCode};

use swarm_cli::{
    CliError, LifecycleCommand, execute, format_status, inspect_legacy_database, parse_command,
    verify_database,
};
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
    if let LifecycleCommand::VerifyDatabase { ref path } = command {
        verify_database(path)?;
        println!("database integrity verified");
        return Ok(());
    }
    if let LifecycleCommand::InspectLegacy { ref path } = command {
        let report = inspect_legacy_database(path)?;
        println!(
            "{}",
            serde_json::to_string(&report)
                .map_err(|error| CliError::LegacyDatabase(error.to_string()))?
        );
        return Ok(());
    }
    let socket =
        env::var_os("SWARM_TERMINAL_SOCKET").map_or_else(default_terminal_socket_path, Into::into);
    let status = execute(&HostClient::new(socket), command).await?;
    println!(
        "{}",
        format_status(&status).map_err(swarm_terminal::IpcError::from)?
    );
    Ok(())
}
