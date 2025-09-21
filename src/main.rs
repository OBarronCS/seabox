use clap::{Args, Parser, Subcommand};
use figment::Figment;
use figment::providers::{Env, Format, Toml};
use std::collections::HashMap;
use std::fs;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio, exit};

const SEABOX_NAME: &str = "seabox";

const NEW_USER_USERNAME: &str = "user";

const DEFAULT_SUDO_PATH: &str = "sudo";

fn get_default_sudo_path() -> String {
    DEFAULT_SUDO_PATH.to_string()
}

static DEFAULT_SHELL: &[&str] = &[
    "/bin/sh",
    "-c",
    r###"USER=$(id -un)
SHELL_PATH=$(awk -F: -v u="$USER" '$1==u {print $7}' /etc/passwd)

if [ -z "$SHELL_PATH" ]; then
    if command -v /bin/bash >/dev/null 2>&1; then
        SHELL_PATH="/bin/bash"
    else
        SHELL_PATH="/bin/sh"
    fi
fi

export SHELL="$SHELL_PATH"
exec "$SHELL_PATH""###,
];

const INIT_SCRIPT: &str = include_str!("init.sh");

struct Context {
    config: Config,
    parsed_config_file: ConfigFileFormat,
}

#[derive(Default, Debug, serde::Deserialize, serde::Serialize)]
struct Config {
    image: Option<String>,

    #[serde(default = "get_default_sudo_path")]
    sudo_command: String,

    #[serde(default)]
    install_sudo: Option<bool>,

    #[serde(default)]
    no_password: bool,

    #[serde(default)]
    unsafe_setup_passwordless_sudo: bool,
}

#[derive(Default, Debug, serde::Deserialize, serde::Serialize)]
struct ConfigFileFormat {
    #[serde(flatten)]
    base: BaseConfig,

    #[serde(flatten)]
    image_specific: HashMap<String, BaseConfig>,
}

#[derive(Default, Debug, serde::Deserialize, serde::Serialize)]
struct BaseConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sudo_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    install_sudo: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    no_password: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unsafe_setup_passwordless_sudo: Option<bool>,
}

#[derive(Default, Debug, serde::Deserialize, serde::Serialize)]
struct BoxInstanceFormat {
    #[serde(rename = "name")]
    name: String,

    #[serde(rename = "image")]
    image: String,

    #[serde(rename = "container_id")]
    container_id: String,

    #[serde(rename = "rootful")]
    rootful: bool,
}

#[derive(Parser)]
#[command(version, about, long_about=None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Create(CreateArgs),
    Enter(EnterArgs),
    #[clap(visible_alias("rm"))]
    Remove(RemoveArgs),
    #[clap(visible_alias("tmp"))]
    Temp(TempArgs),
    #[clap(visible_alias("ls"))]
    List(ListArgs),
    Restart(RestartArgs),
    // #[clap(subcommand)]
    Config {
        #[command(subcommand)]
        inner: Option<ConfigSubcommand>,
    },
}

#[derive(Args)]
struct CreateArgs {
    name: String,

    #[command(flatten)]
    common: CreateAndTempSharedArgs,

    #[command(flatten)]
    all: AllCommandArgs,
}

#[derive(Args)]
struct EnterArgs {
    name: String,

    #[arg(short, long)]
    user: Option<String>,

    #[arg(short, long)]
    shell: Option<String>,

    #[command(flatten)]
    all: AllCommandArgs,
}

#[derive(Args)]
struct RemoveArgs {
    names: Vec<String>,

    #[command(flatten)]
    all: AllCommandArgs,
}

#[derive(Args)]
struct RestartArgs {
    names: Vec<String>,

    #[command(flatten)]
    all: AllCommandArgs,
}

#[derive(Args)]
struct TempArgs {
    #[command(flatten)]
    common: CreateAndTempSharedArgs,

    #[command(flatten)]
    all: AllCommandArgs,
}

#[derive(Args)]
struct ListArgs {
    #[command(flatten)]
    all: AllCommandArgs,
}

#[derive(Subcommand)]
enum ConfigSubcommand {
    Show,
}

#[derive(Args)]
struct AllCommandArgs {
    #[arg(long, default_value = "false")]
    dry_run: bool,

    #[arg(long, default_value = "false")]
    verbose: bool,
}

#[derive(Args, Default, Debug, serde::Deserialize, serde::Serialize)]
struct CreateAndTempSharedArgs {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[arg(short, long)]
    image: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[arg(short, long)]
    shell: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[arg(
        short,
        help = "Directory to mount (defaults to current working directory) to /mount in the container"
    )]
    directory: Option<String>,

    #[arg(
        long,
        help = "Do not mount the current working directory",
        action = clap::ArgAction::SetTrue
    )]
    no_dir: bool,

    #[arg(
        short,
        long,
        help = "Add additional mounts manually",
        long_help = "Add additional mounts with the format 'host_directory:container_directory'. Can be specified multiple times"
    )]
    volume: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[arg(
        short,
        long,
        allow_hyphen_values = true,
        help = "Additional arguments to pass to Podman",
        long_help = "Pass additional arguments to Podman - the string is broken into individual arguments using shell string parsing with shlex.\nExample: seabox create -p \"--pidfile /tmp/pidfile --cidfile /tmp/cidfile\" test"
    )]
    pass_through: Option<String>,

    #[arg(
        short,
        long,
        default_value = "false",
        help = "Use root user in container",
        long_help = "Use the root user in the container. Typically, in case the container doesn't already have an \"normal\" user (id >= 1000), one would be created and given sudo permissions so as to act as a counterpart to the host user. This flag results in such a user not being created."
    )]
    root: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[arg(
        long,
        help = "Attempt to install sudo + su in the container. Useful when using base distro images",
        value_parser = clap::builder::BoolishValueParser::new(), num_args(0..=1), default_missing_value = "true",
    )]
    install_sudo: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[arg(
        long,
        help = "Skip creation of password for user",
        aliases = ["no-passwd"],
        value_parser = clap::builder::BoolishValueParser::new(), num_args(0..=1), default_missing_value = "true",
    )]
    no_password: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[arg(
        long,
        help = "Edit /etc/sudoers file in the container to allow passwordless sudo. WARNING: this gives programs running in the container access to root on the system with no password. Implies --no-password.",
        value_parser = clap::builder::BoolishValueParser::new(), num_args(0..=1), default_missing_value = "true",
    )]
    unsafe_setup_passwordless_sudo: Option<bool>,
}

#[derive(serde::Deserialize)]
struct PodmanContainerInspectFormat {
    #[serde(rename = "Mounts")]
    mounts: Vec<MountType>,

    #[serde(rename = "State")]
    state: StateType,

    #[serde(rename = "Config")]
    config: ConfigType,
}

#[derive(serde::Deserialize)]
struct MountType {
    #[serde(rename = "Source")]
    source: String,
}
#[derive(serde::Deserialize)]
struct StateType {
    #[serde(rename = "Running")]
    running: bool,
}

#[derive(serde::Deserialize)]
struct ConfigType {
    #[serde(rename = "User")]
    user: String,
}

#[derive(serde::Deserialize)]
struct PodmanImageInspectFormat {
    #[serde(rename = "Labels")]
    labels: Option<HashMap<String, String>>,
}

fn get_configuration_file_path() -> String {
    let project = directories::ProjectDirs::from("rs", "", SEABOX_NAME).unwrap();

    let config_dir = project.config_dir();

    config_dir
        .join(format!("{SEABOX_NAME}.toml"))
        .to_str()
        .unwrap()
        .to_string()
}

fn read_configuration_file() -> ConfigFileFormat {
    // Returns default values of Config file if not found
    let config_file_path = get_configuration_file_path();
    let config_as_str = std::fs::read_to_string(&config_file_path).unwrap_or_default();
    toml::from_str(&config_as_str).unwrap()
}

fn create_config(base: &BaseConfig, profile: Option<&BaseConfig>) -> Config {
    let mut config = Figment::new().merge(figment::providers::Serialized::defaults(base));

    if let Some(p) = profile {
        config = config.merge(figment::providers::Serialized::defaults(p));
    }

    config.merge(Env::prefixed("SEABOX_")).extract().unwrap()
}

fn main() {
    let parsed: ConfigFileFormat = read_configuration_file();

    let config = Figment::new()
        .merge(figment::providers::Serialized::defaults(&parsed.base))
        .merge(Env::prefixed("SEABOX_"))
        .extract()
        .unwrap();

    let mut context: Context = Context {
        config,
        parsed_config_file: parsed,
    };

    let cli = Cli::parse();

    context.run(cli);
}

impl Context {
    fn run(&mut self, cli: Cli) {
        match &cli.command {
            Some(Commands::Create(args)) => {
                self.resolve_config_args_create_tmp(&args.common);
                eprintln!("{:?}", self.config);
                self.handle_create(args)
            }
            Some(Commands::Enter(args)) => self.handle_enter(args),
            Some(Commands::Remove(args)) => self.handle_remove(args),
            Some(Commands::Temp(args)) => {
                self.resolve_config_args_create_tmp(&args.common);
                self.handle_temp(args)
            }
            Some(Commands::List(args)) => self.handle_list(args),
            Some(Commands::Restart(args)) => self.handle_restart(args),
            Some(Commands::Config {
                inner: Some(ConfigSubcommand::Show),
            }) => self.handle_config_show(),
            Some(Commands::Config { inner: None }) => {
                println!("{}", get_configuration_file_path())
            }
            _ => {}
        }
    }

    fn resolve_config_args_create_tmp(&mut self, cli_config_args: &CreateAndTempSharedArgs) {
        // Config merge hierarchy:
        // CLI > Env > Profile in config > config > defaults

        // Two passes of merging config - first we need to resolve the image
        // Once image has been resolved, insert the "image profile" into the merge hierarchy.

        let config_with_cli_flags: Config =
            Figment::from(figment::providers::Serialized::defaults(&self.config))
                .merge(figment::providers::Serialized::defaults(&cli_config_args))
                .extract()
                .unwrap();

        // If we have a profile for this image, apply it it to the config merge hierarchy
        if let Some(cli_image) = &config_with_cli_flags.image {
            for profile in &self.parsed_config_file.image_specific {
                if profile.0 == cli_image {
                    self.config = create_config(&self.parsed_config_file.base, Some(profile.1));
                }
            }
        }

        // Override with CLI args again
        self.config = Figment::from(figment::providers::Serialized::defaults(&self.config))
            .merge(figment::providers::Serialized::defaults(&cli_config_args))
            .extract()
            .unwrap();
    }

    fn resolve_image(&self, image: Option<String>) -> Option<String> {
        match image {
            Some(i) => Some(i),
            _ => {
                let image = &self.config.image;
                if let Some(x) = image
                    && !x.is_empty()
                {
                    Some(x.to_string())
                } else {
                    None
                }
            }
        }
    }

    fn generate_create_container_command(
        &self,
        image: Option<String>,
        name: &str,
        root: bool,
        temp: bool,
        passthrough: Option<String>,
        directory: Option<String>,
        no_dir: bool,
        additional_mounts: Vec<String>,
        dry_run: bool,
    ) -> (Vec<String>, bool, i64, i64, String) {
        let image = &self.resolve_image(image);

        let image: &str = {
            if let Some(x) = image {
                x
            } else {
                eprintln!("No default image found and no image provided with --image");
                exit(1);
            }
        };

        let hostname = format!("{}-{}", SEABOX_NAME, name);

        let host_user_id = nix::unistd::geteuid();
        let host_user_gid = nix::unistd::getegid();

        const DEFAULT_USER_ID: i64 = 1000;
        let mut container_user_id = DEFAULT_USER_ID;
        let mut container_user_gid = DEFAULT_USER_ID;

        let mut create_user = false;

        if !root {
            let target_uid_gid = self.determine_container_uid_gid(image, dry_run);

            // If target uid/gid not found, we may want to create a user if not --root setting
            // This also changes the idmap_parameters
            match target_uid_gid {
                Some((x, y)) => {
                    container_user_id = x;
                    container_user_gid = y;
                }
                None => {
                    create_user = true;
                }
            }
        } else {
            container_user_id = 0;
            container_user_gid = 0;
        }

        let current_dir: std::path::PathBuf = {
            if let Some(x) = directory {
                let path = std::path::PathBuf::from(&x);
                match fs::canonicalize(path) {
                    Ok(p) => p,
                    Err(_) => {
                        eprintln!("Directory '{}' does not exist", x);
                        exit(1);
                    }
                }
            } else {
                std::env::current_dir().expect("Current working directory not found")
            }
        };

        let current_dir = String::from(current_dir.to_str().unwrap());

        let idmap_parameters: String = {
            if root {
                "0-0-2000;gids=0-0-2000".to_string()
            } else {
                format!(
                    "{host_user_id}-{container_user_id}-1#0-0-1;gids={host_user_gid}-{container_user_gid}-1#0-0-1",
                )
            }
        };

        let mount = &format!(
            "type=bind,source={},destination=/mount/,idmap=uids={}",
            current_dir, idmap_parameters
        );

        let mut additional_mount_strings: Vec<String> = vec![];

        for mount_specifier in additional_mounts {
            let values: Vec<&str> = mount_specifier.split(":").collect();
            if values.len() != 2 {
                eprintln!("Invalid format for mount: {}", mount_specifier);
                exit(1);
            }
            let host_dir = values[0];
            let container_dir = values[1];

            additional_mount_strings.extend(vec![
                "--mount".to_string(),
                format!(
                    "type=bind,source={},destination={},idmap=uids={}",
                    host_dir, container_dir, idmap_parameters
                )
                .to_string(),
            ]);
        }

        let mut arguments: Vec<String> = [
            &self.config.sudo_command,
            "podman",
            "run",
            "--label",
            &format!("{}=true", SEABOX_NAME),
            "--privileged",
            "-it",
        ]
        .iter()
        .map(|x| x.to_string())
        .collect::<Vec<String>>();

        if temp {
            arguments.push("--rm".to_string())
        } else {
            arguments.push("-d".to_string())
        }

        if let Some(passthrough) = passthrough
            && let Some(pass_through_args) = shlex::split(&passthrough)
        {
            arguments.extend(pass_through_args);
        }

        let add_host_str = &format!("{hostname}:127.0.0.1");
        let user_string = {
            if temp {
                "0:0"
            } else {
                &format!("{container_user_id}:{container_user_gid}")
            }
        };

        arguments.extend(
            [
                "--network",
                "host",
                "--hostname",
                &hostname,
                "--add-host",
                add_host_str,
                "-u",
                user_string,
                "--passwd=false",
                "-w",
                "/mount/",
            ]
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<String>>(),
        );

        if !no_dir {
            arguments.push("--mount".to_string());
            arguments.push(mount.to_string());
        }

        arguments.extend(additional_mount_strings);

        arguments.extend(
            ["--name", name, image]
                .iter()
                .map(|x| x.to_string())
                .collect::<Vec<String>>(),
        );

        (
            arguments,
            create_user,
            container_user_id,
            container_user_gid,
            image.to_string(),
        )
    }

    fn generate_container_inspect_command(&self, name: &str) -> Vec<String> {
        vec![
            &self.config.sudo_command,
            "podman",
            "container",
            "inspect",
            name,
        ]
        .into_iter()
        .map(String::from)
        .collect()
    }

    fn handle_create(&mut self, args: &CreateArgs) {
        let container_inspect_command = self.generate_container_inspect_command(&args.name);

        if args.all.dry_run {
            print_command(container_inspect_command.clone());
        }

        let (
            mut create_container_command,
            create_user,
            container_user_id,
            _container_user_gid,
            _image,
        ) = self.generate_create_container_command(
            args.common.image.clone(),
            &args.name,
            args.common.root,
            false,
            args.common.pass_through.clone(),
            args.common.directory.clone(),
            args.common.no_dir,
            args.common.volume.clone(),
            args.all.dry_run,
        );

        create_container_command.push("/bin/sh".to_string());

        if args.all.dry_run {
            print_command(create_container_command);
            return;
        }

        let result = std::process::Command::new(&container_inspect_command[0])
            .args(&container_inspect_command[1..])
            .stderr(Stdio::piped())
            .stdout(Stdio::piped())
            .status()
            .expect("Failed to run command");

        if let Some(0) = result.code() {
            eprintln!("A container with name '{}' already exists", args.name);
            exit(1);
        }

        std::process::Command::new(&create_container_command[0])
            .args(&create_container_command[1..])
            .output()
            .expect("Failed to run command");

        let initial_enter_script = {
            if !args.common.root {
                vec![
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    create_initial_enter_script(
                        create_user,
                        NEW_USER_USERNAME,
                        container_user_id,
                        self.config.unsafe_setup_passwordless_sudo,
                        self.config.no_password,
                        self.config.install_sudo,
                        args.common.shell.clone(),
                        args.all.verbose,
                    ),
                ]
            } else {
                vec![]
            }
        };

        self.enter_container(
            &args.name,
            Some("root".to_string()),
            args.common.shell.clone(),
            args.all.dry_run,
            initial_enter_script,
        );
    }

    fn generate_image_inspect_command(&self, image: &str) -> Vec<String> {
        vec![
            &self.config.sudo_command,
            "podman",
            "image",
            "inspect",
            image,
        ]
        .into_iter()
        .map(String::from)
        .collect()
    }

    fn image_inspect(&self, image: &str, dry_run: bool) -> Option<String> {
        let inspect_image_command = self.generate_image_inspect_command(image);

        if dry_run {
            print_command(inspect_image_command.clone());
        }

        let result = std::process::Command::new(&inspect_image_command[0])
            .args(&inspect_image_command[1..])
            .output();

        match result {
            Ok(r) => match r.status.code() {
                Some(0) => {
                    let x = r.stdout.to_owned();
                    Some(String::from_utf8_lossy(&x).to_string())
                }
                _ => None,
            },
            Err(_) => None,
        }
    }

    fn generate_image_pull_command(&self, image: &str) -> Vec<String> {
        vec![&self.config.sudo_command, "podman", "pull", image]
            .into_iter()
            .map(String::from)
            .collect()
    }

    fn generate_cat_etc_password_command(&self, image: &str) -> Vec<String> {
        vec![
            &self.config.sudo_command,
            "podman",
            "run",
            "--rm",
            "--entrypoint",
            "cat",
            image,
            "/etc/passwd",
        ]
        .into_iter()
        .map(String::from)
        .collect()
    }

    fn determine_container_uid_gid(&self, image: &str, dry_run: bool) -> Option<(i64, i64)> {
        let result = {
            match self.image_inspect(image, dry_run) {
                Some(x) => x,
                None => {
                    let image_pull_command = self.generate_image_pull_command(image);

                    if dry_run {
                        println!(
                            "# Need to pull image at this point - cannot proceed with dry run"
                        );
                        print_command(image_pull_command);
                        exit(1);
                    }

                    let pull = std::process::Command::new(&image_pull_command[0])
                        .args(&image_pull_command[1..])
                        .status()
                        .expect("Failed to run command");

                    if let Some(x) = pull.code()
                        && x != 0
                    {
                        exit(1);
                    }

                    self.image_inspect(image, dry_run).unwrap()
                }
            }
        };

        let inspect: Vec<PodmanImageInspectFormat> =
            serde_json::from_str(&result).expect("JSON parse error");

        if let Some(element) = inspect.first()
            && let Some(map) = &element.labels
            && let Some(uid) = map.get("SEABOX_USER_ID")
        {
            let uid: i64 = uid.parse().unwrap();
            return Some((uid, uid));
        }

        let cat_etc_passwd_command = self.generate_cat_etc_password_command(image);

        if dry_run {
            print_command(cat_etc_passwd_command.clone());
        }

        let ect_passwd = std::process::Command::new(&cat_etc_passwd_command[0])
            .args(&cat_etc_passwd_command[1..])
            .output();

        if let Ok(output) = ect_passwd {
            let mut user_info: Vec<(&str, i64, i64)> = vec![];

            let string = String::from_utf8_lossy(&output.stdout);

            for line in string.lines() {
                let values: Vec<&str> = line.split(":").collect();
                let username = values[0];
                let uid = values[2];
                let gid = values[3];

                let uid = uid.parse::<i64>().unwrap();
                let gid = gid.parse::<i64>().unwrap();

                user_info.push((username, uid, gid))
            }

            let mut user_info: Vec<_> = user_info
                .iter()
                .filter(|x| x.1 >= 1000 && x.1 < 2000)
                .collect();
            user_info.sort_by_key(|k| k.1);

            if let Some(x) = user_info.last() {
                return Some((x.1, x.2));
            }
        }

        None
    }

    fn handle_enter(&self, args: &EnterArgs) {
        self.enter_container(
            &args.name,
            args.user.clone(),
            args.shell.clone(),
            args.all.dry_run,
            vec![],
        );
    }

    fn generate_container_enter_command(
        &self,
        user: &str,
        name: &str,
        exec_command: Vec<String>,
        relative_path: &str,
    ) -> Vec<String> {
        let dir = &format!("/mount/{relative_path}");
        let mut command: Vec<String> = vec![
            &self.config.sudo_command,
            "podman",
            "exec",
            "-it",
            "-w",
            dir,
            "--user",
            &user,
            name,
        ]
        .into_iter()
        .map(String::from)
        .collect();

        command.extend(exec_command);

        command
    }

    fn enter_container(
        &self,
        name: &str,
        username: Option<String>,
        shell: Option<String>,
        dry_run: bool,
        append_args: Vec<String>,
    ) {
        let shell_command: Vec<String> = {
            if !append_args.is_empty() {
                append_args
            } else if let Some(s) = &shell {
                vec![s.to_string()]
            } else {
                DEFAULT_SHELL.iter().map(|x| x.to_string()).collect()
            }
        };

        let container_inspect_command = self.generate_container_inspect_command(name);
        let container_start_command = self.generate_container_start_command(name);

        let result = std::process::Command::new(&container_inspect_command[0])
            .args(&container_inspect_command[1..])
            .stderr(Stdio::piped())
            .stdout(Stdio::piped())
            .output()
            .expect("Failed to run command");

        match result.status.code() {
            Some(code) if code != 0 => {
                eprintln!("A container with name '{}' does not exist", name);
                exit(1);
            }
            _ => {}
        }

        let stdout_text = String::from_utf8_lossy(&result.stdout);
        let info: Vec<PodmanContainerInspectFormat> =
            serde_json::from_str(&stdout_text).expect("JSON parse error");

        let host_dir = &info[0].mounts[0].source;
        let absolute_path = std::path::absolute(host_dir).expect("Couldn't make path absolute");

        let current_dir = std::env::current_dir().expect("Current working directory not found");
        let current_dir = current_dir.to_str().unwrap();
        let cwd_path = std::path::absolute(current_dir).expect("Couldn't make path absolute");

        let rel = cwd_path
            .strip_prefix(absolute_path)
            .ok()
            .and_then(|x| x.to_str())
            .unwrap_or("");

        let user = match username {
            Some(x) => x,
            _ => info[0].config.user.to_string(),
        };

        let container_enter_command =
            self.generate_container_enter_command(&user, name, shell_command, rel);

        if dry_run {
            print_command(container_inspect_command);
            print_command(container_start_command);
            print_command(container_enter_command);
            return;
        }

        if !info[0].state.running {
            let result = std::process::Command::new(&container_start_command[0])
                .args(&container_start_command[1..])
                .status()
                .expect("Failed to run command");

            if let Some(x) = result.code()
                && x != 0
            {
                eprintln!("Failed to start container");
                exit(1);
            }
        }

        let exec = std::process::Command::new(&container_enter_command[0])
            .args(&container_enter_command[1..])
            .exec();

        eprintln!("Error: {exec}");
        exit(1);
    }

    fn handle_remove(&self, args: &RemoveArgs) {
        for name in &args.names {
            let stop_container_command = self.generate_container_stop_command(name);
            let delete_container_command = self.generate_container_delete_command(name);

            if args.all.dry_run {
                print_command(stop_container_command);
                print_command(delete_container_command);
            } else {
                println!("Deleting container {name}");

                let _result = Command::new(&stop_container_command[0])
                    .args(&stop_container_command[1..])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .status()
                    .expect("Failed to execute command");

                let _result = Command::new(&delete_container_command[0])
                    .args(&delete_container_command[1..])
                    .status()
                    .expect("Failed to execute command");
            }
        }
    }

    fn handle_temp(&self, args: &TempArgs) {
        let shell: Vec<String> = {
            if let Some(s) = &args.common.shell {
                vec![s.to_string()]
            } else {
                DEFAULT_SHELL.iter().map(|x| x.to_string()).collect()
            }
        };

        let (
            mut create_container_command,
            create_user,
            container_user_id,
            _container_user_gid,
            _image,
        ) = self.generate_create_container_command(
            args.common.image.clone(),
            "",
            args.common.root,
            true,
            args.common.pass_through.clone(),
            args.common.directory.clone(),
            args.common.no_dir,
            args.common.volume.clone(),
            args.all.dry_run,
        );

        let user_command = {
            if !args.common.root {
                vec![
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    create_initial_enter_script(
                        create_user,
                        NEW_USER_USERNAME,
                        container_user_id,
                        self.config.unsafe_setup_passwordless_sudo,
                        self.config.no_password,
                        self.config.install_sudo,
                        args.common.shell.clone(),
                        args.all.verbose,
                    ),
                ]
            } else {
                shell
            }
        };

        create_container_command.extend(user_command);

        if args.all.dry_run {
            print_command(create_container_command);
            return;
        }

        let result = std::process::Command::new(&create_container_command[0])
            .args(&create_container_command[1..])
            .status();

        if result.is_err() {
            eprintln!("{:?}", result.expect_err(""))
        }
    }

    fn generate_list_containers_command(&self) -> Vec<String> {
        vec![
            &self.config.sudo_command,
            "podman",
            "ps",
            "--all",
            "--filter",
            &format!("label={}=true", SEABOX_NAME),
        ]
        .into_iter()
        .map(String::from)
        .collect()
    }

    fn handle_list(&self, args: &ListArgs) {
        let list_containers_command = self.generate_list_containers_command();

        if args.all.dry_run {
            print_command(list_containers_command);
        } else {
            let _result = Command::new(&list_containers_command[0])
                .args(&list_containers_command[1..])
                .status()
                .expect("Failed to execute command");
        }
    }

    fn generate_container_stop_command(&self, name: &str) -> Vec<String> {
        vec![&self.config.sudo_command, "podman", "kill", name]
            .into_iter()
            .map(String::from)
            .collect()
    }

    fn generate_container_delete_command(&self, name: &str) -> Vec<String> {
        vec![
            &self.config.sudo_command,
            "podman",
            "container",
            "rm",
            "--force",
            name,
        ]
        .into_iter()
        .map(String::from)
        .collect()
    }

    fn generate_container_start_command(&self, name: &str) -> Vec<String> {
        vec![&self.config.sudo_command, "podman", "start", name]
            .into_iter()
            .map(String::from)
            .collect()
    }

    fn handle_restart(&self, args: &RestartArgs) {
        for name in &args.names {
            let stop_container_command = self.generate_container_stop_command(name);
            let start_container_command = self.generate_container_start_command(name);

            if args.all.dry_run {
                print_command(stop_container_command);
                print_command(start_container_command);
            } else {
                let _result = Command::new(&stop_container_command[0])
                    .args(&stop_container_command[1..])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .status()
                    .expect("Failed to execute command");

                let _result = Command::new(&start_container_command[0])
                    .args(&start_container_command[1..])
                    .status()
                    .expect("Failed to execute command");
            }
        }
    }

    fn handle_config_show(&self) {
        let cfg = get_configuration_file_path();

        let file_contents = fs::read_to_string(&cfg);
        if let Ok(x) = file_contents {
            println!("# Viewing '{}'", &cfg);
            println!("{}", x);
        } else {
            eprintln!("Config file not found at {}", &cfg);
        }
    }
}

fn print_command(command_args: Vec<String>) {
    let command = shlex::try_join(command_args.iter().map(|x| &**x)).unwrap();
    println!("{}", &command)
}

fn create_initial_enter_script(
    create_user: bool,
    username: &str,
    container_user_id: i64,
    passwordless_sudo: bool,
    no_password: bool,
    install_sudo: Option<bool>,
    shell: Option<String>,
    verbose: bool,
) -> String {
    let param_sudo_install_prompt = {
        if let Some(x) = install_sudo {
            if x { "install" } else { "no_install" }
        } else {
            "prompt"
        }
    };

    let shell = shell.unwrap_or("".to_string());

    INIT_SCRIPT
        .replace("INSERT_CREATE_USER", if create_user { "1" } else { "" })
        .replace("INSERT_NEW_USERNAME", username)
        .replace("INSERT_CONTAINER_ID", &container_user_id.to_string())
        .replace("INSERT_SUDO_INSTALL", param_sudo_install_prompt)
        .replace(
            "INSERT_PASSWORDLESS_SUDO",
            if passwordless_sudo { "1" } else { "" },
        )
        .replace("INSERT_CREATE_PASSWORD", if no_password { "1" } else { "" })
        .replace("INSERT_VERBOSE", if verbose { "1" } else { "" })
        .replace("INSERT_SHELL", &shell)
}
