pub mod agent;
pub mod channel;
pub mod checkpoint;
pub mod command;
pub mod config;
pub mod directive;
pub mod error;
pub mod event;
pub mod id;
pub mod interrupt;
pub mod memory;
pub mod objective;
#[cfg(feature = "rusqlite")]
mod sql;
pub mod store;
pub mod summarizer;

pub use error::{NephilaError, Result};
pub use id::*;

pub mod ferrex_types {
    pub use ferrex_core::{
        CoreError, EmbedError, Embedder, FerrexConfig, Filter, FreshnessLabel, Memory,
        MemoryService, MemoryType, ModelTier, Payload, Reranker, RerankerTier, ScoringBreakdown,
        VectorStore,
    };
    pub use ferrex_core::{
        ForgetRequest, ForgetResponse, RecallRequest, RecallResult, ReflectRequest,
        ReflectResponse, StoreRequest, StoreResponse,
    };
}
