//! Global quotation search (prompt 037): scoring, bounded FTS retrieval, the
//! in-memory licensed index, the worker thread and the corpus coordinator.

pub mod corpus;
pub mod index;
pub mod matching;
pub mod retrieval;
pub mod worker;

pub use corpus::QuoteCorpus;
pub use matching::{
    GlobalQuoteCandidate, GlobalQuoteMatch, fresh_speech_supports, match_global_quote,
};
pub use worker::{QuoteSearchOutcome, QuoteWorkerClient};
