//! Pieces every page builds with: labelled rows, choices, path fields, opening
//! files and folders.

use std::path::{Path, PathBuf};

use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{ActiveTheme, Selectable, Sizable, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;
use rust_i18n::t;

use crate::model::Link;

/// A page's heading and the line under it.
pub fn page_header(title: String, subtitle: Option<String>, cx: &App) -> Div {
	v_flex()
		.gap_1()
		.child(
			div()
				.text_xl()
				.font_weight(FontWeight::SEMIBOLD)
				.child(title),
		)
		.when_some(subtitle, |this, subtitle| {
			this.child(
				div()
					.text_sm()
					.text_color(cx.theme().muted_foreground)
					.child(subtitle),
			)
		})
}

/// A label on the left, its control on the right.
pub fn field(label: String, control: impl IntoElement, cx: &App) -> Div {
	h_flex()
		.gap_4()
		.items_center()
		.child(
			div()
				.w(px(180.))
				.flex_shrink_0()
				.text_color(cx.theme().muted_foreground)
				.child(label),
		)
		.child(div().flex_1().min_w_0().child(control))
}

pub fn hint(text: impl Into<SharedString>, cx: &App) -> Div {
	div()
		.text_sm()
		.text_color(cx.theme().muted_foreground)
		.child(text.into())
}

pub fn empty_state(text: impl Into<SharedString>, cx: &App) -> AnyElement {
	// gpui wraps text only at a known width; without w_full it spills over its neighbours.
	div()
		.size_full()
		.flex()
		.items_center()
		.justify_center()
		.p_4()
		.child(
			div()
				.w_full()
				.text_center()
				.text_sm()
				.text_color(cx.theme().muted_foreground)
				.child(text.into()),
		)
		.into_any_element()
}

/// One button of a group where one is selected.
pub fn choice(id: impl Into<ElementId>, label: impl Into<SharedString>, selected: bool) -> Button {
	Button::new(id)
		.label(label)
		.small()
		.outline()
		.selected(selected)
}

/// An input with a Browse button.
pub fn path_row(input: &Entity<InputState>, browse: Button) -> Div {
	h_flex()
		.gap_2()
		.child(div().flex_1().min_w_0().child(Input::new(input).small()))
		.child(browse)
}

pub fn browse_button(id: impl Into<ElementId>) -> Button {
	Button::new(id).label(t!("common.browse")).small().outline()
}

/// The trimmed text of an input.
pub fn text(input: &Entity<InputState>, cx: &App) -> String {
	input.read(cx).value().trim().to_owned()
}

/// The input's text as a path; `None` when it is empty.
pub fn path(input: &Entity<InputState>, cx: &App) -> Option<PathBuf> {
	let text = text(input, cx);
	(!text.is_empty()).then(|| expand_home(&text))
}

fn expand_home(text: &str) -> PathBuf {
	match text.strip_prefix("~/") {
		Some(rest) => directories::BaseDirs::new()
			.map_or_else(|| PathBuf::from(text), |d| d.home_dir().join(rest)),
		None => PathBuf::from(text),
	}
}

pub fn set_text(input: &Entity<InputState>, value: String, window: &mut Window, cx: &mut App) {
	input.update(cx, |state, cx| state.set_value(value, window, cx));
}

pub fn new_input(
	value: &str,
	placeholder: String,
	window: &mut Window,
	cx: &mut App,
) -> Entity<InputState> {
	let value = value.to_owned();
	cx.new(|cx| {
		InputState::new(window, cx)
			.placeholder(placeholder)
			.default_value(value)
	})
}

pub struct PickOptions {
	pub folders: bool,
	pub multiple: bool,
}

/// Opens the system file picker; `on_pick` gets the chosen paths.
pub fn pick_paths<V: 'static>(
	options: PickOptions,
	window: &mut Window,
	cx: &mut Context<V>,
	on_pick: impl FnOnce(&mut V, Vec<PathBuf>, &mut Window, &mut Context<V>) + 'static,
) {
	let paths = cx.prompt_for_paths(PathPromptOptions {
		files: !options.folders,
		directories: options.folders,
		multiple: options.multiple,
		prompt: None,
	});
	cx.spawn_in(window, async move |this, cx| {
		if let Ok(Ok(Some(paths))) = paths.await
			&& !paths.is_empty()
		{
			let _ = this.update_in(cx, |this, window, cx| on_pick(this, paths, window, cx));
		}
	})
	.detach();
}

/// Fills `input` with a folder the user picks.
pub fn pick_folder_into<V: 'static>(
	input: Entity<InputState>,
	window: &mut Window,
	cx: &mut Context<V>,
) {
	pick_paths(
		PickOptions {
			folders: true,
			multiple: false,
		},
		window,
		cx,
		move |_, paths, window, cx| {
			set_text(&input, paths[0].display().to_string(), window, cx);
		},
	);
}

/// Fills `input` with a file the user picks.
pub fn pick_file_into<V: 'static>(
	input: Entity<InputState>,
	window: &mut Window,
	cx: &mut Context<V>,
) {
	pick_paths(
		PickOptions {
			folders: false,
			multiple: false,
		},
		window,
		cx,
		move |_, paths, window, cx| {
			set_text(&input, paths[0].display().to_string(), window, cx);
		},
	);
}

/// Opens a file, folder or link with the desktop's default program.
pub fn open_with_desktop(target: impl AsRef<std::ffi::OsStr>) {
	let target = target.as_ref();
	if let Err(e) = std::process::Command::new("xdg-open").arg(target).spawn() {
		tracing::warn!("could not open {}: {e}", target.to_string_lossy());
	}
}

pub fn open_folder_of(path: &Path) {
	if path.is_dir() {
		open_with_desktop(path);
	} else if let Some(parent) = path.parent() {
		open_with_desktop(parent);
	}
}

const TEXT_FILES: [&str; 12] = [
	"qc", "qci", "smd", "vta", "txt", "vmt", "lua", "json", "cfg", "fgd", "res", "log",
];

/// A folder opens in the file manager, a text file in the editor at its line,
/// anything else shows its folder.
pub fn open_link(link: &Link, editor: &str) {
	let path = &link.path;
	if path.is_dir() {
		open_with_desktop(path);
		return;
	}
	let is_text = path
		.extension()
		.is_some_and(|e| TEXT_FILES.contains(&e.to_string_lossy().to_lowercase().as_str()));
	if is_text {
		open_in_editor(path, link.line.unwrap_or(1), editor);
	} else {
		open_folder_of(path);
	}
}

/// `editor` is a command with %f for the file and %l for the line, e.g.
/// "code -g %f:%l"; empty uses the desktop's default program.
pub fn open_in_editor(file: &Path, line: u32, editor: &str) {
	let mut words = editor.split_whitespace();
	if let Some(program) = words.next() {
		let file = file.display().to_string();
		let line = line.max(1).to_string();
		let args: Vec<String> = words
			.map(|w| w.replace("%f", &file).replace("%l", &line))
			.collect();
		match std::process::Command::new(program).args(&args).spawn() {
			Ok(_) => return,
			Err(e) => tracing::warn!("could not start {program}: {e}"),
		}
	}
	open_with_desktop(file);
}

/// The format of an image file by its first bytes, for the ones gpui shows.
pub fn image_format(data: &[u8]) -> Option<ImageFormat> {
	if data.starts_with(b"\x89PNG") {
		Some(ImageFormat::Png)
	} else if data.starts_with(&[0xff, 0xd8, 0xff]) {
		Some(ImageFormat::Jpeg)
	} else if data.starts_with(b"GIF8") {
		Some(ImageFormat::Gif)
	} else if data.starts_with(b"BM") {
		Some(ImageFormat::Bmp)
	} else if data.len() > 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
		Some(ImageFormat::Webp)
	} else {
		None
	}
}

/// Items without a value go after the rest whichever way the column sorts;
/// `descending` turns around only the order of the values themselves.
pub fn missing_last<T: Ord>(a: Option<T>, b: Option<T>, descending: bool) -> std::cmp::Ordering {
	use std::cmp::Ordering;
	match (a, b) {
		(Some(a), Some(b)) if descending => b.cmp(&a),
		(Some(a), Some(b)) => a.cmp(&b),
		(Some(_), None) => Ordering::Less,
		(None, Some(_)) => Ordering::Greater,
		(None, None) => Ordering::Equal,
	}
}

pub fn file_name(path: &Path) -> String {
	path.file_name()
		.map(|n| n.to_string_lossy().into_owned())
		.unwrap_or_else(|| path.display().to_string())
}

pub fn format_size(bytes: u64) -> String {
	const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
	let mut value = bytes as f64;
	let mut unit = 0;
	while value >= 1024. && unit + 1 < UNITS.len() {
		value /= 1024.;
		unit += 1;
	}
	if unit == 0 {
		format!("{bytes} B")
	} else {
		format!("{value:.1} {}", UNITS[unit])
	}
}

pub fn format_date(unix: i64) -> String {
	chrono::DateTime::from_timestamp(unix, 0)
		.map(|t| {
			t.with_timezone(&chrono::Local)
				.format("%Y-%m-%d")
				.to_string()
		})
		.unwrap_or_default()
}

/// Shell-style words: quotes group, a backslash escapes.
pub fn split_words(text: &str) -> Vec<String> {
	let mut words = Vec::new();
	let mut current = String::new();
	let mut in_word = false;
	let mut quote = None;
	let mut chars = text.chars();
	while let Some(c) = chars.next() {
		match (quote, c) {
			(Some(q), c) if c == q => quote = None,
			(None, '"' | '\'') => {
				quote = Some(c);
				in_word = true;
			}
			(_, '\\') => {
				if let Some(next) = chars.next() {
					current.push(next);
					in_word = true;
				}
			}
			(None, c) if c.is_whitespace() => {
				if in_word {
					words.push(std::mem::take(&mut current));
					in_word = false;
				}
			}
			(_, c) => {
				current.push(c);
				in_word = true;
			}
		}
	}
	if in_word {
		words.push(current);
	}
	words
}

#[cfg(test)]
mod tests {
	use super::*;
	// gpui_kit's prelude has a `test` attribute of its own.
	use ::core::prelude::v1::test;

	#[test]
	fn words() {
		assert_eq!(
			split_words(r#"-dev +map "gm construct" it\'s ''"#),
			["-dev", "+map", "gm construct", "it's", ""]
		);
		assert!(split_words("   ").is_empty());
	}

	#[test]
	fn sizes() {
		assert_eq!(format_size(512), "512 B");
		assert_eq!(format_size(1536), "1.5 KiB");
		assert_eq!(format_size(3 * 1024 * 1024 * 1024), "3.0 GiB");
	}
}
