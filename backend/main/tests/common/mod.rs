//! Shared process and HTTP helpers for integration tests.

#![allow(
    dead_code,
    reason = "each integration test binary compiles this shared module independently"
)]

use std::error::Error;
use std::ffi::OsString;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::process::{Child, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use http::{Response, StatusCode};
use nix::sys::signal::Signal;
use nix::unistd::Pid;
use tempfile::TempDir;

/// Maximum time to wait for a clean shutdown.
pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);

/// A running `crumbs server start` process listening on an ephemeral port.
pub struct ServerProcess {
    child: Child,
    address: SocketAddr,
    exited: bool,
    _directory: TempDir,
}

impl ServerProcess {
    /// Starts the server with command-line settings in a temporary directory.
    pub fn spawn() -> Result<Self, Box<dyn Error>> {
        Self::spawn_with(
            &server_args(Path::new("crumbs.db"), "127.0.0.1", "0", "http://127.0.0.1"),
            &[],
        )
    }

    /// Starts the server with settings supplied only through environment variables.
    pub fn spawn_env_only() -> Result<Self, Box<dyn Error>> {
        Self::spawn_with(
            &[],
            &[
                ("CRUMBS_DATABASE_PATH", "crumbs.db"),
                ("CRUMBS_HTTP_BIND", "127.0.0.1"),
                ("CRUMBS_HTTP_PORT", "0"),
                ("CRUMBS_OAUTH_ISSUER", "http://127.0.0.1:9191"),
            ],
        )
    }

    /// Starts the server with the given arguments and environment in a temporary directory.
    pub fn spawn_with(
        arguments: &[OsString],
        environment: &[(&str, &str)],
    ) -> Result<Self, Box<dyn Error>> {
        let directory = tempfile::tempdir()?;
        let mut child = server_command(directory.path(), arguments, environment).spawn()?;

        let stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                stop(&mut child);
                return Err(io::Error::other("server stdout was not piped").into());
            }
        };
        let address = match await_readiness(stdout) {
            Ok(address) => address,
            Err(error) => {
                stop(&mut child);
                return Err(error);
            }
        };

        Ok(Self {
            child,
            address,
            exited: false,
            _directory: directory,
        })
    }

    /// Returns the address advertised in the readiness message.
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// Returns the working directory used by the server process.
    pub fn directory(&self) -> &Path {
        self._directory.path()
    }

    pub fn signal(&self, signal: Signal) -> Result<(), Box<dyn Error>> {
        let raw_pid = i32::try_from(self.child.id())?;
        nix::sys::signal::kill(Pid::from_raw(raw_pid), signal)?;
        Ok(())
    }

    /// Waits for the server process to exit, up to `timeout`.
    pub fn wait_for_exit(&mut self, timeout: Duration) -> Result<ExitStatus, Box<dyn Error>> {
        let status = wait_for_status(&mut self.child, timeout)?;
        self.exited = true;
        Ok(status)
    }
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        if !self.exited {
            stop(&mut self.child);
        }
    }
}

pub type HttpStatusCode = StatusCode;
pub type HttpResponse = Response<String>;

/// The result of running the server until it exits on its own.
pub struct CommandOutput {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

/// Builds server options for a process invocation.
pub fn server_args(database_path: &Path, bind: &str, port: &str, issuer: &str) -> Vec<OsString> {
    [
        OsString::from("--database-path"),
        database_path.as_os_str().to_owned(),
        OsString::from("--http-bind"),
        OsString::from(bind),
        OsString::from("--http-port"),
        OsString::from(port),
        OsString::from("--oauth-issuer"),
        OsString::from(issuer),
    ]
    .into()
}

/// Runs `crumbs server start` and waits for it to exit.
pub fn run_server_to_exit(
    arguments: &[OsString],
    environment: &[(&str, &str)],
) -> Result<CommandOutput, Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    run_server_to_exit_in(directory.path(), arguments, environment)
}

/// Runs `crumbs server start` from `directory` and waits for it to exit.
pub fn run_server_to_exit_in(
    directory: &Path,
    arguments: &[OsString],
    environment: &[(&str, &str)],
) -> Result<CommandOutput, Box<dyn Error>> {
    let mut child = server_command(directory, arguments, environment)
        .stderr(Stdio::piped())
        .spawn()?;

    let status = match wait_for_status(&mut child, STARTUP_TIMEOUT) {
        Ok(status) => status,
        Err(error) => {
            stop(&mut child);
            return Err(error);
        }
    };

    Ok(CommandOutput {
        status,
        stdout: read_pipe(child.stdout.take())?,
        stderr: read_pipe(child.stderr.take())?,
    })
}

/// Sends `GET <path>` to the server over a raw HTTP/1.1 connection.
pub fn get(server: &ServerProcess, path: &str) -> Result<HttpResponse, Box<dyn Error>> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, server.address().port()));
    let mut stream = TcpStream::connect_timeout(&address, REQUEST_TIMEOUT)?;
    stream.set_read_timeout(Some(REQUEST_TIMEOUT))?;
    stream.set_write_timeout(Some(REQUEST_TIMEOUT))?;

    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )?;

    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    parse_response(response)
}

/// Creates the executable command with inherited application settings removed.
pub fn command() -> Command {
    let mut command = Command::new(BIN);
    for variable in CONFIGURATION_ENVIRONMENT {
        command.env_remove(variable);
    }
    command
}

const BIN: &str = env!("CARGO_BIN_EXE_crumbs");
const READINESS_PREFIX: &str = "Listening on http://";
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const CONFIGURATION_ENVIRONMENT: [&str; 7] = [
    "CRUMBS_DATABASE_PATH",
    "CRUMBS_HTTP_BIND",
    "CRUMBS_HTTP_PORT",
    "CRUMBS_OAUTH_ISSUER",
    "CRUMBS_IMPORT_FILE",
    "CRUMBS_USERNAME",
    "CRUMBS_VERBOSE",
];

fn server_command(
    directory: &Path,
    arguments: &[OsString],
    environment: &[(&str, &str)],
) -> Command {
    let mut command = command();
    command
        .args(["server", "start"])
        .args(arguments)
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped());
    command.envs(environment.iter().copied());
    command
}

fn await_readiness(stdout: ChildStdout) -> Result<SocketAddr, Box<dyn Error>> {
    let (sender, receiver) = mpsc::channel();
    let _reader = thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let result = reader.read_line(&mut line).map(|_| line);
        let _ = sender.send(result);
    });

    let line = match receiver.recv_timeout(STARTUP_TIMEOUT) {
        Ok(result) => result?,
        Err(_) => {
            return Err(io::Error::other("timed out waiting for server readiness").into());
        }
    };
    let address = line
        .strip_prefix(READINESS_PREFIX)
        .ok_or_else(|| io::Error::other(format!("unexpected readiness line: {line:?}")))?
        .trim()
        .parse::<SocketAddr>()?;
    Ok(address)
}

fn parse_response(response: String) -> Result<HttpResponse, Box<dyn Error>> {
    let (head, body) = response
        .split_once("\r\n\r\n")
        .ok_or_else(|| io::Error::other("HTTP response is missing a header terminator"))?;
    let mut lines = head.lines();
    let status_line = lines
        .next()
        .ok_or_else(|| io::Error::other("HTTP response is missing a status line"))?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| io::Error::other("HTTP status line is missing a status code"))?
        .parse::<HttpStatusCode>()?;
    let mut builder = Response::builder().status(status);
    for line in lines {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| io::Error::other("HTTP header line is missing a colon"))?;
        builder = builder.header(name.trim(), value.trim());
    }

    Ok(builder.body(body.to_owned())?)
}

fn wait_for_status(child: &mut Child, timeout: Duration) -> Result<ExitStatus, Box<dyn Error>> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| io::Error::other("timeout overflowed"))?;

    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }

        if Instant::now() >= deadline {
            return Err(io::Error::other("timed out waiting for the server to exit").into());
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn read_pipe(pipe: Option<impl Read>) -> Result<String, Box<dyn Error>> {
    let mut pipe = pipe.ok_or_else(|| io::Error::other("process pipe was not captured"))?;
    let mut output = String::new();
    pipe.read_to_string(&mut output)?;
    Ok(output)
}

fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}
