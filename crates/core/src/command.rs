use crate::directive::Directive;
use crate::id::{AgentId, CheckpointId, ObjectiveId};
use std::path::PathBuf;

#[derive(Debug)]
pub enum OrchestratorCommand {
    Spawn {
        objective_id: ObjectiveId,
        content: String,
        dir: PathBuf,
    },
    SpawnAgent {
        objective_id: ObjectiveId,
        content: String,
        dir: PathBuf,
        spawned_by: AgentId,
    },
    Kill {
        agent_id: AgentId,
    },
    Pause {
        agent_id: AgentId,
    },
    Resume {
        agent_id: AgentId,
    },
    Suspend {
        agent_id: AgentId,
    },
    HitlRespond {
        agent_id: AgentId,
        response: String,
    },
    TokenThreshold {
        agent_id: AgentId,
        directive: Directive,
    },
    AgentExited {
        agent_id: AgentId,
        success: bool,
    },
    Respawn {
        objective_id: ObjectiveId,
        content: String,
        dir: PathBuf,
        restore_checkpoint_id: CheckpointId,
    },
}
