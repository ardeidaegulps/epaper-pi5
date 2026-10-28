//! Model: holds the application state only.
use serde::Deserialize;
use time::OffsetDateTime;

pub const REPORT_PATHS: [&str; 3] = [
    "/massdata/ota/update-report-pending.toml.tmp",
    "/massdata/ota/update-report-pending.toml",
    "/massdata/ota/update-report.toml",
];

#[derive(Clone, Debug, Default)]
pub struct Model {
    pub report: Option<UpdateReport>,
    pub service_status: String,
    pub ethernet_status: String,
    pub next_refresh: u64,
    pub pending_refresh: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct UpdateReport {
    pub fingerprint: String,
    pub passed: u64,
    pub failed: u64,
    pub runs: Vec<UpdateRun>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct UpdateRun {
    #[serde(with = "toml_datetime_compat")]
    pub update_start: OffsetDateTime,
    #[serde(with = "toml_datetime_compat")]
    pub update_end: OffsetDateTime,
    pub passed: bool,
    pub reason: Option<String>,
}
