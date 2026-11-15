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

case "$(basename "$0")" in
    lilvault-add-vault) set -- add-vault "$@" ;;
esac

if [ "${1:-}" = serve ]; then
    shift
    [ "$#" -eq 0 ] || { echo 'serve does not accept arguments.' >&2; exit 1; }
    exec node /usr/local/lib/lilvault/sync-supervisor.mjs "$prefix/bin/ob"
fi

if [ "${1:-}" = add-vault ]; then
    shift
    [ "$#" -le 2 ] || { echo 'usage: lilvault-add-vault [local-id [remote-vault]]' >&2; exit 1; }
    registry=${LILVAULT_VAULT_REGISTRY:-/registry}
    [ -d /vaults ] && [ -w /vaults ] || { echo '/vaults is not writable.' >&2; exit 1; }
    [ -d "$registry" ] && [ -w "$registry" ] || { echo "$registry is not writable." >&2; exit 1; }

    remote=${2:-}
    id=${1:-}
    if [ -z "$id" ]; then
        echo 'Available remote vaults:' >&2
        "$prefix/bin/ob" sync-list-remote >&2
        printf 'Remote vault name or ID: ' >&2
        IFS= read -r remote
        [ -n "$remote" ] || { echo 'A remote vault is required.' >&2; exit 1; }
        suggested=$(printf '%s' "$remote" | tr '[:upper:]' '[:lower:]' | tr -cs 'a-z0-9-' '-' | sed 's/^-*//;s/-*$//' | cut -c1-64)
        printf 'Local vault ID [%s]: ' "$suggested" >&2
        IFS= read -r id
        id=${id:-$suggested}
    else
        remote=${remote:-$id}
    fi
    case "$id" in
        '' | *[!a-zA-Z0-9_-]*) echo 'Local ID must be 1-64 letters, digits, underscores, or hyphens.' >&2; exit 1 ;;
    esac
    [ "${#id}" -le 64 ] || { echo 'Local ID is longer than 64 characters.' >&2; exit 1; }
    path=/vaults/$id
    marker=$registry/$id
    { [ ! -e "$marker" ] && [ ! -L "$marker" ]; } || {
        echo "Vault '$id' is already or unsafely registered." >&2
        exit 1
    }
    if [ ! -e "$path" ]; then
        mkdir -m 0700 "$path"
    elif [ ! -d "$path" ] || [ -L "$path" ] || [ -n "$(find "$path" -mindepth 1 -maxdepth 1 -print -quit)" ]; then
        echo "$path must be a missing or empty real directory." >&2
        exit 1
    fi
    echo "Configuring remote vault '$remote' at $path" >&2
    "$prefix/bin/ob" sync-setup --vault "$remote" --path "$path" --device-name "lilvault-$id"
    temporary=$(mktemp "$registry/.${id}.XXXXXX")
    printf '%s\n' "$remote" > "$temporary"
    chmod 0600 "$temporary"
    mv -f "$temporary" "$marker"
    echo "Vault '$id' registered. Continuous sync starts within 5 seconds."
    echo "Grant agents access with lilvault-add-agent in the gateway container."
    exit 0
fi

exec "$prefix/bin/ob" "$@"
