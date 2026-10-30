#![forbid(unsafe_code)]
// Release builds run without a console window; failures go to %TEMP% instead.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::fmt::Write as _;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;
use std::{env, io, thread};

use serde_json::json;

use crate::chrome_debugger::{ChromeDebugger, Message};
use crate::error::{Error, LocateError, io_error};

mod chrome_debugger;
mod error;

/// How long Lunar has to publish its debugger URL on stderr.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(15);

fn find_lunar_executable() -> Result<PathBuf, LocateError> {
    let paths = lunar_candidates()?;

    paths
        .iter()
        .find(|path| path.exists())
        .cloned()
        .ok_or_else(|| {
            let locations = paths.iter().fold(String::new(), |mut locations, path| {
                let _ = write!(locations, "\n - {}", path.display());
                locations
            });
            LocateError::NotFound(locations)
        })
}

#[cfg(windows)]
fn lunar_candidates() -> Result<Vec<PathBuf>, LocateError> {
    let local_appdata =
        PathBuf::from(env::var_os("LOCALAPPDATA").ok_or(LocateError::MissingEnv("LOCALAPPDATA"))?);

    Ok(vec![
        local_appdata.join(r"Programs\Lunar Client\Lunar Client.exe"),
    ])
}

#[cfg(target_os = "macos")]
fn lunar_candidates() -> Result<Vec<PathBuf>, LocateError> {
    const BUNDLE: &str = "Lunar Client.app/Contents/MacOS/Lunar Client";

    let mut paths = vec![PathBuf::from("/Applications").join(BUNDLE)];
    if let Some(home) = env::var_os("HOME") {
        paths.push(PathBuf::from(home).join("Applications").join(BUNDLE));
    }

    Ok(paths)
}

#[cfg(target_os = "linux")]
fn lunar_candidates() -> Result<Vec<PathBuf>, LocateError> {
    Ok(vec![PathBuf::from("/usr/bin/lunarclient")])
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
compile_error!("lunar-launcher-inject only supports Windows, macOS and Linux");

fn wait_for_websocket_url(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    for line in reader.lines() {
        if let Some(url) = line?.strip_prefix("Debugger listening on ") {
            return Ok(Some(url.into()));
        }
    }

    Ok(None)
}

fn inject_url(url: &str) -> Result<(), Error> {
    const ENABLE: u32 = 1;
    const RUN: u32 = 2;
    const EVALUATE: u32 = 3;
    const DISABLE: u32 = 4;

    eprintln!("[LLI] connecting to {url}");
    let mut debugger = ChromeDebugger::connect_url(url).map_err(Error::Connect)?;

    let executable =
        env::current_exe().map_err(io_error("failed to locate injector executable"))?;

    // Agents are read from an "agents" directory next to the injector executable.
    let agent_directory = executable
        .parent()
        .ok_or(Error::MissingParent)?
        .join("agents");

    let payload = format!(
        "{}({})",
        include_str!("payload.js"),
        serde_json::to_string(&agent_directory)?
    );

    debugger.send(ENABLE, "Debugger.enable", &json!({}))?;
    let mut evaluating = false;
    loop {
        match debugger.read()? {
            Message::Failure { id, error } => {
                return Err(Error::Cdp { id, source: error });
            }
            Message::Success { id: ENABLE, .. } => {
                debugger.send(RUN, "Runtime.runIfWaitingForDebugger", &json!({}))?;
            }
            Message::Event { method, params } if method == "Debugger.paused" && !evaluating => {
                let frame = params
                    .pointer("/callFrames/0/callFrameId")
                    .and_then(serde_json::Value::as_str)
                    .ok_or(Error::MissingCallFrame)?;
                debugger.send(
                    EVALUATE,
                    "Debugger.evaluateOnCallFrame",
                    &json!({
                        "callFrameId": frame,
                        "expression": payload
                    }),
                )?;
                evaluating = true;
            }
            Message::Success {
                id: EVALUATE,
                result,
            } => {
                if result.get("exceptionDetails").is_some() {
                    return Err(Error::JavaScript);
                }
                // Disabling the debugger also resumes execution.
                debugger.send(DISABLE, "Debugger.disable", &json!({}))?;
            }
            Message::Success { id: DISABLE, .. } => break,
            _ => {}
        }
    }

    Ok(())
}

fn run() -> Result<(), Error> {
    let lunar_exe = match env::args_os().nth(1) {
        Some(arg) => PathBuf::from(arg),
        _ => find_lunar_executable()?,
    };

    let mut cp = scopeguard::guard(
        Command::new(&lunar_exe)
            .arg("--inspect-brk=0")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(io_error("failed to start lunar"))?,
        |mut cp| {
            let _ = cp.kill();
            let _ = cp.wait();
        },
    );

    let mut stderr = BufReader::new(cp.stderr.take().ok_or(Error::MissingStderr)?);
    let (url_tx, url_rx) = mpsc::channel();

    let stderr_thread = thread::Builder::new()
        .name("lunar-stderr".into())
        .spawn(move || {
            if let Some(result) = wait_for_websocket_url(&mut stderr).transpose() {
                let _ = url_tx.send(result);
            }
            drop(url_tx);
            io::copy(&mut stderr, &mut io::sink())
        })
        .map_err(io_error("failed to start stderr reader"))?;

    let url = match url_rx.recv_timeout(STARTUP_TIMEOUT) {
        Ok(Ok(url)) => url,
        Ok(Err(source)) => {
            return Err(Error::Io {
                operation: "failed to read lunar stderr",
                source,
            });
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            return Err(Error::StartupTimeout {
                seconds: STARTUP_TIMEOUT.as_secs(),
            });
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            return Err(Error::StderrDisconnected);
        }
    };

    inject_url(&url)?;

    let mut child = scopeguard::ScopeGuard::into_inner(cp);
    child.wait().map_err(io_error("failed to wait for lunar"))?;
    stderr_thread
        .join()
        .map_err(|_| Error::StderrThreadPanicked)?
        .map_err(io_error("failed to drain lunar stderr"))?;

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("[error] {e}");
        log_error(&e);
        std::process::exit(1);
    }
}

/// Release builds have no console, so a failure is also written next to the
/// other temporary files instead of disappearing.
fn log_error(error: &Error) {
    let path = env::temp_dir().join("lunar-launcher-inject.log");
    let _ = std::fs::write(&path, format!("[error] {error}\n"));
}

#[cfg(test)]
mod tests {
    mod wait_for_websocket_url {
        use std::io::Cursor;

        use super::super::wait_for_websocket_url;

        #[test]
        fn returns_url_when_lunar_reports_the_debugger() {
            let mut stderr = Cursor::new(
                b"noise on the first line\nDebugger listening on ws://127.0.0.1:9229/uuid\n",
            );

            let url = wait_for_websocket_url(&mut stderr);

            assert_eq!(
                url.expect("reading stderr should not fail"),
                Some("ws://127.0.0.1:9229/uuid".into())
            );
        }

        #[test]
        fn returns_none_when_lunar_never_reports_the_debugger() {
            let mut stderr = Cursor::new(b"just noise\n");

            let url = wait_for_websocket_url(&mut stderr);

            assert_eq!(url.expect("reading stderr should not fail"), None);
        }
    }
}
