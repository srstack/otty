//! Windows placeholder backend: local PTY sessions are not supported yet.
//!
//! The types mirror the unix backend so higher layers compile unchanged;
//! spawning always fails with a guidance error until a ConPTY backend lands.

use std::path::{Path, PathBuf};
use std::process::ExitStatus;

use mio::Token;

use crate::{Pollable, PtySize, Session, SessionError};

const UNSUPPORTED: &str = "local terminal sessions are not supported on \
Windows yet; use a Quick Launch SSH target to connect to a remote host";

/// Placeholder local session that can never be constructed on Windows.
pub struct LocalSession {
    _private: (),
}

/// Builder mirroring the unix `LocalSessionBuilder` API surface.
pub struct LocalSessionBuilder {
    program: String,
    args: Vec<String>,
    envs: Vec<(String, Option<String>)>,
    size: PtySize,
    work_dir: Option<PathBuf>,
    controlling_tty: bool,
}

/// Start building a local session for the provided executable.
pub fn local(program: &str) -> LocalSessionBuilder {
    LocalSessionBuilder {
        program: program.to_string(),
        args: Vec::new(),
        envs: Vec::new(),
        size: PtySize::default(),
        work_dir: None,
        controlling_tty: false,
    }
}

impl Default for LocalSessionBuilder {
    fn default() -> Self {
        local("cmd.exe")
    }
}

impl LocalSessionBuilder {
    /// Append a single argument to the command line.
    pub fn with_arg(mut self, arg: &str) -> Self {
        self.args.push(arg.to_string());
        self
    }

    /// Append a list of arguments to the command line.
    pub fn with_args(mut self, args: &[String]) -> Self {
        self.args.extend(args.iter().cloned());
        self
    }

    /// Set an environment variable for the spawned child process.
    pub fn with_env(mut self, key: &str, value: &str) -> Self {
        self.envs.push((key.to_string(), Some(value.to_string())));
        self
    }

    /// Remove an environment variable from the child process environment.
    pub fn with_env_remove(mut self, key: &str) -> Self {
        self.envs.push((key.to_string(), None));
        self
    }

    /// Advertise the initial PTY size for the child process.
    pub fn with_size(mut self, size: PtySize) -> Self {
        self.size = size;
        self
    }

    /// Change the working directory of the spawned child process.
    pub fn with_cwd(mut self, path: &Path) -> Self {
        self.work_dir = Some(path.to_path_buf());
        self
    }

    /// Kept for API parity with the unix backend; no-op on Windows.
    pub fn set_controling_tty_enable(mut self) -> Self {
        self.controlling_tty = true;
        self
    }

    /// Always fails: there is no local PTY backend on Windows yet.
    pub fn spawn(self) -> Result<LocalSession, SessionError> {
        let _ = (
            self.program,
            self.args,
            self.envs,
            self.size,
            self.work_dir,
            self.controlling_tty,
        );
        Err(SessionError::Internal(UNSUPPORTED.to_string()))
    }
}

impl Session for LocalSession {
    fn read(&mut self, _buf: &mut [u8]) -> Result<usize, SessionError> {
        Err(SessionError::Internal(UNSUPPORTED.to_string()))
    }

    fn write(&mut self, _input: &[u8]) -> Result<usize, SessionError> {
        Err(SessionError::Internal(UNSUPPORTED.to_string()))
    }

    fn resize(&mut self, _size: PtySize) -> Result<(), SessionError> {
        Err(SessionError::Internal(UNSUPPORTED.to_string()))
    }

    fn close(&mut self) -> Result<i32, SessionError> {
        Err(SessionError::Internal(UNSUPPORTED.to_string()))
    }

    fn try_get_child_exit_status(
        &mut self,
    ) -> Result<Option<ExitStatus>, SessionError> {
        Err(SessionError::Internal(UNSUPPORTED.to_string()))
    }
}

impl Pollable for LocalSession {
    fn register(
        &mut self,
        _registry: &mio::Registry,
        _interest: mio::Interest,
        _io_token: Token,
        _child_token: Token,
    ) -> Result<(), SessionError> {
        Err(SessionError::Internal(UNSUPPORTED.to_string()))
    }

    fn reregister(
        &mut self,
        _registry: &mio::Registry,
        _interest: mio::Interest,
        _io_token: Token,
        _child_token: Token,
    ) -> Result<(), SessionError> {
        Err(SessionError::Internal(UNSUPPORTED.to_string()))
    }

    fn deregister(
        &mut self,
        _registry: &mio::Registry,
    ) -> Result<(), SessionError> {
        Err(SessionError::Internal(UNSUPPORTED.to_string()))
    }
}

#[cfg(test)]
mod test {
    use crate::{SessionError, local};

    #[test]
    fn windows_local_session_reports_unsupported() {
        let err = match local("cmd.exe").spawn() {
            Ok(_) => panic!("local sessions must not spawn on Windows yet"),
            Err(err) => err,
        };

        match err {
            SessionError::Internal(message) => {
                assert!(
                    message.contains("Windows"),
                    "error should name the platform: {message}"
                );
                assert!(
                    message.contains("SSH"),
                    "error should point to the SSH alternative: {message}"
                );
            },
            other => panic!("expected SessionError::Internal, got {other:?}"),
        }
    }

    #[test]
    fn windows_builder_accepts_full_option_chain() {
        // The engine wires every builder method; the chain must compile and
        // still funnel into the unsupported-spawn error.
        let result = local("cmd.exe")
            .with_arg("/c")
            .with_args(&[String::from("echo"), String::from("hi")])
            .with_env("OTTY_TEST", "1")
            .with_env_remove("OTTY_TEST")
            .with_size(crate::PtySize::default())
            .with_cwd(std::path::Path::new("C:\\"))
            .set_controling_tty_enable()
            .spawn();

        assert!(matches!(result, Err(SessionError::Internal(_))));
    }
}
