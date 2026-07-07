use super::super::config::{ResolvedPocketBase, ResolvedService, ResolvedServiceKind, ServiceMap};
use super::remote::{remote_child, sh_quote};

pub(super) fn install_script(map: &ServiceMap) -> String {
    let expected_units = map.systemd_service_names().join(" ");
    let mut script = String::new();

    script.push_str("set -eu\n");
    script.push_str("tmp_dir=");
    script.push_str(&sh_quote(&map.remote.tmp_dir));
    script.push('\n');
    script.push_str("sync_dir=\"$tmp_dir/sync\"\n");
    script.push_str("systemd_tmp=\"$tmp_dir/systemd\"\n");
    script.push_str("caddy_tmp=\"$tmp_dir/Caddyfile\"\n");
    script.push_str("caddyfile_path=");
    script.push_str(&sh_quote(&map.remote.caddyfile_path));
    script.push('\n');
    script.push_str("systemd_dir=");
    script.push_str(&sh_quote(&map.remote.systemd_dir));
    script.push('\n');
    script.push_str("managed_prefix=");
    script.push_str(&sh_quote(&map.remote.managed_prefix));
    script.push('\n');
    script.push_str("expected_units=");
    script.push_str(&sh_quote(&expected_units));
    script.push('\n');
    script.push_str("report_systemctl_failure() {\n");
    script.push_str("    failed_unit=\"$1\"\n");
    script.push_str("    echo \"systemd failed for $failed_unit\" >&2\n");
    script.push_str("    systemctl status \"$failed_unit\" --no-pager --lines=80 || true\n");
    script.push_str("    journalctl -u \"$failed_unit\" --no-pager --lines=120 || true\n");
    script.push_str("}\n");
    script.push_str("backup_dir=\"$tmp_dir/backups/$(date +%Y%m%d%H%M%S)\"\n");
    script.push_str("mkdir -p \"$backup_dir/systemd\"\n");
    script.push_str("if [ -f \"$caddyfile_path\" ]; then cp \"$caddyfile_path\" \"$backup_dir/Caddyfile\"; fi\n");
    script.push_str("for unit_path in \"$systemd_dir\"/\"$managed_prefix\"*.service; do\n");
    script.push_str("    [ -e \"$unit_path\" ] || continue\n");
    script.push_str("    cp \"$unit_path\" \"$backup_dir/systemd/\" || true\n");
    script.push_str("done\n");

    if let Some(pocketbase) = &map.pocketbase {
        append_pocketbase_preflight(&mut script, pocketbase);
    }

    for service in &map.services {
        append_service_install_sync(&mut script, service);
    }

    if let Some(pocketbase) = &map.pocketbase {
        append_pocketbase_install_sync(&mut script, pocketbase);
    }
    script.push_str("caddy_changed=0\n");
    script.push_str(
        "if [ ! -f \"$caddyfile_path\" ] || ! cmp -s \"$caddy_tmp\" \"$caddyfile_path\"; then\n",
    );
    script.push_str("    install -m 0644 \"$caddy_tmp\" \"$caddyfile_path\"\n");
    script.push_str("    caddy_changed=1\n");
    script.push_str("fi\n");
    script.push_str("systemd_changed=0\n");

    for service in &map.services {
        if let ResolvedServiceKind::DenoApp { service_name, .. } = &service.kind {
            script.push_str("unit=");
            script.push_str(&sh_quote(service_name));
            script.push('\n');
            script.push_str("source_unit=\"$systemd_tmp/$unit\"\n");
            script.push_str("target_unit=\"$systemd_dir/$unit\"\n");
            script.push_str("if [ ! -f \"$target_unit\" ] || ! cmp -s \"$source_unit\" \"$target_unit\"; then\n");
            script.push_str("    install -m 0644 \"$source_unit\" \"$target_unit\"\n");
            script.push_str("    systemd_changed=1\n");
            script.push_str("fi\n");
        }
    }

    if let Some(pocketbase) = &map.pocketbase {
        append_systemd_unit_install(&mut script, &pocketbase.service_name);
    }
    script.push_str("for unit_path in \"$systemd_dir\"/\"$managed_prefix\"*.service; do\n");
    script.push_str("    [ -e \"$unit_path\" ] || continue\n");
    script.push_str("    unit=\"$(basename \"$unit_path\")\"\n");
    script.push_str("    case \" $expected_units \" in\n");
    script.push_str("        *\" $unit \"*) ;;\n");
    script.push_str("        *)\n");
    script.push_str("            systemctl stop \"$unit\" || true\n");
    script.push_str("            systemctl disable \"$unit\" || true\n");
    script.push_str("            rm -f \"$unit_path\"\n");
    script.push_str("            systemd_changed=1\n");
    script.push_str("            ;;\n");
    script.push_str("    esac\n");
    script.push_str("done\n");
    script.push_str("if [ \"$systemd_changed\" -eq 1 ]; then systemctl daemon-reload; fi\n");
    script.push_str("for unit in $expected_units; do\n");
    script.push_str("    if ! systemctl enable --now \"$unit\"; then\n");
    script.push_str("        report_systemctl_failure \"$unit\"\n");
    script.push_str("        exit 1\n");
    script.push_str("    fi\n");
    script.push_str("done\n");
    script.push_str("# Artifact syncs can change server code without changing the systemd unit.\n");
    script.push_str("for unit in $expected_units; do\n");
    script.push_str("    if ! systemctl restart \"$unit\"; then\n");
    script.push_str("        report_systemctl_failure \"$unit\"\n");
    script.push_str("        exit 1\n");
    script.push_str("    fi\n");
    script.push_str("done\n");
    script.push_str("if [ \"$caddy_changed\" -eq 1 ]; then systemctl reload caddy || systemctl restart caddy; fi\n");

    script
}

pub(super) fn append_pocketbase_preflight(script: &mut String, pocketbase: &ResolvedPocketBase) {
    script.push_str("if [ ! -x ");
    script.push_str(&sh_quote(&pocketbase.binary));
    script.push_str(" ]; then\n");
    script.push_str("    echo ");
    script.push_str(&sh_quote(&format!(
        "missing PocketBase binary: {}",
        pocketbase.binary,
    )));
    script.push_str(" >&2\n");
    script.push_str("    echo ");
    script.push_str(&sh_quote(
        "install PocketBase on the remote or update pocketbase.binary in services.toml",
    ));
    script.push_str(" >&2\n");
    script.push_str("    exit 1\n");
    script.push_str("fi\n");

    if let Some(environment_file) = &pocketbase.environment_file {
        script.push_str("if [ ! -f ");
        script.push_str(&sh_quote(environment_file));
        script.push_str(" ]; then\n");
        script.push_str("    echo ");
        script.push_str(&sh_quote(&format!(
            "missing PocketBase environment file: {}",
            environment_file,
        )));
        script.push_str(" >&2\n");
        script.push_str("    echo ");
        script.push_str(&sh_quote(
            "create it on the remote; it holds secrets and is not deployed from this repo",
        ));
        script.push_str(" >&2\n");
        script.push_str("    exit 1\n");
        script.push_str("fi\n");

        if let Some(encryption_env) = &pocketbase.encryption_env {
            script.push_str("if ! grep -Eq ");
            script.push_str(&sh_quote(&format!(
                "^[[:space:]]*{}[[:space:]]*=",
                encryption_env,
            )));
            script.push(' ');
            script.push_str(&sh_quote(environment_file));
            script.push_str("; then\n");
            script.push_str("    echo ");
            script.push_str(&sh_quote(&format!(
                "PocketBase environment file does not define {}: {}",
                encryption_env, environment_file,
            )));
            script.push_str(" >&2\n");
            script.push_str("    exit 1\n");
            script.push_str("fi\n");
        }
    }
}

fn append_systemd_unit_install(script: &mut String, service_name: &str) {
    script.push_str("unit=");
    script.push_str(&sh_quote(service_name));
    script.push('\n');
    script.push_str("source_unit=\"$systemd_tmp/$unit\"\n");
    script.push_str("target_unit=\"$systemd_dir/$unit\"\n");
    script.push_str(
        "if [ ! -f \"$target_unit\" ] || ! cmp -s \"$source_unit\" \"$target_unit\"; then\n",
    );
    script.push_str("    install -m 0644 \"$source_unit\" \"$target_unit\"\n");
    script.push_str("    systemd_changed=1\n");
    script.push_str("fi\n");
}
fn append_service_install_sync(script: &mut String, service: &ResolvedService) {
    script.push_str("mkdir -p ");
    script.push_str(&sh_quote(&service.remote_path));
    script.push('\n');
    script.push_str("rsync -a --delete ");
    script.push_str("\"$sync_dir/");
    script.push_str(&service.name);
    script.push_str("/\"");
    script.push(' ');
    script.push_str(&sh_quote(&remote_child(&service.remote_path, "")));
    script.push('\n');
}

fn append_pocketbase_install_sync(script: &mut String, pocketbase: &ResolvedPocketBase) {
    script.push_str("mkdir -p ");
    script.push_str(&sh_quote(&pocketbase.remote_path));
    script.push(' ');
    script.push_str(&sh_quote(&pocketbase.data_dir));

    if let Some(backup_dir) = &pocketbase.backup_dir {
        script.push(' ');
        script.push_str(&sh_quote(backup_dir));
    }

    script.push('\n');
    script.push_str("rsync -a --delete --exclude pb_data/ ");
    script.push_str("\"$sync_dir/");
    script.push_str(&pocketbase.name);
    script.push_str("/\"");
    script.push(' ');
    script.push_str(&sh_quote(&remote_child(&pocketbase.remote_path, "")));
    script.push('\n');
}
