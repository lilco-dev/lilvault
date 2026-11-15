#!/bin/sh
set -eu
umask 077

dir=${LILVAULT_KEYS_DIR:-/agents}
registry=${LILVAULT_VAULT_REGISTRY:-/registry}
name=${1:-}
[ -n "$name" ] || { echo 'usage: lilvault-add-agent <name> [vault:read|vault:read-write ...]' >&2; exit 1; }
shift
case "$name" in
    *[!a-z0-9-]* | -* | '') echo 'Agent name must be lowercase letters, digits, and hyphens.' >&2; exit 1 ;;
esac
[ "${#name}" -le 64 ] || { echo 'Agent name must be at most 64 characters.' >&2; exit 1; }
[ -d "$dir" ] && [ -w "$dir" ] || { echo "$dir is not writable." >&2; exit 1; }
[ -d "$registry" ] || { echo "$registry is missing." >&2; exit 1; }

if [ "$#" -eq 0 ]; then
    echo 'Registered vaults:' >&2
    found=''
    for marker in "$registry"/*; do
        [ -f "$marker" ] || continue
        basename "$marker" >&2
        found=1
    done
    [ -n "$found" ] || { echo 'No vaults are registered; add a vault first.' >&2; exit 1; }
    printf 'Permissions (comma-separated, e.g. agents:read-write,brain:read): ' >&2
    IFS= read -r input
    old_ifs=$IFS
    IFS=,
    # Intentional word splitting turns comma-separated input into arguments.
    set -- $input
    IFS=$old_ifs
fi
[ "$#" -gt 0 ] || { echo 'At least one vault permission is required.' >&2; exit 1; }

temporary_permissions=$(mktemp "$dir/.${name}.permissions.XXXXXX")
temporary_token=''
cleanup() {
    rm -f "$temporary_permissions"
    [ -z "$temporary_token" ] || rm -f "$temporary_token"
}
trap cleanup EXIT HUP INT TERM
seen=' '
for permission in "$@"; do
    permission=$(printf '%s' "$permission" | tr -d '[:space:]')
    vault=${permission%%:*}
    access=${permission#*:}
    [ "$vault" != "$permission" ] || { echo "Invalid permission: $permission" >&2; exit 1; }
    case "$vault" in '' | *[!a-zA-Z0-9_-]*) echo "Invalid vault ID: $vault" >&2; exit 1 ;; esac
    [ "${#vault}" -le 64 ] || { echo "Invalid vault ID: $vault" >&2; exit 1; }
    case "$access" in read | read-write) ;; *) echo "Invalid access in: $permission" >&2; exit 1 ;; esac
    [ -f "$registry/$vault" ] && [ ! -L "$registry/$vault" ] || {
        echo "Vault '$vault' is not safely registered." >&2
        exit 1
    }
    case "$seen" in *" $vault "*) echo "Duplicate vault permission: $vault" >&2; exit 1 ;; esac
    seen="$seen$vault "
    printf '%s:%s\n' "$vault" "$access" >> "$temporary_permissions"
done
chmod 0600 "$temporary_permissions"

file=$dir/$name.token
legacy=$dir/$name.ro.token
permissions_file=$dir/$name.permissions
if { [ -e "$permissions_file" ] || [ -L "$permissions_file" ]; } \
    && { [ -L "$permissions_file" ] || [ ! -f "$permissions_file" ]; }; then
    echo "Refusing unsafe permissions target: $permissions_file" >&2
    exit 1
fi
for target in "$file" "$legacy"; do
    if { [ -e "$target" ] || [ -L "$target" ]; } \
        && { [ -L "$target" ] || [ ! -f "$target" ]; }; then
        echo "Refusing unsafe key target: $target" >&2
        exit 1
    fi
done
if [ -e "$file" ] && [ -e "$legacy" ]; then
    echo "Agent '$name' has ambiguous token files; remove one first." >&2
    exit 1
fi
if [ -f "$legacy" ] && [ ! -e "$file" ]; then
    file=$legacy
    if grep -q ':read-write$' "$temporary_permissions"; then
        echo "Agent '$name' uses a legacy read-only key; create a new token to grant write access." >&2
        exit 1
    fi
fi
if [ ! -e "$file" ]; then
    echo 'Paste one 64-character hex token, then press Enter:' >&2
    IFS= read -r token || :
    if [ "${#token}" -ne 64 ]; then
        echo 'Token must be exactly 64 hex characters; generate with openssl rand -hex 32.' >&2
        exit 1
    fi
    case "$token" in *[!0-9a-fA-F]*) echo 'Token must be exactly 64 hex characters.' >&2; exit 1 ;; esac
    temporary_token=$(mktemp "$dir/.${name}.token.XXXXXX")
    printf '%s\n' "$token" > "$temporary_token"
    chmod 0600 "$temporary_token"
    mv "$temporary_token" "$file"
    temporary_token=''
elif [ ! -f "$file" ] || [ -L "$file" ]; then
    echo "Refusing unsafe key target: $file" >&2
    exit 1
else
    echo "Keeping existing token for '$name'; updating only permissions." >&2
fi
mv "$temporary_permissions" "$permissions_file"
trap - EXIT HUP INT TERM
echo "Agent '$name' permissions updated."
echo 'Restart the gateway container so it reloads credentials.'
