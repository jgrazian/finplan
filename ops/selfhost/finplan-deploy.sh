#!/usr/bin/env bash
# Build and deploy the newest vX.Y.Z release tag on GitHub, if it is not already live.
# Installed as /usr/local/sbin/finplan-deploy and run as root by finplan-deploy.timer.
# `finplan-deploy --force` rebuilds and redeploys the selected tag even if it is live.
#
# FINPLAN_TAG in deploy.env pins one tag instead of following the newest. Without a
# pin the script never moves to a tag older than the live commit: the live release
# may already have migrated the database past what that tag understands.
set -euo pipefail

[[ -f /etc/finplan/deploy.env ]] && source /etc/finplan/deploy.env
REPO=${FINPLAN_REPO:-https://github.com/jgrazian/finplan.git}
PIN=${FINPLAN_TAG:-}
BUILD_USER=${FINPLAN_BUILD_USER:?set FINPLAN_BUILD_USER in /etc/finplan/deploy.env}
BUILD_HOME=$(getent passwd "$BUILD_USER" | cut -d: -f6)
BUILD_PATH=${FINPLAN_BUILD_PATH:-$BUILD_HOME/.cargo/bin:$BUILD_HOME/.local/bin:/usr/local/bin:/usr/bin:/bin}
API_ORIGIN=${FINPLAN_API_ORIGIN:-http://127.0.0.1:8480}
WEB_URL=${FINPLAN_WEB_URL:-http://127.0.0.1:3480}
PNPM=pnpm@12.3.4

ROOT=/opt/finplan
SRC=$ROOT/src
RELEASES=$ROOT/releases
STATE=/var/lib/finplan
KEEP_RELEASES=5
KEEP_BACKUPS=14

exec 9>/run/lock/finplan-deploy.lock
flock -n 9 || { echo "another deploy is running"; exit 0; }

as_builder() {
    runuser -u "$BUILD_USER" -- env -i HOME="$BUILD_HOME" PATH="$BUILD_PATH" \
        NEXT_TELEMETRY_DISABLED=1 CARGO_TERM_COLOR=never "$@"
}

wait_healthy() {
    for _ in $(seq 60); do
        curl --fail --silent --output /dev/null "$1" && return 0
        sleep 1
    done
    return 1
}

if [[ ! -d $SRC/.git ]]; then
    install -d -o "$BUILD_USER" -g "$BUILD_USER" "$SRC"
    as_builder git clone --quiet "$REPO" "$SRC"
fi
cd "$SRC"
if [[ -n $PIN ]]; then
    tag=$PIN
else
    tag=$(as_builder git ls-remote --tags --refs origin 'v*' | sed 's#.*refs/tags/##' \
        | { grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' || true; } | sort -V | tail -n 1)
    [[ -n $tag ]] || { echo "no vX.Y.Z tags on origin"; exit 1; }
fi
as_builder git fetch --quiet --force origin "refs/tags/$tag:refs/tags/$tag"
sha=$(as_builder git rev-parse "$tag^{commit}")
live=$(basename "$(readlink -f "$ROOT/current" 2>/dev/null || echo none)")
if [[ $sha == "$live" && ${1:-} != --force ]]; then
    exit 0
fi
if [[ -z $PIN && $live != none ]] && as_builder git merge-base --is-ancestor "$sha" "$live" 2>/dev/null; then
    echo "newest tag $tag ($sha) is older than live $live; set FINPLAN_TAG to roll back"
    exit 0
fi
echo "deploying $tag $sha (live: $live)"

as_builder git checkout --quiet --force --detach "$sha"
# Keep the cargo target and node_modules caches; everything else is rebuilt.
as_builder git clean -ffdxq -e /target -e /web/node_modules
as_builder cargo build --locked --release --bin finplan-server
as_builder bash -c "cd web && npx --yes $PNPM install --frozen-lockfile \
    && FINPLAN_API_ORIGIN=$API_ORIGIN npx --yes $PNPM build"

release=$RELEASES/$sha
rm -rf "$release.tmp"
install -d "$release.tmp/bin" "$release.tmp/web/.next"
install -m 755 target/release/finplan-server "$release.tmp/bin/"
echo "$tag" > "$release.tmp/TAG"
install -m 755 scripts/sqlite-backup.py "$release.tmp/bin/"
cp -a web/.next/standalone/. "$release.tmp/web/"
cp -a web/.next/static "$release.tmp/web/.next/static"
[[ -d web/public ]] && cp -a web/public "$release.tmp/web/public"
chown -R root:root "$release.tmp"
rm -rf "$release"
mv "$release.tmp" "$release"

# The new server applies migrations on start; keep a restorable copy first.
if [[ -f $STATE/finplan.db ]]; then
    install -d -o finplan -g finplan -m 700 "$STATE/backups"
    runuser -u finplan -- python3 "$release/bin/sqlite-backup.py" backup \
        "$STATE/finplan.db" "$STATE/backups/predeploy-$(date +%Y%m%dT%H%M%S)-${live:0:8}.db"
    ls -1t "$STATE"/backups/predeploy-*.db | tail -n +$((KEEP_BACKUPS + 1)) | xargs -r rm -f
fi

ln -sfn "$release" "$ROOT/current.tmp"
mv -T "$ROOT/current.tmp" "$ROOT/current"
systemctl restart finplan-server
wait_healthy "$API_ORIGIN/api/health" || { echo "API failed health check"; exit 1; }
systemctl restart finplan-web
wait_healthy "$WEB_URL/" || { echo "web failed health check"; exit 1; }
echo "live: $tag $sha"

ls -1dt "$RELEASES"/* | { grep -v -e "/$sha\$" || true; } | tail -n +"$KEEP_RELEASES" | xargs -r rm -rf
