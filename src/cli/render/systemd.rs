use super::super::config::{ResolvedPocketBase, ResolvedService, ResolvedServiceKind, ServiceMap};
use super::SystemdUnit;

pub(super) fn render_systemd_unit(map: &ServiceMap, service: &ResolvedService) -> SystemdUnit {
    let ResolvedServiceKind::DenoApp {
        port,
        entrypoint,
        service_name,
        environment,
    } = &service.kind
    else {
        unreachable!("systemd unit rendering only receives deno services");
    };

    let mut env = environment.clone();
    env.entry("HOST".to_string())
        .or_insert_with(|| "127.0.0.1".to_string());
    env.entry("PORT".to_string())
        .or_insert_with(|| port.to_string());

    let mut output = String::new();
    output.push_str("[Unit]\n");
    output.push_str("Description=vaie.art managed Deno service - ");
    output.push_str(&service.name);
    output.push('\n');
    output.push_str("After=network-online.target\n");
    output.push_str("Wants=network-online.target\n\n");
    output.push_str("[Service]\n");
    output.push_str("Type=simple\n");
    output.push_str("User=root\n");
    output.push_str("Group=root\n");
    output.push_str("WorkingDirectory=");
    output.push_str(&service.remote_path);
    output.push('\n');

    for (key, value) in env {
        output.push_str("Environment=");
        output.push_str(&systemd_environment_value(&key, &value));
        output.push('\n');
    }

    output.push_str("ExecStart=");
    output.push_str(&map.remote.deno_bin);
    output.push_str(" run -A ");
    output.push_str(&entrypoint_argument(entrypoint));
    output.push('\n');
    output.push_str("Restart=on-failure\n");
    output.push_str("RestartSec=3\n\n");
    output.push_str("[Install]\n");
    output.push_str("WantedBy=multi-user.target\n");

    SystemdUnit {
        name: service_name.clone(),
        content: output,
    }
}

pub(super) fn render_pocketbase_systemd_unit(pocketbase: &ResolvedPocketBase) -> SystemdUnit {
    let mut output = String::new();
    output.push_str("[Unit]\n");
    output.push_str("Description=vaie.art managed PocketBase service - ");
    output.push_str(&pocketbase.name);
    output.push('\n');
    output.push_str("After=network-online.target\n");
    output.push_str("Wants=network-online.target\n\n");
    output.push_str("[Service]\n");
    output.push_str("Type=simple\n");
    output.push_str("User=root\n");
    output.push_str("Group=root\n");
    output.push_str("WorkingDirectory=");
    output.push_str(&pocketbase.remote_path);
    output.push('\n');
    output.push_str("LimitNOFILE=4096\n");

    if let Some(environment_file) = &pocketbase.environment_file {
        output.push_str("EnvironmentFile=");
        output.push_str(environment_file);
        output.push('\n');
    }

    output.push_str("ExecStart=");
    output.push_str(&pocketbase.binary);
    output.push_str(" serve --http=127.0.0.1:");
    output.push_str(&pocketbase.port.to_string());
    output.push_str(" --dir=");
    output.push_str(&pocketbase.data_dir);
    output.push_str(" --migrationsDir=");
    output.push_str(&remote_child(&pocketbase.remote_path, "pb_migrations"));

    if let Some(encryption_env) = &pocketbase.encryption_env {
        output.push_str(" --encryptionEnv=");
        output.push_str(encryption_env);
    }

    output.push('\n');
    output.push_str("Restart=on-failure\n");
    output.push_str("RestartSec=3\n\n");
    output.push_str("[Install]\n");
    output.push_str("WantedBy=multi-user.target\n");

    SystemdUnit {
        name: pocketbase.service_name.clone(),
        content: output,
    }
}

fn remote_child(parent: &str, child: &str) -> String {
    format!(
        "{}/{}",
        parent.trim_end_matches('/'),
        child.trim_start_matches('/'),
    )
}

fn entrypoint_argument(entrypoint: &str) -> String {
    if entrypoint.starts_with('/') || entrypoint.starts_with("./") {
        entrypoint.to_string()
    } else {
        format!("./{entrypoint}")
    }
}

fn systemd_environment_value(key: &str, value: &str) -> String {
    format!(
        "\"{}={}\"",
        key,
        value.replace('\\', "\\\\").replace('"', "\\\"")
    )
}
