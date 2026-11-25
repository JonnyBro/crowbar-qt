//! Publishing to the Garry's Mod workshop through the game's gmpublish.

use std::path::{Path, PathBuf};

use image::{DynamicImage, ImageFormat, ImageReader, Rgb, RgbImage};
use rust_i18n::t;

use crate::launcher::{self, Command, Mode};
use crate::steam::App;

const ICON_SIZE: u32 = 512;

#[derive(Debug, thiserror::Error)]
pub enum Error {
	#[error(transparent)]
	Launch(#[from] launcher::Error),
	#[error("{}", t!("pub.needs_gma_and_icon"))]
	NeedsGmaAndIcon,
	#[error("{}", t!("pub.needs_something"))]
	NeedsSomething,
	#[error("{}", t!("pub.needs_icon"))]
	NeedsIcon,
	#[error("{}", t!("pub.icon_read", path = .0.display(), error = .1))]
	IconRead(PathBuf, String),
	#[error("{}", t!("pub.icon_write", path = .0.display(), error = .1))]
	IconWrite(PathBuf, String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkshopItem {
	pub id: u64,
	/// As gmpublish prints it, e.g. "145.0 KB".
	pub size: String,
	pub title: String,
}

impl WorkshopItem {
	/// The size in bytes; `None` when gmpublish printed nothing sensible, as
	/// it does for some items ("17179869184.0 GB").
	pub fn size_bytes(&self) -> Option<u64> {
		parse_size(&self.size)
	}
}

/// gmpublish sizes use 1024-based units with SI names.
fn parse_size(text: &str) -> Option<u64> {
	const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
	/// Bigger than any addon the workshop accepts.
	const LIMIT: f64 = 1024. * 1024. * 1024. * 1024.;
	let (number, unit) = text.trim().split_once(' ')?;
	let number: f64 = number.parse().ok()?;
	let power = UNITS
		.iter()
		.position(|u| u.eq_ignore_ascii_case(unit.trim()))?;
	let bytes = number * 1024f64.powi(i32::try_from(power).ok()?);
	(bytes.is_finite() && (0.0..LIMIT).contains(&bytes)).then_some(bytes.round() as u64)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Request {
	/// `None` creates a new item.
	pub id: Option<u64>,
	pub gma: Option<PathBuf>,
	pub icon: Option<PathBuf>,
	/// Change notes; only used when updating.
	pub changes: String,
}

pub fn list_command(app: &App, user_env: &str) -> Result<Command> {
	let executable = launcher::require_tool(app, "gmpublish")?;
	let mut command = launcher::build(
		app,
		&executable,
		vec!["list".into()],
		Mode::Console,
		user_env,
	)?;
	command.working_dir = executable.parent().map(Path::to_path_buf);
	Ok(command)
}

pub fn publish_command(app: &App, request: &Request, user_env: &str) -> Result<Command> {
	let executable = launcher::require_tool(app, "gmpublish")?;
	let create = request.id.is_none();
	if request.gma.is_none() && (create || request.icon.is_none()) {
		return Err(if create {
			Error::NeedsGmaAndIcon
		} else {
			Error::NeedsSomething
		});
	}
	if create && request.icon.is_none() {
		return Err(Error::NeedsIcon);
	}
	for file in [&request.gma, &request.icon].into_iter().flatten() {
		if !file.is_file() {
			return Err(launcher::Error::Missing(file.clone()).into());
		}
	}

	let windows = launcher::is_windows_executable(&executable);
	let path = |host: &Path| {
		let absolute = std::path::absolute(host).unwrap_or_else(|_| host.to_path_buf());
		launcher::tool_path(windows, &absolute)
	};

	let mut args = match request.id {
		None => vec!["create".to_owned()],
		Some(id) => vec!["update".to_owned(), "-id".into(), id.to_string()],
	};
	if let Some(gma) = &request.gma {
		args.extend(["-addon".to_owned(), path(gma)]);
	}
	if let Some(icon) = &request.icon {
		args.extend(["-icon".to_owned(), path(icon)]);
	}
	let changes = request.changes.trim();
	if !create && !changes.is_empty() {
		args.extend(["-changes".to_owned(), changes.to_owned()]);
	}

	let mut command = launcher::build(app, &executable, args, Mode::Console, user_env)?;
	command.working_dir = executable.parent().map(Path::to_path_buf);
	Ok(command)
}

/// Collects items from `gmpublish list` output. Titles can contain line breaks,
/// so lines are fed one by one and an item may span several of them.
#[derive(Debug, Default)]
pub struct ListParser {
	items: Vec<WorkshopItem>,
	in_title: bool,
}

impl ListParser {
	pub fn add_line(&mut self, line: &str) {
		if self.in_title {
			let text = match line.strip_suffix('"') {
				Some(text) => {
					self.in_title = false;
					text
				}
				None => line,
			};
			if let Some(item) = self.items.last_mut()
				&& !text.trim().is_empty()
			{
				item.title.push(' ');
				item.title.push_str(text.trim());
			}
			return;
		}

		// "\t<id>\t<size>\t\"<title>"
		let Some(rest) = line.strip_prefix('\t') else {
			return;
		};
		let mut fields = rest.splitn(3, '\t');
		let (Some(id), Some(size), Some(title)) = (fields.next(), fields.next(), fields.next())
		else {
			return;
		};
		if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
			return;
		}
		let Ok(id) = id.parse() else {
			return;
		};
		let Some(title) = title.strip_prefix('"') else {
			return;
		};
		let title = match title.strip_suffix('"') {
			Some(title) => title,
			None => {
				self.in_title = true;
				title
			}
		};
		self.items.push(WorkshopItem {
			id,
			size: size.trim().to_owned(),
			title: title.trim().to_owned(),
		});
	}

	pub fn items(&self) -> &[WorkshopItem] {
		&self.items
	}
}

/// The ID printed by `gmpublish create`.
pub fn parse_created_id(line: &str) -> Option<u64> {
	const MARKER: &str = "Your new item ID is ";
	let rest = &line[line.find(MARKER)? + MARKER.len()..];
	let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
	digits.parse().ok()
}

/// gmpublish only takes a baseline 512x512 JPEG. Anything else is cropped to a
/// square, scaled and written to `converted`. Returns the file to pass to gmpublish.
pub fn prepare_icon(icon: &Path, converted: &Path) -> Result<PathBuf> {
	let read_error = |e: &dyn std::fmt::Display| Error::IconRead(icon.to_path_buf(), e.to_string());
	let reader = ImageReader::open(icon)
		.and_then(ImageReader::with_guessed_format)
		.map_err(|e| read_error(&e))?;
	if reader.format() == Some(ImageFormat::Jpeg)
		&& image::image_dimensions(icon).ok() == Some((ICON_SIZE, ICON_SIZE))
		&& is_baseline_jpeg(icon)
	{
		return Ok(icon.to_path_buf());
	}
	let image = reader.decode().map_err(|e| read_error(&e))?;

	let side = image.width().min(image.height());
	let square = image.crop_imm(
		(image.width() - side) / 2,
		(image.height() - side) / 2,
		side,
		side,
	);
	let scaled = square.resize_exact(ICON_SIZE, ICON_SIZE, image::imageops::FilterType::Lanczos3);
	let flat = flatten_on_white(&scaled);

	let write_error =
		|e: &dyn std::fmt::Display| Error::IconWrite(converted.to_path_buf(), e.to_string());
	let file = std::fs::File::create(converted).map_err(|e| write_error(&e))?;
	let mut encoder =
		image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(file), 92);
	encoder.encode_image(&flat).map_err(|e| write_error(&e))?;
	Ok(converted.to_path_buf())
}

/// JPEG has no alpha; transparent parts become white rather than black.
fn flatten_on_white(image: &DynamicImage) -> RgbImage {
	let rgba = image.to_rgba8();
	RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
		let [r, g, b, a] = rgba.get_pixel(x, y).0;
		let blend = |c: u8| {
			let c = u32::from(c) * u32::from(a) + 255 * (255 - u32::from(a));
			u8::try_from(c / 255).unwrap_or(u8::MAX)
		};
		Rgb([blend(r), blend(g), blend(b)])
	})
}

/// Progressive JPEGs carry an SOF2 marker; gmpublish rejects them.
fn is_baseline_jpeg(file: &Path) -> bool {
	let Ok(data) = std::fs::read(file) else {
		return false;
	};
	if !data.starts_with(&[0xff, 0xd8]) {
		return false;
	}
	let mut i = 2;
	while i + 3 < data.len() {
		if data[i] != 0xff {
			return false;
		}
		match data[i + 1] {
			0xc0 | 0xc1 => return true,
			0xc2 => return false,
			_ => {}
		}
		let length = usize::from(data[i + 2]) << 8 | usize::from(data[i + 3]);
		i += 2 + length;
	}
	false
}

#[cfg(test)]
mod tests {
	use super::*;
	use launcher::to_windows_path;

	fn write(path: &Path, content: &[u8]) {
		std::fs::create_dir_all(path.parent().unwrap()).unwrap();
		std::fs::write(path, content).unwrap();
	}

	#[test]
	fn parse_list() {
		let mut parser = ListParser::default();
		for line in [
			"Garry's Mod Workshop Publisher 1.2",
			"Getting published files..",
			"\t3835381011\t145.0 KB\t\"Beatrun Weapon HUD\"",
			"\t3030826644\t12.8 MB\t\"MW 2019 SMG | Compressed 512x512",
			"\"",
			"\t3030826521\t49.7 MB\t\"Two",
			"lines\"",
			"\t42\t1 B\t\"\"",
		] {
			parser.add_line(line);
		}
		let items = parser.items();
		assert_eq!(items.len(), 4);
		assert_eq!(items[0].id, 3_835_381_011);
		assert_eq!(items[0].size, "145.0 KB");
		assert_eq!(items[0].title, "Beatrun Weapon HUD");
		assert_eq!(items[1].title, "MW 2019 SMG | Compressed 512x512");
		assert_eq!(items[2].title, "Two lines");
		assert_eq!(items[3].title, "");
	}

	#[test]
	fn sizes() {
		let item = |size: &str| WorkshopItem {
			size: size.into(),
			..WorkshopItem::default()
		};
		assert_eq!(item("1 B").size_bytes(), Some(1));
		assert_eq!(item("145.0 KB").size_bytes(), Some(148_480));
		assert_eq!(item("12.8 MB").size_bytes(), Some(13_421_773));
		assert_eq!(item("17179869184.0 GB").size_bytes(), None);
		assert_eq!(item("").size_bytes(), None);
		assert_eq!(item("big").size_bytes(), None);
	}

	#[test]
	fn created_id() {
		assert_eq!(
			parse_created_id("Success! Your new item ID is 3835381011!"),
			Some(3_835_381_011)
		);
		assert_eq!(parse_created_id("Success!"), None);
	}

	#[test]
	fn commands() {
		let temp = tempfile::tempdir().unwrap();
		let dir = temp.path();
		write(&dir.join("bin/gmpublish.exe"), b"MZ");
		write(&dir.join("a.gma"), b"GMAD");
		write(&dir.join("icon.jpg"), b"x");
		let app = App {
			id: 4000,
			tools_path: Some(dir.join("bin")),
			proton_path: Some(dir.join("proton")),
			compat_data_path: Some(dir.join("compatdata")),
			..App::default()
		};

		let list = list_command(&app, "").unwrap();
		assert_eq!(
			list.args,
			[
				"runinprefix".to_owned(),
				dir.join("bin/gmpublish.exe").display().to_string(),
				"list".into()
			]
		);

		let mut request = Request {
			gma: Some(dir.join("a.gma")),
			..Request::default()
		};
		assert!(matches!(
			publish_command(&app, &request, ""),
			Err(Error::NeedsIcon)
		));
		request.icon = Some(dir.join("icon.jpg"));
		request.changes = "ignored for create".into();
		let command = publish_command(&app, &request, "").unwrap();
		assert_eq!(
			command.args[2..],
			[
				"create".to_owned(),
				"-addon".into(),
				to_windows_path(&dir.join("a.gma")),
				"-icon".into(),
				to_windows_path(&dir.join("icon.jpg"))
			]
		);

		request.id = Some(1337);
		request.icon = None;
		request.changes = " fixed something ".into();
		let command = publish_command(&app, &request, "").unwrap();
		assert_eq!(
			command.args[2..],
			[
				"update".to_owned(),
				"-id".into(),
				"1337".into(),
				"-addon".into(),
				to_windows_path(&dir.join("a.gma")),
				"-changes".into(),
				"fixed something".into()
			]
		);

		request.gma = None;
		assert!(matches!(
			publish_command(&app, &request, ""),
			Err(Error::NeedsSomething)
		));
		request.gma = Some(dir.join("missing.gma"));
		assert!(matches!(
			publish_command(&app, &request, ""),
			Err(Error::Launch(launcher::Error::Missing(_)))
		));
		request.id = None;
		request.gma = None;
		assert!(matches!(
			publish_command(&app, &request, ""),
			Err(Error::NeedsGmaAndIcon)
		));
	}

	#[test]
	fn icon() {
		let temp = tempfile::tempdir().unwrap();
		let wide = temp.path().join("wide.png");
		image::RgbaImage::new(800, 400).save(&wide).unwrap();

		let out = temp.path().join("out.jpg");
		assert_eq!(prepare_icon(&wide, &out).unwrap(), out);
		let reader = ImageReader::open(&out)
			.unwrap()
			.with_guessed_format()
			.unwrap();
		assert_eq!(reader.format(), Some(ImageFormat::Jpeg));
		let result = reader.decode().unwrap().to_rgb8();
		assert_eq!(result.dimensions(), (512, 512));
		// Transparency became white.
		assert!(result.get_pixel(256, 256).0.iter().all(|&c| c > 240));
		assert!(is_baseline_jpeg(&out));

		// An already valid icon is used as is.
		let again = temp.path().join("again.jpg");
		assert_eq!(prepare_icon(&out, &again).unwrap(), out);
		assert!(!again.exists());

		assert!(matches!(
			prepare_icon(&temp.path().join("missing.png"), &temp.path().join("x.jpg")),
			Err(Error::IconRead(..))
		));
	}
}
