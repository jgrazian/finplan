# Self-hosted deployment (Fedora + Cloudflare Tunnel)

Runs the newest `vX.Y.Z` release tag on GitHub as native release builds under systemd,
exposed through an existing Cloudflare Tunnel. No ports are opened on the host
and TLS terminates at Cloudflare.

```
browser ──https──> Cloudflare ──tunnel──> cloudflared
                                           └─> 127.0.0.1:3480  finplan-web (Next standalone)
                                                └─ /api/* ─> 127.0.0.1:8480  finplan-server
```

| Path | Purpose |
|---|---|
| `/opt/finplan/src` | Git checkout and build caches, owned by the build user |
| `/opt/finplan/releases/<sha>` | Installed server binary and web bundle, owned by root |
| `/opt/finplan/current` | Symlink to the live release |
| `/var/lib/finplan/finplan.db` | SQLite database, owned by the `finplan` system user |
| `/var/lib/finplan/backups` | Hourly, daily and pre-deploy backups |
| `/etc/finplan/server.env` | Server settings, including SMTP credentials |
| `/etc/finplan/deploy.env` | Build user and optional pinned tag for the deploy script |

## How deploys happen

`finplan-deploy.timer` runs `/usr/local/sbin/finplan-deploy` five minutes after
each previous run. The script picks the highest `vX.Y.Z` tag on GitHub (or the
tag pinned by `FINPLAN_TAG` in `deploy.env`), exits if that commit is already
live, and otherwise:

1. builds `finplan-server` with `cargo build --release` and the web app with
   `pnpm build` as the build user, using that user's toolchains;
2. installs the result into a new release directory;
3. backs up the database, then switches `current` and restarts both services;
4. checks the API and web health, then prunes to the newest five releases.

Pushes to `main` do not deploy. Without a pin, the script also refuses to move
to a tag whose commit is older than the live one, and logs why instead.

The server applies migrations when it starts. A failed health check leaves the
deploy unit failed and does not roll back. Because migrations may already have
run, recovery means an older release plus a matching backup (see below).

## Install

```sh
sudo dnf install nodejs24          # runtime for finplan-web (/usr/bin/node-24)
sudo useradd --system --home-dir /var/lib/finplan --no-create-home \
    --shell /sbin/nologin finplan
sudo install -d -m 755 /opt/finplan /opt/finplan/releases
sudo install -d -m 750 -g finplan /etc/finplan
sudo install -m 640 -g finplan server.env.example /etc/finplan/server.env
sudo install -m 644 deploy.env.example /etc/finplan/deploy.env
sudo install -m 755 finplan-deploy.sh /usr/local/sbin/finplan-deploy
sudo install -m 755 finplan-backup.sh /usr/local/sbin/finplan-backup
sudo install -m 644 *.service *.timer /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable finplan-server finplan-web
sudo systemctl enable --now finplan-deploy.timer \
    finplan-backup-hourly.timer finplan-backup-daily.timer
```

The build user needs `cargo` in `~/.cargo/bin` and `node`/`npx` in
`~/.local/bin`. pnpm is fetched with `npx`, so it does not need to be installed.

In Cloudflare Zero Trust, open the tunnel, go to **Public Hostnames**, and add
`finplan.rayknot.com` with service `HTTP` → `127.0.0.1:3480`. The zone must
be in the same Cloudflare account as the tunnel.

## Release

Cutting a release deploys it within five minutes. `release.yml` builds the
TUI binaries from the same tag.

```sh
# bump `version` in crates/*/Cargo.toml and web/package.json, then:
cargo check                                  # refreshes Cargo.lock
git commit -am "chore: release X.Y.Z"
git tag -a vX.Y.Z -m "Release X.Y.Z"
git push origin main vX.Y.Z
```

## Operate

```sh
sudo systemctl start finplan-deploy          # deploy now instead of waiting
sudo finplan-deploy --force                  # rebuild and redeploy the selected tag
journalctl -u finplan-deploy -e              # build and deploy output
journalctl -u finplan-server -u finplan-web -f
cat /opt/finplan/current/TAG                 # live tag (readlink for the commit)
sudo systemctl stop finplan-deploy.timer     # pause automatic deploys
```

After editing `/etc/finplan/server.env`, run
`sudo systemctl restart finplan-server`.

### Roll back

Pin the last good tag so the timer stops chasing the broken one:

```sh
echo FINPLAN_TAG=v<good> | sudo tee -a /etc/finplan/deploy.env
sudo systemctl start finplan-deploy           # rebuilds and deploys that tag
```

If the broken release migrated the schema, the older server may refuse the
database. Then restore the pre-deploy backup taken before the bad release:

```sh
sudo systemctl stop finplan-deploy.timer finplan-web finplan-server
sudo -u finplan python3 /opt/finplan/current/bin/sqlite-backup.py restore \
    /var/lib/finplan/backups/<backup>.db /var/lib/finplan/restored.db
# ...then move restored.db into place as finplan.db (remove the -wal/-shm files)
sudo systemctl start finplan-server finplan-web finplan-deploy.timer
```

Once a fixed release is tagged, remove the `FINPLAN_TAG` line to resume
following new tags.

## Email

Without SMTP the app works, but password recovery and email verification are
unavailable. To enable them, fill in the `FINPLAN_SMTP_*`, `FINPLAN_MAIL_FROM`
and `FINPLAN_PUBLIC_URL` lines in `server.env` using a transactional provider,
add the provider's SPF/DKIM records to the domain, and restart the server.

Registration is open in the example config. To close it, set
`FINPLAN_REGISTRATION_OPEN=false` and restart the server. Existing users can
still sign in.

## Monitoring

Prometheus scrapes finplan-server and node-exporter every 15 seconds and
keeps 90 days. Grafana serves a provisioned **FinPlan** dashboard on the LAN
at `http://fedora.local:3000`.

| Service | Listens on | Scrapes / serves |
|---|---|---|
| finplan-server metrics | `127.0.0.1:9480` | `FINPLAN_METRICS_BIND` in `server.env` |
| prometheus | `127.0.0.1:9090` | TSDB in `/var/lib/prometheus/data` |
| prometheus-node-exporter | `127.0.0.1:9100` | host stats, finplan-related systemd units |
| grafana-server | `0.0.0.0:3000` | login required, sign-up disabled |

The FedoraWorkstation firewall zone admits LAN traffic to every port above
1024, so anything not meant for the LAN must bind to loopback. None of these
ports go through the tunnel.

Install, from `monitoring/`:

```sh
sudo dnf install grafana prometheus node-exporter
sudo install -m 644 prometheus.yml /etc/prometheus/prometheus.yml
sudo install -m 644 prometheus.default /etc/default/prometheus
sudo install -m 644 node-exporter.default /etc/default/prometheus-node-exporter
sudo install -d -m 755 /etc/systemd/system/grafana-server.service.d /etc/grafana/dashboards
sudo install -m 644 grafana-server.conf /etc/systemd/system/grafana-server.service.d/finplan.conf
sudo install -m 640 -g grafana grafana-datasource.yaml /etc/grafana/provisioning/datasources/finplan.yaml
sudo install -m 640 -g grafana grafana-dashboards.yaml /etc/grafana/provisioning/dashboards/finplan.yaml
sudo install -m 644 finplan-dashboard.json /etc/grafana/dashboards/finplan.json
sudo install -m 640 -g grafana grafana.env.example /etc/grafana/finplan.env  # then set a password
sudo systemctl daemon-reload
sudo systemctl enable --now prometheus prometheus-node-exporter grafana-server
sudo firewall-cmd --permanent --add-service=grafana && sudo firewall-cmd --reload
```

Sign in as `admin` with the password in `/etc/grafana/finplan.env`. That file
only seeds the first start; to change it later run
`sudo grafana cli admin reset-admin-password <new>`.

The dashboard is read-only in the UI. To change it, edit
`finplan-dashboard.json`, copy it to `/etc/grafana/dashboards/`, and Grafana
picks it up within a minute. After editing `prometheus.yml`, run
`promtool check config` and `sudo systemctl reload prometheus`.

## Backups

`finplan-backup` copies the live database with SQLite's online backup API and
verifies each copy's integrity. It runs as the `finplan` user on two timers:

| File | When | Retention |
|---|---|---|
| `hourly.db` | top of every hour | overwritten each time |
| `daily-YYYY-MM-DD.db` | 03:30, or at the next boot if missed | newest 30 |
| `predeploy-<time>-<old sha>.db` | before each deploy | newest 14 |

```sh
sudo systemctl start finplan-backup@hourly   # take a hot backup now
journalctl -u 'finplan-backup@*' -e          # checksum and row-count receipts
```

All backups live on the same disk as the database. Copy
`/var/lib/finplan/backups` off the host if the data matters.
