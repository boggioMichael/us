//! Game knowledge with provenance.
//!
//! [`KnowledgeGraph`] holds facts (each with its source, retrieval date, game
//! version, confidence, kind and spoiler level) and the nodes and relations
//! read from them. [`ResearchAgent`] finds facts when Syrup notices it does
//! not know something, and [`ResearchWorker`] runs it off the realtime loop.
//! Fetching goes through [`fetch::Fetcher`]: `curl` for real, recorded
//! responses in tests, nothing when research is off.

pub mod fetch;
pub mod graph;
pub mod research;

use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

pub use fetch::{Cached, Curl, Fetcher, Fixtures, Offline};
pub use graph::{KnowledgeGraph, ResearchRecord};
pub use research::{ResearchAgent, ResearchOutcome, ResearchQuestion, apply};

/// Research on a thread of its own: questions in, outcomes out, the loop never waits.
pub struct ResearchWorker {
    tx: Sender<ResearchQuestion>,
    rx: Receiver<(ResearchQuestion, ResearchOutcome)>,
    pending: usize,
}

impl ResearchWorker {
    pub fn start(fetcher: Arc<dyn Fetcher>) -> Self {
        let (tx, jobs) = channel::<ResearchQuestion>();
        let (done, rx) = channel();
        std::thread::Builder::new()
            .name("syrup-research".into())
            .spawn(move || {
                let agent = ResearchAgent::new(fetcher);
                for q in jobs {
                    let outcome = agent.research(&q);
                    if done.send((q, outcome)).is_err() {
                        break;
                    }
                }
            })
            .ok();
        ResearchWorker { tx, rx, pending: 0 }
    }

    pub fn ask(&mut self, q: ResearchQuestion) {
        if self.tx.send(q).is_ok() {
            self.pending += 1;
        }
    }

    /// Finished research, if any.
    pub fn poll(&mut self) -> Vec<(ResearchQuestion, ResearchOutcome)> {
        let done: Vec<_> = self.rx.try_iter().collect();
        self.pending = self.pending.saturating_sub(done.len());
        done
    }

    /// Waits for everything asked so far (for tools and tests).
    pub fn wait(&mut self, timeout: std::time::Duration) -> Vec<(ResearchQuestion, ResearchOutcome)> {
        let deadline = std::time::Instant::now() + timeout;
        let mut out = Vec::new();
        while self.pending > 0 {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(r) => {
                    self.pending -= 1;
                    out.push(r);
                }
                Err(_) => break,
            }
        }
        out
    }

    pub fn pending(&self) -> usize {
        self.pending
    }
}

#[cfg(test)]
mod tests;
