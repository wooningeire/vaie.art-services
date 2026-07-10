use super::super::{
    build_deployment_command_list_for_output_dir, build_pocketbase_migrations_pull_command,
};
use super::fixture::{DeployFixture, RecordingRunner};
use crate::cli::process::ProcessCommand;
use std::fs;

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

[pocketbase.warp_proxy]
port = 40000

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
    assert!(prepare_script.contains("if [ ! -x /usr/bin/warp-cli ]; then"));
    assert!(install_script.contains("missing WARP CLI: /usr/bin/warp-cli"));
    assert!(install_script.contains("if ! systemctl cat warp-svc.service >/dev/null 2>&1; then",),);
    assert!(install_script.contains("if ! systemctl start warp-svc.service; then",),);
    assert!(
        install_script.contains("if ! /usr/bin/warp-cli registration show >/dev/null 2>&1; then",),
    );
    assert!(install_script.contains("WARP consumer registration is missing"));
    assert!(install_script.contains("if [ ! -x /usr/bin/curl ]; then"));
    assert!(install_script.contains(
        "PocketBase environment file must not define proxy variables when warp_proxy is enabled",
    ));
    let warp_install_index = install_script
        .find("unit=vaieart-site-pocketbase-warp-proxy.service")
        .expect("WARP proxy unit install");
    let pocketbase_install_index = install_script
        .find("unit=vaieart-site-pocketbase.service")
        .expect("PocketBase unit install");

    assert!(warp_install_index < pocketbase_install_index);
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

#[test]
fn migration_pull_command_is_add_only_and_handles_ipv6_ssh_options() {
    let fixture = DeployFixture::with_config(
        r#"
manifest_version = 1

[remote]
host = "2001:db8::1"
user = "root"
port = 2222
identity_file = "C:/Users/V/SSH Keys/vaie_art"
extra_ssh_args = ["-o", "BatchMode=yes"]

[caddy]
primary_host = "vaie.art"

[pocketbase]
name = "site-pocketbase"
host = "pb.vaie.art"
source_path = "src/pocketbase"
remote_path = "/srv/vaieart-pocketbase"
data_dir = "/var/lib/vaieart-pocketbase/pb_data"

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
    let command = build_pocketbase_migrations_pull_command(&map).expect("migration pull command");

    assert_eq!(command.program, "rsync");
    assert_eq!(
        &command.args[..9],
        &[
            "-rtz",
            "--ignore-existing",
            "--itemize-changes",
            "--include",
            "*.js",
            "--exclude",
            "*",
            "-e",
            "ssh -p 2222 -i 'C:/Users/V/SSH Keys/vaie_art' -o 'BatchMode=yes'",
        ],
    );
    assert_eq!(
        command.args[9],
        "root@[2001:db8::1]:/srv/vaieart-pocketbase/pb_migrations/",
    );
    assert!(command.args[10].ends_with("/src/pocketbase/pb_migrations/"));
    assert!(!command.args.iter().any(|arg| arg == "--delete"));
    assert!(!command.args.iter().any(|arg| arg == "-a"));
}

#[test]
fn migration_pull_creates_the_destination_and_runs_only_rsync() {
    let fixture = DeployFixture::with_config(
        r#"
manifest_version = 1

[remote]
host = "vaie.art"
user = "root"
ssh_program = "cargo"
rsync_program = "cargo"

[caddy]
primary_host = "vaie.art"

[pocketbase]
name = "site-pocketbase"
host = "pb.vaie.art"
source_path = "src/pocketbase"
remote_path = "/srv/vaieart-pocketbase"
data_dir = "/var/lib/vaieart-pocketbase/pb_data"

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
    let destination = map
        .pocketbase
        .as_ref()
        .expect("pocketbase config")
        .source_path
        .join("pb_migrations");
    fs::remove_dir(&destination).expect("remove migrations directory");
    let runner = RecordingRunner::default();

    let pulled_destination =
        super::super::pull_pocketbase_migrations(&map, &runner).expect("pull migrations");

    assert_eq!(pulled_destination, destination);
    assert!(destination.is_dir());
    assert_eq!(runner.commands.borrow().len(), 1);
}
