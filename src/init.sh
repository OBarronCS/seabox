#!/bin/sh
PARAM_CREATE_USER="INSERT_CREATE_USER"
PARAM_NEW_USER_USERNAME="INSERT_NEW_USERNAME"
PARAM_USER_ID="INSERT_CONTAINER_ID"
# Possible values: "install", "no_install", "prompt"
PARAM_SUDO_INSTALL_PROMPT="INSERT_SUDO_INSTALL"
PARAM_PASSWORDLESS_SUDO="INSERT_PASSWORDLESS_SUDO"
PARAM_NO_PASSWORD="INSERT_CREATE_PASSWORD"
PARAM_VERBOSE="INSERT_VERBOSE"
PARAM_SHELL="INSERT_SHELL"

SHELL="$PARAM_SHELL"

if [ -z "$SHELL" ];
then
    SHELL=/bin/bash
    if ! command -v /bin/bash >/dev/null 2>&1;
    then
        SHELL=/bin/sh
    fi
fi

verbose_echo () {
    if [ -n "$PARAM_VERBOSE" ];
    then
        echo "$@"
    fi
}

# Create user
if [ -n "$PARAM_CREATE_USER" ];
then
    if command -v useradd >/dev/null 2>&1;
    then
        useradd --uid "$PARAM_USER_ID" --shell "$SHELL" --create-home "$PARAM_NEW_USER_USERNAME"
    elif command -v adduser >/dev/null 2>&1;
    then
        adduser --gecos "" -D -u "$PARAM_USER_ID" "$PARAM_NEW_USER_USERNAME"
    fi
fi

# Extract username
USERNAME=$(awk -F: -v uid="$PARAM_USER_ID" '$3 == uid {print $1; exit}' /etc/passwd)

# Install sudo
if ! command -v sudo >/dev/null 2>&1 || ! command -v su >/dev/null 2>&1;
then
    if [ "$PARAM_SUDO_INSTALL_PROMPT" = "install" ]; then
        ANSWER="yes"
    fi
    
    if [ "$PARAM_SUDO_INSTALL_PROMPT" = "no_install" ]; then
        ANSWER="no"
    fi

    if [ -z "$ANSWER" ];
    then
        printf "sudo and su not found in the container - install? [Y/n] "
        IFS= read -r ANSWER || exit 1

        case $ANSWER in
            [Nn])
                ANSWER="no"
                ;;
            *)
                ANSWER="yes"
                ;;
        esac
    fi

    if [ "$ANSWER" = "yes" ]; then
        echo "Installing sudo and su"
        if command -v apt >/dev/null 2>&1;
        then
            apt update
            apt install -y sudo
        elif command -v dnf >/dev/null 2>&1;
        then
            dnf install -y sudo su
        elif command -v pacman >/dev/null 2>&1;
        then
            pacman -Syu --noconfirm sudo
        elif command -v apk >/dev/null 2>&1;
        then
            apk add sudo
        else
            echo "Couldn't find package manager to install sudo/su"
        fi
    fi 
fi


# Add to sudoers groups
attempt_add_user_to_group() {
    if awk -F: -v g="$1" '$1 == g {found=1; exit} END {exit !found}' /etc/group;
    then
        if command -v usermod >/dev/null 2>&1;
        then
            verbose_echo "Adding user to $1 group"
            usermod -a -G "$1" "$USERNAME"
        elif command -v addgroup >/dev/null 2>&1;
        then
            verbose_echo "Adding user to $1 group"
            addgroup "$USERNAME" "$1"
        else
            echo "Found group $1 but usermod/addgroup do not exist"
        fi
    fi
}

attempt_add_user_to_group sudo
attempt_add_user_to_group wheel

## The lines that allow users in these groups to get sudo are often commented out by default
if [ -f "/etc/sudoers" ];
then
    for group in sudo wheel; do
        if awk -F: -v g="$group" '$1 == g {found=1; exit} END {exit !found}' /etc/group;
        then
            if ! grep -q "^[[:space:]]*%$group[[:space:]]\{1,\}ALL=" /etc/sudoers;
            then
                verbose_echo "Granting '$group' group sudo access"
                echo "%$group	ALL=(ALL:ALL) ALL" >>/etc/sudoers.d/00-$group
                chmod 0440 "/etc/sudoers.d/00-$group"
            fi
        fi
    done
else
    verbose_echo "Attempted to grant sudo/wheel groups sudo access, but sudo not installed"
fi

# Add passwordless sudo
if [ -n "$PARAM_PASSWORDLESS_SUDO" ];
then
    if command -v sudo >/dev/null 2>&1;
    then
        if [ -f "/etc/sudoers" ];
        then
            echo "Enabling passwordless sudo"
            echo "$USERNAME ALL=(ALL) NOPASSWD:ALL" >/etc/sudoers.d/zz-$USERNAME
            chmod 0440 "/etc/sudoers.d/zz-$USERNAME"
        fi
    else
        echo "Specified passwordless sudo, but sudo is not installed in the container."
    fi
fi

# Prompt to create password
if [ -z "$PARAM_NO_PASSWORD" ] && [ -z "$PARAM_PASSWORDLESS_SUDO" ];
then
    echo "Setting password for user '$USERNAME' with uid=$PARAM_USER_ID"
    passwd "$USERNAME"
fi

if [ -z "$PARAM_SHELL" ];
then
    SHELL=$(awk -F: -v u="$USERNAME" '$1==u {print $7}' /etc/passwd)

    if [ -z "$SHELL" ]; then
        if command -v /bin/bash >/dev/null 2>&1; then
            SHELL="/bin/bash"
        else
            SHELL="/bin/sh"
        fi
    fi
fi

# su to user
if command -v su >/dev/null 2>&1;
then
    exec su -s "$SHELL" - "$USERNAME"
elif  command -v sudo >/dev/null 2>&1;
then
    exec sudo -iu "$USERNAME"
else
    echo "sudo / su not installed in the container. Manually enter the container with 'distrobox enter'"
fi

