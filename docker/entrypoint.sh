#!/bin/sh
set -eu
umask 077

data_dir=${STREAM_DB_PATH:-/data}
mkdir -p "$data_dir"
key="$data_dir/api-encryption"

if [ ! -e "$key" ]; then
    # Existing databases must retain the key used to encrypt their credentials.
    if [ -e "$data_dir/accounts.db" ]; then
        echo 'Existing accounts.db has no api-encryption key. Restore its original key before starting.' >&2
        exit 1
    fi
    temporary_key=$(mktemp "$data_dir/.api-encryption.XXXXXX")
    trap 'rm -f "$temporary_key"' EXIT HUP INT TERM
    head -c 32 /dev/urandom > "$temporary_key"
    # Publish atomically without replacing a key created by another startup.
    ln "$temporary_key" "$key" || test -f "$key"
    rm -f "$temporary_key"
    trap - EXIT HUP INT TERM
fi

key_size=$(wc -c < "$key")
if [ "$key_size" -ne 32 ]; then
    echo 'api-encryption must contain exactly 32 bytes. Restore the original key; do not replace it.' >&2
    exit 1
fi

exec "$@"
