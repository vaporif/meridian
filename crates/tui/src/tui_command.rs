use meridian_core::id::{AgentId, CheckpointVersion, ObjectiveId};
use std::path::PathBuf;

#[derive(Debug)]
pub enum TuiCommand {
    Spawn {
        objective_id: ObjectiveId,
        content: String,
        dir: PathBuf,
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
    ListCheckpoints {
        agent_id: AgentId,
    },
    Rollback {
        agent_id: AgentId,
        version: CheckpointVersion,
    },
    HitlRespond {
        agent_id: AgentId,
        response: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use meridian_core::id::{AgentId, CheckpointVersion, ObjectiveId};
    use std::path::PathBuf;

    #[test]
    fn tui_command_variants_constructible() {
        let oid = ObjectiveId::new();
        let aid = AgentId::new();

        let cmds = vec![
            TuiCommand::Spawn {
                objective_id: oid,
                content: "do stuff".into(),
                dir: PathBuf::from("/tmp"),
            },
            TuiCommand::Kill { agent_id: aid },
            TuiCommand::Pause { agent_id: aid },
            TuiCommand::Resume { agent_id: aid },
            TuiCommand::ListCheckpoints { agent_id: aid },
            TuiCommand::Rollback {
                agent_id: aid,
                version: CheckpointVersion(1),
            },
            TuiCommand::HitlRespond {
                agent_id: aid,
                response: "option 1".into(),
            },
        ];
        assert_eq!(cmds.len(), 7);
    }
}
