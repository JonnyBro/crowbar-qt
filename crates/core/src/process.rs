//! Running a tool line by line and stopping it with its children.

use std::future::Future;
use std::io::{BufRead, BufReader};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use rustix::process::{Pid, Signal};
use tokio::sync::mpsc;

use crate::launcher::{self, Command};

/// Set in every tool's environment; see [`kill_marked`].
const MARKER_VAR: &str = "PRYBAR_RUN";
/// How long a cancelled tool gets to quit before it is killed.
const TERM_GRACE: Duration = Duration::from_secs(2);
/// Children holding the pipe open must not hold the run up.
const DRAIN_TIMEOUT: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exit {
	pub code: Option<i32>,
	pub success: bool,
	pub cancelled: bool,
}

/// Lines lose terminal codes, Proton noise and Z:\ paths; cancel sends SIGTERM, then SIGKILL.
pub async fn run(
	command: &Command,
	mut on_line: impl FnMut(String),
	cancel: impl Future<Output = ()>,
) -> std::io::Result<Exit> {
	let marker_value = unique_id();
	let marker = format!("{MARKER_VAR}={marker_value}");
	let (reader, writer) = std::io::pipe()?;

	let mut child = {
		let mut cmd = tokio::process::Command::new(&command.program);
		cmd.args(&command.args)
			.env_clear()
			.envs(&command.env)
			.env(MARKER_VAR, &marker_value)
			.stdin(Stdio::null())
			.stdout(writer.try_clone()?)
			.stderr(writer)
			// Own process group so cancelling reaches Proton and every child at once.
			.process_group(0);
		if let Some(dir) = &command.working_dir {
			cmd.current_dir(dir);
		}
		cmd.spawn()?
		// Drop our write ends of the pipe so the reader sees EOF.
	};
	let pid = child
		.id()
		.and_then(|id| i32::try_from(id).ok())
		.and_then(Pid::from_raw);

	let (tx, mut rx) = mpsc::unbounded_channel();
	tokio::task::spawn_blocking(move || read_lines(reader, &tx));

	tokio::pin!(cancel);
	let mut cancelled = false;
	let kill_deadline = tokio::time::sleep(Duration::MAX);
	tokio::pin!(kill_deadline);
	let status = loop {
		tokio::select! {
			status = child.wait() => break status?,
			Some(line) = rx.recv() => emit(&line, &mut on_line),
			() = &mut cancel, if !cancelled => {
				cancelled = true;
				signal_all(pid, &marker, Signal::TERM);
				kill_deadline.as_mut().reset(tokio::time::Instant::now() + TERM_GRACE);
			}
			() = &mut kill_deadline, if cancelled => {
				signal_all(pid, &marker, Signal::KILL);
				kill_deadline.as_mut().reset(tokio::time::Instant::now() + Duration::from_secs(3600));
			}
		}
	};

	while let Ok(Some(line)) = tokio::time::timeout(DRAIN_TIMEOUT, rx.recv()).await {
		emit(&line, &mut on_line);
	}
	Ok(Exit {
		code: status.code(),
		success: status.success() && !cancelled,
		cancelled,
	})
}

fn emit(raw: &str, on_line: &mut impl FnMut(String)) {
	let line = launcher::strip_terminal_codes(raw);
	let line = line.strip_suffix('\r').unwrap_or(&line);
	if !launcher::is_proton_noise(line) {
		on_line(launcher::to_host_paths(line));
	}
}

fn read_lines(reader: std::io::PipeReader, tx: &mpsc::UnboundedSender<String>) {
	let mut reader = BufReader::new(reader);
	let mut buffer = Vec::new();
	loop {
		buffer.clear();
		match reader.read_until(b'\n', &mut buffer) {
			Ok(0) | Err(_) => return,
			Ok(_) => {
				if buffer.last() == Some(&b'\n') {
					buffer.pop();
				}
				if tx
					.send(String::from_utf8_lossy(&buffer).into_owned())
					.is_err()
				{
					return;
				}
			}
		}
	}
}

fn unique_id() -> String {
	static COUNTER: AtomicU64 = AtomicU64::new(0);
	let nanos = std::time::SystemTime::now()
		.duration_since(std::time::UNIX_EPOCH)
		.map_or(0, |d| d.as_nanos());
	format!(
		"{}-{}-{nanos}",
		std::process::id(),
		COUNTER.fetch_add(1, Ordering::Relaxed)
	)
}

fn signal_all(group: Option<Pid>, marker: &str, signal: Signal) {
	if let Some(group) = group {
		let _ = rustix::process::kill_process_group(group, signal);
	}
	kill_marked(marker, signal);
}

/// Wine runs programs in their own sessions; the marker in their environment finds them.
fn kill_marked(marker: &str, signal: Signal) {
	let Ok(proc) = std::fs::read_dir("/proc") else {
		return;
	};
	for entry in proc.flatten() {
		let Some(pid) = entry
			.file_name()
			.to_str()
			.and_then(|name| name.parse::<i32>().ok())
			.and_then(Pid::from_raw)
		else {
			continue;
		};
		let Ok(environ) = std::fs::read(entry.path().join("environ")) else {
			continue;
		};
		if environ
			.split(|&b| b == 0)
			.any(|var| var == marker.as_bytes())
		{
			let _ = rustix::process::kill_process(pid, signal);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn shell(script: &str) -> Command {
		Command::new("/bin/sh", vec!["-c".into(), script.into()])
	}

	#[tokio::test]
	async fn output_and_exit_code() {
		// printf, not echo: dash's echo turns \t and \a into control characters.
		let command = shell(
			"printf '%s\\n' 'ntsync: up and running.' 'writing Z:\\tmp\\a.mdl'; printf 'err\\n' >&2; printf 'tail'; exit 3",
		);
		let mut lines = Vec::new();
		let exit = run(&command, |line| lines.push(line), std::future::pending())
			.await
			.unwrap();
		assert_eq!(lines, ["writing /tmp/a.mdl", "err", "tail"]);
		assert_eq!(
			exit,
			Exit {
				code: Some(3),
				success: false,
				cancelled: false
			}
		);
	}

	#[tokio::test]
	async fn background_children_do_not_hold_the_run() {
		let command = shell("sleep 3 & printf 'done\\n'");
		let started = std::time::Instant::now();
		let mut lines = Vec::new();
		let exit = run(&command, |line| lines.push(line), std::future::pending())
			.await
			.unwrap();
		assert!(exit.success);
		assert_eq!(lines, ["done"]);
		assert!(started.elapsed() < Duration::from_secs(2));
	}

	#[tokio::test]
	async fn cancel_kills_children() {
		let command = shell("setsid sleep 31.7 & sleep 30 & sleep 30");
		let cancel = tokio::time::sleep(Duration::from_millis(300));
		let exit = run(&command, |_| {}, cancel).await.unwrap();
		assert!(exit.cancelled);
		assert!(!exit.success);

		// The setsid child left our process group, like Windows programs under Wine do.
		let mut gone = false;
		for _ in 0..50 {
			let status = std::process::Command::new("pgrep")
				.args(["-f", "^sleep 31\\.7$"])
				.status()
				.unwrap();
			if status.code() == Some(1) {
				gone = true;
				break;
			}
			tokio::time::sleep(Duration::from_millis(100)).await;
		}
		assert!(gone);
	}

	#[tokio::test]
	async fn failed_to_start() {
		let command = Command::new("/nonexistent/program", vec![]);
		assert!(run(&command, |_| {}, std::future::pending()).await.is_err());
	}
}
