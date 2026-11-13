#!/bin/sh
set -eu
umask 077

dir=${LILVAULT_KEYS_DIR:-/agents}
name=${1:-}
mode=${2:-}
usage() {
    echo 'usage: lilvault-add-agent <name> [--read-only]   (token on stdin)' >&2
    exit 1
}
[ -n "$name" ] && [ "$#" -le 2 ] || usage
case "$name" in
    *[!a-z0-9-]* | -* | '') echo 'Agent name must be lowercase letters, digits, and hyphens.' >&2; exit 1 ;;
esac
case "$mode" in '' | --read-only) ;; *) usage ;; esac
[ -d "$dir" ] || {
    echo "Agent key directory $dir is missing; run the stack initializer first." >&2
    exit 1
}
if [ "$mode" = --read-only ]; then
    file=$dir/$name.ro.token
else
    file=$dir/$name.token
fi
for existing in "$dir/$name.token" "$dir/$name.ro.token"; do
    if [ -e "$existing" ]; then
        echo "Agent '$name' already has a key; remove $existing first." >&2
        exit 1
    fi
done

echo 'Paste one 64-character hex token, then press Enter:' >&2
IFS= read -r token || :
if [ "${#token}" -ne 64 ]; then
    echo 'Token must be exactly 64 hex characters; generate one with openssl rand -hex 32.' >&2
    exit 1
fi
case "$token" in
    *[!0-9a-fA-F]*) echo 'Token must be exactly 64 hex characters.' >&2; exit 1 ;;
esac
printf '%s\n' "$token" > "$file"
chmod 0600 "$file"
echo "Added key for agent '$name': $file"
echo 'Restart the gateway container so it loads the new key.'
