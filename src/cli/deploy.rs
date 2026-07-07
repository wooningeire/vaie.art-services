use std::path::Path;

use anyhow::{Context, Result};

use super::config::{ResolvedService, ServiceMap};
use super::process::{
    CommandRunner, ProcessCommand, ensure_command_can_start, ensure_program_available,
};
use super::render::{RenderedPaths, write_artifacts};

mod install_script;
mod remote;

#[cfg(test)]
mod tests;

use install_script::{append_pocketbase_preflight, install_script};
use remote::{
    pocketbase_rsync_command, remote_child, remote_rsync_target, remote_ssh_target, rsync_command,
    sh_quote, ssh_command,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeploymentCommandList {
    pub commands: Vec<ProcessCommand>,
}

pub fn deploy(map: &ServiceMap, output_dir: &Path, runner: &dyn CommandRunner) -> Result<()> {
    ensure_deploy_programs_available(map)?;

    let rendered_paths = write_artifacts(map, output_dir)?;

    run_deployment_commands(map, &rendered_paths, runner)
}

pub(super) fn deploy_rendered(
    map: &ServiceMap,
    rendered_paths: &RenderedPaths,
    runner: &dyn CommandRunner,
) -> Result<()> {
    ensure_deploy_programs_available(map)?;

    run_deployment_commands(map, rendered_paths, runner)
}

fn run_deployment_commands(
    map: &ServiceMap,
    rendered_paths: &RenderedPaths,
    runner: &dyn CommandRunner,
) -> Result<()> {
    let plan = build_deployment_command_list(map, rendered_paths)?;

    for command in &plan.commands {
        runner.run(command)?;
    }

    Ok(())
}

pub fn build_deployment_command_list_for_output_dir(
    map: &ServiceMap,
    output_dir: &Path,
) -> Result<DeploymentCommandList> {
    let rendered_paths = RenderedPaths {
        caddyfile: output_dir.join("Caddyfile"),
        systemd_dir: output_dir.join("systemd"),
    };

    build_deployment_command_list(map, &rendered_paths)
}

pub fn build_deployment_command_list(
    map: &ServiceMap,
    rendered_paths: &RenderedPaths,
) -> Result<DeploymentCommandList> {
    let mut commands = Vec::new();
    let ssh_target = remote_ssh_target(&map.remote)?;
    let rsync_target = remote_rsync_target(&map.remote)?;

    let mut prepare_remote_script = format!(
        "set -eu\nmkdir -p {} {} {}",
        sh_quote(&remote_child(&map.remote.tmp_dir, "sync")),
        sh_quote(&remote_child(&map.remote.tmp_dir, "systemd")),
        sh_quote(&remote_child(&map.remote.tmp_dir, "backups")),
    );

    if let Some(pocketbase) = &map.pocketbase {
        prepare_remote_script.push('\n');
        append_pocketbase_preflight(&mut prepare_remote_script, pocketbase);
    }

    commands.push(ssh_command(
        &map.remote,
        &ssh_target,
        &prepare_remote_script,
    ));

    for service in &map.services {
        for command in service_build_commands(service) {
            commands.push(command);
        }

        let sync_source = service.local_path.join(&service.sync_source);
        commands.push(rsync_command(
            &map.remote,
            &rsync_target,
            &sync_source,
            &remote_child(&remote_child(&map.remote.tmp_dir, "sync"), &service.name),
            true,
            true,
        ));
    }

    if let Some(pocketbase) = &map.pocketbase {
        commands.push(pocketbase_rsync_command(
            &map.remote,
            &rsync_target,
            pocketbase,
            &remote_child(&remote_child(&map.remote.tmp_dir, "sync"), &pocketbase.name),
        ));
    }
    commands.push(rsync_command(
        &map.remote,
        &rsync_target,
        &rendered_paths.caddyfile,
        &remote_child(&map.remote.tmp_dir, "Caddyfile"),
        false,
        false,
    ));
    commands.push(rsync_command(
        &map.remote,
        &rsync_target,
        &rendered_paths.systemd_dir,
        &remote_child(&map.remote.tmp_dir, "systemd"),
        true,
        true,
    ));
    commands.push(ssh_command(
        &map.remote,
        &ssh_target,
        &format!(
            "caddy validate --config {}",
            sh_quote(&remote_child(&map.remote.tmp_dir, "Caddyfile")),
        ),
    ));
    commands.push(ssh_command(&map.remote, &ssh_target, &install_script(map)));

    Ok(DeploymentCommandList { commands })
}

fn ensure_deploy_programs_available(map: &ServiceMap) -> Result<()> {
    for service in &map.services {
        for command in service_build_commands(service) {
            ensure_command_can_start(&command).with_context(|| {
                format!(
                    "service `{}` build program `{}` is unavailable",
                    service.name, command.program,
                )
            })?;
        }
    }

    ensure_program_available(&map.remote.ssh_program)?;
    ensure_program_available(&map.remote.rsync_program)?;

    Ok(())
}

fn service_build_commands(service: &ResolvedService) -> Vec<ProcessCommand> {
    let Some(build) = &service.build else {
        return Vec::new();
    };

    build
        .commands()
        .into_iter()
        .map(|command| {
            let mut parts = command.iter();
            let program = parts.next().expect("validated build command");

            ProcessCommand::new(program.as_str())
                .args(parts.cloned())
                .cwd(service.local_path.clone())
                .envs(build.environment.clone())
        })
        .collect()
}

pub fn format_plan(plan: &DeploymentCommandList) -> String {
    let mut output = String::new();

    for (index, command) in plan.commands.iter().enumerate() {
        output.push_str(&(index + 1).to_string());
        output.push_str(". ");
        output.push_str(&command.display());
        output.push('\n');
    }

    output
}
