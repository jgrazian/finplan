#!/usr/bin/env bash
#
# Build the browser engine (spec 19): crates/finplan_wasm -> web/lib/engine/pkg/.
#
#   cargo build  (wasm32-unknown-unknown, the `wasm-release` profile)
#   wasm-bindgen --target web  (JS glue + .d.ts + the .wasm)
#   wasm-opt -Os               (if binaryen is installed; warns and goes on if not)
#
# The output is binary and not committed. `pnpm dev` and `pnpm build` run this
# first (web/package.json), and CI and the self-host deploy do before the web
# build.
#
# The wasm-bindgen CLI must be the same version as the crate's pinned
# `wasm-bindgen` dependency, or it refuses the module. One-time setup:
#
#   rustup target add wasm32-unknown-unknown
#   cargo install wasm-bindgen-cli --version '=0.2.106' --locked
#   brew install binaryen          # optional: wasm-opt, ~10-15% smaller
#
# Env: FINPLAN_WASM_PROFILE (default wasm-release), FINPLAN_WASM_OUT.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
profile="${FINPLAN_WASM_PROFILE:-wasm-release}"
out="${FINPLAN_WASM_OUT:-$root/web/lib/engine/pkg}"
target=wasm32-unknown-unknown

# The version the crate pins, read from its manifest.
want="$(sed -n 's/^wasm-bindgen = "=\([0-9.]*\)".*/\1/p' "$root/crates/finplan_wasm/Cargo.toml")"
if [ -z "$want" ]; then
    echo "build-wasm: cannot read the pinned wasm-bindgen version from crates/finplan_wasm/Cargo.toml" >&2
    exit 1
fi
install_hint="cargo install wasm-bindgen-cli --version '=$want' --locked"

if ! command -v wasm-bindgen >/dev/null 2>&1; then
    echo "build-wasm: wasm-bindgen CLI not found. Install it with:" >&2
    echo "  $install_hint" >&2
    exit 1
fi
have="$(wasm-bindgen --version | awk '{print $2}')"
if [ "$have" != "$want" ]; then
    echo "build-wasm: wasm-bindgen CLI is $have but finplan_wasm pins $want. Install the right one:" >&2
    echo "  $install_hint" >&2
    exit 1
fi

if command -v rustup >/dev/null 2>&1 \
    && ! rustup target list --installed | grep -qx "$target"; then
    echo "build-wasm: the $target target is missing. Install it with:" >&2
    echo "  rustup target add $target" >&2
    exit 1
fi

cargo build --manifest-path "$root/Cargo.toml" -p finplan_wasm \
    --profile "$profile" --target "$target"

# `--release` is cargo's alias for the profile named `release`; any other
# profile builds into a directory of its own name.
built="$root/target/$target/$profile/finplan_wasm.wasm"

rm -rf "$out"
mkdir -p "$out"
wasm-bindgen --target web --out-dir "$out" --out-name finplan_wasm "$built"

wasm="$out/finplan_wasm_bg.wasm"
raw_before="$(wc -c < "$wasm" | tr -d ' ')"
if command -v wasm-opt >/dev/null 2>&1; then
    wasm-opt -Os --enable-bulk-memory --enable-nontrapping-float-to-int \
        --enable-sign-ext --enable-mutable-globals --enable-reference-types \
        "$wasm" -o "$wasm.opt"
    mv "$wasm.opt" "$wasm"
else
    echo "build-wasm: wasm-opt (binaryen) not found; skipping the size pass (brew install binaryen)" >&2
fi

raw="$(wc -c < "$wasm" | tr -d ' ')"
gz="$(gzip -9 -c "$wasm" | wc -c | tr -d ' ')"
if command -v brotli >/dev/null 2>&1; then
    br="$(brotli -q 11 -c "$wasm" | wc -c | tr -d ' ')"
elif command -v node >/dev/null 2>&1; then
    br="$(node -e 'const z=require("zlib"),fs=require("fs");process.stdout.write(String(z.brotliCompressSync(fs.readFileSync(process.argv[1]),{params:{[z.constants.BROTLI_PARAM_QUALITY]:11}}).length))' "$wasm")"
else
    br=n/a
fi
echo "finplan_wasm_bg.wasm: raw $raw B (before wasm-opt $raw_before), gzip $gz B, brotli $br B"
echo "wrote ${out#"$root"/}"
