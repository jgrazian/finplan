# Self-hosted deployment (Fedora + Cloudflare Tunnel)

Runs the newest commit on GitHub `main` as native release builds under systemd,
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
| `/etc/finplan/deploy.env` | Build user for the deploy script |

## How deploys happen

`finplan-deploy.timer` runs `/usr/local/sbin/finplan-deploy` five minutes after
each previous run. The script fetches `main`, exits if that commit is already
live, and otherwise:

1. builds `finplan-server` with `cargo build --release` and the web app with
   `pnpm build` as the build user, using that user's toolchains;
2. installs the result into a new release directory;
3. backs up the database, then switches `current` and restarts both services;
4. checks the API and web health, then prunes to the newest five releases.

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

## Operate

```sh
sudo systemctl start finplan-deploy          # deploy now instead of waiting
sudo finplan-deploy --force                  # rebuild and redeploy the live commit
journalctl -u finplan-deploy -e              # build and deploy output
journalctl -u finplan-server -u finplan-web -f
readlink /opt/finplan/current                # live commit
sudo systemctl stop finplan-deploy.timer     # pause automatic deploys
```

After editing `/etc/finplan/server.env`, run
`sudo systemctl restart finplan-server`.

### Roll back

```sh
sudo systemctl stop finplan-deploy.timer finplan-web finplan-server
sudo ln -sfn /opt/finplan/releases/<older-sha> /opt/finplan/current
# Only if the newer release migrated the schema:
sudo -u finplan python3 /opt/finplan/current/bin/sqlite-backup.py restore \
    /var/lib/finplan/backups/<backup>.db /var/lib/finplan/restored.db
# ...then move restored.db into place as finplan.db (remove the -wal/-shm files)
sudo systemctl start finplan-server finplan-web
```

Leave the timer stopped until a fix is on `main`; otherwise it redeploys the
broken commit on its next run.

## Email

Without SMTP the app works, but password recovery and email verification are
unavailable. To enable them, fill in the `FINPLAN_SMTP_*`, `FINPLAN_MAIL_FROM`
and `FINPLAN_PUBLIC_URL` lines in `server.env` using a transactional provider,
add the provider's SPF/DKIM records to the domain, and restart the server.

Registration is open in the example config. To close it, set
`FINPLAN_REGISTRATION_OPEN=false` and restart the server. Existing users can
still sign in.

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
