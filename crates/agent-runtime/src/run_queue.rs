use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use thiserror::Error;

use crate::{AgentRunRecord, AgentRunStore, AgentRunStoreError, DurableAgentRunStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRunLease {
    pub run_id: String,
    pub leased_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Error)]
pub enum AgentRunQueueError {
    #[error("agent run lease not found: {run_id}")]
    LeaseNotFound { run_id: String },

    #[error("agent run queue store error: {0}")]
    Store(#[from] AgentRunStoreError),
}

pub type AgentRunQueueResult<T> = Result<T, AgentRunQueueError>;

#[derive(Clone)]
pub struct AgentRunQueue<S>
where
    S: AgentRunStore,
{
    store: Arc<S>,
    lease_timeout: Duration,
}

impl<S> AgentRunQueue<S>
where
    S: AgentRunStore,
{
    pub fn new(store: Arc<S>, lease_timeout: Duration) -> Self {
        Self {
            store,
            lease_timeout,
        }
    }

    pub fn store(&self) -> &Arc<S> {
        &self.store
    }

    pub async fn enqueue(&self, record: AgentRunRecord) -> AgentRunQueueResult<()> {
        self.store.insert(record).await?;
        Ok(())
    }

    pub async fn lease(&self) -> AgentRunQueueResult<Option<AgentRunLease>> {
        let queued = self
            .store
            .list()
            .await?
            .into_iter()
            .filter(|record| record.status == DurableAgentRunStatus::Queued)
            .min_by(|left, right| {
                left.created_at
                    .cmp(&right.created_at)
                    .then_with(|| left.run_id.cmp(&right.run_id))
            });

        let Some(record) = queued else {
            return Ok(None);
        };

        self.store
            .transition(&record.run_id, DurableAgentRunStatus::Running)
            .await?;
        Ok(Some(self.lease_for(record.run_id)))
    }

    pub async fn ack(&self, run_id: &str) -> AgentRunQueueResult<()> {
        self.store
            .transition(run_id, DurableAgentRunStatus::Completed)
            .await?;
        Ok(())
    }

    pub async fn nack(&self, run_id: &str) -> AgentRunQueueResult<()> {
        self.store
            .transition(run_id, DurableAgentRunStatus::Queued)
            .await?;
        Ok(())
    }

    pub async fn recover_expired_leases(&self) -> AgentRunQueueResult<Vec<String>> {
        let now = Utc::now();
        let mut recovered = Vec::new();
        for record in self.store.list().await? {
            if record.status != DurableAgentRunStatus::Running {
                continue;
            }
            let expires_at = record.updated_at
                + chrono::Duration::from_std(self.lease_timeout)
                    .unwrap_or_else(|_| chrono::Duration::seconds(i64::MAX));
            if expires_at <= now {
                self.store
                    .transition(&record.run_id, DurableAgentRunStatus::Queued)
                    .await?;
                recovered.push(record.run_id);
            }
        }
        recovered.sort();
        Ok(recovered)
    }

    pub async fn active_leases(&self) -> AgentRunQueueResult<Vec<AgentRunLease>> {
        let leases = self
            .store
            .list()
            .await?
            .into_iter()
            .filter(|record| record.status == DurableAgentRunStatus::Running)
            .map(|record| self.lease_from_record(record))
            .collect::<BTreeMap<_, _>>()
            .into_values()
            .collect();
        Ok(leases)
    }

    pub async fn queued_run_ids(&self) -> AgentRunQueueResult<VecDeque<String>> {
        let mut run_ids = self
            .store
            .list()
            .await?
            .into_iter()
            .filter(|record| record.status == DurableAgentRunStatus::Queued)
            .map(|record| (record.created_at, record.run_id))
            .collect::<Vec<_>>();
        run_ids.sort();
        Ok(run_ids.into_iter().map(|(_, run_id)| run_id).collect())
    }

    fn lease_for(&self, run_id: String) -> AgentRunLease {
        let leased_at = Utc::now();
        AgentRunLease {
            run_id,
            leased_at,
            expires_at: leased_at
                + chrono::Duration::from_std(self.lease_timeout)
                    .unwrap_or_else(|_| chrono::Duration::seconds(i64::MAX)),
        }
    }

    fn lease_from_record(&self, record: AgentRunRecord) -> (String, AgentRunLease) {
        let expires_at = record.updated_at
            + chrono::Duration::from_std(self.lease_timeout)
                .unwrap_or_else(|_| chrono::Duration::seconds(i64::MAX));
        (
            record.run_id.clone(),
            AgentRunLease {
                run_id: record.run_id,
                leased_at: record.updated_at,
                expires_at,
            },
        )
    }
}
