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

    for path in unique_repo_roots(map, runner)? {
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
    runner.run(&reset_origin_head_command(path))?;

    Ok(())
}

fn reset_origin_head_command(path: &Path) -> ProcessCommand {
    git_command(path)
        .arg("reset")
        .arg("--hard")
        .arg("origin/HEAD")
}

fn git_command(path: &Path) -> ProcessCommand {
    ProcessCommand::new("git")
        .arg("-C")
        .arg(path.display().to_string())
}

fn unique_repo_roots(map: &ServiceMap, runner: &dyn CommandRunner) -> Result<Vec<PathBuf>> {
    let mut paths = map
        .services
        .iter()
        .map(|service| service.local_path.as_path())
        .collect::<Vec<_>>();

    if let Some(pocketbase) = &map.pocketbase {
        paths.push(pocketbase.source_path.as_path());
    }

    unique_roots_for_paths(paths, runner)
}

fn unique_roots_for_paths<'a>(
    paths: impl IntoIterator<Item = &'a Path>,
    runner: &dyn CommandRunner,
) -> Result<Vec<PathBuf>> {
    let mut seen = BTreeSet::new();
    let mut roots = Vec::new();

    for path in paths {
        let root = repo_root_for_path(path, runner)
            .with_context(|| format!("failed to find git repository for `{}`", path.display()))?;

        if seen.insert(root.clone()) {
            roots.push(root);
        }
    }

    Ok(roots)
}

fn repo_root_for_path(path: &Path, runner: &dyn CommandRunner) -> Result<PathBuf> {
    let root = runner.output(&git_command(path).arg("rev-parse").arg("--show-toplevel"))?;
    let root = root.trim();

    if root.is_empty() {
        bail!("git repository root was empty for `{}`", path.display());
    }

    Ok(PathBuf::from(root))
}

#[cfg(test)]
mod tests {
    use super::super::config::{
        CaddyConfig, RemoteConfig, ResolvedPocketBase, ResolvedService, ResolvedServiceKind,
    };
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    #[test]
    fn repository_resets_to_origin_head() {
        let runner = RecordingRunner::new();

        update_repository(Path::new("repo"), &runner).expect("update repo");

        assert_eq!(
            runner.displays(),
            vec![
                "git -C repo fetch --all --prune",
                "git -C repo reset --hard origin/HEAD",
            ],
        );
    }

    #[test]
    fn reset_origin_head_command_matches_manual_update() {
        assert_eq!(
            reset_origin_head_command(Path::new("repo")).display(),
            "git -C repo reset --hard origin/HEAD",
        );
    }

    #[test]
    fn service_paths_resolve_to_unique_repo_roots() {
        let runner = RecordingRunner::with_outputs(["repo\n", "repo\n", "other\n"]);

        let roots = unique_roots_for_paths(
            [
                Path::new("repo/web"),
                Path::new("repo"),
                Path::new("other/app"),
            ],
            &runner,
        )
        .expect("repo roots");

        assert_eq!(roots, vec![PathBuf::from("repo"), PathBuf::from("other")]);
        assert_eq!(
            runner.displays(),
            vec![
                "git -C repo/web rev-parse --show-toplevel",
                "git -C repo rev-parse --show-toplevel",
                "git -C other/app rev-parse --show-toplevel",
            ],
        );
    }

    #[test]
    fn update_repository_does_not_need_captured_output() {
        let runner = RecordingRunner::new();

        update_repository(Path::new("repo"), &runner).expect("update repo");
        assert_eq!(runner.outputs_requested(), 0);
    }

    #[test]
    fn pocketbase_source_path_resolves_to_repo_root() {
        let map = service_map(["service/web"], Some("pb.vaie.art"));
        let runner = RecordingRunner::with_outputs(["service\n", "pb.vaie.art\n"]);

        let roots = unique_repo_roots(&map, &runner).expect("repo roots");

        assert_eq!(
            roots,
            vec![PathBuf::from("service"), PathBuf::from("pb.vaie.art")],
        );
        assert_eq!(
            runner.displays(),
            vec![
                "git -C service/web rev-parse --show-toplevel",
                "git -C pb.vaie.art rev-parse --show-toplevel",
            ],
        );
    }

    fn service_map<const N: usize>(
        service_paths: [&str; N],
        pocketbase_path: Option<&str>,
    ) -> ServiceMap {
        ServiceMap {
            root: PathBuf::from("."),
            remote: remote_config(),
            caddy: CaddyConfig {
                primary_host: "vaie.art".to_string(),
                www_redirect_host: None,
            },
            pocketbase: pocketbase_path.map(pocketbase_source),
            services: service_paths.into_iter().map(static_service).collect(),
        }
    }

    fn remote_config() -> RemoteConfig {
        RemoteConfig {
            host: None,
            user: None,
            port: 22,
            identity_file: None,
            extra_ssh_args: Vec::new(),
            ssh_program: "ssh".to_string(),
            rsync_program: "rsync".to_string(),
            tmp_dir: "/tmp/vaieart-services".to_string(),
            caddyfile_path: "/etc/caddy/Caddyfile".to_string(),
            systemd_dir: "/etc/systemd/system".to_string(),
            managed_prefix: "vaieart-".to_string(),
            deno_bin: "deno".to_string(),
        }
    }

    fn static_service(path: &str) -> ResolvedService {
        ResolvedService {
            name: "service".to_string(),
            kind: ResolvedServiceKind::StaticSite,
            local_path: PathBuf::from(path),
            remote_path: "/srv/service".to_string(),
            sync_source: PathBuf::from("."),
            host: "vaie.art".to_string(),
            route_path: "/".to_string(),
            build: None,
        }
    }

    fn pocketbase_source(path: &str) -> ResolvedPocketBase {
        ResolvedPocketBase {
            name: "pb".to_string(),
            host: "pb.vaie.art".to_string(),
            source_path: PathBuf::from(path),
            remote_path: "/srv/vaieart-pocketbase".to_string(),
            data_dir: "/var/lib/vaieart-pocketbase/pb_data".to_string(),
            backup_dir: None,
            port: 8090,
            binary: "/opt/pocketbase/pocketbase".to_string(),
            service_name: "vaieart-pb.service".to_string(),
            environment_file: None,
            request_body_max_size: "25MB".to_string(),
            read_timeout: "360s".to_string(),
            encryption_env: None,
        }
    }
    struct RecordingRunner {
        commands: RefCell<Vec<ProcessCommand>>,
        outputs: RefCell<VecDeque<String>>,
        outputs_requested: RefCell<usize>,
    }

    impl RecordingRunner {
        fn new() -> Self {
            Self::with_outputs([])
        }

        fn with_outputs<const N: usize>(outputs: [&str; N]) -> Self {
            Self {
                commands: RefCell::new(Vec::new()),
                outputs: RefCell::new(outputs.into_iter().map(str::to_string).collect()),
                outputs_requested: RefCell::new(0),
            }
        }

        fn displays(&self) -> Vec<String> {
            self.commands
                .borrow()
                .iter()
                .map(ProcessCommand::display)
                .collect()
        }

        fn outputs_requested(&self) -> usize {
            *self.outputs_requested.borrow()
        }
    }

    impl CommandRunner for RecordingRunner {
        fn run(&self, command: &ProcessCommand) -> Result<()> {
            self.commands.borrow_mut().push(command.clone());
            Ok(())
        }

        fn output(&self, command: &ProcessCommand) -> Result<String> {
            self.commands.borrow_mut().push(command.clone());
            *self.outputs_requested.borrow_mut() += 1;
            self.outputs
                .borrow_mut()
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("missing command output"))
        }
    }
}
