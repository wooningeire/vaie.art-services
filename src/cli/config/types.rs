use super::defaults::{
    default_caddyfile_path, default_deno_bin, default_managed_prefix, default_manifest_version,
    default_pocketbase_binary, default_pocketbase_port, default_pocketbase_read_timeout,
    default_pocketbase_request_body_max_size, default_rsync_program, default_ssh_port,
    default_ssh_program, default_sync_source, default_systemd_dir, default_tmp_dir,
    default_warp_cli, default_warp_daemon_service, default_warp_proxy_port,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    #[serde(default = "default_manifest_version")]
    pub manifest_version: u8,
    pub remote: RemoteConfig,
    pub caddy: CaddyConfig,
    #[serde(default)]
    pub pocketbase: Option<PocketBaseConfig>,
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
pub struct PocketBaseConfig {
    pub name: String,
    pub host: String,
    pub source_path: PathBuf,
    pub remote_path: String,
    pub data_dir: String,
    #[serde(default)]
    pub backup_dir: Option<String>,
    #[serde(default = "default_pocketbase_port")]
    pub port: u16,
    #[serde(default = "default_pocketbase_binary")]
    pub binary: String,
    #[serde(default)]
    pub service_name: Option<String>,
    #[serde(default)]
    pub environment_file: Option<String>,
    #[serde(default = "default_pocketbase_request_body_max_size")]
    pub request_body_max_size: String,
    #[serde(default = "default_pocketbase_read_timeout")]
    pub read_timeout: String,
    #[serde(default)]
    pub encryption_env: Option<String>,
    #[serde(default)]
    pub warp_proxy: Option<WarpProxyConfig>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct WarpProxyConfig {
    #[serde(default = "default_warp_proxy_port")]
    pub port: u16,
    #[serde(default = "default_warp_cli")]
    pub cli: String,
    #[serde(default = "default_warp_daemon_service")]
    pub daemon_service: String,
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
    #[serde(default)]
    pub command: Option<Vec<String>>,
    #[serde(default)]
    pub commands: Vec<Vec<String>>,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
}

impl BuildConfig {
    pub fn commands(&self) -> Vec<&Vec<String>> {
        if let Some(command) = &self.command {
            return vec![command];
        }

        self.commands.iter().collect()
    }
}

#[derive(Clone, Debug)]
pub struct ServiceMap {
    pub root: PathBuf,
    pub remote: RemoteConfig,
    pub caddy: CaddyConfig,
    pub pocketbase: Option<ResolvedPocketBase>,
    pub services: Vec<ResolvedService>,
}

#[derive(Clone, Debug)]
pub struct ResolvedPocketBase {
    pub name: String,
    pub host: String,
    pub source_path: PathBuf,
    pub remote_path: String,
    pub data_dir: String,
    pub backup_dir: Option<String>,
    pub port: u16,
    pub binary: String,
    pub service_name: String,
    pub environment_file: Option<String>,
    pub request_body_max_size: String,
    pub read_timeout: String,
    pub encryption_env: Option<String>,
    pub warp_proxy: Option<ResolvedWarpProxy>,
}

#[derive(Clone, Debug)]
pub struct ResolvedWarpProxy {
    pub port: u16,
    pub cli: String,
    pub daemon_service: String,
    pub service_name: String,
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

impl ServiceMap {
    pub fn deno_services(&self) -> impl Iterator<Item = &ResolvedService> {
        self.services
            .iter()
            .filter(|service| matches!(service.kind, ResolvedServiceKind::DenoApp { .. }))
    }

    pub fn systemd_service_names(&self) -> Vec<String> {
        let mut service_names = self
            .deno_services()
            .filter_map(|service| match &service.kind {
                ResolvedServiceKind::DenoApp { service_name, .. } => Some(service_name.clone()),
                ResolvedServiceKind::StaticSite => None,
            })
            .collect::<Vec<_>>();

        if let Some(pocketbase) = &self.pocketbase {
            if let Some(warp_proxy) = &pocketbase.warp_proxy {
                service_names.push(warp_proxy.service_name.clone());
            }

            service_names.push(pocketbase.service_name.clone());
        }

        service_names
    }
}
