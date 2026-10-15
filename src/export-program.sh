#!/bin/sh

CONTAINER_NAME=#INSERT_CONTAINER_NAME#
MOUNT_HOST_DIR=#INSERT_MOUNT_HOST_DIR#
MOUNT_CONTAINER_DIR=#INSERT_MOUNT_CONTAINER_DIR#
COMMAND_NAME=#INSERT_COMMAND_NAME#

cwd=$(pwd -P)

relative_path_in_container=${cwd#"$MOUNT_HOST_DIR"}
absolute_path_in_container="$MOUNT_CONTAINER_DIR/$relative_path_in_container"

sudo podman exec -it -w "$absolute_path_in_container" --user 1000: "$CONTAINER_NAME" /bin/sh -c '
USER=$(id -un)
SHELL_PATH=$(awk -F: -v u="$USER" '\''$1==u {print $7}'\'' /etc/passwd)

if [ -z "$SHELL_PATH" ]; then
    if [ -x /bin/bash ]; then
        SHELL_PATH=/bin/bash
    else
        SHELL_PATH=/bin/sh
    fi
fi

export SHELL="$SHELL_PATH"
exec "$SHELL_PATH" -lic "exec \"\$@\"" _ "$@"
' _ "$COMMAND_NAME" "$@"
