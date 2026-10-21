//! Spectra Labs experiment registry and holdout vault.
//!
//! * [`registry`]: an append-only, hash-chained log of experiments, runs,
//!   artifacts and notes, file-compatible with the C++ reference engine.
//! * [`vault`]: sealed holdout datasets with a metered, audited evaluation budget.

#[cfg(feature = "postgres")]
pub mod pg;
pub mod registry;
pub mod vault;
#[cfg(feature = "postgres")]
pub use pg::{PgRegistry, PgRegistryError, RunArtifact, RunPage, StoredArtifact, RUN_STATUSES};

pub use registry::{
    config_from_json, config_to_json, metrics_from_json, metrics_to_json, params_from_json, params_to_json,
    repair_incomplete_tail, reproduce_run, system_clock_ns, ArtifactInfo, CheckedBuilder, Clock, Event,
    ExperimentInfo, Registry, RepairReport, ReproductionReport, RunComparison, RunFilter, RunInfo, RunInput,
    VerifyReport,
};
pub use vault::{HoldoutResult, HoldoutStatus, Vault};
