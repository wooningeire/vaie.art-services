use std::collections::BTreeSet;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

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

        update_repository(&path, runner)
            .with_context(|| format!("failed to update `{}`", path.display()))?;
    }

    Ok(())
}

fn update_repository(path: &Path, runner: &dyn CommandRunner) -> Result<()> {
    runner.run(&git_command(path).arg("fetch").arg("--all").arg("--prune"))?;

    let target = pull_target(path, runner)?;
    runner.run(&pull_command(path, target))?;
    ensure_no_unmerged_files(path, runner)?;

    Ok(())
}

fn ensure_no_unmerged_files(path: &Path, runner: &dyn CommandRunner) -> Result<()> {
    let files = runner.output(
        &git_command(path)
            .arg("diff")
            .arg("--name-only")
            .arg("--diff-filter")
            .arg("U"),
    )?;
    let files = files.trim();

    if files.is_empty() {
        return Ok(());
    }

    bail!(
        "autostash conflicted while updating `{}`; resolve unmerged files:\n{}",
        path.display(),
        files,
    );
}

fn pull_target(path: &Path, runner: &dyn CommandRunner) -> Result<PullTarget> {
    let branch = runner.output(&git_command(path).arg("branch").arg("--show-current"))?;

    if branch.trim().is_empty() {
        return Ok(PullTarget::OriginHead);
    }

    Ok(PullTarget::ConfiguredUpstream)
}

fn pull_command(path: &Path, target: PullTarget) -> ProcessCommand {
    let command = git_command(path)
        .arg("pull")
        .arg("--ff-only")
        .arg("--autostash");

    match target {
        PullTarget::ConfiguredUpstream => command,
        PullTarget::OriginHead => command.arg("origin").arg("HEAD"),
    }
}

fn git_command(path: &Path) -> ProcessCommand {
    ProcessCommand::new("git")
        .arg("-C")
        .arg(path.display().to_string())
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PullTarget {
    ConfiguredUpstream,
    OriginHead,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    #[test]
    fn branch_repository_pulls_configured_upstream() {
        let runner = RecordingRunner::with_outputs(["main\n", ""]);

        update_repository(Path::new("repo"), &runner).expect("update repo");

        assert_eq!(
            runner.displays(),
            vec![
                "git -C repo fetch --all --prune",
                "git -C repo branch --show-current",
                "git -C repo pull --ff-only --autostash",
                "git -C repo diff --name-only --diff-filter U",
            ],
        );
    }

    #[test]
    fn detached_repository_pulls_origin_head() {
        let runner = RecordingRunner::with_outputs(["\n", ""]);

        update_repository(Path::new("repo"), &runner).expect("update repo");

        assert_eq!(
            runner.displays(),
            vec![
                "git -C repo fetch --all --prune",
                "git -C repo branch --show-current",
                "git -C repo pull --ff-only --autostash origin HEAD",
                "git -C repo diff --name-only --diff-filter U",
            ],
        );
    }

    #[test]
    fn autostash_conflicts_report_unmerged_files() {
        let runner = RecordingRunner::with_outputs(["\n", "file.txt\nother.txt\n"]);
        let error = update_repository(Path::new("repo"), &runner)
            .expect_err("autostash conflict should fail update");
        let message = error.to_string();

        assert!(message.contains("autostash conflicted"));
        assert!(message.contains("file.txt"));
        assert!(message.contains("other.txt"));
    }

    struct RecordingRunner {
        commands: RefCell<Vec<ProcessCommand>>,
        outputs: RefCell<VecDeque<String>>,
    }

    impl RecordingRunner {
        fn with_outputs<const N: usize>(outputs: [&str; N]) -> Self {
            Self {
                commands: RefCell::new(Vec::new()),
                outputs: RefCell::new(outputs.into_iter().map(str::to_string).collect()),
            }
        }

        fn displays(&self) -> Vec<String> {
            self.commands
                .borrow()
                .iter()
                .map(ProcessCommand::display)
                .collect()
        }
    }

    impl CommandRunner for RecordingRunner {
        fn run(&self, command: &ProcessCommand) -> Result<()> {
            self.commands.borrow_mut().push(command.clone());
            Ok(())
        }

        fn output(&self, command: &ProcessCommand) -> Result<String> {
            self.commands.borrow_mut().push(command.clone());
            self.outputs
                .borrow_mut()
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("missing command output"))
        }
    }
}
