use std::collections::BTreeSet;
use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};

use super::config::ServiceMap;
use super::process::{CommandRunner, ProcessCommand, ensure_program_available};

pub fn update_repositories(map: &ServiceMap, runner: &dyn CommandRunner) -> Result<()> {
    ensure_program_available("git").context(
        "git is required for `update`; install it in the same environment that runs cargo",
    )?;

    for path in unique_repo_paths(map) {
        println!("\x1b[35mupdating {} :: \x1b[0m", path.display());
        io::stdout()
            .flush()
            .context("failed to flush update status")?;

        runner.run(
            &ProcessCommand::new("git")
                .arg("-C")
                .arg(path.display().to_string())
                .arg("fetch")
                .arg("--all")
                .arg("--prune"),
        )?;
        runner.run(
            &ProcessCommand::new("git")
                .arg("-C")
                .arg(path.display().to_string())
                .arg("pull")
                .arg("--ff-only"),
        )?;
    }

    Ok(())
}

fn unique_repo_paths(map: &ServiceMap) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    let mut paths = Vec::new();

    for service in &map.services {
        if seen.insert(service.local_path.clone()) {
            paths.push(service.local_path.clone());
        }
    }

    paths
}
