pub mod analysis;
pub mod budget;
pub mod evidence;
pub mod fetch;
pub mod hitgap;
pub mod jobs;
pub mod model;
pub mod number_rank;
pub mod operations;
pub mod plan;
pub mod prizes;
pub mod replacements;
pub mod stability;
pub mod store;
pub mod structure_audit;
pub mod web;

pub fn digest(value: &impl serde::Serialize) -> anyhow::Result<String> {
    use sha2::{Digest, Sha256};
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(value)?)))
}

pub mod optimizer;
pub mod research;
pub mod reviews;
