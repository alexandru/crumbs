//! Process-level startup and shutdown checks for `crumbs server start`.

use std::error::Error;
use std::io::{self, BufRead, BufReader};
use std::process::{Child, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use nix::sys::signal::Signal;
use nix::unistd::Pid;

const BIN: &str = env!("CARGO_BIN_EXE_crumbs");
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

#[test]
fn terminates_cleanly_on_hangup() -> Result<(), Box<dyn Error>> {
    assert_clean_shutdown(Signal::SIGHUP)
}

#[test]
fn terminates_cleanly_on_terminate() -> Result<(), Box<dyn Error>> {
    assert_clean_shutdown(Signal::SIGTERM)
}

#[test]
fn terminates_cleanly_on_interrupt() -> Result<(), Box<dyn Error>> {
    assert_clean_shutdown(Signal::SIGINT)
}

fn assert_clean_shutdown(signal: Signal) -> Result<(), Box<dyn Error>> {
    let mut server = ServerProcess::spawn()?;
    server.signal(signal)?;

    let status = server.wait_for_exit(SHUTDOWN_TIMEOUT)?;
    assert!(
        status.success(),
        "server exited with {status} after {signal:?}"
    );
    Ok(())
}

struct ServerProcess {
    child: Child,
    exited: bool,
}

impl ServerProcess {
    fn spawn() -> Result<Self, Box<dyn Error>> {
        let mut child = Command::new(BIN)
            .args(["server", "start"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("server stdout was not piped"))?;

        let mut server = Self {
            child,
            exited: false,
        };
        server.await_greeting(stdout)?;
        Ok(server)
    }

    fn await_greeting(&mut self, stdout: ChildStdout) -> Result<(), Box<dyn Error>> {
        let (sender, receiver) = mpsc::channel();
        let _reader = thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut greeting = String::new();
            let result = reader.read_line(&mut greeting).map(|_| greeting);
            let _ = sender.send(result);
        });

        let greeting = match receiver.recv_timeout(STARTUP_TIMEOUT) {
            Ok(result) => result?,
            Err(_) => {
                return Err(io::Error::other("timed out waiting for server greeting").into());
            }
        };

        assert_eq!(
            greeting, "Hello, world!\n",
            "server greeting was: {greeting:?}"
        );
        Ok(())
    }

    fn signal(&self, signal: Signal) -> Result<(), Box<dyn Error>> {
        let raw_pid = i32::try_from(self.child.id())?;
        nix::sys::signal::kill(Pid::from_raw(raw_pid), signal)?;
        Ok(())
    }

    fn wait_for_exit(&mut self, timeout: Duration) -> Result<ExitStatus, Box<dyn Error>> {
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| io::Error::other("shutdown timeout overflowed"))?;

        loop {
            if let Some(status) = self.child.try_wait()? {
                self.exited = true;
                return Ok(status);
            }

            if Instant::now() >= deadline {
                return Err(io::Error::other("timed out waiting for server shutdown").into());
            }

            thread::sleep(POLL_INTERVAL);
        }
    }
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        if self.exited {
            return;
        }

        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
