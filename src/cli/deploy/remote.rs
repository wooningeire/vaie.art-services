use std::path::Path;

use anyhow::{Result, bail};

use super::super::config::{RemoteConfig, ResolvedPocketBase};
use super::super::process::ProcessCommand;

pub(super) fn rsync_command(
    remote: &RemoteConfig,
    remote_target: &str,
    source: &Path,
    remote_destination: &str,
    delete: bool,
    source_is_dir: bool,
) -> ProcessCommand {
    let mut command = ProcessCommand::new(remote.rsync_program.as_str())
        .arg("-az")
        .arg("-e")
        .arg(ssh_transport(remote));

    if delete {
        command = command.arg("--delete");
    }

    command
        .arg(rsync_source(source, source_is_dir))
        .arg(format!("{remote_target}:{remote_destination}"))
}

pub(super) fn pocketbase_rsync_command(
    remote: &RemoteConfig,
    remote_target: &str,
    pocketbase: &ResolvedPocketBase,
    remote_destination: &str,
) -> ProcessCommand {
    ProcessCommand::new(remote.rsync_program.as_str())
        .arg("-az")
        .arg("--delete")
        .arg("--exclude")
        .arg("pb_data/")
        .arg("-e")
        .arg(ssh_transport(remote))
        .arg(rsync_source(&pocketbase.source_path, true))
        .arg(format!("{remote_target}:{remote_destination}"))
}
pub(super) fn ssh_command(
    remote: &RemoteConfig,
    remote_target: &str,
    script: &str,
) -> ProcessCommand {
    ProcessCommand::new(remote.ssh_program.as_str())
        .args(ssh_args(remote))
        .arg(remote_target)
        .arg(script)
}

fn ssh_args(remote: &RemoteConfig) -> Vec<String> {
    let mut args = vec!["-p".to_string(), remote.port.to_string()];

    if let Some(identity_file) = &remote.identity_file {
        args.push("-i".to_string());
        args.push(identity_file.display().to_string());
    }

    args.extend(remote.extra_ssh_args.clone());
    args
}

fn ssh_transport(remote: &RemoteConfig) -> String {
    let mut parts = vec![sh_quote(&remote.ssh_program)];
    parts.extend(ssh_args(remote).iter().map(|arg| sh_quote(arg)));
    parts.join(" ")
}

pub(super) fn remote_ssh_target(remote: &RemoteConfig) -> Result<String> {
    let host = required_remote_host(remote)?;

    Ok(remote_target(remote, host))
}

pub(super) fn remote_rsync_target(remote: &RemoteConfig) -> Result<String> {
    let host = required_remote_host(remote)?;
    let host = rsync_host(host);

    Ok(remote_target(remote, &host))
}

fn required_remote_host(remote: &RemoteConfig) -> Result<&str> {
    let Some(host) = &remote.host else {
        bail!(
            "remote.host is required for plan/deploy; set it in services.local.toml or use an SSH config host alias",
        );
    };

    Ok(host)
}

fn remote_target(remote: &RemoteConfig, host: &str) -> String {
    if let Some(user) = &remote.user {
        return format!("{user}@{host}");
    }

    host.to_string()
}

fn rsync_host(host: &str) -> String {
    if host.contains(':') && !(host.starts_with('[') && host.ends_with(']')) {
        return format!("[{host}]");
    }

    host.to_string()
}

fn rsync_source(path: &Path, source_is_dir: bool) -> String {
    let mut source = path.display().to_string().replace('\\', "/");

    if source_is_dir && !source.ends_with('/') {
        source.push('/');
    }

    source
}

pub(super) fn remote_child(parent: &str, child: &str) -> String {
    format!(
        "{}/{}",
        parent.trim_end_matches('/'),
        child.trim_start_matches('/')
    )
}

pub(super) fn sh_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }

    let safe = value.chars().all(|char| {
        char.is_ascii_alphanumeric() || matches!(char, '_' | '-' | '.' | '/' | ':' | '@')
    });

    if safe {
        return value.to_string();
    }

    format!("'{}'", value.replace('\'', "'\\''"))
}
