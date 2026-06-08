use anyhow::Result;
use clap::{Parser, Subcommand};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub mod config;
use config::Config;

pub mod deploy;
use deploy::{build_deployment_command_list_for_output_dir, deploy, format_plan};

pub mod git;
use git::update_repositories;

pub mod process;
use process::{CommandRunner, RealCommandRunner};

pub mod render;
use render::{render_artifacts, write_artifacts};

#[derive(Debug, Parser)]
#[command(name = "vaieart-services")]
#[command(about = "Manage the vaie.art service map and remote deployment artifacts")]
pub struct Cli {
    #[arg(long, default_value = "services.toml")]
    config: PathBuf,
    #[arg(long, default_value = "target/vaieart-services")]
    output_dir: PathBuf,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Check,
    Update,
    Render,
    Plan,
    Deploy,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let output = route_cli_subcommand(cli, &RealCommandRunner)?;

    if !output.is_empty() {
        print!("{output}");
    }

    Ok(())
}

pub fn run_from<I, T>(args: I, runner: &dyn CommandRunner) -> Result<String>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = Cli::try_parse_from(args)?;
    route_cli_subcommand(cli, runner)
}

fn route_cli_subcommand(cli: Cli, runner: &dyn CommandRunner) -> Result<String> {
    let map = load_map(&cli.config)?;

    match cli.command {
        Commands::Check => {
            let artifacts = render_artifacts(&map);
            Ok(format!(
                "Config OK: {} services, {} systemd units\n",
                map.services.len(),
                artifacts.systemd_units.len(),
            ))
        }

        Commands::Update => {
            update_repositories(&map, runner)?;
            Ok("Repositories updated\n".to_string())
        }

        Commands::Render => {
            let paths = write_artifacts(&map, &cli.output_dir)?;
            Ok(format!(
                "Rendered Caddyfile to {}\nRendered systemd units to {}\n",
                paths.caddyfile.display(),
                paths.systemd_dir.display(),
            ))
        }

        Commands::Plan => {
            let command_list = build_deployment_command_list_for_output_dir(&map, &cli.output_dir)?;
            Ok(format_plan(&command_list))
        }

        Commands::Deploy => {
            deploy(&map, &cli.output_dir, runner)?;
            Ok("Deployment complete\n".to_string())
        }
    }
}

fn load_map(config_path: &Path) -> Result<config::ServiceMap> {
    Config::load(config_path)?.validate(config_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use process::{CommandRunner, ProcessCommand};
    use std::cell::RefCell;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn check_command_validates_and_renders_templates() {
        let fixture = CliFixture::new();
        let output = fixture.run(["bin", "--config", fixture.config(), "check"]);

        assert_eq!(output, "Config OK: 2 services, 1 systemd units\n");
    }

    #[test]
    fn render_command_writes_artifacts() {
        let fixture = CliFixture::new();
        let output_dir = fixture.path("out");
        let output = fixture.run([
            "bin",
            "--config",
            fixture.config(),
            "--output-dir",
            output_dir.as_str(),
            "render",
        ]);

        assert!(output.contains("Rendered Caddyfile"));
        assert!(fixture.dir.path().join("out/Caddyfile").exists());
        assert!(
            fixture
                .dir
                .path()
                .join("out/systemd/vaieart-vaie-art.service")
                .exists(),
        );
    }

    #[test]
    fn plan_command_prints_build_rsync_and_remote_validation() {
        let fixture = CliFixture::new();
        let output = fixture.run(["bin", "--config", fixture.config(), "plan"]);

        assert!(output.contains("deno task build"));
        assert!(output.contains("rsync"));
        assert!(output.contains("caddy validate"));
    }

    struct CliFixture {
        dir: TempDir,
        config_path: PathBuf,
        runner: NoopRunner,
    }

    impl CliFixture {
        fn new() -> Self {
            let dir = TempDir::new().expect("temp dir");
            fs::create_dir_all(dir.path().join("src/submodules/vaie.art")).expect("vaie dir");
            fs::create_dir_all(dir.path().join("src/submodules/pudle/build")).expect("pudle build");
            let config_path = dir.path().join("services.toml");
            fs::write(&config_path, sample_config()).expect("write config");

            Self {
                dir,
                config_path,
                runner: NoopRunner::default(),
            }
        }

        fn config(&self) -> &str {
            self.config_path.to_str().expect("utf8 path")
        }

        fn path(&self, relative: &str) -> String {
            self.dir
                .path()
                .join(relative)
                .to_str()
                .expect("utf8 path")
                .to_string()
        }

        fn run<const N: usize>(&self, args: [&str; N]) -> String {
            run_from(args, &self.runner).expect("run cli")
        }
    }

    #[derive(Default)]
    struct NoopRunner {
        commands: RefCell<Vec<ProcessCommand>>,
    }

    impl CommandRunner for NoopRunner {
        fn run(&self, command: &ProcessCommand) -> Result<()> {
            self.commands.borrow_mut().push(command.clone());
            Ok(())
        }
    }

    fn sample_config() -> &'static str {
        r#"
manifest_version = 1

[remote]
host = "vaie.art"
user = "root"

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
sync_source = "build"
route_path = "/pudle"

[services.build]
command = ["deno", "task", "build"]
"#
    }
}
