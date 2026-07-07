use std::path::Path;

use anyhow::{Context, Result, bail};

use super::config::{
    RemoteConfig, ResolvedPocketBase, ResolvedService, ResolvedServiceKind, ServiceMap,
};
use super::process::{
    CommandRunner, ProcessCommand, ensure_command_can_start, ensure_program_available,
};
use super::render::{RenderedPaths, write_artifacts};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeploymentCommandList {
    pub commands: Vec<ProcessCommand>,
}

pub fn deploy(map: &ServiceMap, output_dir: &Path, runner: &dyn CommandRunner) -> Result<()> {
    ensure_deploy_programs_available(map)?;

    let rendered_paths = write_artifacts(map, output_dir)?;
    let plan = build_deployment_command_list(map, &rendered_paths)?;

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

fn rsync_command(
    remote: &RemoteConfig,
    remote_target: &str,
    source: &Path,
    remote_destination: &str,
    delete: bool,
    source_is_dir: bool,
) -> ProcessCommand {
    let mut command = ProcessCommand::new(remote.rsync_program.as_str())
        .arg("-az")
        .arg("-e")
        .arg(ssh_transport(remote));

    if delete {
        command = command.arg("--delete");
    }

    command
        .arg(rsync_source(source, source_is_dir))
        .arg(format!("{remote_target}:{remote_destination}"))
}

fn pocketbase_rsync_command(
    remote: &RemoteConfig,
    remote_target: &str,
    pocketbase: &ResolvedPocketBase,
    remote_destination: &str,
) -> ProcessCommand {
    ProcessCommand::new(remote.rsync_program.as_str())
        .arg("-az")
        .arg("--delete")
        .arg("--exclude")
        .arg("pb_data/")
        .arg("-e")
        .arg(ssh_transport(remote))
        .arg(rsync_source(&pocketbase.source_path, true))
        .arg(format!("{remote_target}:{remote_destination}"))
}
fn ssh_command(remote: &RemoteConfig, remote_target: &str, script: &str) -> ProcessCommand {
    ProcessCommand::new(remote.ssh_program.as_str())
        .args(ssh_args(remote))
        .arg(remote_target)
        .arg(script)
}

fn ssh_args(remote: &RemoteConfig) -> Vec<String> {
    let mut args = vec!["-p".to_string(), remote.port.to_string()];

    if let Some(identity_file) = &remote.identity_file {
        args.push("-i".to_string());
        args.push(identity_file.display().to_string());
    }

    args.extend(remote.extra_ssh_args.clone());
    args
}

fn ssh_transport(remote: &RemoteConfig) -> String {
    let mut parts = vec![sh_quote(&remote.ssh_program)];
    parts.extend(ssh_args(remote).iter().map(|arg| sh_quote(arg)));
    parts.join(" ")
}

fn remote_ssh_target(remote: &RemoteConfig) -> Result<String> {
    let host = required_remote_host(remote)?;

    Ok(remote_target(remote, host))
}

fn remote_rsync_target(remote: &RemoteConfig) -> Result<String> {
    let host = required_remote_host(remote)?;
    let host = rsync_host(host);

    Ok(remote_target(remote, &host))
}

fn required_remote_host(remote: &RemoteConfig) -> Result<&str> {
    let Some(host) = &remote.host else {
        bail!(
            "remote.host is required for plan/deploy; set it in services.local.toml or use an SSH config host alias",
        );
    };

    Ok(host)
}

fn remote_target(remote: &RemoteConfig, host: &str) -> String {
    if let Some(user) = &remote.user {
        return format!("{user}@{host}");
    }

    host.to_string()
}

fn rsync_host(host: &str) -> String {
    if host.contains(':') && !(host.starts_with('[') && host.ends_with(']')) {
        return format!("[{host}]");
    }

    host.to_string()
}

fn rsync_source(path: &Path, source_is_dir: bool) -> String {
    let mut source = path.display().to_string().replace('\\', "/");

    if source_is_dir && !source.ends_with('/') {
        source.push('/');
    }

    source
}

fn remote_child(parent: &str, child: &str) -> String {
    format!(
        "{}/{}",
        parent.trim_end_matches('/'),
        child.trim_start_matches('/')
    )
}

fn install_script(map: &ServiceMap) -> String {
    let expected_units = map.systemd_service_names().join(" ");
    let mut script = String::new();

    script.push_str("set -eu\n");
    script.push_str("tmp_dir=");
    script.push_str(&sh_quote(&map.remote.tmp_dir));
    script.push('\n');
    script.push_str("sync_dir=\"$tmp_dir/sync\"\n");
    script.push_str("systemd_tmp=\"$tmp_dir/systemd\"\n");
    script.push_str("caddy_tmp=\"$tmp_dir/Caddyfile\"\n");
    script.push_str("caddyfile_path=");
    script.push_str(&sh_quote(&map.remote.caddyfile_path));
    script.push('\n');
    script.push_str("systemd_dir=");
    script.push_str(&sh_quote(&map.remote.systemd_dir));
    script.push('\n');
    script.push_str("managed_prefix=");
    script.push_str(&sh_quote(&map.remote.managed_prefix));
    script.push('\n');
    script.push_str("expected_units=");
    script.push_str(&sh_quote(&expected_units));
    script.push('\n');
    script.push_str("report_systemctl_failure() {\n");
    script.push_str("    failed_unit=\"$1\"\n");
    script.push_str("    echo \"systemd failed for $failed_unit\" >&2\n");
    script.push_str("    systemctl status \"$failed_unit\" --no-pager --lines=80 || true\n");
    script.push_str("    journalctl -u \"$failed_unit\" --no-pager --lines=120 || true\n");
    script.push_str("}\n");
    script.push_str("backup_dir=\"$tmp_dir/backups/$(date +%Y%m%d%H%M%S)\"\n");
    script.push_str("mkdir -p \"$backup_dir/systemd\"\n");
    script.push_str("if [ -f \"$caddyfile_path\" ]; then cp \"$caddyfile_path\" \"$backup_dir/Caddyfile\"; fi\n");
    script.push_str("for unit_path in \"$systemd_dir\"/\"$managed_prefix\"*.service; do\n");
    script.push_str("    [ -e \"$unit_path\" ] || continue\n");
    script.push_str("    cp \"$unit_path\" \"$backup_dir/systemd/\" || true\n");
    script.push_str("done\n");

    if let Some(pocketbase) = &map.pocketbase {
        append_pocketbase_preflight(&mut script, pocketbase);
    }

    for service in &map.services {
        append_service_install_sync(&mut script, service);
    }

    if let Some(pocketbase) = &map.pocketbase {
        append_pocketbase_install_sync(&mut script, pocketbase);
    }
    script.push_str("caddy_changed=0\n");
    script.push_str(
        "if [ ! -f \"$caddyfile_path\" ] || ! cmp -s \"$caddy_tmp\" \"$caddyfile_path\"; then\n",
    );
    script.push_str("    install -m 0644 \"$caddy_tmp\" \"$caddyfile_path\"\n");
    script.push_str("    caddy_changed=1\n");
    script.push_str("fi\n");
    script.push_str("systemd_changed=0\n");

    for service in &map.services {
        if let ResolvedServiceKind::DenoApp { service_name, .. } = &service.kind {
            script.push_str("unit=");
            script.push_str(&sh_quote(service_name));
            script.push('\n');
            script.push_str("source_unit=\"$systemd_tmp/$unit\"\n");
            script.push_str("target_unit=\"$systemd_dir/$unit\"\n");
            script.push_str("if [ ! -f \"$target_unit\" ] || ! cmp -s \"$source_unit\" \"$target_unit\"; then\n");
            script.push_str("    install -m 0644 \"$source_unit\" \"$target_unit\"\n");
            script.push_str("    systemd_changed=1\n");
            script.push_str("fi\n");
        }
    }

    if let Some(pocketbase) = &map.pocketbase {
        append_systemd_unit_install(&mut script, &pocketbase.service_name);
    }
    script.push_str("for unit_path in \"$systemd_dir\"/\"$managed_prefix\"*.service; do\n");
    script.push_str("    [ -e \"$unit_path\" ] || continue\n");
    script.push_str("    unit=\"$(basename \"$unit_path\")\"\n");
    script.push_str("    case \" $expected_units \" in\n");
    script.push_str("        *\" $unit \"*) ;;\n");
    script.push_str("        *)\n");
    script.push_str("            systemctl stop \"$unit\" || true\n");
    script.push_str("            systemctl disable \"$unit\" || true\n");
    script.push_str("            rm -f \"$unit_path\"\n");
    script.push_str("            systemd_changed=1\n");
    script.push_str("            ;;\n");
    script.push_str("    esac\n");
    script.push_str("done\n");
    script.push_str("if [ \"$systemd_changed\" -eq 1 ]; then systemctl daemon-reload; fi\n");
    script.push_str("for unit in $expected_units; do\n");
    script.push_str("    if ! systemctl enable --now \"$unit\"; then\n");
    script.push_str("        report_systemctl_failure \"$unit\"\n");
    script.push_str("        exit 1\n");
    script.push_str("    fi\n");
    script.push_str("done\n");
    script.push_str("# Artifact syncs can change server code without changing the systemd unit.\n");
    script.push_str("for unit in $expected_units; do\n");
    script.push_str("    if ! systemctl restart \"$unit\"; then\n");
    script.push_str("        report_systemctl_failure \"$unit\"\n");
    script.push_str("        exit 1\n");
    script.push_str("    fi\n");
    script.push_str("done\n");
    script.push_str("if [ \"$caddy_changed\" -eq 1 ]; then systemctl reload caddy || systemctl restart caddy; fi\n");

    script
}

fn append_pocketbase_preflight(script: &mut String, pocketbase: &ResolvedPocketBase) {
    script.push_str("if [ ! -x ");
    script.push_str(&sh_quote(&pocketbase.binary));
    script.push_str(" ]; then\n");
    script.push_str("    echo ");
    script.push_str(&sh_quote(&format!(
        "missing PocketBase binary: {}",
        pocketbase.binary,
    )));
    script.push_str(" >&2\n");
    script.push_str("    echo ");
    script.push_str(&sh_quote(
        "install PocketBase on the remote or update pocketbase.binary in services.toml",
    ));
    script.push_str(" >&2\n");
    script.push_str("    exit 1\n");
    script.push_str("fi\n");

    if let Some(environment_file) = &pocketbase.environment_file {
        script.push_str("if [ ! -f ");
        script.push_str(&sh_quote(environment_file));
        script.push_str(" ]; then\n");
        script.push_str("    echo ");
        script.push_str(&sh_quote(&format!(
            "missing PocketBase environment file: {}",
            environment_file,
        )));
        script.push_str(" >&2\n");
        script.push_str("    echo ");
        script.push_str(&sh_quote(
            "create it on the remote; it holds secrets and is not deployed from this repo",
        ));
        script.push_str(" >&2\n");
        script.push_str("    exit 1\n");
        script.push_str("fi\n");

        if let Some(encryption_env) = &pocketbase.encryption_env {
            script.push_str("if ! grep -Eq ");
            script.push_str(&sh_quote(&format!(
                "^[[:space:]]*{}[[:space:]]*=",
                encryption_env,
            )));
            script.push(' ');
            script.push_str(&sh_quote(environment_file));
            script.push_str("; then\n");
            script.push_str("    echo ");
            script.push_str(&sh_quote(&format!(
                "PocketBase environment file does not define {}: {}",
                encryption_env, environment_file,
            )));
            script.push_str(" >&2\n");
            script.push_str("    exit 1\n");
            script.push_str("fi\n");
        }
    }
}

fn append_systemd_unit_install(script: &mut String, service_name: &str) {
    script.push_str("unit=");
    script.push_str(&sh_quote(service_name));
    script.push('\n');
    script.push_str("source_unit=\"$systemd_tmp/$unit\"\n");
    script.push_str("target_unit=\"$systemd_dir/$unit\"\n");
    script.push_str(
        "if [ ! -f \"$target_unit\" ] || ! cmp -s \"$source_unit\" \"$target_unit\"; then\n",
    );
    script.push_str("    install -m 0644 \"$source_unit\" \"$target_unit\"\n");
    script.push_str("    systemd_changed=1\n");
    script.push_str("fi\n");
}
fn append_service_install_sync(script: &mut String, service: &ResolvedService) {
    script.push_str("mkdir -p ");
    script.push_str(&sh_quote(&service.remote_path));
    script.push('\n');
    script.push_str("rsync -a --delete ");
    script.push_str("\"$sync_dir/");
    script.push_str(&service.name);
    script.push_str("/\"");
    script.push(' ');
    script.push_str(&sh_quote(&remote_child(&service.remote_path, "")));
    script.push('\n');
}

fn append_pocketbase_install_sync(script: &mut String, pocketbase: &ResolvedPocketBase) {
    script.push_str("mkdir -p ");
    script.push_str(&sh_quote(&pocketbase.remote_path));
    script.push(' ');
    script.push_str(&sh_quote(&pocketbase.data_dir));

    if let Some(backup_dir) = &pocketbase.backup_dir {
        script.push(' ');
        script.push_str(&sh_quote(backup_dir));
    }

    script.push('\n');
    script.push_str("rsync -a --delete --exclude pb_data/ ");
    script.push_str("\"$sync_dir/");
    script.push_str(&pocketbase.name);
    script.push_str("/\"");
    script.push(' ');
    script.push_str(&sh_quote(&remote_child(&pocketbase.remote_path, "")));
    script.push('\n');
}
fn sh_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }

    let safe = value.chars().all(|char| {
        char.is_ascii_alphanumeric() || matches!(char, '_' | '-' | '.' | '/' | ':' | '@')
    });

    if safe {
        return value.to_string();
    }

    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::{super::config::Config, super::process::CommandRunner, *};
    use std::{cell::RefCell, fs, path::PathBuf};
    use tempfile::TempDir;

    #[test]
    fn deploy_plan_builds_before_service_rsync() {
        let fixture = DeployFixture::new();
        let map = fixture.map();
        let plan = build_deployment_command_list_for_output_dir(
            &map,
            &fixture.dir.path().join("rendered"),
        )
        .expect("deployment plan");
        let displays = plan
            .commands
            .iter()
            .map(ProcessCommand::display)
            .collect::<Vec<_>>();

        let build_index = displays
            .iter()
            .position(|command| command.contains("deno task build"))
            .expect("build command");
        let rsync_index = displays
            .iter()
            .position(|command| command.contains("rsync") && command.contains("/build/"))
            .expect("rsync command");

        assert!(build_index < rsync_index);
    }

    #[test]
    fn deploy_plan_runs_ordered_build_commands_before_service_rsync() {
        let fixture = DeployFixture::with_config(
            r#"
manifest_version = 1

[remote]
host = "vaie.art"
user = "root"

[caddy]
primary_host = "vaie.art"

[[services]]
name = "pudle"
kind = "static_site"
local_path = "src/submodules/pudle"
remote_path = "/web/pudle"
sync_source = "build"
route_path = "/pudle"

[services.build]
commands = [
    ["deno", "task", "convert-media"],
    ["deno", "task", "build"],
]
"#,
        );
        let map = fixture.map();
        let plan = build_deployment_command_list_for_output_dir(
            &map,
            &fixture.dir.path().join("rendered"),
        )
        .expect("deployment plan");
        let displays = plan
            .commands
            .iter()
            .map(ProcessCommand::display)
            .collect::<Vec<_>>();

        let convert_index = displays
            .iter()
            .position(|command| command.contains("deno task convert-media"))
            .expect("convert-media command");
        let build_index = displays
            .iter()
            .position(|command| command.contains("deno task build"))
            .expect("build command");
        let rsync_index = displays
            .iter()
            .position(|command| command.contains("rsync") && command.contains("/build/"))
            .expect("rsync command");

        assert!(convert_index < build_index);
        assert!(build_index < rsync_index);
    }

    #[test]
    fn deploy_plan_validates_caddy_before_remote_install_script() {
        let fixture = DeployFixture::new();
        let map = fixture.map();
        let plan = build_deployment_command_list_for_output_dir(
            &map,
            &fixture.dir.path().join("rendered"),
        )
        .expect("deployment plan");
        let displays = plan
            .commands
            .iter()
            .map(ProcessCommand::display)
            .collect::<Vec<_>>();

        let validate_index = displays
            .iter()
            .position(|command| command.contains("caddy validate"))
            .expect("caddy validate command");
        let install_index = displays
            .iter()
            .position(|command| command.contains("systemctl daemon-reload"))
            .expect("remote install script");

        assert!(validate_index < install_index);
    }

    #[test]
    fn remote_install_restarts_deno_services_after_sync() {
        let fixture = DeployFixture::new();
        let map = fixture.map();
        let plan = build_deployment_command_list_for_output_dir(
            &map,
            &fixture.dir.path().join("rendered"),
        )
        .expect("deployment plan");
        let install_script = plan
            .commands
            .last()
            .expect("install command")
            .args
            .last()
            .expect("install script");

        assert!(install_script.contains(
            "# Artifact syncs can change server code without changing the systemd unit.",
        ));
        assert!(install_script.contains("if ! systemctl restart \"$unit\"; then"));
        assert!(install_script.contains("report_systemctl_failure \"$unit\""));
        assert!(install_script.contains("journalctl -u \"$failed_unit\" --no-pager --lines=120"));
        assert!(!install_script.contains("changed_units"));
    }

    #[test]
    fn deployment_plan_preflights_pocketbase_runtime_inputs() {
        let fixture = DeployFixture::with_config(
            r#"
manifest_version = 1

[remote]
host = "vaie.art"
user = "root"

[caddy]
primary_host = "vaie.art"

[pocketbase]
name = "site-pocketbase"
host = "pb.vaie.art"
source_path = "src/pocketbase"
remote_path = "/srv/vaieart-pocketbase"
data_dir = "/var/lib/vaieart-pocketbase/pb_data"
backup_dir = "/var/backups/vaieart-pocketbase"
port = 8090
binary = "/opt/pocketbase/pocketbase"
environment_file = "/etc/vaieart/pocketbase.env"
encryption_env = "PB_ENCRYPTION_KEY"

[[services]]
name = "pudle"
kind = "static_site"
local_path = "src/submodules/pudle"
remote_path = "/web/pudle"
sync_source = "build"
route_path = "/pudle"
"#,
        );
        let map = fixture.map();
        let plan = build_deployment_command_list_for_output_dir(
            &map,
            &fixture.dir.path().join("rendered"),
        )
        .expect("deployment plan");
        let prepare_script = plan
            .commands
            .first()
            .expect("prepare command")
            .args
            .last()
            .expect("prepare script");
        let install_script = plan
            .commands
            .last()
            .expect("install command")
            .args
            .last()
            .expect("install script");

        assert!(prepare_script.contains("if [ ! -x /opt/pocketbase/pocketbase ]; then"));
        assert!(install_script.contains("if [ ! -x /opt/pocketbase/pocketbase ]; then"));
        assert!(install_script.contains("missing PocketBase binary: /opt/pocketbase/pocketbase"));
        assert!(install_script.contains("if [ ! -f /etc/vaieart/pocketbase.env ]; then"));
        assert!(
            install_script
                .contains("missing PocketBase environment file: /etc/vaieart/pocketbase.env")
        );
        assert!(install_script.contains(
            "grep -Eq '^[[:space:]]*PB_ENCRYPTION_KEY[[:space:]]*=' /etc/vaieart/pocketbase.env",
        ));
        assert!(install_script.contains(
            "PocketBase environment file does not define PB_ENCRYPTION_KEY: /etc/vaieart/pocketbase.env",
        ));
    }

    #[test]
    fn dry_run_executor_records_commands_without_contacting_remote() {
        let fixture = DeployFixture::new();
        let map = fixture.map();
        let plan = build_deployment_command_list_for_output_dir(
            &map,
            &fixture.dir.path().join("rendered"),
        )
        .expect("deployment plan");
        let runner = RecordingRunner::default();

        for command in &plan.commands {
            runner.run(command).expect("record command");
        }

        assert_eq!(runner.commands.borrow().len(), plan.commands.len());
        assert!(
            runner
                .commands
                .borrow()
                .iter()
                .any(|command| command.program == "rsync"),
        );
    }

    #[test]
    fn deployment_plan_syncs_pocketbase_without_data_dir() {
        let fixture = DeployFixture::with_config(
            r#"
manifest_version = 1

[remote]
host = "vaie.art"
user = "root"

[caddy]
primary_host = "vaie.art"

[pocketbase]
name = "site-pocketbase"
host = "pb.vaie.art"
source_path = "src/pocketbase"
remote_path = "/srv/vaieart-pocketbase"
data_dir = "/var/lib/vaieart-pocketbase/pb_data"
backup_dir = "/var/backups/vaieart-pocketbase"
port = 8090

[[services]]
name = "pudle"
kind = "static_site"
local_path = "src/submodules/pudle"
remote_path = "/web/pudle"
sync_source = "build"
route_path = "/pudle"
"#,
        );
        let map = fixture.map();
        let plan = build_deployment_command_list_for_output_dir(
            &map,
            &fixture.dir.path().join("rendered"),
        )
        .expect("deployment plan");
        let displays = plan
            .commands
            .iter()
            .map(ProcessCommand::display)
            .collect::<Vec<_>>();
        let pocketbase_rsync = displays
            .iter()
            .find(|command| {
                command.contains("rsync")
                    && command.contains("--exclude pb_data/")
                    && command.contains("site-pocketbase")
            })
            .expect("pocketbase rsync command");
        let install_script = plan
            .commands
            .last()
            .expect("install command")
            .args
            .last()
            .expect("install script");

        assert!(pocketbase_rsync.contains("src/pocketbase/"));
        assert!(install_script.contains(
            "mkdir -p /srv/vaieart-pocketbase /var/lib/vaieart-pocketbase/pb_data /var/backups/vaieart-pocketbase",
        ));
        assert!(install_script.contains(
            "rsync -a --delete --exclude pb_data/ \"$sync_dir/site-pocketbase/\" /srv/vaieart-pocketbase/",
        ));
        assert!(install_script.contains("vaieart-site-pocketbase.service"));
    }

    #[test]
    fn deployment_plan_requires_remote_host() {
        let fixture = DeployFixture::new_without_remote_host();
        let map = fixture.map();
        let error = build_deployment_command_list_for_output_dir(
            &map,
            &fixture.dir.path().join("rendered"),
        )
        .expect_err("deployment plan should require remote host");

        assert!(error.to_string().contains("remote.host is required"));
    }

    #[test]
    fn deploy_preflights_build_programs_before_remote_commands() {
        let fixture = DeployFixture::with_config(
            r#"
manifest_version = 1

[remote]
host = "vaie.art"
user = "root"

[caddy]
primary_host = "vaie.art"

[[services]]
name = "pudle"
kind = "static_site"
local_path = "src/submodules/pudle"
remote_path = "/web/pudle"
sync_source = "build"
route_path = "/pudle"

[services.build]
command = ["__vaieart_missing_build_tool__", "task", "build"]
"#,
        );
        let map = fixture.map();
        let runner = RecordingRunner::default();
        let error = deploy(&map, &fixture.dir.path().join("rendered"), &runner)
            .expect_err("deploy should reject missing build program");
        let error_chain = format!("{error:#}");

        assert!(error_chain.contains("service `pudle` build program"));
        assert!(error_chain.contains("was not found on PATH"));
        assert!(runner.commands.borrow().is_empty());
    }

    #[test]
    fn deployment_plan_brackets_ipv6_hosts_for_rsync_only() {
        let fixture = DeployFixture::with_config(
            r#"
manifest_version = 1

[remote]
host = "2a01:4ff:f0:8c51::1"
user = "root"

[caddy]
primary_host = "vaie.art"

[[services]]
name = "pudle"
kind = "static_site"
local_path = "src/submodules/pudle"
remote_path = "/web/pudle"
sync_source = "build"
route_path = "/pudle"
"#,
        );
        let map = fixture.map();
        let plan = build_deployment_command_list_for_output_dir(
            &map,
            &fixture.dir.path().join("rendered"),
        )
        .expect("deployment plan");
        let ssh_command = plan
            .commands
            .iter()
            .find(|command| command.program == "ssh")
            .expect("ssh command");
        let rsync_destinations = plan
            .commands
            .iter()
            .filter(|command| command.program == "rsync")
            .map(|command| command.args.last().expect("rsync destination"))
            .collect::<Vec<_>>();

        assert!(
            ssh_command
                .args
                .iter()
                .any(|arg| arg == "root@2a01:4ff:f0:8c51::1"),
        );
        assert!(
            rsync_destinations
                .iter()
                .all(|destination| destination.starts_with("root@[2a01:4ff:f0:8c51::1]:")),
        );
    }

    struct DeployFixture {
        dir: TempDir,
        config_path: PathBuf,
    }

    impl DeployFixture {
        fn new() -> Self {
            Self::with_config(
                r#"
manifest_version = 1

[remote]
host = "vaie.art"
user = "root"

[caddy]
primary_host = "vaie.art"

[[services]]
name = "pudle"
kind = "static_site"
local_path = "src/submodules/pudle"
remote_path = "/web/pudle"
sync_source = "build"
route_path = "/pudle"

[services.build]
command = ["deno", "task", "build"]
"#,
            )
        }

        fn new_without_remote_host() -> Self {
            Self::with_config(
                r#"
manifest_version = 1

[remote]

[caddy]
primary_host = "vaie.art"

[[services]]
name = "pudle"
kind = "static_site"
local_path = "src/submodules/pudle"
remote_path = "/web/pudle"
sync_source = "build"
route_path = "/pudle"
"#,
            )
        }

        fn with_config(source: &str) -> Self {
            let dir = TempDir::new().expect("temp dir");
            fs::create_dir_all(dir.path().join("src/pocketbase/pb_migrations")).expect("pb dir");
            fs::create_dir_all(dir.path().join("src/submodules/pudle/build")).expect("build dir");
            let config_path = dir.path().join("services.toml");
            fs::write(&config_path, source).expect("write config");

            Self { dir, config_path }
        }

        fn map(&self) -> ServiceMap {
            Config::load(&self.config_path)
                .expect("load config")
                .validate(&self.config_path)
                .expect("validate config")
        }
    }

    #[derive(Default)]
    struct RecordingRunner {
        commands: RefCell<Vec<ProcessCommand>>,
    }

    impl CommandRunner for RecordingRunner {
        fn run(&self, command: &ProcessCommand) -> Result<()> {
            self.commands.borrow_mut().push(command.clone());
            Ok(())
        }
    }
}
