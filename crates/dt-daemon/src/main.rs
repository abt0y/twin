//! dtd: DT Agent IPC Daemon
//!
//! A daemon that manages agent execution with WASM sandboxing.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tokio::signal;
use tracing::{error, info};
use tracing_subscriber;

mod sandbox;
mod ipc;
mod registry;

use ipc::IpcServer;
use registry::AgentRegistry;

#[derive(Parser, Debug)]
#[command(name = "dtd")]
#[command(about = "DT Agent IPC Daemon with WASM sandbox", long_about = None)]
struct Cli {
    /// Socket path for IPC
    #[arg(short, long, default_value = "/tmp/dtd.sock")]
    socket: PathBuf,

    /// Data root directory
    #[arg(short, long, default_value = "~/.dt")]
    data_root: PathBuf,

    /// Verbose logging
    #[arg(short, long)]
    verbose: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start the daemon
    Start,
    /// Stop the daemon
    Stop,
    /// List registered agents
    List,
    /// Register an agent
    Register {
        /// Agent name
        name: String,
        /// WASM module path
        wasm_path: PathBuf,
    },
    /// Unregister an agent
    Unregister {
        /// Agent name
        name: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let log_level = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(format!("dtd={},wasmtime=info", log_level))
        .init();

    // Expand tilde in paths
    let data_root = shellexpand::tilde(&cli.data_root.to_string_lossy())
        .to_string();
    let data_root = PathBuf::from(data_root);

    // Ensure data root exists
    std::fs::create_dir_all(&data_root)?;

    match cli.command {
        Some(Commands::Start) => {
            info!("Starting dtd daemon on socket: {}", cli.socket.display());
            start_daemon(cli.socket, data_root).await?;
        }
        Some(Commands::Stop) => {
            info!("Stopping dtd daemon");
            stop_daemon(cli.socket).await?;
        }
        Some(Commands::List) => {
            list_agents(data_root).await?;
        }
        Some(Commands::Register { name, wasm_path }) => {
            register_agent(name, wasm_path, data_root).await?;
        }
        Some(Commands::Unregister { name }) => {
            unregister_agent(name, data_root).await?;
        }
        None => {
            // Default: start daemon
            info!("Starting dtd daemon on socket: {}", cli.socket.display());
            start_daemon(cli.socket, data_root).await?;
        }
    }

    Ok(())
}

/// Start the daemon.
async fn start_daemon(socket_path: PathBuf, data_root: PathBuf) -> Result<()> {
    // Remove existing socket if present
    if socket_path.exists() {
        std::fs::remove_file(&socket_path)?;
    }

    // Initialize agent registry
    let registry = AgentRegistry::new(data_root.join("agents"))?;

    // Create IPC server
    let ipc_server = IpcServer::new(socket_path.clone(), registry)?;

    // Handle shutdown signals
    let shutdown = signal::ctrl_c();

    tokio::select! {
        result = ipc_server.run() => {
            error!("IPC server stopped: {:?}", result);
        }
        _ = shutdown => {
            info!("Received shutdown signal");
        }
    }

    // Cleanup socket
    if socket_path.exists() {
        std::fs::remove_file(&socket_path)?;
    }

    Ok(())
}

/// Stop the daemon.
async fn stop_daemon(socket_path: PathBuf) -> Result<()> {
    // Send stop signal via socket
    use tokio::net::UnixStream;
    use tokio::io::AsyncWriteExt;
    let mut stream = UnixStream::connect(&socket_path).await?;
    stream.write_all(b"STOP").await?;
    Ok(())
}

/// List registered agents.
async fn list_agents(data_root: PathBuf) -> Result<()> {
    let registry = AgentRegistry::new(data_root.join("agents"))?;
    let agents = registry.list()?;
    
    println!("Registered agents:");
    for agent in agents {
        println!("  - {}", agent);
    }
    
    Ok(())
}

/// Register an agent.
async fn register_agent(name: String, wasm_path: PathBuf, data_root: PathBuf) -> Result<()> {
    let mut registry = AgentRegistry::new(data_root.join("agents"))?;
    registry.register(&name, &wasm_path)?;
    println!("Registered agent: {}", name);
    Ok(())
}

/// Unregister an agent.
async fn unregister_agent(name: String, data_root: PathBuf) -> Result<()> {
    let mut registry = AgentRegistry::new(data_root.join("agents"))?;
    registry.unregister(&name)?;
    println!("Unregistered agent: {}", name);
    Ok(())
}
