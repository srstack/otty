use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use otty_libterm::pty::{self, SSHAuth, Session};
use otty_ui_term::settings::{
    LocalSessionOptions, SSHSessionOptions, SessionKind,
};

use super::constants::SSH_DEFAULT_PORT;
use super::errors::{QuickLaunchError, QuickLaunchWizardError};
use super::state::WizardEditorState;
use super::types::{
    CommandSpec, CustomCommand, EnvVar, NodePath, PreparedQuickLaunch,
    QuickLaunch, QuickLaunchSetupOutcome, QuickLaunchType, SshCommand,
};
use crate::services::terminal_settings_for_session;

const QUICK_LAUNCH_SSH_TIMEOUT: Duration = Duration::from_secs(15);

/// Prepare a quick launch command asynchronously.
///
/// Validates the command, resolves paths, and returns a prepared launch
/// or an error/canceled outcome.
pub(crate) async fn prepare_quick_launch_setup(
    command: QuickLaunch,
    path: NodePath,
    launch_id: u64,
    terminal_settings: otty_ui_term::settings::Settings,
    cancel: Arc<AtomicBool>,
) -> QuickLaunchSetupOutcome {
    if cancel.load(Ordering::Relaxed) {
        return QuickLaunchSetupOutcome::Canceled { path, launch_id };
    }

    if let Err(error) = validate_command(&command) {
        return QuickLaunchSetupOutcome::Failed {
            path,
            launch_id,
            command: Box::new(command),
            error: Arc::new(error),
        };
    }

    if cancel.load(Ordering::Relaxed) {
        return QuickLaunchSetupOutcome::Canceled { path, launch_id };
    }

    if let Err(error) = probe_launch_runtime(&command, &cancel) {
        return QuickLaunchSetupOutcome::Failed {
            path,
            launch_id,
            command: Box::new(command),
            error: Arc::new(error),
        };
    }

    let session = command_session(&command, &cancel);
    let settings = terminal_settings_for_session(&terminal_settings, session);
    let title = command.title().to_string();

    if cancel.load(Ordering::Relaxed) {
        return QuickLaunchSetupOutcome::Canceled { path, launch_id };
    }

    QuickLaunchSetupOutcome::Prepared(Box::new(PreparedQuickLaunch {
        path,
        launch_id,
        title,
        settings,
        command: Box::new(command),
    }))
}

/// Validate a quick launch command before execution.
fn validate_command(command: &QuickLaunch) -> Result<(), QuickLaunchError> {
    match command.spec() {
        CommandSpec::Custom { custom } => {
            if custom.program().trim().is_empty() {
                return Err(QuickLaunchError::Validation {
                    message: String::from("Program path is empty."),
                });
            }

            validate_custom_runtime(custom)?;
        },
        CommandSpec::Ssh { ssh } => {
            if ssh.host().trim().is_empty() {
                return Err(QuickLaunchError::Validation {
                    message: String::from("SSH host is empty."),
                });
            }
            if ssh.port() == 0 {
                return Err(QuickLaunchError::Validation {
                    message: String::from("SSH port must be greater than 0."),
                });
            }

            validate_ssh_runtime(ssh)?;
        },
    }
    Ok(())
}

fn probe_launch_runtime(
    command: &QuickLaunch,
    cancel: &Arc<AtomicBool>,
) -> Result<(), QuickLaunchError> {
    match command.spec() {
        CommandSpec::Custom { .. } => Ok(()),
        CommandSpec::Ssh { ssh } => probe_ssh_session(ssh, cancel),
    }
}

fn probe_ssh_session(
    ssh: &super::types::SshCommand,
    cancel: &Arc<AtomicBool>,
) -> Result<(), QuickLaunchError> {
    let options = ssh_session(ssh, cancel);
    let mut builder = pty::ssh()
        .with_host(options.host())
        .with_user(options.user())
        .with_auth(options.auth());

    if let Some(timeout) = options.timeout() {
        builder = builder.with_timeout(timeout);
    }

    if let Some(cancel_token) = options.cancel_token() {
        builder = builder.with_cancel_token(cancel_token.clone());
    }

    let mut session =
        builder
            .spawn()
            .map_err(|err| QuickLaunchError::Validation {
                message: format!("SSH connection failed: {err}"),
            })?;
    let _ = session.close();
    Ok(())
}

fn command_session(
    command: &QuickLaunch,
    cancel: &Arc<AtomicBool>,
) -> SessionKind {
    match command.spec() {
        CommandSpec::Custom { custom } => {
            SessionKind::from_local_options(custom_session(custom))
        },
        CommandSpec::Ssh { ssh } => {
            SessionKind::from_ssh_options(ssh_session(ssh, cancel))
        },
    }
}

fn custom_session(custom: &super::types::CustomCommand) -> LocalSessionOptions {
    let mut options = LocalSessionOptions::default()
        .with_program(custom.program())
        .with_args(custom.args().to_vec());

    for env in custom.env() {
        options = options.with_env(env.key(), env.value());
    }

    if let Some(dir) = custom.working_directory() {
        options = options.with_working_directory(PathBuf::from(dir));
    }

    options
}

fn ssh_session(
    ssh: &super::types::SshCommand,
    cancel: &Arc<AtomicBool>,
) -> SSHSessionOptions {
    let host = format!("{}:{}", ssh.host(), ssh.port());
    let user = ssh
        .user()
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string)
        .or_else(|| std::env::var("USER").ok())
        .or_else(|| std::env::var("USERNAME").ok())
        .unwrap_or_default();

    let auth = ssh
        .identity_file()
        .filter(|path| !path.trim().is_empty())
        .map(|path| SSHAuth::KeyFile {
            private_key_path: path.to_string(),
            passphrase: None,
        })
        .or_else(|| {
            default_identity_file().map(|path| SSHAuth::KeyFile {
                private_key_path: path.display().to_string(),
                passphrase: None,
            })
        })
        .unwrap_or_else(|| SSHAuth::Password(String::new()));

    SSHSessionOptions::default()
        .with_host(&host)
        .with_user(&user)
        .with_auth(auth)
        .with_timeout(QUICK_LAUNCH_SSH_TIMEOUT)
        .with_cancel_token(cancel.clone())
}

fn validate_custom_runtime(
    custom: &super::types::CustomCommand,
) -> Result<(), QuickLaunchError> {
    let program = custom.program().trim();
    let _ = find_program_path(program)?;

    if let Some(dir) = custom.working_directory() {
        let expanded = expand_tilde(dir);
        let path = Path::new(&expanded);

        if !path.exists() {
            return Err(QuickLaunchError::Validation {
                message: format!("Working directory not found: {expanded}"),
            });
        }

        if !path.is_dir() {
            return Err(QuickLaunchError::Validation {
                message: format!(
                    "Working directory is not a directory: {expanded}"
                ),
            });
        }
    }

    Ok(())
}

fn validate_ssh_runtime(
    ssh: &super::types::SshCommand,
) -> Result<(), QuickLaunchError> {
    if let Some(identity) = ssh.identity_file() {
        let identity = identity.trim();
        if !identity.is_empty() {
            let expanded = expand_tilde(identity);
            let path = Path::new(&expanded);

            if !path.exists() {
                return Err(QuickLaunchError::Validation {
                    message: format!("Identity file not found: {expanded}"),
                });
            }

            if !path.is_file() {
                return Err(QuickLaunchError::Validation {
                    message: format!("Identity file is not a file: {expanded}"),
                });
            }
        }
    }

    Ok(())
}

fn find_program_path(program: &str) -> Result<PathBuf, QuickLaunchError> {
    let program = program.trim();
    let has_separator = program.contains('/') || program.contains('\\');
    let is_explicit = has_separator || program.starts_with('~');

    if is_explicit {
        let expanded = expand_tilde(program);
        let path = PathBuf::from(&expanded);
        return validate_program_path(&path, &expanded);
    }

    let paths: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|value| std::env::split_paths(&value).collect())
        .unwrap_or_default();

    let exts = executable_extensions();

    for dir in paths {
        for candidate in program_candidates(&dir, program, &exts) {
            if is_executable_path(&candidate) {
                return Ok(candidate);
            }
        }
    }

    Err(QuickLaunchError::Validation {
        message: format!("Program not found in PATH: {program}"),
    })
}

/// Candidate paths for a bare program name within one PATH directory.
/// On Windows, PATHEXT extensions are appended when the program has no
/// recognized extension yet; unix always yields the single direct path.
fn program_candidates(
    dir: &Path,
    program: &str,
    exts: &[String],
) -> Vec<PathBuf> {
    let mut candidates = vec![dir.join(program)];

    let lower = program.to_ascii_lowercase();
    let has_known_ext = exts
        .iter()
        .any(|ext| lower.ends_with(&ext.to_ascii_lowercase()));

    if !has_known_ext {
        candidates
            .extend(exts.iter().map(|ext| dir.join(format!("{program}{ext}"))));
    }

    candidates
}

/// Executable extensions consulted during PATH lookup (Windows PATHEXT).
#[cfg(windows)]
fn executable_extensions() -> Vec<String> {
    const DEFAULT_PATHEXT: [&str; 4] = [".COM", ".EXE", ".BAT", ".CMD"];

    match std::env::var("PATHEXT") {
        Ok(value) => value
            .split(';')
            .filter(|ext| !ext.is_empty())
            .map(ToString::to_string)
            .collect(),
        Err(_) => DEFAULT_PATHEXT.iter().map(ToString::to_string).collect(),
    }
}

/// Executable extensions consulted during PATH lookup (none on unix).
#[cfg(not(windows))]
fn executable_extensions() -> Vec<String> {
    Vec::new()
}

fn validate_program_path(
    path: &Path,
    label: &str,
) -> Result<PathBuf, QuickLaunchError> {
    if !path.exists() {
        return Err(QuickLaunchError::Validation {
            message: format!("Program not found: {label}"),
        });
    }

    if path.is_dir() {
        return Err(QuickLaunchError::Validation {
            message: format!("Program is a directory: {label}"),
        });
    }

    if !is_executable_path(path) {
        return Err(QuickLaunchError::Validation {
            message: format!("Program is not executable: {label}"),
        });
    }

    Ok(path.to_path_buf())
}

fn is_executable_path(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|meta| meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }

    #[cfg(not(unix))]
    {
        true
    }
}

fn expand_tilde(path: &str) -> String {
    let Some(home) = crate::paths::home_dir() else {
        return path.to_string();
    };

    if path == "~" {
        return home;
    }

    if let Some(rest) = path.strip_prefix("~/") {
        return format!("{home}/{rest}");
    }

    path.to_string()
}

/// Build a domain quick launch command from editor draft state.
pub(crate) fn build_command(
    editor: &WizardEditorState,
) -> Result<QuickLaunch, QuickLaunchWizardError> {
    let title = editor.title().trim();
    if title.is_empty() {
        return Err(QuickLaunchWizardError::TitleRequired);
    }

    let spec = match editor.command_type() {
        QuickLaunchType::Custom => {
            let Some(custom) = editor.custom() else {
                return Err(QuickLaunchWizardError::MissingCustomDraft);
            };
            let program = custom.program().trim();
            if program.is_empty() {
                return Err(QuickLaunchWizardError::ProgramRequired);
            }

            let env = custom
                .env()
                .iter()
                .filter_map(|(key, value)| {
                    let key = key.trim();
                    if key.is_empty() {
                        return None;
                    }
                    Some(EnvVar {
                        key: key.to_string(),
                        value: value.clone(),
                    })
                })
                .collect::<Vec<_>>();

            let working_directory = custom.working_directory().trim();

            CommandSpec::Custom {
                custom: CustomCommand {
                    program: program.to_string(),
                    args: custom.args().to_vec(),
                    env,
                    working_directory: if working_directory.is_empty() {
                        None
                    } else {
                        Some(working_directory.to_string())
                    },
                },
            }
        },
        QuickLaunchType::Ssh => {
            let Some(ssh) = editor.ssh() else {
                return Err(QuickLaunchWizardError::MissingSshDraft);
            };
            let host = ssh.host().trim();
            if host.is_empty() {
                return Err(QuickLaunchWizardError::HostRequired);
            }

            let port = ssh.port().trim();
            let port = if port.is_empty() {
                SSH_DEFAULT_PORT
            } else {
                port.parse::<u16>()
                    .map_err(|_| QuickLaunchWizardError::InvalidPort)?
            };

            CommandSpec::Ssh {
                ssh: SshCommand {
                    host: host.to_string(),
                    port,
                    user: optional_string(ssh.user()),
                    identity_file: optional_string(ssh.identity_file()),
                    extra_args: ssh.extra_args().to_vec(),
                },
            }
        },
    };

    Ok(QuickLaunch {
        title: title.to_string(),
        spec,
    })
}

fn optional_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Default SSH identity candidates in OpenSSH order.
const DEFAULT_IDENTITY_FILES: [&str; 4] =
    ["id_rsa", "id_ecdsa", "id_ed25519", "id_dsa"];

/// First existing default SSH identity file under the user's `~/.ssh`.
fn default_identity_file() -> Option<PathBuf> {
    let home = crate::paths::home_dir()?;
    default_identity_file_in(&PathBuf::from(home).join(".ssh"))
}

/// First existing default identity file within the given ssh directory.
fn default_identity_file_in(ssh_dir: &Path) -> Option<PathBuf> {
    DEFAULT_IDENTITY_FILES
        .iter()
        .map(|name| ssh_dir.join(name))
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use super::*;
    use crate::widgets::quick_launch::types::{CustomCommand, SshCommand};

    #[cfg(unix)]
    const EXISTING_PROGRAM: &str = "bash";
    #[cfg(windows)]
    const EXISTING_PROGRAM: &str = "cmd";

    #[test]
    fn given_empty_program_when_validating_then_error_returned() {
        let cmd = QuickLaunch {
            title: String::from("Bad"),
            spec: CommandSpec::Custom {
                custom: CustomCommand {
                    program: String::new(),
                    args: Vec::new(),
                    env: Vec::new(),
                    working_directory: None,
                },
            },
        };
        assert!(validate_command(&cmd).is_err());
    }

    #[test]
    fn given_valid_custom_command_when_validating_then_ok() {
        let cmd = QuickLaunch {
            title: String::from("Good"),
            spec: CommandSpec::Custom {
                custom: CustomCommand {
                    program: String::from(EXISTING_PROGRAM),
                    args: Vec::new(),
                    env: Vec::new(),
                    working_directory: None,
                },
            },
        };
        assert!(validate_command(&cmd).is_ok());
    }

    #[test]
    fn given_empty_ssh_host_when_validating_then_error_returned() {
        let cmd = QuickLaunch {
            title: String::from("SSH"),
            spec: CommandSpec::Ssh {
                ssh: SshCommand {
                    host: String::new(),
                    port: 22,
                    user: None,
                    identity_file: None,
                    extra_args: Vec::new(),
                },
            },
        };
        assert!(validate_command(&cmd).is_err());
    }

    #[test]
    fn given_missing_program_when_validating_then_error_returned() {
        let cmd = QuickLaunch {
            title: String::from("Missing"),
            spec: CommandSpec::Custom {
                custom: CustomCommand {
                    program: String::from("otty-v3-test-bin-does-not-exist"),
                    args: Vec::new(),
                    env: Vec::new(),
                    working_directory: None,
                },
            },
        };

        assert!(validate_command(&cmd).is_err());
    }

    #[test]
    fn given_custom_command_when_prepared_then_settings_use_saved_program() {
        let command = QuickLaunch {
            title: String::from("Run htop"),
            spec: CommandSpec::Custom {
                custom: CustomCommand {
                    program: String::from("htop"),
                    args: vec![
                        String::from("--sort-key"),
                        String::from("PERCENT_CPU"),
                    ],
                    env: Vec::new(),
                    working_directory: None,
                },
            },
        };

        let cancel = Arc::new(AtomicBool::new(false));
        let session = command_session(&command, &cancel);
        let SessionKind::Local(options) = session else {
            panic!("expected local session");
        };
        assert_eq!(options.program(), "htop");
        assert_eq!(
            options.args(),
            &[String::from("--sort-key"), String::from("PERCENT_CPU")]
        );
    }

    #[test]
    fn given_empty_title_when_building_command_then_returns_title_required() {
        let editor = WizardEditorState::new(vec![], QuickLaunchType::Custom);
        let result = build_command(&editor);
        assert!(matches!(result, Err(QuickLaunchWizardError::TitleRequired)));
    }

    #[test]
    fn given_custom_editor_when_building_command_then_returns_custom_launch() {
        let mut editor =
            WizardEditorState::new(vec![], QuickLaunchType::Custom);
        editor.set_title(String::from("Build"));
        editor.set_program(String::from("cargo"));
        editor.add_arg();
        editor.update_arg(0, String::from("check"));
        editor.add_env();
        editor.update_env_key(0, String::from("RUST_LOG"));
        editor.update_env_value(0, String::from("debug"));
        editor.set_working_directory(String::from("/tmp/project"));

        let quick_launch =
            build_command(&editor).expect("build should succeed");
        assert_eq!(quick_launch.title(), "Build");
        let CommandSpec::Custom { custom } = &quick_launch.spec else {
            panic!("expected custom command");
        };
        assert_eq!(custom.program(), "cargo");
        assert_eq!(custom.args(), &[String::from("check")]);
        assert_eq!(custom.env().len(), 1);
        assert_eq!(custom.env()[0].key(), "RUST_LOG");
        assert_eq!(custom.env()[0].value(), "debug");
        assert_eq!(custom.working_directory(), Some("/tmp/project"));
    }

    #[test]
    fn given_no_exts_when_program_candidates_then_yields_direct_path_only() {
        let dir = Path::new("/usr/bin");
        let candidates = program_candidates(dir, "bash", &[]);

        assert_eq!(candidates, vec![dir.join("bash")]);
    }

    #[test]
    fn given_exts_and_bare_program_when_program_candidates_then_appends_exts() {
        let dir = Path::new("C:/Windows/System32");
        let exts = vec![String::from(".EXE"), String::from(".BAT")];
        let candidates = program_candidates(dir, "cmd", &exts);

        assert_eq!(
            candidates,
            vec![dir.join("cmd"), dir.join("cmd.EXE"), dir.join("cmd.BAT"),]
        );
    }

    #[test]
    fn given_program_with_known_ext_when_program_candidates_then_no_double_ext()
    {
        let dir = Path::new("C:/tools");
        let exts = vec![String::from(".exe"), String::from(".bat")];
        let candidates = program_candidates(dir, "pwsh.EXE", &exts);

        assert_eq!(candidates, vec![dir.join("pwsh.EXE")]);
    }

    #[test]
    fn given_program_with_unknown_ext_when_program_candidates_then_appends_exts()
     {
        let dir = Path::new("C:/tools");
        let exts = vec![String::from(".COM"), String::from(".EXE")];
        let candidates = program_candidates(dir, "script.sh", &exts);

        assert_eq!(
            candidates,
            vec![
                dir.join("script.sh"),
                dir.join("script.sh.COM"),
                dir.join("script.sh.EXE"),
            ]
        );
    }

    #[test]
    fn given_multiple_default_identities_when_probing_then_openssh_order_wins()
    {
        let root = test_temp_dir("order_wins");
        fs::write(root.join("id_rsa"), "rsa-key")
            .expect("id_rsa should be written");
        fs::write(root.join("id_ed25519"), "ed25519-key")
            .expect("id_ed25519 should be written");

        let probed = default_identity_file_in(&root);

        assert_eq!(probed, Some(root.join("id_rsa")));

        fs::remove_dir_all(&root)
            .expect("temporary directory should be removed");
    }

    #[test]
    fn given_only_ed25519_when_probing_then_it_is_returned() {
        let root = test_temp_dir("only_ed25519");
        fs::write(root.join("id_ed25519"), "ed25519-key")
            .expect("id_ed25519 should be written");

        let probed = default_identity_file_in(&root);

        assert_eq!(probed, Some(root.join("id_ed25519")));

        fs::remove_dir_all(&root)
            .expect("temporary directory should be removed");
    }

    #[test]
    fn given_empty_ssh_dir_when_probing_then_none_returned() {
        let root = test_temp_dir("empty_dir");

        let probed = default_identity_file_in(&root);

        assert_eq!(probed, None);

        fs::remove_dir_all(&root)
            .expect("temporary directory should be removed");
    }

    #[test]
    fn given_nonexistent_ssh_dir_when_probing_then_none_returned() {
        let root = std::env::temp_dir()
            .join(format!("otty-quick-launch-missing-{}", std::process::id()));

        let probed = default_identity_file_in(&root);

        assert_eq!(probed, None);
    }

    #[test]
    fn given_identity_as_directory_when_probing_then_it_is_skipped() {
        let root = test_temp_dir("dir_identity");
        fs::create_dir_all(root.join("id_rsa"))
            .expect("id_rsa directory should be created");
        fs::write(root.join("id_ecdsa"), "ecdsa-key")
            .expect("id_ecdsa should be written");

        let probed = default_identity_file_in(&root);

        assert_eq!(probed, Some(root.join("id_ecdsa")));

        fs::remove_dir_all(&root)
            .expect("temporary directory should be removed");
    }

    fn test_temp_dir(test_name: &str) -> std::path::PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be monotonic")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "otty-quick-launch-{test_name}-{stamp}-{}",
            std::process::id()
        ));

        fs::create_dir_all(&dir)
            .expect("temporary directory should be created");
        dir
    }

    #[test]
    fn given_invalid_ssh_port_when_building_command_then_returns_error() {
        let mut editor =
            WizardEditorState::new(vec![], QuickLaunchType::Custom);
        editor.set_title(String::from("SSH"));
        editor.set_command_type(QuickLaunchType::Ssh);
        editor.set_host(String::from("example.com"));
        editor.set_port(String::from("not-a-port"));

        let result = build_command(&editor);
        assert!(matches!(result, Err(QuickLaunchWizardError::InvalidPort)));
    }
}
