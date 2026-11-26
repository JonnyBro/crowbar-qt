//! A file from a package in a window of its own, read-only, with Lua highlighted.

use std::path::Path;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::input::{Editor, EditorState};
use gpui_kit::prelude::*;
use gpui_kit::*;

/// The highlighter for a file, by its extension; plain text for the rest.
pub fn language_for(path: &str) -> &'static str {
	let is_lua = Path::new(path)
		.extension()
		.is_some_and(|e| e.eq_ignore_ascii_case("lua"));
	if is_lua { "lua" } else { "text" }
}

/// A read-only editor that shows `text` highlighted as `language`.
pub fn viewer(
	text: &str,
	language: &str,
	window: &mut Window,
	cx: &mut App,
) -> Entity<EditorState> {
	let text = text.to_owned();
	let language = language.to_owned();
	cx.new(|cx| {
		let mut editor = EditorState::new(window, cx)
			.language(language)
			.line_number(true)
			.soft_wrap(false);
		editor.set_readonly(true, cx);
		editor.set_value(text, window, cx);
		editor
	})
}

pub fn editor_element(state: &Entity<EditorState>) -> Editor {
	Editor::new(state).readonly(true).bordered(false).h_full()
}

pub struct TextWindow {
	editor: Entity<EditorState>,
}

impl TextWindow {
	/// `title` names the window, e.g. the file's path in its package.
	pub fn open(title: String, text: String, cx: &mut App) {
		let language = language_for(&title);
		let options = WindowOptions {
			window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
				None,
				size(px(1000.), px(760.)),
				cx,
			))),
			window_min_size: Some(size(px(400.), px(300.))),
			titlebar: Some(TitlebarOptions {
				title: Some(format!("{title} - Prybar").into()),
				..Default::default()
			}),
			app_id: Some("prybar".into()),
			..Default::default()
		};
		let result = gpui_kit::open_window(options, cx, move |window, cx| {
			let editor = viewer(&text, language, window, cx);
			cx.new(|_| Self { editor })
		});
		if let Err(e) = result {
			tracing::error!("could not open a window for {title}: {e}");
		}
	}
}

impl Render for TextWindow {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		div()
			.size_full()
			.bg(cx.theme().background)
			.text_color(cx.theme().foreground)
			.child(editor_element(&self.editor))
	}
}
