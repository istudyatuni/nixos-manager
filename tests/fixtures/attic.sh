#!/bin/sh
set -eu
umask 077
export PATH="$ATTIC_TEST_TOOLS_PATH"

config="$XDG_CONFIG_HOME/attic/config.toml"
nix_dir="$XDG_CONFIG_HOME/nix"
phase=verify
if [ "$XDG_CONFIG_HOME" = "$ATTIC_TEST_USER_CONFIG" ]; then
    phase=apply
fi

case "$1" in
    login)
        [ "$#" -eq 5 ] && [ "$2" = --set-default ] && [ -n "$5" ] || exit 2
        printf '%s %s %s %s %s ***\n' "$phase" "$1" "$2" "$3" "$4" >> "$ATTIC_TEST_LOG"
        printf '%s\n' "$XDG_CONFIG_HOME" >> "$ATTIC_TEST_XDG_LOG"
        if [ "$ATTIC_TEST_SCENARIO" = "$phase-login-write-failed" ]; then
            printf 'Could not save modified configuration: %s\n' "$5" >&2
            exit 0
        fi
        mkdir -p "$XDG_CONFIG_HOME/attic"
        printf '%s\n%s\n%s\n' "$3" "$4" "$5" > "$config"
        printf 'Configuring %s\n' "$3" >&2
        ;;
    cache)
        [ "$#" -eq 3 ] && [ "$2" = info ] || exit 2
        printf '%s %s %s %s\n' "$phase" "$1" "$2" "$3" >> "$ATTIC_TEST_LOG"
        printf '%s\n' "$XDG_CONFIG_HOME" >> "$ATTIC_TEST_XDG_LOG"
        [ -s "$config" ]
        case "$ATTIC_TEST_SCENARIO" in
            unauthorized)
                printf 'Error: Unauthorized: Unauthorized.\n' >&2
                exit 1
                ;;
            network)
                printf 'Error: could not connect to server\n' >&2
                exit 1
                ;;
            timeout)
                printf 'Waiting for the server\n'
                exec sleep 300
                ;;
        esac
        printf 'Cache: %s\n' "${3#*:}"
        ;;
    use)
        [ "$#" -eq 2 ] && [ "$phase" = apply ] || exit 2
        printf '%s %s %s\n' "$phase" "$1" "$2" >> "$ATTIC_TEST_LOG"
        printf '%s\n' "$XDG_CONFIG_HOME" >> "$ATTIC_TEST_XDG_LOG"
        [ -s "$config" ]
        mkdir -p "$nix_dir"
        token=$(tail -n 1 "$config")
        printf 'machine cache.example.org password %s\n' "$token" > "$nix_dir/netrc"
        if [ "$ATTIC_TEST_SCENARIO" = partial-use ]; then
            printf 'Error: Permission denied writing nix.conf\n' >&2
            printf '3 | token = "existing-token"\n' >&2
            printf 'Error: Unknown token existing-token outside a machine block\n' >&2
            printf 'Error: Line "existing-token" isn\047t valid\n' >&2
            exit 1
        fi
        printf 'substituters = https://cache.example.org/%s\n' "${2#*:}" > "$nix_dir/nix.conf"
        printf 'Using cache %s\n' "$2" >&2
        if [ "$ATTIC_TEST_SCENARIO" = metadata-commit-failed ]; then
            # Leave the previous metadata in place and make the prepared replacement unavailable.
            for file in "$ATTIC_TEST_METADATA_DIR"/.tmp*; do
                if [ -f "$file" ]; then
                    rm -- "$file"
                fi
            done
        fi
        ;;
    *)
        exit 2
        ;;
esac
