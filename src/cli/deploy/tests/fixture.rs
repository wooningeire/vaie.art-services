use crate::cli::config::{Config, ServiceMap};
use crate::cli::process::{CommandRunner, ProcessCommand};
use anyhow::Result;
use std::{cell::RefCell, fs, path::PathBuf};
use tempfile::TempDir;

pub(super) struct DeployFixture {
    pub(super) dir: TempDir,
    config_path: PathBuf,
}

impl DeployFixture {
    pub(super) fn new() -> Self {
        Self::with_config(
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
command = ["deno", "task", "build"]
"#,
        )
    }

    pub(super) fn new_without_remote_host() -> Self {
        Self::with_config(
            r#"
manifest_version = 1

[remote]

[caddy]
primary_host = "vaie.art"

[[services]]
name = "pudle"
kind = "static_site"
local_path = "src/submodules/pudle"
remote_path = "/web/pudle"
sync_source = "build"
route_path = "/pudle"
"#,
        )
    }

    pub(super) fn with_config(source: &str) -> Self {
        let dir = TempDir::new().expect("temp dir");
        fs::create_dir_all(dir.path().join("src/pocketbase/pb_migrations")).expect("pb dir");
        fs::create_dir_all(dir.path().join("src/submodules/pudle/build")).expect("build dir");
        let config_path = dir.path().join("services.toml");
        fs::write(&config_path, source).expect("write config");

        Self { dir, config_path }
    }

    pub(super) fn map(&self) -> ServiceMap {
        Config::load(&self.config_path)
            .expect("load config")
            .validate(&self.config_path)
            .expect("validate config")
    }
}

#[derive(Default)]
pub(super) struct RecordingRunner {
    pub(super) commands: RefCell<Vec<ProcessCommand>>,
}

impl CommandRunner for RecordingRunner {
    fn run(&self, command: &ProcessCommand) -> Result<()> {
        self.commands.borrow_mut().push(command.clone());
        Ok(())
    }
}
