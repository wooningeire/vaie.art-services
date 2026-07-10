use super::paths::config_directory;
use super::*;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn valid_config_parses_and_resolves_defaults() {
    let fixture = ConfigFixture::new();
    let map = fixture.load(sample_config());

    assert_eq!(map.remote.host, None);
    assert_eq!(map.remote.user, None);
    assert_eq!(map.remote.port, 22);
    assert_eq!(map.remote.ssh_program, "ssh");
    assert_eq!(map.services.len(), 2);
    assert_eq!(
        map.systemd_service_names(),
        vec!["vaieart-vaie-art.service"]
    );
}

#[test]
fn pocketbase_config_parses_and_owns_a_systemd_unit() {
    let fixture = ConfigFixture::new();
    let map = fixture.load(
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
environment_file = "/etc/vaieart/pocketbase.env"
encryption_env = "PB_ENCRYPTION_KEY"

[pocketbase.warp_proxy]

[[services]]
name = "pudle"
kind = "static_site"
local_path = "src/submodules/pudle"
remote_path = "/web/pudle"
route_path = "/pudle"
"#,
    );
    let pocketbase = map.pocketbase.as_ref().expect("pocketbase config");

    assert_eq!(pocketbase.host, "pb.vaie.art");
    assert_eq!(pocketbase.port, 8090);
    assert_eq!(pocketbase.service_name, "vaieart-site-pocketbase.service");
    let warp_proxy = pocketbase.warp_proxy.as_ref().expect("WARP proxy config");
    assert_eq!(warp_proxy.port, 40000);
    assert_eq!(warp_proxy.cli, "/usr/bin/warp-cli");
    assert_eq!(warp_proxy.daemon_service, "warp-svc.service");
    assert_eq!(
        warp_proxy.service_name,
        "vaieart-site-pocketbase-warp-proxy.service",
    );
    assert_eq!(
        map.systemd_service_names(),
        vec![
            "vaieart-site-pocketbase-warp-proxy.service",
            "vaieart-site-pocketbase.service",
        ],
    );
}

#[test]
fn pocketbase_warp_proxy_rejects_zero_port() {
    let fixture = ConfigFixture::new();
    let error = fixture.load_error(
        r#"
manifest_version = 1

[remote]

[caddy]
primary_host = "vaie.art"

[pocketbase]
name = "site-pocketbase"
host = "pb.vaie.art"
source_path = "src/pocketbase"
remote_path = "/srv/vaieart-pocketbase"
data_dir = "/var/lib/vaieart-pocketbase/pb_data"

[pocketbase.warp_proxy]
port = 0

[[services]]
name = "pudle"
kind = "static_site"
local_path = "src/submodules/pudle"
remote_path = "/web/pudle"
route_path = "/pudle"
"#,
    );

    assert!(
        error
            .to_string()
            .contains("pocketbase.warp_proxy.port must be greater than 0"),
    );
}

#[test]
fn pocketbase_encryption_env_requires_environment_file() {
    let fixture = ConfigFixture::new();
    let error = fixture.load_error(
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
port = 8090
encryption_env = "PB_ENCRYPTION_KEY"

[[services]]
name = "pudle"
kind = "static_site"
local_path = "src/submodules/pudle"
remote_path = "/web/pudle"
route_path = "/pudle"
"#,
    );

    assert!(
        error
            .to_string()
            .contains("pocketbase.encryption_env requires")
    );
}

#[test]
fn duplicate_ports_are_rejected() {
    let fixture = ConfigFixture::new();
    let error = fixture.load_error(
        r#"
manifest_version = 1

[remote]
host = "vaie.art"
user = "root"

[caddy]
primary_host = "vaie.art"

[[services]]
name = "one"
kind = "deno_app"
local_path = "src/submodules/vaie.art"
remote_path = "/srv/one"
port = 3000
entrypoint = "index.js"

[[services]]
name = "two"
kind = "deno_app"
local_path = "src/submodules/pudle"
remote_path = "/srv/two"
route_path = "/two"
port = 3000
entrypoint = "index.js"
"#,
    );

    assert!(error.to_string().contains("duplicate Deno port"));
}

#[test]
fn duplicate_routes_are_rejected() {
    let fixture = ConfigFixture::new();
    let error = fixture.load_error(
        r#"
manifest_version = 1

[remote]
host = "vaie.art"
user = "root"

[caddy]
primary_host = "vaie.art"

[[services]]
name = "one"
kind = "static_site"
local_path = "src/submodules/vaie.art"
remote_path = "/web/one"
route_path = "/pudle"

[[services]]
name = "two"
kind = "static_site"
local_path = "src/submodules/pudle"
remote_path = "/web/two"
route_path = "/pudle/"
"#,
    );

    assert!(error.to_string().contains("duplicate Caddy route"));
}

#[test]
fn invalid_mount_paths_are_rejected() {
    let fixture = ConfigFixture::new();
    let error = fixture.load_error(
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
route_path = "pudle"
"#,
    );

    assert!(error.to_string().contains("route_path"));
}

#[test]
fn invalid_service_names_are_rejected() {
    let fixture = ConfigFixture::new();
    let error = fixture.load_error(
        r#"
manifest_version = 1

[remote]
host = "vaie.art"
user = "root"

[caddy]
primary_host = "vaie.art"

[[services]]
name = "Vaie"
kind = "static_site"
local_path = "src/submodules/pudle"
remote_path = "/web/pudle"
route_path = "/pudle"
"#,
    );

    assert!(error.to_string().contains("service name"));
}

#[test]
fn build_rejects_command_and_commands_together() {
    let fixture = ConfigFixture::new();
    let error = fixture.load_error(
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
route_path = "/pudle"

[services.build]
command = ["deno", "task", "build"]
commands = [
    ["deno", "task", "convert-media"],
    ["deno", "task", "build"],
]
"#,
    );

    assert!(error.to_string().contains("either command or commands"));
}

#[test]
fn bare_config_paths_resolve_from_current_directory() {
    assert_eq!(config_directory(Path::new("services.toml")), Path::new("."));
}

#[test]
fn local_config_overrides_private_remote_access() {
    let fixture = ConfigFixture::new();
    fs::write(
        fixture.config_path.with_file_name("services.local.toml"),
        r#"
[remote]
host = "vaie-prod"
user = "deploy"
identity_file = "C:/Users/V/.ssh/vaie_art"
"#,
    )
    .expect("write local config");

    let map = fixture.load(sample_config());

    assert_eq!(map.remote.host.as_deref(), Some("vaie-prod"));
    assert_eq!(map.remote.user.as_deref(), Some("deploy"));
    assert_eq!(
        map.remote.identity_file.as_deref(),
        Some(Path::new("C:/Users/V/.ssh/vaie_art")),
    );
}

struct ConfigFixture {
    _dir: TempDir,
    config_path: PathBuf,
}

impl ConfigFixture {
    fn new() -> Self {
        let dir = TempDir::new().expect("temp dir");
        fs::create_dir_all(dir.path().join("src/pocketbase/pb_migrations")).expect("pb dir");
        fs::create_dir_all(dir.path().join("src/submodules/vaie.art")).expect("vaie dir");
        fs::create_dir_all(dir.path().join("src/submodules/pudle")).expect("pudle dir");

        Self {
            config_path: dir.path().join("services.toml"),
            _dir: dir,
        }
    }

    fn load(&self, source: &str) -> ServiceMap {
        fs::write(&self.config_path, source).expect("write config");
        Config::load(&self.config_path)
            .expect("load config")
            .validate(&self.config_path)
            .expect("validate config")
    }

    fn load_error(&self, source: &str) -> anyhow::Error {
        fs::write(&self.config_path, source).expect("write config");
        Config::load(&self.config_path)
            .expect("load config")
            .validate(&self.config_path)
            .expect_err("validation should fail")
    }
}

fn sample_config() -> &'static str {
    r#"
manifest_version = 1

[remote]

[caddy]
primary_host = "vaie.art"
www_redirect_host = "www.vaie.art"

[[services]]
name = "vaie.art"
kind = "deno_app"
local_path = "src/submodules/vaie.art"
remote_path = "/srv/vaie.art"
port = 3000
entrypoint = "index.js"

[[services]]
name = "pudle"
kind = "static_site"
local_path = "src/submodules/pudle"
remote_path = "/web/pudle"
route_path = "/pudle"
"#
}
