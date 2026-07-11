# https://vaie.art/ service map & deployment
Central location for managing, deploying, and updating separate services running on https://vaie.art/.

**Problem:**
1. I have a lot of disparate SvelteKit projects with backends I need to run at the same time, but I don't want a monorepo or a single god-service managing literally everything!
1. I want to keep all of those SvelteKit servers in sync on my host server!

**To this end...** this repository exposes a locally-run CLI that:
1. Takes a number of local Git repositories or submodules (each one hosting server code like Node.js servers, SvelteKit projects, etc.)
    1. ...and keeps them up to date when prompted
1. Based on per-service configuration you provide in `services.toml`:
    1. Autogenerates a `Caddyfile` web routing config
    1. Autogenerates a `systemd` config for each service, for concurrently running all the services
    1. Builds each project using a set of commands
    1. Uploads the `Caddyfile`, `systemd`, and build artifacts to a remote server
        1. ...and cleans up stale builds/configs from previous runs

Outside of work this repository does, you will still need to manually consider:
1. Initial setup of the environment on the remote server
    1. SSH setup 
    1. One-time package installs
1. DNS and nameserver setup on the domain registrar or any proxies

## Usage
1. Clone any new repositories to build services from under `./src/submodules/`, or keep sibling checkouts next to this repo and point `local_path` at them
1. Configure service locations, build commands, build types, etc. in `services.toml`
1. Configure private SSH connection details in `services.local.toml` (gitignored)
1. `cargo run -- check` to validate the format of `services.toml`
1. `cargo run -- update` to pull latest versions of the listed submodules
1. `cargo run -- render` to generate a `Caddyfile` and `systemd` configs
1. `cargo run -- plan` to view the remote build, rsync, validation, and install commands
1. `cargo run -- deploy` to build configured repositories, rsync files to a remote temp directory, validate Caddy, remove stale artifacts and config files on the server, and restart changed services

Shorthand:
1. `cargo run` to update repositories, render artifacts, and deploy

## Commands
- No command: update repositories, render artifacts, and deploy
- `check`: validate config, local repo paths, PocketBase paths, ports, routes, and generated templates
- `update`: find the Git repo root for each configured source path, dedupe those repos, run `git fetch --all --prune`, then move each checkout to `origin/HEAD` with `git reset --hard origin/HEAD`. Untracked files are left alone
- `pull-pb-migrations`: copy new remote PocketBase JavaScript migrations into the configured `pocketbase.source_path/pb_migrations`. Existing local files are not overwritten and local-only files are not deleted
- `render`: write generated artifacts to `target/vaieart-services/`
- `plan`: print the deployment command sequence without running it
- `deploy`: render artifacts, run local build commands, upload with `rsync`, validate Caddy, and apply changes over SSH.

## Dependencies
CLI needs `git`, `ssh`, `rsync`, `cargo`

Configured build commands also need their local tools. For Svelte apps, `deno` must resolve on `PATH`; in WSL this can be either Linux `deno` or Windows `deno.exe` through interop

Remote hosts must already have
1. `caddy`
1. `systemd`
1. Deno at the location specified by `remote.deno_bin`
1. PocketBase at the configured `pocketbase.binary` path, currently `/opt/pocketbase/pocketbase`
1. Cloudflare WARP and `curl` when `pocketbase.warp_proxy` is configured
1. ... any non-Deno service binaries configured in `services.toml`

Windows: consider using WSL with one of the below distros
```sh
# in this repo's directory (example, your distro and version will likely not be the same):
wsl install fedora  # install a new distro
wsl -d FedoraLinux-44  # enter the installed distro
```

Fedora:
```sh
sudo dnf install -y git rsync openssh-clients
```

Debian/Ubuntu:
```sh
sudo apt update
sudo apt install -y git rsync openssh-client
```

## Builds and rsync
In `services.toml`, each service can define build commands:
```toml
[services.build]
commands = [
    ["deno", "task", "convert-media"],
    ["deno", "task", "build"],
]
```

The build runs locally in the configured repo before uploading the artifact with `rsync` by default. Use `sync_source` to specify the build output directory (e.g. `build`, `dist`). The build output directory is mirrored to the specified remote path; any stale files in that path are deleted

## PocketBase
`[pocketbase]` config manages one PocketBase service for the entire site

PocketBase files other than `pb_data` are located in the `pb.vaie.art` submodule; production data is stored on the remote at `/var/lib/vaieart-pocketbase/pb_data` instead of in `/srv` (where all the other PocketBase stuff goes, overwritten on each deploy)

Run `cargo run -- pull-pb-migrations` after changing a collection in the remote PocketBase Dashboard and before publishing again. The command pulls only new `*.js` files into the configured `pb.vaie.art` source checkout. Review, commit, and push them from that checkout; the command does not commit, push, overwrite, or delete files.

### IPv6-only Discord OAuth egress

Discord redirects the browser back to the site, but PocketBase must then exchange the authorization code and fetch the Discord user. An IPv6-only host needs an IPv4-capable outbound path for those server requests. Cloudflare's inbound DNS proxy does not provide that path.

`[pocketbase.warp_proxy]` sends PocketBase HTTPS requests through Cloudflare WARP's localhost HTTP proxy. It does not add a public IPv4 address to the host.

One-time host setup:

1. Copy `scripts/setup-warp-proxy.sh` to the remote host.
1. Run it there as root in an interactive terminal:

    ```sh
    bash setup-warp-proxy.sh
    ```

    The first run may ask you to accept Cloudflare's terms. Consumer WARP registration does not need a Cloudflare Zero Trust organization or a service token.

1. Deploy the generated units:

    ```sh
    cargo run -- deploy
    ```

The setup script installs or updates `cloudflare-warp` on supported APT, DNF, or YUM hosts and registers the device. The deployment config remains the single source of truth for WARP mode and port.

Each deploy then:

1. Verifies the WARP CLI, daemon, registration, and `curl`.
1. Generates `vaieart-pb-warp-proxy.service` to enforce MASQUE and proxy mode.
1. Probes Discord through the proxy before PocketBase starts.
1. Sets `HTTPS_PROXY=http://127.0.0.1:40000` and a loopback-only `NO_PROXY` value for PocketBase.

The proxy applies to every PocketBase HTTPS request, not only Discord. Do not define proxy variables in `pocketbase.environment_file`; deployment rejects them so the generated settings remain the single source of truth.

The managed unit configures the host-wide WARP client. Dedicate that WARP registration to this server; another service must not depend on a different WARP mode or port.

Cloudflare WARP Local Proxy has a ten-second request limit. The generated startup probe uses an eight-second timeout so deployment fails before PocketBase starts when the route is unusable. WARP handles reconnects after startup; `systemctl restart vaieart-pb-warp-proxy.service` forces a new probe.

Verify the live host with:

```sh
systemctl status warp-svc.service vaieart-pb-warp-proxy.service vaieart-pb.service
curl --fail --max-time 8 \
    --proxy http://127.0.0.1:40000 \
    https://discord.com/api/v10/gateway
```

Finish with a real Discord login. That verifies PocketBase's token exchange and user-info request, not only the proxy itself.
