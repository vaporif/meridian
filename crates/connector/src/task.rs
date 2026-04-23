use std::path::PathBuf;
use std::sync::Arc;

use nephila_core::id::AgentId;
use serde::{Deserialize, Serialize};
use tokio::process::Child;
use tokio::sync::Mutex;

use crate::config::SpawnConfig;
use crate::error::ConnectorError;
use crate::types::Usage;

pub trait TaskConnector: Send + Sync {
    fn spawn(
        &self,
        agent_id: AgentId,
        config: &SpawnConfig,
        prompt: &str,
        session_id: &str,
    ) -> impl std::future::Future<Output = Result<(TaskHandle, ProcessHandle), ConnectorError>> + Send;

    fn status(
        &self,
        handle: &ProcessHandle,
    ) -> impl std::future::Future<Output = Result<TaskStatus, ConnectorError>> + Send;

    fn kill(
        &self,
        handle: &ProcessHandle,
    ) -> impl std::future::Future<Output = Result<(), ConnectorError>> + Send;

    fn wait(
        &self,
        handle: &ProcessHandle,
    ) -> impl std::future::Future<Output = Result<TaskResult, ConnectorError>> + Send;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TaskHandle {
    ClaudeCode {
        session_id: String,
        directory: PathBuf,
    },
    Api {
        conversation_id: String,
    },
}

#[derive(Debug, Clone)]
pub enum TaskStatus {
    Running,
    Completed(TaskResult),
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct TaskResult {
    pub output: String,
    pub usage: Option<Usage>,
}

#[derive(Clone)]
pub struct ProcessHandle {
    child: Arc<Mutex<Option<Child>>>,
}

impl ProcessHandle {
    pub fn new(child: Child) -> Self {
        Self {
            child: Arc::new(Mutex::new(Some(child))),
        }
    }

    pub async fn is_running(&self) -> bool {
        let mut guard = self.child.lock().await;
        match guard.as_mut() {
            Some(child) => child.try_wait().ok().flatten().is_none(),
            None => false,
        }
    }

    /// Send SIGTERM, wait up to 5s, then SIGKILL if still alive.
    pub async fn kill(&self) -> Result<(), ConnectorError> {
        let mut guard = self.child.lock().await;
        let child = match guard.take() {
            Some(c) => c,
            None => return Ok(()),
        };
        Self::kill_child(child).await
    }

    async fn kill_child(mut child: Child) -> Result<(), ConnectorError> {
        let pid = match child.id() {
            Some(pid) => pid,
            None => return Ok(()), // already exited
        };

        // SAFETY: pid is valid (we just got it from the child) and SIGTERM is a standard signal.
        unsafe {
            libc::kill(pid as i32, libc::SIGTERM);
        }

        match tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await {
            Ok(_) => return Ok(()),
            Err(_) => {
                tracing::warn!(pid, "process did not exit after SIGTERM, sending SIGKILL");
            }
        }

        child.kill().await.map_err(|e| ConnectorError::Process {
            exit_code: None,
            stderr: format!("failed to SIGKILL process: {e}"),
        })?;
        let _ = child.wait().await;
        Ok(())
    }

    pub async fn wait(&self) -> Result<TaskResult, ConnectorError> {
        let mut guard = self.child.lock().await;
        let child = match guard.take() {
            Some(c) => c,
            None => {
                return Ok(TaskResult {
                    output: String::new(),
                    usage: None,
                });
            }
        };

        let output = child
            .wait_with_output()
            .await
            .map_err(|e| ConnectorError::Process {
                exit_code: None,
                stderr: format!("wait failed: {e}"),
            })?;

        Ok(TaskResult {
            output: String::from_utf8_lossy(&output.stdout).into_owned(),
            usage: None,
        })
    }

    #[cfg(test)]
    pub fn noop() -> Self {
        Self {
            child: Arc::new(Mutex::new(None)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_handle_claude_code_serializes() {
        let handle = TaskHandle::ClaudeCode {
            session_id: "sess-123".into(),
            directory: PathBuf::from("/tmp/work"),
        };
        let json = serde_json::to_string(&handle).expect("serialize");
        let decoded: TaskHandle = serde_json::from_str(&json).expect("deserialize");
        assert!(matches!(decoded, TaskHandle::ClaudeCode { .. }));
    }

    #[test]
    fn task_handle_api_serializes() {
        let handle = TaskHandle::Api {
            conversation_id: "conv-456".into(),
        };
        let json = serde_json::to_string(&handle).expect("serialize");
        assert!(json.contains("conv-456"));
    }

    #[test]
    fn task_result_with_usage() {
        let result = TaskResult {
            output: "done".into(),
            usage: Some(Usage {
                input_tokens: 100,
                output_tokens: 50,
            }),
        };
        assert_eq!(result.output, "done");
        assert_eq!(result.usage.expect("has usage").input_tokens, 100);
    }

    #[test]
    fn task_result_without_usage() {
        let result = TaskResult {
            output: "done".into(),
            usage: None,
        };
        assert!(result.usage.is_none());
    }

    #[tokio::test]
    async fn process_handle_noop_reports_not_running() {
        let handle = ProcessHandle::noop();
        assert!(!handle.is_running().await);
    }

    #[tokio::test]
    async fn process_handle_noop_kill_is_ok() {
        let handle = ProcessHandle::noop();
        assert!(handle.kill().await.is_ok());
    }

    #[tokio::test]
    async fn process_handle_noop_wait_returns_empty() {
        let handle = ProcessHandle::noop();
        let result = handle.wait().await.expect("wait on noop");
        assert!(result.output.is_empty());
        assert!(result.usage.is_none());
    }

    #[tokio::test]
    async fn process_handle_kill_terminates_child() {
        let child = tokio::process::Command::new("sleep")
            .arg("60")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .stdin(std::process::Stdio::null())
            .spawn()
            .expect("spawn sleep");

        let handle = ProcessHandle::new(child);
        assert!(handle.is_running().await);

        handle.kill().await.expect("kill should succeed");
        assert!(!handle.is_running().await);
    }

    #[tokio::test]
    async fn process_handle_wait_collects_output() {
        let child = tokio::process::Command::new("echo")
            .arg("hello from test")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::null())
            .spawn()
            .expect("spawn echo");

        let handle = ProcessHandle::new(child);
        let result = handle.wait().await.expect("wait should succeed");
        assert!(result.output.contains("hello from test"));
    }
}
