use std::path::PathBuf;

pub(super) fn default_manifest_version() -> u8 {
    1
}

pub(super) fn default_ssh_port() -> u16 {
    22
}

pub(super) fn default_ssh_program() -> String {
    "ssh".to_string()
}

pub(super) fn default_rsync_program() -> String {
    "rsync".to_string()
}

pub(super) fn default_tmp_dir() -> String {
    "/tmp/vaieart-services".to_string()
}

pub(super) fn default_caddyfile_path() -> String {
    "/etc/caddy/Caddyfile".to_string()
}

pub(super) fn default_systemd_dir() -> String {
    "/etc/systemd/system".to_string()
}

pub(super) fn default_managed_prefix() -> String {
    "vaieart-".to_string()
}

pub(super) fn default_deno_bin() -> String {
    "/root/.deno/bin/deno".to_string()
}

pub(super) fn default_sync_source() -> PathBuf {
    PathBuf::from(".")
}

pub(super) fn default_pocketbase_port() -> u16 {
    8090
}

pub(super) fn default_pocketbase_binary() -> String {
    "/usr/local/bin/pocketbase".to_string()
}

pub(super) fn default_pocketbase_request_body_max_size() -> String {
    "25MB".to_string()
}

pub(super) fn default_pocketbase_read_timeout() -> String {
    "360s".to_string()
}

pub(super) fn default_warp_proxy_port() -> u16 {
    40000
}

pub(super) fn default_warp_cli() -> String {
    "/usr/bin/warp-cli".to_string()
}

pub(super) fn default_warp_daemon_service() -> String {
    "warp-svc.service".to_string()
}
