use super::super::config::{
    ResolvedPocketBase, ResolvedService, ResolvedServiceKind, ResolvedWarpProxy, ServiceMap,
};
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

pub(super) fn render_warp_proxy_systemd_unit(
    pocketbase: &ResolvedPocketBase,
    warp_proxy: &ResolvedWarpProxy,
) -> SystemdUnit {
    let mut output = String::new();
    output.push_str("[Unit]\n");
    output.push_str("Description=vaie.art managed WARP proxy for PocketBase - ");
    output.push_str(&pocketbase.name);
    output.push('\n');
    output.push_str("After=network-online.target ");
    output.push_str(&warp_proxy.daemon_service);
    output.push('\n');
    output.push_str("Wants=network-online.target\n");
    output.push_str("Requires=");
    output.push_str(&warp_proxy.daemon_service);
    output.push('\n');
    output.push_str("Before=");
    output.push_str(&pocketbase.service_name);
    output.push_str("\n\n");
    output.push_str("[Service]\n");
    output.push_str("Type=oneshot\n");
    output.push_str("RemainAfterExit=yes\n");
    output.push_str("ExecStart=");
    output.push_str(&warp_proxy.cli);
    output.push_str(" tunnel protocol set MASQUE\n");
    output.push_str("ExecStart=");
    output.push_str(&warp_proxy.cli);
    output.push_str(" mode proxy\n");
    output.push_str("ExecStart=");
    output.push_str(&warp_proxy.cli);
    output.push_str(" proxy port ");
    output.push_str(&warp_proxy.port.to_string());
    output.push('\n');
    output.push_str("ExecStart=");
    output.push_str(&warp_proxy.cli);
    output.push_str(" connect\n");
    output.push_str("ExecStart=/usr/bin/curl --fail --silent --show-error --retry 10 --retry-all-errors --retry-delay 1 --retry-max-time 60 --connect-timeout 3 --max-time 8 --proxy ");
    output.push_str(&warp_proxy_url(warp_proxy.port));
    output.push_str(" https://discord.com/api/v10/gateway\n");
    output.push_str("TimeoutStartSec=120\n\n");
    output.push_str("[Install]\n");
    output.push_str("WantedBy=multi-user.target\n");

    SystemdUnit {
        name: warp_proxy.service_name.clone(),
        content: output,
    }
}

pub(super) fn render_pocketbase_systemd_unit(pocketbase: &ResolvedPocketBase) -> SystemdUnit {
    let mut output = String::new();
    output.push_str("[Unit]\n");
    output.push_str("Description=vaie.art managed PocketBase service - ");
    output.push_str(&pocketbase.name);
    output.push('\n');

    if let Some(warp_proxy) = &pocketbase.warp_proxy {
        output.push_str("After=network-online.target ");
        output.push_str(&warp_proxy.service_name);
        output.push('\n');
        output.push_str("Wants=network-online.target\n");
        output.push_str("Requires=");
        output.push_str(&warp_proxy.service_name);
        output.push_str("\n\n");
    } else {
        output.push_str("After=network-online.target\n");
        output.push_str("Wants=network-online.target\n\n");
    }

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

    if let Some(warp_proxy) = &pocketbase.warp_proxy {
        output.push_str("Environment=");
        output.push_str(&systemd_environment_value(
            "HTTPS_PROXY",
            &warp_proxy_url(warp_proxy.port),
        ));
        output.push('\n');
        output.push_str("Environment=\"NO_PROXY=127.0.0.1,localhost,::1\"\n");
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

fn warp_proxy_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
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
