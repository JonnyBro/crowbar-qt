//! One window per user session. A second start hands its files to the window
//! that is already open over a Unix socket and exits.

use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

use prybar_core::AppPaths;

pub enum Start {
	/// Another window took the files.
	Forwarded,
	/// This is the only window; requests from later starts arrive here.
	First(Option<UnixListener>),
}

fn socket_path(paths: &AppPaths) -> PathBuf {
	std::env::var_os("XDG_RUNTIME_DIR")
		.map(PathBuf::from)
		.filter(|dir| dir.is_dir())
		.unwrap_or_else(|| paths.cache_dir().to_path_buf())
		.join("prybar.sock")
}

pub fn start(paths: &AppPaths, files: &[PathBuf]) -> Start {
	let socket = socket_path(paths);
	if let Ok(stream) = UnixStream::connect(&socket) {
		match send(stream, files) {
			Ok(()) => return Start::Forwarded,
			Err(e) => tracing::warn!("the open window did not take the files: {e}"),
		}
	}
	// Nothing answers, so the socket is left over from a crash.
	let _ = std::fs::remove_file(&socket);
	match UnixListener::bind(&socket) {
		Ok(listener) => Start::First(Some(listener)),
		Err(e) => {
			tracing::warn!("single instance disabled: {}: {e}", socket.display());
			Start::First(None)
		}
	}
}

/// Paths are absolute and end with a NUL each: a path may hold a newline.
fn send(mut stream: UnixStream, files: &[PathBuf]) -> std::io::Result<()> {
	stream.set_write_timeout(Some(Duration::from_secs(2)))?;
	let mut message = Vec::new();
	for file in files {
		let file = std::path::absolute(file).unwrap_or_else(|_| file.clone());
		message.extend_from_slice(file.as_os_str().as_bytes());
		message.push(0);
	}
	stream.write_all(&message)?;
	stream.shutdown(std::net::Shutdown::Write)
}

fn receive(mut stream: UnixStream) -> std::io::Result<Vec<PathBuf>> {
	stream.set_read_timeout(Some(Duration::from_secs(2)))?;
	let mut message = Vec::new();
	stream.read_to_end(&mut message)?;
	Ok(message
		.split(|&b| b == 0)
		.filter(|part| !part.is_empty())
		.map(|part| PathBuf::from(std::ffi::OsStr::from_bytes(part)))
		.collect())
}

/// Calls `on_request` from its own thread with the files of every later start;
/// an empty list just asks to bring the window forward.
pub fn listen(listener: UnixListener, on_request: impl Fn(Vec<PathBuf>) + Send + 'static) {
	let spawned = std::thread::Builder::new()
		.name("single-instance".into())
		.spawn(move || {
			for stream in listener.incoming() {
				match stream.and_then(receive) {
					Ok(files) => on_request(files),
					Err(e) => tracing::warn!("bad request from another start: {e}"),
				}
			}
		});
	if let Err(e) = spawned {
		tracing::warn!("single instance disabled: {e}");
	}
}

/// Removes the socket when the window closes, so the next start does not try it.
pub struct Cleanup(PathBuf);

impl Cleanup {
	pub fn new(paths: &AppPaths) -> Self {
		Self(socket_path(paths))
	}
}

impl Drop for Cleanup {
	fn drop(&mut self) {
		let _ = std::fs::remove_file(&self.0);
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn paths_round_trip() {
		let (a, b) = UnixStream::pair().unwrap();
		let files = vec![
			PathBuf::from("/x/a b.qc"),
			PathBuf::from("/y/line\nbreak.vpk"),
		];
		send(a, &files).unwrap();
		assert_eq!(receive(b).unwrap(), files);
	}

	#[test]
	fn second_start_forwards() {
		let temp = tempfile::tempdir().unwrap();
		let socket = temp.path().join("s.sock");
		let listener = UnixListener::bind(&socket).unwrap();
		send(
			UnixStream::connect(&socket).unwrap(),
			&[PathBuf::from("/m.mdl")],
		)
		.unwrap();
		let (stream, _) = listener.accept().unwrap();
		assert_eq!(receive(stream).unwrap(), [PathBuf::from("/m.mdl")]);
	}
}
