use super::types::{Config, RemoteConfig};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

impl Config {
    pub(super) fn apply_local(&mut self, local_config: LocalConfig) {
        if let Some(remote) = local_config.remote {
            self.remote.apply_local(remote);
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct LocalConfig {
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
    pub(super) fn from_str(source: &str) -> Result<Self> {
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
