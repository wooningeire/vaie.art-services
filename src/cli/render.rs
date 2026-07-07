use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use super::config::ServiceMap;

mod caddy;
mod systemd;

#[cfg(test)]
mod tests;

use caddy::render_caddyfile;
use systemd::{render_pocketbase_systemd_unit, render_systemd_unit};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderedArtifacts {
    pub caddyfile: String,
    pub systemd_units: Vec<SystemdUnit>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemdUnit {
    pub name: String,
    pub content: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderedPaths {
    pub caddyfile: PathBuf,
    pub systemd_dir: PathBuf,
}

pub fn render_artifacts(map: &ServiceMap) -> RenderedArtifacts {
    let mut systemd_units = map
        .deno_services()
        .map(|service| render_systemd_unit(map, service))
        .collect::<Vec<_>>();

    if let Some(pocketbase) = &map.pocketbase {
        systemd_units.push(render_pocketbase_systemd_unit(pocketbase));
    }

    systemd_units.sort_by(|left, right| left.name.cmp(&right.name));

    RenderedArtifacts {
        caddyfile: render_caddyfile(map),
        systemd_units,
    }
}

pub fn write_artifacts(map: &ServiceMap, output_dir: &Path) -> Result<RenderedPaths> {
    let artifacts = render_artifacts(map);
    let systemd_dir = output_dir.join("systemd");

    fs::create_dir_all(&systemd_dir)
        .with_context(|| format!("failed to create `{}`", systemd_dir.display()))?;

    let caddyfile = output_dir.join("Caddyfile");
    fs::write(&caddyfile, artifacts.caddyfile)
        .with_context(|| format!("failed to write `{}`", caddyfile.display()))?;

    for unit in artifacts.systemd_units {
        let path = systemd_dir.join(&unit.name);
        fs::write(&path, unit.content)
            .with_context(|| format!("failed to write `{}`", path.display()))?;
    }

    Ok(RenderedPaths {
        caddyfile,
        systemd_dir,
    })
}
