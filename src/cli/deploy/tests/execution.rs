use super::super::{build_deployment_command_list_for_output_dir, deploy};
use super::fixture::{DeployFixture, RecordingRunner};
use crate::cli::process::CommandRunner;

#[test]
fn remote_install_restarts_deno_services_after_sync() {
    let fixture = DeployFixture::new();
    let map = fixture.map();
    let plan =
        build_deployment_command_list_for_output_dir(&map, &fixture.dir.path().join("rendered"))
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
fn recording_executor_records_commands_without_contacting_remote() {
    let fixture = DeployFixture::new();
    let map = fixture.map();
    let plan =
        build_deployment_command_list_for_output_dir(&map, &fixture.dir.path().join("rendered"))
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
