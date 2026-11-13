#!/bin/sh
set -eu
umask 077

# Public images carry only this installer, not the proprietary Headless package.
version=0.0.14
runtime=/opt/obsidian
platform=$(node -p 'process.platform + "-" + process.arch + "-node-" + process.versions.node')
prefix=$runtime/$platform/$version

if [ ! -d "$runtime" ] || [ ! -w "$runtime" ]; then
    echo 'Headless runtime cache is not writable; run the stack initializer first.' >&2
    exit 1
fi
exec 9> "$runtime/.install.lock"
flock -w 180 9
if [ ! -f "$prefix/.complete" ] || [ ! -x "$prefix/bin/ob" ]; then
    echo 'Installing pinned official Obsidian Headless from npm; see https://obsidian.md/terms.' >&2
    rm -rf "$prefix"
    mkdir -p "$prefix"
    NPM_CONFIG_CACHE=/tmp/npm-cache npm install --global --prefix "$prefix" \
        --omit=dev --no-audit --no-fund --userconfig=/dev/null \
        --registry=https://registry.npmjs.org "obsidian-headless@$version" >&2
    installed=$(node -p "require(process.argv[1]).version" "$prefix/lib/node_modules/obsidian-headless/package.json")
    if [ "$installed" != "$version" ] || [ ! -x "$prefix/bin/ob" ]; then
        echo 'Installed Headless package did not match the pinned version.' >&2
        exit 1
    fi
    printf '%s\n' "$version" > "$prefix/.complete"
fi
flock -u 9
exec 9>&-

if [ "${1:-}" = serve ]; then
    shift
    if [ "$#" -ne 0 ]; then
        echo 'serve does not accept extra arguments; use ob for official CLI commands.' >&2
        exit 1
    fi
    trap 'exit 0' INT TERM
    announced=''
    until "$prefix/bin/ob" sync-list-local --json | node -e '
        let data = "";
        process.stdin.on("data", chunk => data += chunk);
        process.stdin.on("end", () => {
            try {
                const vaults = JSON.parse(data).vaults;
                process.exit(Array.isArray(vaults) && vaults.some(v => v.path === "/vaults/agents") ? 0 : 1);
            } catch { process.exit(1); }
        });
    '; do
        if [ -z "$announced" ]; then
            echo 'Waiting for setup. In the Sync container console (/bin/sh, app user) run: ob login'
            echo 'Then: ob sync-list-remote && ob sync-setup --vault agents --path /vaults/agents'
            echo 'See docs/PORTAINER.md, Activate Obsidian Sync.'
            announced=1
        fi
        sleep 10
    done
    exec "$prefix/bin/ob" sync --path /vaults/agents --continuous
fi
exec "$prefix/bin/ob" "$@"
