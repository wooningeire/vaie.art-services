use super::super::build_deployment_command_list_for_output_dir;
use super::fixture::DeployFixture;
use crate::cli::process::ProcessCommand;

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
    let plan =
        build_deployment_command_list_for_output_dir(&map, &fixture.dir.path().join("rendered"))
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
        install_script.contains("missing PocketBase environment file: /etc/vaieart/pocketbase.env")
    );
    assert!(install_script.contains(
        "grep -Eq '^[[:space:]]*PB_ENCRYPTION_KEY[[:space:]]*=' /etc/vaieart/pocketbase.env",
    ));
    assert!(install_script.contains(
            "PocketBase environment file does not define PB_ENCRYPTION_KEY: /etc/vaieart/pocketbase.env",
        ));
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
    let plan =
        build_deployment_command_list_for_output_dir(&map, &fixture.dir.path().join("rendered"))
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
