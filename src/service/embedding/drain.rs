use super::{EmbeddingService, compute_vector, map_storage};
use crate::{
    domain::{KnowledgeId, Sha256},
    service::{
        DrainOutcome, EmbeddingConfig, EmbeddingError, EmbeddingErrorKind, EmbeddingFiles,
        EmbeddingProvider, EmbeddingResult, EmbeddingWork, MarkdownCodec, Storage, StorageRead,
        VectorOutcome,
    },
};
use std::{
    any::Any,
    collections::BTreeSet,
    fmt,
    num::NonZeroUsize,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
};
use threadpool::ThreadPool;

/// Bounds worker threads and all submitted results until coordinator handling finishes.
const DRAIN_WORKERS: usize = 4;

type WorkerOutcome = Result<EmbeddingResult<Vec<f32>>, Box<dyn Any + Send>>;

struct Completion {
    hash: Sha256,
    outcome: WorkerOutcome,
}

struct DrainJobs {
    pool: ThreadPool,
    provider: Arc<dyn EmbeddingProvider + Send + Sync>,
    sender: Sender<Completion>,
    receiver: Receiver<Completion>,
    pending: Vec<Sha256>,
    panic: Option<Box<dyn Any + Send>>,
}

impl<S: Storage, F: EmbeddingFiles, M: MarkdownCodec> EmbeddingService<S, F, M> {
    /// Run bounded inference and join every admitted job before normal return.
    ///
    /// # Errors
    /// Returns the original operational failure after outstanding work is joined.
    ///
    /// # Panics
    /// Propagates worker startup or programming failures; a worker panic remains
    /// a panic even if it is first observed during operational-error cleanup.
    pub(super) fn drain_ready(
        &mut self,
        config: &EmbeddingConfig,
    ) -> EmbeddingResult<DrainOutcome> {
        let mut jobs = DrainJobs::new(Arc::clone(&self.provider));
        // Unwinding must join outstanding HTTP work before the caller's lock drops.
        let outcome = catch_unwind(AssertUnwindSafe(|| self.run_jobs(config, &mut jobs)));
        let worker_panic = jobs.finish();
        let result = match outcome {
            Ok(result) => result,
            Err(payload) => resume_unwind(payload),
        };
        // A programming panic must not become an ordinary provider/storage error.
        if let Some(payload) = worker_panic {
            resume_unwind(payload);
        }
        result
    }

    fn run_jobs(
        &mut self,
        config: &EmbeddingConfig,
        jobs: &mut DrainJobs,
    ) -> EmbeddingResult<DrainOutcome> {
        let mut skipped = BTreeSet::<KnowledgeId>::new();
        let mut after = None;
        loop {
            let next = if jobs.has_capacity() {
                self.select_work(config, after.as_ref(), &jobs.pending)?
            } else {
                VectorOutcome::Current(None)
            };
            let work = match next {
                VectorOutcome::ConfigChanged => return Ok(DrainOutcome::ConfigChanged),
                VectorOutcome::Current(work) => work,
            };
            if let Some(work) = work {
                if skipped.contains(&work.id) {
                    after = Some(work.id);
                    continue;
                }
                let prepared = match self.prepare_text(&work) {
                    Ok(None) => {
                        after = Some(work.id.clone());
                        skipped.insert(work.id);
                        continue;
                    }
                    Ok(Some(text)) => Ok(text),
                    Err(error) => Err(error),
                };
                if self.start_work(config, &work, prepared, jobs)? == DrainOutcome::ConfigChanged {
                    return Ok(DrainOutcome::ConfigChanged);
                }
                after = None;
                continue;
            }
            if jobs.pending.is_empty() {
                return self.prune(config);
            }
            if self.consume_job(config, jobs)? == DrainOutcome::ConfigChanged {
                return Ok(DrainOutcome::ConfigChanged);
            }
            after = None;
        }
    }

    fn select_work(
        &mut self,
        config: &EmbeddingConfig,
        after: Option<&KnowledgeId>,
        excluded: &[Sha256],
    ) -> EmbeddingResult<VectorOutcome<Option<EmbeddingWork>>> {
        self.storage
            .begin_read()
            .map_err(map_storage)?
            .read_next_embedding_work(
                &config.get_identity(),
                config.get_max_attempts(),
                after,
                excluded,
            )
            .map_err(map_storage)
    }

    fn start_work(
        &mut self,
        config: &EmbeddingConfig,
        work: &EmbeddingWork,
        text: EmbeddingResult<String>,
        jobs: &mut DrainJobs,
    ) -> EmbeddingResult<DrainOutcome> {
        let VectorOutcome::Current(dimensions) = self.read_dimensions(config)? else {
            return Ok(DrainOutcome::ConfigChanged);
        };
        match self.reserve(config, &work.search_hash)? {
            VectorOutcome::ConfigChanged => return Ok(DrainOutcome::ConfigChanged),
            VectorOutcome::Current(false) => return Ok(DrainOutcome::Complete),
            VectorOutcome::Current(true) => {}
        }
        #[cfg(test)]
        super::tests::reach_checkpoint("reserved");
        let text = match text {
            Ok(text) => text,
            Err(error) => return self.finish_failure(config, &work.search_hash, &error),
        };
        jobs.submit(config, work.search_hash, text, dimensions);
        Ok(DrainOutcome::Complete)
    }

    fn consume_job(
        &mut self,
        config: &EmbeddingConfig,
        jobs: &mut DrainJobs,
    ) -> EmbeddingResult<DrainOutcome> {
        let completion = jobs.receiver.recv().map_err(|cause| {
            EmbeddingError::new_with_cause(EmbeddingErrorKind::InternalError, cause)
        })?;
        let vector = match completion.outcome {
            Ok(vector) => vector,
            Err(payload) => {
                jobs.panic = Some(payload);
                return Err(EmbeddingError::new(EmbeddingErrorKind::InternalError));
            }
        };
        let outcome = match vector {
            Ok(vector) => self.publish_completion(config, &completion.hash, &vector)?,
            Err(error) => self.finish_failure(config, &completion.hash, &error)?,
        };
        jobs.pending.retain(|hash| *hash != completion.hash);
        Ok(outcome)
    }

    fn publish_completion(
        &mut self,
        config: &EmbeddingConfig,
        hash: &Sha256,
        vector: &[f32],
    ) -> EmbeddingResult<DrainOutcome> {
        match self.publish_vector(config, hash, vector) {
            Ok(VectorOutcome::ConfigChanged) => Ok(DrainOutcome::ConfigChanged),
            Ok(VectorOutcome::Current(())) => Ok(DrainOutcome::Complete),
            Err(error) if error.get_kind() == EmbeddingErrorKind::InvalidResponse => {
                self.finish_failure(config, hash, &error)
            }
            Err(error) => Err(error),
        }
    }
}

impl fmt::Debug for DrainJobs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DrainJobs")
            .field("pending", &self.pending)
            .finish_non_exhaustive()
    }
}

impl DrainJobs {
    fn new(provider: Arc<dyn EmbeddingProvider + Send + Sync>) -> Self {
        let pool = ThreadPool::new(DRAIN_WORKERS);
        let (sender, receiver) = mpsc::channel();
        Self {
            pool,
            provider,
            sender,
            receiver,
            pending: Vec::new(),
            panic: None,
        }
    }

    fn has_capacity(&self) -> bool {
        self.pending.len() < DRAIN_WORKERS
    }

    fn submit(
        &mut self,
        config: &EmbeddingConfig,
        hash: Sha256,
        text: String,
        dimensions: Option<NonZeroUsize>,
    ) {
        let provider = Arc::clone(&self.provider);
        let config = config.clone();
        let sender = self.sender.clone();
        self.pending.push(hash);
        self.pool.execute(move || {
            // The panic is propagated after joining; no provider state is recovered.
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                compute_vector(provider.as_ref(), &config, &text, dimensions)
            }));
            let _ = sender.send(Completion { hash, outcome });
        });
    }

    fn finish(&mut self) -> Option<Box<dyn Any + Send>> {
        self.pool.join();
        while let Ok(completion) = self.receiver.try_recv() {
            if let Err(payload) = completion.outcome {
                self.panic.get_or_insert(payload);
            }
        }
        self.panic.take()
    }
}

impl Drop for DrainJobs {
    fn drop(&mut self) {
        self.pool.join();
    }
}

#[cfg(test)]
mod tests {
    use super::DrainJobs;
    use crate::{
        domain::Sha256,
        service::{EmbeddingConfig, EmbeddingProvider, EmbeddingResult},
    };
    use std::{num::NonZeroUsize, sync::Arc};

    #[test]
    fn completed_results_hold_capacity_until_coordinator_handling_finishes() {
        // Given: four real worker jobs whose results will all be ready before consumption.
        let config = EmbeddingConfig::parse(br#"{"provider":"ollama","model":"fixture","parameters":{"base_url":"http://127.0.0.1/api"},"max_retries":0}"#).unwrap();
        let mut jobs = DrainJobs::new(Arc::new(ReadyProvider));
        for id in 0..4 {
            jobs.submit(&config, Sha256::digest(&[id]), "fixture text".into(), None);
        }
        // When: workers finish and all messages are received without publication acknowledgment.
        jobs.pool.join();
        for _ in 0..4 {
            let completion = jobs.receiver.recv().unwrap();
            let vector = completion
                .outcome
                .map_err(|_| "unexpected panic")
                .unwrap()
                .unwrap();
            assert_eq!(vector, vec![1., 2., 3.]);
        }
        // Then: completed/received results still own every slot until outcome handling ends.
        assert!(
            !jobs.has_capacity(),
            "completed but unpublished work must retain its slot"
        );
    }

    struct ReadyProvider;
    impl EmbeddingProvider for ReadyProvider {
        fn validate_config(&self, _: &EmbeddingConfig) -> EmbeddingResult<()> {
            Ok(())
        }
        fn embed(
            &self,
            _: &EmbeddingConfig,
            _: &[&str],
            _: Option<NonZeroUsize>,
        ) -> EmbeddingResult<Vec<Vec<f32>>> {
            Ok(vec![vec![1., 2., 3.]])
        }
    }
}
