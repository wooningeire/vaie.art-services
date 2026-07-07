mod defaults;
mod local;
mod paths;
mod types;
mod validation;

#[cfg(test)]
mod tests;

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

use local::LocalConfig;
use paths::local_config_path;

pub use types::{
    BuildConfig, CaddyConfig, Config, PocketBaseConfig, RemoteConfig, ResolvedPocketBase,
    ResolvedService, ResolvedServiceKind, ServiceConfig, ServiceKind, ServiceMap,
};

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
}
