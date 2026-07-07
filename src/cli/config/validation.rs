use super::paths::{config_directory, resolve_local_path};
use super::types::*;
use anyhow::{Context, Result, bail};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

impl Config {
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
        let pocketbase = self
            .pocketbase
            .map(|pocketbase| {
                resolve_pocketbase(
                    pocketbase,
                    &root,
                    &self.remote.managed_prefix,
                    &mut ports,
                    &mut routes,
                    &mut systemd_units,
                )
            })
            .transpose()?;

        for service in self.services {
            validate_service_name(&service.name)?;

            if !names.insert(service.name.clone()) {
                bail!("duplicate service name `{}`", service.name);
            }

            let local_path = resolve_local_path(&root, &service.local_path)?;
            validate_service_local_path(&service.name, &local_path, &root, &submodules_root)?;

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
            pocketbase,
            services,
        })
    }
}

fn resolve_pocketbase(
    pocketbase: PocketBaseConfig,
    root: &Path,
    managed_prefix: &str,
    ports: &mut BTreeSet<u16>,
    routes: &mut BTreeSet<String>,
    systemd_units: &mut BTreeSet<String>,
) -> Result<ResolvedPocketBase> {
    validate_service_name(&pocketbase.name)?;
    validate_host("pocketbase.host", &pocketbase.host)?;

    if pocketbase.port == 0 {
        bail!("pocketbase port must be greater than 0");
    }

    if !ports.insert(pocketbase.port) {
        bail!("duplicate Deno/PocketBase port `{}`", pocketbase.port);
    }

    let route_key = format!("{}:/", pocketbase.host);
    if !routes.insert(route_key.clone()) {
        bail!("duplicate Caddy route `{route_key}`");
    }

    let service_name = pocketbase
        .service_name
        .unwrap_or_else(|| generated_service_name(managed_prefix, &pocketbase.name));
    validate_systemd_service_name(&pocketbase.name, managed_prefix, &service_name)?;

    if !systemd_units.insert(service_name.clone()) {
        bail!("duplicate systemd service `{service_name}`");
    }

    let source_path = resolve_local_path(root, &pocketbase.source_path)?;
    if !source_path.starts_with(root) {
        bail!("pocketbase source_path must be under `{}`", root.display(),);
    }

    validate_remote_path("pocketbase.remote_path", &pocketbase.remote_path)?;
    validate_remote_path("pocketbase.data_dir", &pocketbase.data_dir)?;
    validate_remote_path("pocketbase.binary", &pocketbase.binary)?;

    if let Some(backup_dir) = &pocketbase.backup_dir {
        validate_remote_path("pocketbase.backup_dir", backup_dir)?;
    }

    if let Some(environment_file) = &pocketbase.environment_file {
        validate_remote_path("pocketbase.environment_file", environment_file)?;
    }

    validate_token(
        "pocketbase.request_body_max_size",
        &pocketbase.request_body_max_size,
    )?;
    validate_token("pocketbase.read_timeout", &pocketbase.read_timeout)?;

    if let Some(encryption_env) = &pocketbase.encryption_env {
        validate_environment_key("pocketbase.encryption_env", encryption_env)?;
    }

    if pocketbase.encryption_env.is_some() && pocketbase.environment_file.is_none() {
        bail!(
            "pocketbase.encryption_env requires pocketbase.environment_file so systemd can load the secret"
        );
    }

    Ok(ResolvedPocketBase {
        name: pocketbase.name,
        host: pocketbase.host,
        source_path,
        remote_path: pocketbase.remote_path,
        data_dir: pocketbase.data_dir,
        backup_dir: pocketbase.backup_dir,
        port: pocketbase.port,
        binary: pocketbase.binary,
        service_name,
        environment_file: pocketbase.environment_file,
        request_body_max_size: pocketbase.request_body_max_size,
        read_timeout: pocketbase.read_timeout,
        encryption_env: pocketbase.encryption_env,
    })
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

fn validate_service_local_path(
    service_name: &str,
    local_path: &Path,
    root: &Path,
    submodules_root: &Path,
) -> Result<()> {
    let local_repositories_root = root.parent().unwrap_or(root);

    if local_path.starts_with(submodules_root) || local_path.starts_with(local_repositories_root) {
        return Ok(());
    }

    bail!(
        "service `{service_name}` local_path must be under `{}` or sibling repo root `{}`",
        submodules_root.display(),
        local_repositories_root.display(),
    );
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

fn validate_token(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.contains(char::is_whitespace) || value.contains('\0') {
        bail!("{label} must be a single non-empty token");
    }

    Ok(())
}

fn validate_environment_key(label: &str, value: &str) -> Result<()> {
    let valid = !value.is_empty()
        && value
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || char == '_')
        && value
            .chars()
            .next()
            .is_some_and(|char| char.is_ascii_alphabetic() || char == '_');

    if !valid {
        bail!("{label} must be a valid environment variable name");
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
        if build.command.is_some() && !build.commands.is_empty() {
            bail!("service `{service_name}` build must use either command or commands, not both");
        }

        let commands = build.commands();

        if commands.is_empty() {
            bail!("service `{service_name}` build must contain at least one command");
        }

        for command in commands {
            if command.is_empty() || command[0].trim().is_empty() {
                bail!("service `{service_name}` build command must contain a program");
            }

            if command.iter().any(|part| part.contains('\0')) {
                bail!("service `{service_name}` build command contains an invalid argument");
            }
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
