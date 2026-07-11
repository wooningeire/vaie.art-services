use super::super::build_deployment_command_list_for_output_dir;
use super::fixture::DeployFixture;
use crate::cli::process::ProcessCommand;

#[test]
fn deploy_plan_builds_before_service_rsync() {
    let fixture = DeployFixture::new();
    let map = fixture.map();
    let plan =
        build_deployment_command_list_for_output_dir(&map, &fixture.dir.path().join("rendered"))
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
    let plan =
        build_deployment_command_list_for_output_dir(&map, &fixture.dir.path().join("rendered"))
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
    let plan =
        build_deployment_command_list_for_output_dir(&map, &fixture.dir.path().join("rendered"))
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
fn deploy_plan_validates_systemd_after_sync_and_before_install() {
    let fixture = DeployFixture::new();
    let map = fixture.map();
    let plan =
        build_deployment_command_list_for_output_dir(&map, &fixture.dir.path().join("rendered"))
            .expect("deployment plan");
    let displays = plan
        .commands
        .iter()
        .map(ProcessCommand::display)
        .collect::<Vec<_>>();

    let systemd_rsync_index = plan
        .commands
        .iter()
        .position(|command| {
            command.program == "rsync"
                && command
                    .args
                    .last()
                    .is_some_and(|arg| arg.ends_with("/systemd"))
        })
        .expect("systemd rsync command");
    let verify_index = displays
        .iter()
        .position(|command| command.contains("systemd-analyze verify"))
        .expect("systemd verify command");
    let install_index = displays
        .iter()
        .position(|command| command.contains("systemctl daemon-reload"))
        .expect("remote install script");

    assert!(systemd_rsync_index < verify_index);
    assert!(verify_index < install_index);
}

#[test]
fn deployment_plan_requires_remote_host() {
    let fixture = DeployFixture::new_without_remote_host();
    let map = fixture.map();
    let error =
        build_deployment_command_list_for_output_dir(&map, &fixture.dir.path().join("rendered"))
            .expect_err("deployment plan should require remote host");

    assert!(error.to_string().contains("remote.host is required"));
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
    let plan =
        build_deployment_command_list_for_output_dir(&map, &fixture.dir.path().join("rendered"))
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
