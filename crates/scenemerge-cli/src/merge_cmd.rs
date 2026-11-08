//! `merge` command (implemented in a later milestone).

use std::path::PathBuf;

use anyhow::Result;

/// `scenemerge merge`
#[derive(Debug, clap::Args)]
pub struct MergeArgs {
    /// Common ancestor.
    pub base: PathBuf,
    /// Our version.
    pub ours: PathBuf,
    /// Their version.
    pub theirs: PathBuf,
}

pub fn run(_args: &MergeArgs) -> Result<u8> {
    anyhow::bail!("merge is not implemented yet")
}
