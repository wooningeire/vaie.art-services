use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessCommand {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
}

impl ProcessCommand {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            env: BTreeMap::new(),
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    pub fn envs(mut self, env: BTreeMap<String, String>) -> Self {
        self.env = env;
        self
    }

    pub fn display(&self) -> String {
        let mut parts = Vec::new();

        if let Some(cwd) = &self.cwd {
            parts.push(format!("cd {}", display_arg(&cwd.display().to_string())));
            parts.push("&&".to_string());
        }

        for (key, value) in &self.env {
            parts.push(format!("{key}={}", display_arg(value)));
        }

        parts.push(display_arg(&self.program));
        parts.extend(self.args.iter().map(|arg| display_arg(arg)));

        parts.join(" ")
    }
}

pub trait CommandRunner {
    fn run(&self, command: &ProcessCommand) -> Result<()>;

    fn output(&self, command: &ProcessCommand) -> Result<String> {
        bail!(
            "runner does not support captured output for `{}`",
            command.display(),
        );
    }
}

#[derive(Default)]
pub struct RealCommandRunner;

impl CommandRunner for RealCommandRunner {
    fn run(&self, command: &ProcessCommand) -> Result<()> {
        let mut process = process_for_command(command)?;

        let status = process.status().with_context(|| {
            format!(
                "failed to start program `{}` for `{}`",
                command.program,
                command.display(),
            )
        })?;

        if !status.success() {
            bail!("command failed with {status}: `{}`", command.display());
        }

        Ok(())
    }

    fn output(&self, command: &ProcessCommand) -> Result<String> {
        let mut process = process_for_command(command)?;

        let output = process.output().with_context(|| {
            format!(
                "failed to start program `{}` for `{}`",
                command.program,
                command.display(),
            )
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stderr = stderr.trim();

            if stderr.is_empty() {
                bail!(
                    "command failed with {}: `{}`",
                    output.status,
                    command.display()
                );
            }

            bail!(
                "command failed with {}: `{}`: {}",
                output.status,
                command.display(),
                stderr,
            );
        }

        String::from_utf8(output.stdout)
            .with_context(|| format!("command output was not UTF-8 for `{}`", command.display()))
    }
}

pub fn ensure_command_can_start(command: &ProcessCommand) -> Result<()> {
    ensure_working_directory(command)?;
    ensure_program_available_from(&command.program, command.cwd.as_deref())
}

pub fn ensure_program_available(program: &str) -> Result<()> {
    ensure_program_available_from(program, None)
}

fn ensure_program_available_from(program: &str, cwd: Option<&Path>) -> Result<()> {
    if resolve_program(program, cwd).is_some() {
        return Ok(());
    }

    if program_has_path(program) {
        if let Some(cwd) = cwd
            && !Path::new(program).is_absolute()
        {
            bail!(
                "program `{program}` does not exist relative to `{}`",
                cwd.display(),
            );
        }

        bail!("program `{program}` does not exist");
    }

    bail!("program `{program}` was not found on PATH")
}

fn ensure_working_directory(command: &ProcessCommand) -> Result<()> {
    let Some(cwd) = &command.cwd else {
        return Ok(());
    };

    if !cwd.exists() {
        bail!(
            "working directory `{}` does not exist for `{}`",
            cwd.display(),
            command.display(),
        );
    }

    if !cwd.is_dir() {
        bail!(
            "working directory `{}` is not a directory for `{}`",
            cwd.display(),
            command.display(),
        );
    }

    Ok(())
}

fn process_for_command(command: &ProcessCommand) -> Result<Command> {
    ensure_working_directory(command)?;

    let program = resolve_program(&command.program, command.cwd.as_deref())
        .unwrap_or_else(|| PathBuf::from(&command.program));
    let mut process = Command::new(program);
    process.args(&command.args);

    if let Some(cwd) = &command.cwd {
        process.current_dir(cwd);
    }

    for (key, value) in &command.env {
        process.env(key, value);
    }

    Ok(process)
}

fn resolve_program(program: &str, cwd: Option<&Path>) -> Option<PathBuf> {
    let program_path = Path::new(program);

    if program_has_path(program) {
        let path = if program_path.is_absolute() {
            program_path.to_path_buf()
        } else if let Some(cwd) = cwd {
            cwd.join(program_path)
        } else {
            program_path.to_path_buf()
        };

        return path.exists().then_some(path);
    }

    resolve_program_on_path(program)
}

fn resolve_program_on_path(program: &str) -> Option<PathBuf> {
    let Some(paths) = env::var_os("PATH") else {
        return None;
    };

    env::split_paths(&paths).find_map(|path| {
        candidate_program_names(program)
            .into_iter()
            .map(|candidate| path.join(candidate))
            .find(|candidate| candidate.exists())
    })
}

fn program_has_path(program: &str) -> bool {
    Path::new(program).components().count() > 1
}

fn candidate_program_names(program: &str) -> Vec<String> {
    #[cfg(windows)]
    {
        if Path::new(program).extension().is_some() {
            return vec![program.to_string()];
        }

        let extensions = env::var_os("PATHEXT")
            .and_then(|value| value.into_string().ok())
            .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_string());
        let mut candidates = vec![program.to_string()];

        candidates.extend(
            extensions
                .split(';')
                .filter(|extension| !extension.is_empty())
                .map(|extension| format!("{program}{extension}")),
        );

        candidates
    }

    #[cfg(not(windows))]
    {
        unix_candidate_program_names(program, windows_interop_programs_are_available())
    }
}

#[cfg(not(windows))]
fn unix_candidate_program_names(program: &str, include_windows_interop: bool) -> Vec<String> {
    let mut candidates = vec![program.to_string()];

    // WSL can launch Windows tools, but the Linux PATH search does not apply PATHEXT.
    if include_windows_interop && Path::new(program).extension().is_none() {
        candidates.push(format!("{program}.exe"));
    }

    candidates
}

#[cfg(not(windows))]
fn windows_interop_programs_are_available() -> bool {
    env::var_os("WSL_DISTRO_NAME").is_some() || env::var_os("WSL_INTEROP").is_some()
}

fn display_arg(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }

    let safe = value.chars().all(|char| {
        char.is_ascii_alphanumeric() || matches!(char, '_' | '-' | '.' | '/' | ':' | '\\')
    });

    if safe {
        return value.to_string();
    }

    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn missing_bare_program_reports_path_error() {
        let error = ensure_program_available("__vaieart_missing_program__")
            .expect_err("program should be missing");

        assert!(error.to_string().contains("was not found on PATH"));
    }

    #[test]
    fn relative_program_checks_command_working_directory() {
        let dir = TempDir::new().expect("temp dir");
        let scripts_dir = dir.path().join("scripts");
        fs::create_dir(&scripts_dir).expect("scripts dir");
        fs::write(scripts_dir.join("build"), "").expect("build script");
        let command = ProcessCommand::new("./scripts/build").cwd(dir.path());

        ensure_command_can_start(&command).expect("relative program should resolve from cwd");
    }

    #[test]
    fn missing_working_directory_reports_directory_error() {
        let dir = TempDir::new().expect("temp dir");
        let command =
            ProcessCommand::new("__vaieart_missing_program__").cwd(dir.path().join("missing"));
        let runner = RealCommandRunner;
        let error = runner
            .run(&command)
            .expect_err("working directory should be missing");

        assert!(error.to_string().contains("working directory"));
    }

    #[cfg(not(windows))]
    #[test]
    fn wsl_interop_candidates_include_exe_names() {
        assert_eq!(
            unix_candidate_program_names("deno", true),
            vec!["deno".to_string(), "deno.exe".to_string()],
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn explicit_extensions_do_not_add_exe_candidates() {
        assert_eq!(
            unix_candidate_program_names("deno.exe", true),
            vec!["deno.exe".to_string()],
        );
    }
}
