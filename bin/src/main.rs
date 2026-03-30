use clap::Parser;
use color_eyre::eyre::{Result, WrapErr};
use meridian_core::agent::{AgentRecord, AgentState};
use meridian_core::checkpoint::CheckpointSummary;
use meridian_core::config::MeridianConfig;
use meridian_core::directive::Directive;
use meridian_core::event::BusEvent;
use meridian_core::id::AgentId;
use meridian_core::store::{AgentStore, CheckpointStore};
use meridian_store::SqliteStore;
use meridian_tui::tui_command::TuiCommand;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[derive(Parser)]
#[command(name = "meridian", about = "Agent orchestration with persistent context")]
struct Cli {
    /// Config file path
    #[arg(short, long, default_value = "meridian.toml")]
    config: PathBuf,

    /// Override SQLite database path
    #[arg(long)]
    db: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let tui_log = meridian_tui::tui_tracing::TuiLogBuffer::new();
    let tui_layer = meridian_tui::tui_tracing::TuiTracingLayer::new(tui_log.clone());

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "meridian=debug".into()),
        )
        .with(tui_layer)
        .init();

    let cli = Cli::parse();

    let config_str = std::fs::read_to_string(&cli.config).unwrap_or_else(|_| {
        tracing::warn!("Config file not found, using defaults");
        String::new()
    });
    let mut config: MeridianConfig = if config_str.is_empty() {
        MeridianConfig::default()
    } else {
        toml::from_str(&config_str).wrap_err("failed to parse config")?
    };

    if let Some(db) = cli.db {
        config.meridian.sqlite_path = db;
    }

    let embedder = Arc::new(
        meridian_embedding::FastEmbedder::new(&config.meridian.embedding_model)
            .wrap_err("failed to initialize embedding model")?,
    );

    let store = Arc::new(
        meridian_store::SqliteStore::open(
            &config.meridian.sqlite_path,
            meridian_core::embedding::EmbeddingProvider::dimension(embedder.as_ref()),
        )
        .wrap_err("failed to open database")?,
    );

    let (event_tx, event_rx) = broadcast::channel::<BusEvent>(1024);
    let (cmd_tx, cmd_rx) = mpsc::channel::<TuiCommand>(256);

    let _mcp_server = meridian_mcp::server::MeridianMcpServer::new(
        store.clone(),
        event_tx.clone(),
        config.clone(),
    );

    tracing::info!("Meridian initialized, starting TUI...");

    let cmd_handle = tokio::spawn(run_command_handler(
        cmd_rx,
        event_tx.clone(),
        store.clone(),
        config.clone(),
    ));

    let working_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut terminal = ratatui::init();
    let mut app = meridian_tui::App::new(event_rx, cmd_tx, working_dir, tui_log);
    let tui_result = app.run(&mut terminal).await;
    ratatui::restore();

    let _ = event_tx.send(BusEvent::Shutdown);
    cmd_handle.abort();

    tui_result?;
    Ok(())
}

async fn run_command_handler(
    mut cmd_rx: mpsc::Receiver<TuiCommand>,
    event_tx: broadcast::Sender<BusEvent>,
    store: Arc<SqliteStore>,
    _config: MeridianConfig,
) {

    while let Some(cmd) = cmd_rx.recv().await {
        tracing::debug!(?cmd, "command handler received");

        match cmd {
            TuiCommand::Spawn {
                objective_id,
                content,
                dir,
            } => {
                let agent_id = AgentId::new();
                let session_id = uuid::Uuid::new_v4().to_string();

                let record = AgentRecord {
                    id: agent_id,
                    state: AgentState::Active,
                    directory: dir.clone(),
                    objective_id,
                    checkpoint_version: None,
                    spawned_by: None,
                    injected_message: Some(content),
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                };

                if let Err(e) = store.register(record).await {
                    tracing::error!(%agent_id, %e, "failed to register agent");
                    continue;
                }

                tracing::info!(%agent_id, %session_id, "agent registered");

                let _ = event_tx.send(BusEvent::AgentStateChanged {
                    agent_id,
                    old_state: AgentState::Starting,
                    new_state: AgentState::Active,
                });
                let _ = event_tx.send(BusEvent::AgentSessionReady {
                    agent_id,
                    session_id,
                    directory: dir,
                });
            }

            TuiCommand::Kill { agent_id } => {
                let old_state = store
                    .get(agent_id)
                    .await
                    .ok()
                    .flatten()
                    .map(|r| r.state)
                    .unwrap_or(AgentState::Active);

                let _ = store.update_state(agent_id, AgentState::Exited).await;
                let _ = event_tx.send(BusEvent::AgentStateChanged {
                    agent_id,
                    old_state,
                    new_state: AgentState::Exited,
                });
            }

            TuiCommand::Pause { agent_id } => {
                if let Err(e) = store.set_directive(agent_id, Directive::Pause).await {
                    tracing::error!(%agent_id, %e, "failed to set pause directive");
                    continue;
                }

                let old_state = store
                    .get(agent_id)
                    .await
                    .ok()
                    .flatten()
                    .map(|r| r.state)
                    .unwrap_or(AgentState::Active);

                let _ = store.update_state(agent_id, AgentState::Paused).await;
                let _ = event_tx.send(BusEvent::AgentStateChanged {
                    agent_id,
                    old_state,
                    new_state: AgentState::Paused,
                });
            }

            TuiCommand::Resume { agent_id } => {
                if let Err(e) = store.set_directive(agent_id, Directive::Continue).await {
                    tracing::error!(%agent_id, %e, "failed to set continue directive");
                    continue;
                }

                let _ = store.update_state(agent_id, AgentState::Active).await;
                let _ = event_tx.send(BusEvent::AgentStateChanged {
                    agent_id,
                    old_state: AgentState::Paused,
                    new_state: AgentState::Active,
                });
            }

            TuiCommand::ListCheckpoints { agent_id } => {
                match store.list_versions(agent_id).await {
                    Ok(versions) => {
                        let mut summaries = Vec::new();
                        for v in versions {
                            if let Ok(Some(cp)) = store.get_version(agent_id, v).await {
                                summaries.push(CheckpointSummary {
                                    version: cp.version,
                                    timestamp: cp.timestamp,
                                    summary: cp.l1.chars().take(80).collect(),
                                });
                            }
                        }
                        let _ = event_tx.send(BusEvent::CheckpointList {
                            agent_id,
                            versions: summaries,
                        });
                    }
                    Err(e) => {
                        tracing::error!(%agent_id, %e, "failed to list checkpoints");
                        let _ = event_tx.send(BusEvent::CheckpointList {
                            agent_id,
                            versions: vec![],
                        });
                    }
                }
            }

            TuiCommand::Rollback { agent_id, version } => {
                if let Err(e) = store.set_checkpoint_version(agent_id, version).await {
                    tracing::error!(%agent_id, %e, "failed to set rollback version");
                    continue;
                }
                let _ = store.update_state(agent_id, AgentState::Restoring).await;
                let _ = event_tx.send(BusEvent::AgentStateChanged {
                    agent_id,
                    old_state: AgentState::Active,
                    new_state: AgentState::Restoring,
                });
                tracing::info!(%agent_id, ?version, "rollback initiated");
            }

            TuiCommand::HitlRespond {
                agent_id,
                response,
            } => {
                let _ = event_tx.send(BusEvent::HitlResponded {
                    agent_id,
                    response,
                });
            }
        }
    }

    tracing::debug!("command handler exiting");
}
