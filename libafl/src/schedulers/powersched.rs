//! The queue corpus scheduler for power schedules.

use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use crate::{
    corpus::{Corpus, SchedulerTestcaseMetaData, CorpusID},
    inputs::Input,
    schedulers::Scheduler,
    state::{HasCorpus, HasMetadata},
    Error,
};
use core::time::Duration;
use serde::{Deserialize, Serialize};
/// The n fuzz size
pub const N_FUZZ_SIZE: usize = 1 << 21;

crate::impl_serdeany!(SchedulerMetadata);

/// The metadata used for power schedules
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SchedulerMetadata {
    /// Powerschedule strategy
    strat: Option<PowerSchedule>,
    /// Measured exec time during calibration
    exec_time: Duration,
    /// Calibration cycles
    cycles: u64,
    /// Size of the observer map
    bitmap_size: u64,
    /// Number of filled map entries
    bitmap_entries: u64,
    /// Queue cycles
    queue_cycles: u64,
    /// The vector to contain the frequency of each execution path.
    n_fuzz: Vec<u32>,
}

/// The metadata for runs in the calibration stage.
impl SchedulerMetadata {
    /// Creates a new [`struct@SchedulerMetadata`]
    #[must_use]
    pub fn new(strat: Option<PowerSchedule>) -> Self {
        Self {
            strat,
            exec_time: Duration::from_millis(0),
            cycles: 0,
            bitmap_size: 0,
            bitmap_entries: 0,
            queue_cycles: 0,
            n_fuzz: vec![0; N_FUZZ_SIZE],
        }
    }

    /// The powerschedule strategy
    #[must_use]
    pub fn strat(&self) -> Option<PowerSchedule> {
        self.strat
    }

    /// The measured exec time during calibration
    #[must_use]
    pub fn exec_time(&self) -> Duration {
        self.exec_time
    }

    /// Set the measured exec
    pub fn set_exec_time(&mut self, time: Duration) {
        self.exec_time = time;
    }

    /// The cycles
    #[must_use]
    pub fn cycles(&self) -> u64 {
        self.cycles
    }

    /// Sets the cycles
    pub fn set_cycles(&mut self, val: u64) {
        self.cycles = val;
    }

    /// The bitmap size
    #[must_use]
    pub fn bitmap_size(&self) -> u64 {
        self.bitmap_size
    }

    /// Sets the bitmap size
    pub fn set_bitmap_size(&mut self, val: u64) {
        self.bitmap_size = val;
    }

    /// The number of filled map entries
    #[must_use]
    pub fn bitmap_entries(&self) -> u64 {
        self.bitmap_entries
    }

    /// Sets the number of filled map entries
    pub fn set_bitmap_entries(&mut self, val: u64) {
        self.bitmap_entries = val;
    }

    /// The amount of queue cycles
    #[must_use]
    pub fn queue_cycles(&self) -> u64 {
        self.queue_cycles
    }

    /// Sets the amount of queue cycles
    pub fn set_queue_cycles(&mut self, val: u64) {
        self.queue_cycles = val;
    }

    /// Gets the `n_fuzz`.
    #[must_use]
    pub fn n_fuzz(&self) -> &[u32] {
        &self.n_fuzz
    }

    /// Sets the `n_fuzz`.
    #[must_use]
    pub fn n_fuzz_mut(&mut self) -> &mut [u32] {
        &mut self.n_fuzz
    }
}

/// The power schedule to use
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerSchedule {
    /// The `explore" power schedule
    EXPLORE,
    /// The `exploit` power schedule
    EXPLOIT,
    /// The `fast` power schedule
    FAST,
    /// The `coe` power schedule
    COE,
    /// The `lin` power schedule
    LIN,
    /// The `quad` power schedule
    QUAD,
}

/// A corpus scheduler using power schedules
#[derive(Clone, Debug)]
pub struct PowerQueueScheduler {
    strat: PowerSchedule,
}

impl<I, S> Scheduler<I, S> for PowerQueueScheduler
where
    S: HasCorpus<I> + HasMetadata,
    I: Input,
{
    /// Add an entry to the corpus and return its index
    fn on_add(&self, state: &mut S, idx: CorpusID) -> Result<(), Error> {
        if !state.has_metadata::<SchedulerMetadata>() {
            state.add_metadata::<SchedulerMetadata>(SchedulerMetadata::new(Some(self.strat)));
        }

        let current_idx = *state.corpus().current();

        let mut depth = match current_idx {
            Some(parent_idx) => state
                .corpus()
                .get(parent_idx)?
                .borrow_mut()
                .metadata_mut()
                .get_mut::<SchedulerTestcaseMetaData>()
                .ok_or_else(|| {
                    Error::key_not_found("SchedulerTestcaseMetaData not found".to_string())
                })?
                .depth(),
            None => 0,
        };

        // Attach a `SchedulerTestcaseMetaData` to the queue entry.
        depth += 1;
        state
            .corpus()
            .get(idx)?
            .borrow_mut()
            .add_metadata(SchedulerTestcaseMetaData::new(depth));
        Ok(())
    }

    fn next(&self, state: &mut S) -> Result<CorpusID, Error> {
        let first_id = state
            .corpus()
            .id_manager()
            .first_id()
            .ok_or(Error::empty(String::from("No entries in corpus")))?;

        let next_id = state
            .corpus()
            .current()
            .map(|cur| -> Result<CorpusID, Error> {
                match state.corpus().id_manager().find_next(cur) {
                    Some(next_id) => Ok(next_id),
                    None => {
                        // If we can't advance to the next corpus element, we started a new cycle
                        let psmeta = state
                            .metadata_mut()
                            .get_mut::<SchedulerMetadata>()
                            .ok_or_else(|| {
                                Error::key_not_found("SchedulerMetadata not found".to_string())
                            })?;
                        psmeta.set_queue_cycles(psmeta.queue_cycles() + 1);
                        Ok(first_id)
                    },
                }
            })
            .unwrap_or(Ok(first_id))
            ?;
        *state.corpus_mut().current_mut() = Some(next_id);

        // Update the handicap
        let mut testcase = state.corpus().get(next_id)?.borrow_mut();
        let tcmeta = testcase
            .metadata_mut()
            .get_mut::<SchedulerTestcaseMetaData>()
            .ok_or_else(|| {
                Error::key_not_found("SchedulerTestcaseMetaData not found".to_string())
            })?;

        if tcmeta.handicap() >= 4 {
            tcmeta.set_handicap(tcmeta.handicap() - 4);
        } else if tcmeta.handicap() > 0 {
            tcmeta.set_handicap(tcmeta.handicap() - 1);
        }

        Ok(next_id)
    }
}

impl PowerQueueScheduler {
    /// Create a new [`PowerQueueScheduler`]
    #[must_use]
    pub fn new(strat: PowerSchedule) -> Self {
        PowerQueueScheduler { strat }
    }
}
