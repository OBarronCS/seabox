# Seabox

Seabox is a wrapper around various `podman` commands to simplify creating Linux environments using containers. This allows you to spin up workspaces with different Linux distributions to install and run software in, providing flexibility to create independent contexts that suit the needs of given software workflows and toolchains.

Seabox is modeled after [distrobox](https://github.com/89luca89/distrobox) and [toolbx](https://github.com/containers/toolbox), allowing you to instantiate OCI images for use as standalone environments with host integration. Seabox has a couple adjustments - it uses rootful podman to run containers, is less tightly integrated with the host (such as no default mounting of host root and home directories), and has minimal container initialization.

## Install
```sh
cargo install --git https://github.com/OBarronCS/seabox.git
```

## Quick start
```sh
# Create a container
seabox create my-dev-environment -i docker.io/dokken/ubuntu-25.04
## The current working directory is mounted to /mount/ in the container

# List containers created with seabox
seabox ls

# Enter a container
seabox enter my-dev-environment

# Remove a container
seabox rm my-dev-environment
```

## Command reference

Instantiate a container:
```sh
seabox create [options] <name>

# Options
-i, --image <image> 
    Select image for container, defaulting to one specified in config file

-d <directory>
    Directory to mount to /mount/ in the container. Defaults to pwd

-p, --pass-through
    Pass additional arguments to Podman - the string is broken into individual arguments using shell string parsing.
    Example: seabox create -p "--pidfile /tmp/pidfile --cidfile /tmp/cidfile" test

-r, --root
    Use the root user in the container. Typically, seabox matches the host user to an unprivileged user in the container (creating one on entry if it doesn't exist). Passing this flag skips the initialization of such a user, and uses root instead.

--no-password, --no-passwd <true/false>
    Skip creation of password for user. Defaults to false.

--install-sudo <true/false>
    Automatically answer "yes" to installing sudo/su on initial entry to container. Useful when using base distro images where it is not preinstalled. Defaults to prompt to install.

--passwordless-sudo <true/false>
    Setup passwordless sudo access for user in container. Note the security implications - this means software running in the container has passwordless access to root. Implies --no-password. Defaults to false.
```

Enter an existing container:
```sh
seabox enter [options] <name>

# Options
-u, --user <username>
    Enter the container with the given user. Defaults to the user setup on container creation.

-s, --shell <shell>
    Override the shell to use. Defaults to using the user's login shell as specified in /etc/passwd
```

List all containers created with seabox
```sh
seabox ls
```

Delete a container
```sh
seabox rm <container_names...>
```

Create a temporary container.

This acts the same as `seabox create`, but deletes the container upon exiting.
```sh
seabox tmp [options]

Options are identical to seabox create
```

Print help
```sh
seabox help [subcommand]
```

## Images and container initialization

Seabox works best with prebuilt images, such as those [defined here](https://github.com/89luca89/distrobox/blob/main/docs/compatibility.md#containers-distros). [Dokken images](https://github.com/test-kitchen/dokken-images) are also great, as they come pre-installed with tools to make them appear like a full OS, and are built daily with convenience in mind - in the Dokken Ubuntu containers, for example, `apt update` has already been run, avoiding the need to run it manually upon entering the container for the first time.

Most base images, such as `ubuntu:latest`, `fedora`, `alpine`, and `archlinux` do not come pre-installed with sudo. Seabox will attempt to detect this and prompt to install sudo on initial entry. Using the pre-baked images mentioned above can help avoid the time delay on initial entry caused by installing sudo for these cases.

Seabox will match a user in the container to correspond to the user on the host, and set up file mapping permissions correctly so the user can access files through the mount as if it were the host user. In case the container doesn't already have an "normal" user (id >= 1000), one would be created and given sudo permissions so as to act as a counterpart to the host user. 

`seabox` will invoke `sudo podman` with flags such as `--privileged`,`network` mode is set to `host` for easy ability to run networked programs. Additionally, the current working directory will be mounted to `/mount/` inside the container (unless changed by -d), letting you share files with the host. Run `seabox create --dry-run` to see the commandline flags that are passed to podman.


## Idmapped file mounts

Seabox uses Podman's [idmapped file mounts feature](https://github.com/containers/podman/issues/10374) to efficiently allow the "container user" to access mounted host files as if it had the "host user" id. This makes it so the "container user" maps to the "host user" when accessing and modifying mounted files. This requires rootful Podman (which also provides the container other capabilities such as binding ports less than 1024), which is invoked with `sudo podman`. This causes most `seabox` commands to prompt for sudo password.

Idmapped file mounts has the advantage of avoiding a boot-up cost when instantiating an image for the first time. Other methods of matching file permissions so that a given container user can access the files of a given user (`--userns=keep-id`,`--uidmap`) need to [recursively `chown` the image file system](https://github.com/containers/podman/blob/43c95d2c0bdfc71d005e015fe93b3e7a48f39adf/vendor/github.com/containers/storage/drivers/chown.go#L72-L73) which takes significant time for large images.


# Note on security
Seabox uses rootful Podman, which means **root in the container is root on the host**. Do not run any software in these containers that you wouldn't run on your host.


### Configuration
```sh
# Show location of config file
seabox config

# Print the current config
seabox config show
```

##### Example config
```sh
image = "docker.io/dokken/ubuntu-25.04"
sudo_command = "doas"
# Don't prompt for password creation on initial entry (no password will be set)
no_password = true
# Install sudo without prompting on initial entry to containers
install_sudo = true
```

Environment variables can also be used to set certain values:
```sh
SEABOX_SUDO_COMMAND=doas
SEABOX_INSTALL_SUDO=true
SEABOX_NO_PASSWORD=true
```
