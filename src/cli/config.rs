use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_manifest_version")]
    pub manifest_version: u8,
    pub remote: RemoteConfig,
    pub caddy: CaddyConfig,
    #[serde(default)]
    pub services: Vec<ServiceConfig>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RemoteConfig {
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    #[serde(default)]
    pub identity_file: Option<PathBuf>,
    #[serde(default)]
    pub extra_ssh_args: Vec<String>,
    #[serde(default = "default_ssh_program")]
    pub ssh_program: String,
    #[serde(default = "default_rsync_program")]
    pub rsync_program: String,
    #[serde(default = "default_tmp_dir")]
    pub tmp_dir: String,
    #[serde(default = "default_caddyfile_path")]
    pub caddyfile_path: String,
    #[serde(default = "default_systemd_dir")]
    pub systemd_dir: String,
    #[serde(default = "default_managed_prefix")]
    pub managed_prefix: String,
    #[serde(default = "default_deno_bin")]
    pub deno_bin: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CaddyConfig {
    pub primary_host: String,
    #[serde(default)]
    pub www_redirect_host: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ServiceConfig {
    pub name: String,
    pub kind: ServiceKind,
    pub local_path: PathBuf,
    pub remote_path: String,
    #[serde(default = "default_sync_source")]
    pub sync_source: PathBuf,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub route_path: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub entrypoint: Option<String>,
    #[serde(default)]
    pub service_name: Option<String>,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    #[serde(default)]
    pub build: Option<BuildConfig>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ServiceKind {
    DenoApp,
    StaticSite,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct BuildConfig {
    pub command: Vec<String>,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct ServiceMap {
    pub root: PathBuf,
    pub remote: RemoteConfig,
    pub caddy: CaddyConfig,
    pub services: Vec<ResolvedService>,
}

#[derive(Clone, Debug)]
pub struct ResolvedService {
    pub name: String,
    pub kind: ResolvedServiceKind,
    pub local_path: PathBuf,
    pub remote_path: String,
    pub sync_source: PathBuf,
    pub host: String,
    pub route_path: String,
    pub build: Option<BuildConfig>,
}

#[derive(Clone, Debug)]
pub enum ResolvedServiceKind {
    DenoApp {
        port: u16,
        entrypoint: String,
        service_name: String,
        environment: BTreeMap<String, String>,
    },
    StaticSite,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let source = fs::read_to_string(path)
            .with_context(|| format!("failed to read config `{}`", path.display()))?;
        let mut config = Self::from_str(&source)
            .with_context(|| format!("failed to parse config `{}`", path.display()))?;
        let local_path = local_config_path(path);

        if local_path.exists() {
            let source = fs::read_to_string(&local_path)
                .with_context(|| format!("failed to read config `{}`", local_path.display()))?;
            let local_config = LocalConfig::from_str(&source)
                .with_context(|| format!("failed to parse config `{}`", local_path.display()))?;

            config.apply_local(local_config);
        }

        Ok(config)
    }

    pub fn from_str(source: &str) -> Result<Self> {
        toml::from_str(source).context("invalid TOML service config")
    }

    pub fn validate(self, config_path: &Path) -> Result<ServiceMap> {
        if self.manifest_version != 1 {
            bail!(
                "unsupported manifest_version `{}`; expected 1",
                self.manifest_version
            );
        }

        validate_remote(&self.remote)?;
        validate_host("caddy.primary_host", &self.caddy.primary_host)?;

        if let Some(host) = &self.caddy.www_redirect_host {
            validate_host("caddy.www_redirect_host", host)?;
        }

        if self.services.is_empty() {
            bail!("config must contain at least one service");
        }

        let root = config_directory(config_path)
            .canonicalize()
            .with_context(|| {
                format!(
                    "failed to canonicalize config directory for `{}`",
                    config_path.display(),
                )
            })?;
        let submodules_root = root.join("src").join("submodules");

        let mut names = BTreeSet::new();
        let mut ports = BTreeSet::new();
        let mut routes = BTreeSet::new();
        let mut systemd_units = BTreeSet::new();
        let mut services = Vec::with_capacity(self.services.len());

        for service in self.services {
            validate_service_name(&service.name)?;

            if !names.insert(service.name.clone()) {
                bail!("duplicate service name `{}`", service.name);
            }

            let local_path = resolve_local_path(&root, &service.local_path)?;
            if !local_path.starts_with(&submodules_root) {
                bail!(
                    "service `{}` local_path must be under `{}`",
                    service.name,
                    submodules_root.display(),
                );
            }

            let sync_source = validate_sync_source(&service.name, &service.sync_source)?;
            validate_remote_path(&service.name, &service.remote_path)?;

            let host = service
                .host
                .clone()
                .unwrap_or_else(|| self.caddy.primary_host.clone());
            validate_host(&format!("service `{}` host", service.name), &host)?;

            let route_path = match service.kind {
                ServiceKind::DenoApp => service
                    .route_path
                    .as_deref()
                    .map(normalize_route_path)
                    .transpose()?
                    .unwrap_or_else(|| "/".to_string()),
                ServiceKind::StaticSite => {
                    let route_path = service.route_path.as_deref().with_context(|| {
                        format!("static_site `{}` requires route_path", service.name)
                    })?;
                    normalize_route_path(route_path)?
                }
            };

            let route_key = format!("{host}:{route_path}");
            if !routes.insert(route_key.clone()) {
                bail!("duplicate Caddy route `{route_key}`");
            }

            let kind = match service.kind {
                ServiceKind::DenoApp => {
                    let port = service
                        .port
                        .with_context(|| format!("deno_app `{}` requires port", service.name))?;
                    let entrypoint = service.entrypoint.clone().with_context(|| {
                        format!("deno_app `{}` requires entrypoint", service.name)
                    })?;

                    if port == 0 {
                        bail!("deno_app `{}` port must be greater than 0", service.name);
                    }

                    if !ports.insert(port) {
                        bail!("duplicate Deno port `{port}`");
                    }

                    validate_entrypoint(&service.name, &entrypoint)?;

                    let service_name = service.service_name.clone().unwrap_or_else(|| {
                        generated_service_name(&self.remote.managed_prefix, &service.name)
                    });
                    validate_systemd_service_name(
                        &service.name,
                        &self.remote.managed_prefix,
                        &service_name,
                    )?;

                    if !systemd_units.insert(service_name.clone()) {
                        bail!("duplicate systemd service `{service_name}`");
                    }

                    ResolvedServiceKind::DenoApp {
                        port,
                        entrypoint,
                        service_name,
                        environment: service.environment.clone(),
                    }
                }
                ServiceKind::StaticSite => ResolvedServiceKind::StaticSite,
            };

            validate_build(&service.name, &service.build)?;

            services.push(ResolvedService {
                name: service.name,
                kind,
                local_path,
                remote_path: service.remote_path,
                sync_source,
                host,
                route_path,
                build: service.build,
            });
        }

        Ok(ServiceMap {
            root,
            remote: self.remote,
            caddy: self.caddy,
            services,
        })
    }

    fn apply_local(&mut self, local_config: LocalConfig) {
        if let Some(remote) = local_config.remote {
            self.remote.apply_local(remote);
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
struct LocalConfig {
    #[serde(default)]
    remote: Option<LocalRemoteConfig>,
}

#[derive(Clone, Debug, Deserialize)]
struct LocalRemoteConfig {
    #[serde(default)]
    host: Option<String>,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    identity_file: Option<PathBuf>,
    #[serde(default)]
    extra_ssh_args: Option<Vec<String>>,
    #[serde(default)]
    ssh_program: Option<String>,
    #[serde(default)]
    rsync_program: Option<String>,
    #[serde(default)]
    tmp_dir: Option<String>,
    #[serde(default)]
    caddyfile_path: Option<String>,
    #[serde(default)]
    systemd_dir: Option<String>,
    #[serde(default)]
    managed_prefix: Option<String>,
    #[serde(default)]
    deno_bin: Option<String>,
}

impl LocalConfig {
    fn from_str(source: &str) -> Result<Self> {
        toml::from_str(source).context("invalid TOML local service config")
    }
}

impl RemoteConfig {
    fn apply_local(&mut self, local: LocalRemoteConfig) {
        if local.host.is_some() {
            self.host = local.host;
        }

        if local.user.is_some() {
            self.user = local.user;
        }

        if let Some(port) = local.port {
            self.port = port;
        }

        if local.identity_file.is_some() {
            self.identity_file = local.identity_file;
        }

        if let Some(extra_ssh_args) = local.extra_ssh_args {
            self.extra_ssh_args = extra_ssh_args;
        }

        if let Some(ssh_program) = local.ssh_program {
            self.ssh_program = ssh_program;
        }

        if let Some(rsync_program) = local.rsync_program {
            self.rsync_program = rsync_program;
        }

        if let Some(tmp_dir) = local.tmp_dir {
            self.tmp_dir = tmp_dir;
        }

        if let Some(caddyfile_path) = local.caddyfile_path {
            self.caddyfile_path = caddyfile_path;
        }

        if let Some(systemd_dir) = local.systemd_dir {
            self.systemd_dir = systemd_dir;
        }

        if let Some(managed_prefix) = local.managed_prefix {
            self.managed_prefix = managed_prefix;
        }

        if let Some(deno_bin) = local.deno_bin {
            self.deno_bin = deno_bin;
        }
    }
}

impl ServiceMap {
    pub fn deno_services(&self) -> impl Iterator<Item = &ResolvedService> {
        self.services
            .iter()
            .filter(|service| matches!(service.kind, ResolvedServiceKind::DenoApp { .. }))
    }

    pub fn systemd_service_names(&self) -> Vec<String> {
        self.deno_services()
            .filter_map(|service| match &service.kind {
                ResolvedServiceKind::DenoApp { service_name, .. } => Some(service_name.clone()),
                ResolvedServiceKind::StaticSite => None,
            })
            .collect()
    }
}

fn validate_remote(remote: &RemoteConfig) -> Result<()> {
    if let Some(host) = &remote.host {
        validate_host("remote.host", host)?;
    }

    if let Some(user) = &remote.user
        && user.trim().is_empty()
    {
        bail!("remote.user must not be empty when set");
    }

    if remote.ssh_program.trim().is_empty() {
        bail!("remote.ssh_program is required");
    }

    if remote.rsync_program.trim().is_empty() {
        bail!("remote.rsync_program is required");
    }

    if remote.managed_prefix.trim().is_empty() {
        bail!("remote.managed_prefix is required");
    }

    validate_remote_path("remote.tmp_dir", &remote.tmp_dir)?;
    validate_remote_path("remote.caddyfile_path", &remote.caddyfile_path)?;
    validate_remote_path("remote.systemd_dir", &remote.systemd_dir)?;

    if remote.deno_bin.trim().is_empty() {
        bail!("remote.deno_bin is required");
    }

    Ok(())
}

fn validate_service_name(name: &str) -> Result<()> {
    let valid = !name.is_empty()
        && name.chars().enumerate().all(|(index, char)| {
            char.is_ascii_lowercase()
                || char.is_ascii_digit()
                || matches!(char, '-' | '_' | '.') && index > 0
        });

    if !valid {
        bail!(
            "service name `{name}` must use lowercase ASCII letters, digits, dots, dashes, or underscores",
        );
    }

    Ok(())
}

fn validate_host(label: &str, host: &str) -> Result<()> {
    if host.trim().is_empty() || host.contains('/') || host.contains(char::is_whitespace) {
        bail!("{label} is not a valid host name");
    }

    Ok(())
}

fn validate_remote_path(service_name: &str, path: &str) -> Result<()> {
    if !path.starts_with('/') || path.contains("/../") || path.ends_with("/..") {
        bail!("{service_name} remote path `{path}` must be an absolute path without `..`");
    }

    Ok(())
}

fn validate_entrypoint(service_name: &str, entrypoint: &str) -> Result<()> {
    if entrypoint.trim().is_empty() || entrypoint.contains('\0') || entrypoint.contains("..") {
        bail!("deno_app `{service_name}` entrypoint is invalid");
    }

    Ok(())
}

fn validate_sync_source(service_name: &str, source: &Path) -> Result<PathBuf> {
    if source.as_os_str().is_empty()
        || source.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        bail!("service `{service_name}` sync_source must be a relative path without `..`");
    }

    Ok(source.to_path_buf())
}

fn validate_build(service_name: &str, build: &Option<BuildConfig>) -> Result<()> {
    if let Some(build) = build {
        if build.command.is_empty() || build.command[0].trim().is_empty() {
            bail!("service `{service_name}` build.command must contain a program");
        }

        if build.command.iter().any(|part| part.contains('\0')) {
            bail!("service `{service_name}` build.command contains an invalid argument");
        }
    }

    Ok(())
}

fn validate_systemd_service_name(
    service_name: &str,
    managed_prefix: &str,
    systemd_name: &str,
) -> Result<()> {
    if !systemd_name.starts_with(managed_prefix) || !systemd_name.ends_with(".service") {
        bail!(
            "systemd service for `{service_name}` must start with `{managed_prefix}` and end with `.service`",
        );
    }

    let valid = systemd_name
        .chars()
        .all(|char| char.is_ascii_alphanumeric() || matches!(char, '.' | '_' | '-' | '@'));

    if !valid {
        bail!("systemd service name `{systemd_name}` contains invalid characters");
    }

    Ok(())
}

fn normalize_route_path(path: &str) -> Result<String> {
    if !path.starts_with('/') || path.contains("//") || path.contains("..") {
        bail!("route_path `{path}` must start with `/` and must not contain `..`");
    }

    let normalized = path.trim_end_matches('/');

    if normalized.is_empty() {
        Ok("/".to_string())
    } else {
        Ok(normalized.to_string())
    }
}

fn generated_service_name(managed_prefix: &str, service_name: &str) -> String {
    let sanitized = service_name
        .chars()
        .map(|char| {
            if char.is_ascii_alphanumeric() {
                char
            } else {
                '-'
            }
        })
        .collect::<String>();

    format!("{managed_prefix}{sanitized}.service")
}

fn resolve_local_path(root: &Path, local_path: &Path) -> Result<PathBuf> {
    let path = if local_path.is_absolute() {
        local_path.to_path_buf()
    } else {
        root.join(local_path)
    };

    path.canonicalize()
        .with_context(|| format!("local_path `{}` does not exist", local_path.display()))
}

fn config_directory(config_path: &Path) -> &Path {
    config_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn local_config_path(config_path: &Path) -> PathBuf {
    let file_stem = config_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("services");
    let local_name = format!("{file_stem}.local.toml");

    config_directory(config_path).join(local_name)
}

fn default_manifest_version() -> u8 {
    1
}

fn default_ssh_port() -> u16 {
    22
}

fn default_ssh_program() -> String {
    "ssh".to_string()
}

fn default_rsync_program() -> String {
    "rsync".to_string()
}

fn default_tmp_dir() -> String {
    "/tmp/vaieart-services".to_string()
}

fn default_caddyfile_path() -> String {
    "/etc/caddy/Caddyfile".to_string()
}

fn default_systemd_dir() -> String {
    "/etc/systemd/system".to_string()
}

fn default_managed_prefix() -> String {
    "vaieart-".to_string()
}

fn default_deno_bin() -> String {
    "/root/.deno/bin/deno".to_string()
}

fn default_sync_source() -> PathBuf {
    PathBuf::from(".")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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
}
