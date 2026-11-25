use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{EditorState, Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::table::{DataTable, TableEvent, TableState};
use gpui_kit::component::{
	ActiveTheme, Disableable, Icon, IconName, Sizable, WindowExt, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::*;
use prybar_core::Command;
use prybar_core::engine::{Event, RequestId, TaskId};
use prybar_core::package::Package;
use rust_i18n::t;

use crate::model::{EngineEvent, Model};
use crate::ui_state::OutputMode;
use crate::widgets::{self, browse_button, choice, field, page_header, path_row};

use super::packages_table::{EntriesTable, Match, ResultsTable};
use super::text_window::{self, TextWindow};

/// Bigger entries are not read for a preview.
const MAX_PREVIEW: u64 = 4 * 1024 * 1024;
/// Textures keep their largest mipmap at the end, so the whole file is read.
const MAX_TEXTURE_PREVIEW: u64 = 160 * 1024 * 1024;
const TEXT_EXTENSIONS: [&str; 16] = [
	"txt", "vmt", "qc", "qci", "lua", "json", "cfg", "res", "vdf", "nut", "smd", "fgd", "md",
	"ini", "rad", "kv",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
	Package,
	Game,
}

enum Preview {
	None,
	Loading(RequestId),
	/// A read-only editor, with the text and the entry's path for opening it in a window.
	Text {
		editor: Entity<EditorState>,
		text: String,
		path: String,
	},
	Image {
		image: Arc<Image>,
		/// Size and format, for textures.
		caption: Option<String>,
	},
	Unsupported,
	Error(String),
}

pub struct PackagesView {
	model: Entity<Model>,
	file: Entity<InputState>,
	filter: Entity<InputState>,
	output: Entity<InputState>,
	package: Option<Arc<Package>>,
	opening: Option<RequestId>,
	open_error: Option<String>,
	entries: Entity<TableState<EntriesTable>>,
	results: Entity<TableState<ResultsTable>>,
	/// The entry clicked last, shown in the preview; the one "Unpack selected"
	/// takes when none is checked.
	focused: Option<usize>,
	preview: Preview,
	scope: Scope,
	search: Option<RequestId>,
	/// The running search, for cancelling it when a new one starts.
	search_task: Option<TaskId>,
	/// Shown once the package being opened from a search result is there.
	pending_filter: Option<String>,
	_subscriptions: Vec<Subscription>,
}

impl PackagesView {
	pub fn new(model: Entity<Model>, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let file = widgets::new_input("", t!("packages.file_placeholder").into_owned(), window, cx);
		let filter = widgets::new_input(
			"",
			t!("packages.filter_placeholder").into_owned(),
			window,
			cx,
		);
		let output = widgets::new_input(
			"",
			t!("packages.output_placeholder").into_owned(),
			window,
			cx,
		);
		let (entries, entry_widths) =
			super::columns::new_table(EntriesTable::default(), &model, window, cx);
		let (results, result_widths) =
			super::columns::new_table(ResultsTable::default(), &model, window, cx);
		let subscriptions = vec![
			entry_widths,
			result_widths,
			cx.observe(&model, |_, _, cx| cx.notify()),
			cx.observe(&entries, |this, table, cx| {
				// Sorting moves rows; the highlight follows the focused entry.
				if let Some(focused) = this.focused {
					let index = table.read(cx).delegate().row_of(focused);
					if let Some(index) = index
						&& table.read(cx).selected_row() != Some(index)
					{
						table.update(cx, |table, cx| table.set_selected_row(index, cx));
					}
				}
				cx.notify();
			}),
			cx.subscribe(&entries, |this, table, event: &TableEvent, cx| {
				if let TableEvent::SelectRow(row) = event
					&& let Some(index) = table.read(cx).delegate().entry_index(*row)
					&& this.focused != Some(index)
				{
					this.focus(index, cx);
				}
			}),
			cx.subscribe_in(
				&results,
				window,
				|this, table, event: &TableEvent, window, cx| {
					if let TableEvent::SelectRow(row) = event
						&& let Some(found) = table.read(cx).delegate().get(*row).cloned()
					{
						this.pending_filter = Some(found.path);
						this.open_file(&found.package, window, cx);
					}
				},
			),
			cx.subscribe_in(
				&model,
				window,
				|this, _, event: &EngineEvent, window, cx| {
					this.on_event(&event.0, window, cx);
				},
			),
			cx.subscribe_in(
				&filter,
				window,
				|this, _, event: &InputEvent, window, cx| match event {
					InputEvent::Change if this.scope == Scope::Package => this.apply_filter(cx),
					InputEvent::PressEnter { .. } if this.scope == Scope::Game => {
						this.search(window, cx)
					}
					_ => {}
				},
			),
			cx.subscribe_in(&file, window, |this, _, event: &InputEvent, window, cx| {
				if matches!(event, InputEvent::PressEnter { .. }) {
					this.open(window, cx);
				}
			}),
		];
		Self {
			model,
			file,
			filter,
			output,
			package: None,
			opening: None,
			open_error: None,
			entries,
			results,
			focused: None,
			preview: Preview::None,
			scope: Scope::Package,
			search: None,
			search_task: None,
			pending_filter: None,
			_subscriptions: subscriptions,
		}
	}

	pub fn open_file(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
		widgets::set_text(&self.file, path.display().to_string(), window, cx);
		self.open(window, cx);
	}

	fn open(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
		let Some(path) = widgets::path(&self.file, cx) else {
			return;
		};
		let request = self.model.update(cx, |m, _| m.request_id());
		self.opening = Some(request);
		self.open_error = None;
		self.model
			.read(cx)
			.send(Command::OpenPackage { request, path });
		cx.notify();
	}

	fn on_event(&mut self, event: &Event, window: &mut Window, cx: &mut Context<Self>) {
		match event {
			Event::PackageOpened { request, result } if Some(*request) == self.opening => {
				self.opening = None;
				self.focused = None;
				self.preview = Preview::None;
				match result {
					Ok(package) => {
						self.package = Some(package.clone());
						let package = package.clone();
						self.entries.update(cx, |table, cx| {
							table.delegate_mut().set_package(Some(package));
							table.clear_selection(cx);
						});
						self.open_error = None;
						if let Some(filter) = self.pending_filter.take() {
							self.scope = Scope::Package;
							widgets::set_text(&self.filter, filter, window, cx);
						}
					}
					Err(e) => {
						self.package = None;
						self.entries.update(cx, |table, cx| {
							table.delegate_mut().set_package(None);
							table.clear_selection(cx);
						});
						self.open_error = Some(e.clone());
					}
				}
				self.apply_filter(cx);
			}
			Event::EntryRead {
				request,
				result,
				texture,
			} => {
				if matches!(self.preview, Preview::Loading(r) if r == *request) {
					self.preview = match (result, texture) {
						(Ok(png), Some(info)) => Preview::Image {
							image: Arc::new(Image::from_bytes(ImageFormat::Png, png.to_vec())),
							caption: Some(texture_caption(info)),
						},
						(Ok(data), None) => self.make_preview(data, window, cx),
						(Err(e), _) => Preview::Error(e.clone()),
					};
					cx.notify();
				}
			}
			Event::SearchResults {
				request,
				task,
				package,
				matches,
			} if Some(*request) == self.search => {
				self.search_task = Some(*task);
				let found: Vec<Match> = matches
					.iter()
					.map(|(path, size)| Match {
						package: package.clone(),
						path: path.clone(),
						size: *size,
					})
					.collect();
				self.results.update(cx, |table, cx| {
					table.delegate_mut().extend(found);
					cx.notify();
				});
				cx.notify();
			}
			_ => {}
		}
	}

	fn make_preview(&self, data: &[u8], window: &mut Window, cx: &mut App) -> Preview {
		let Some(entry) = self.focused_entry() else {
			return Preview::None;
		};
		let extension = Path::new(&entry.path)
			.extension()
			.map(|e| e.to_string_lossy().to_lowercase())
			.unwrap_or_default();
		if let Some(format) = widgets::image_format(data) {
			return Preview::Image {
				image: Arc::new(Image::from_bytes(format, data.to_vec())),
				caption: None,
			};
		}
		if TEXT_EXTENSIONS.contains(&extension.as_str()) {
			let text = String::from_utf8_lossy(data).into_owned();
			let language = text_window::language_for(&entry.path);
			return Preview::Text {
				editor: text_window::viewer(&text, language, window, cx),
				text,
				path: entry.path.clone(),
			};
		}
		Preview::Unsupported
	}

	fn focused_entry(&self) -> Option<&prybar_core::package::Entry> {
		self.package.as_ref()?.entries.get(self.focused?)
	}

	fn apply_filter(&mut self, cx: &mut Context<Self>) {
		let needle = widgets::text(&self.filter, cx)
			.to_lowercase()
			.replace('\\', "/");
		self.entries.update(cx, |table, cx| {
			table.delegate_mut().set_filter(&needle);
			cx.notify();
		});
		cx.notify();
	}

	fn set_scope(&mut self, scope: Scope, cx: &mut Context<Self>) {
		self.scope = scope;
		if scope == Scope::Package {
			self.apply_filter(cx);
		}
		cx.notify();
	}

	fn search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let needle = widgets::text(&self.filter, cx);
		if needle.chars().count() < 2 {
			return;
		}
		let model = self.model.read(cx);
		let Some(target) = model.target.clone() else {
			window.push_notification(Notification::warning(t!("game.none").into_owned()), cx);
			return;
		};
		if let Some(task) = self.search_task {
			model.send(Command::Cancel(task));
		}
		let request = self.model.update(cx, |m, _| m.request_id());
		self.model.read(cx).send(Command::SearchPackages {
			request,
			target,
			needle,
		});
		self.results.update(cx, |table, cx| {
			table.delegate_mut().clear();
			table.clear_selection(cx);
			cx.notify();
		});
		self.search = Some(request);
		cx.notify();
	}

	fn focus(&mut self, index: usize, cx: &mut Context<Self>) {
		self.focused = Some(index);
		let Some(package) = self.package.clone() else {
			return;
		};
		let Some(entry) = package.entries.get(index) else {
			return;
		};
		let max_size = if entry.path.to_ascii_lowercase().ends_with(".vtf") {
			MAX_TEXTURE_PREVIEW
		} else {
			MAX_PREVIEW
		};
		if entry.size > max_size {
			self.preview = Preview::Unsupported;
			cx.notify();
			return;
		}
		let request = self.model.update(cx, |m, _| m.request_id());
		self.preview = Preview::Loading(request);
		self.model.read(cx).send(Command::ReadEntry {
			request,
			package,
			index,
			max_size,
		});
		cx.notify();
	}

	fn output_dir(&mut self, cx: &mut Context<Self>) -> Option<PathBuf> {
		let package = self.package.as_ref()?;
		let folder = widgets::path(&self.output, cx);
		let mode = self.model.update(cx, |m, _| {
			m.save_ui();
			m.ui.packages.output
		});
		match mode {
			OutputMode::Folder => folder,
			_ => {
				let stem = package
					.file
					.file_stem()
					.map(|s| s.to_string_lossy().into_owned())
					.unwrap_or_else(|| "unpacked".into());
				let stem = stem.strip_suffix("_dir").unwrap_or(&stem).to_owned();
				package.file.parent().map(|dir| dir.join(stem))
			}
		}
	}

	fn unpack(&mut self, selected_only: bool, window: &mut Window, cx: &mut Context<Self>) {
		let Some(package) = self.package.clone() else {
			return;
		};
		let Some(output_dir) = self.output_dir(cx) else {
			window.push_notification(
				Notification::warning(t!("packages.need_folder").into_owned()),
				cx,
			);
			return;
		};
		let indexes = if selected_only {
			self.picked(cx)
		} else {
			Vec::new()
		};
		if selected_only && indexes.is_empty() {
			return;
		}
		self.model.read(cx).send(Command::Unpack {
			package,
			indexes,
			output_dir,
		});
	}

	/// The checked entries, or the focused one when none is checked.
	fn picked(&self, cx: &App) -> Vec<usize> {
		let checked = &self.entries.read(cx).delegate().checked;
		if checked.is_empty() {
			self.focused.into_iter().collect()
		} else {
			checked.iter().copied().collect()
		}
	}

	fn preview(&self, cx: &App) -> AnyElement {
		let theme = cx.theme();
		let centered = |text: String| {
			div()
				.size_full()
				.flex()
				.items_center()
				.justify_center()
				.p_4()
				.text_sm()
				.text_color(theme.muted_foreground)
				.child(text)
				.into_any_element()
		};
		match &self.preview {
			Preview::None => centered(t!("packages.preview_hint").into_owned()),
			Preview::Loading(_) => centered("…".into()),
			Preview::Unsupported => centered(t!("packages.no_preview").into_owned()),
			Preview::Error(e) => centered(e.clone()),
			Preview::Image { image, caption } => v_flex()
				.size_full()
				.p_3()
				.gap_2()
				.items_center()
				.justify_center()
				.child(
					div()
						.flex_1()
						.min_h_0()
						.w_full()
						.flex()
						.items_center()
						.justify_center()
						.child(img(image.clone()).max_w_full().max_h_full()),
				)
				.children(caption.clone().map(|caption| {
					div()
						.text_xs()
						.text_color(theme.muted_foreground)
						.child(caption)
				}))
				.into_any_element(),
			Preview::Text { editor, text, path } => {
				let text = text.clone();
				let path = path.clone();
				v_flex()
					.size_full()
					.child(
						h_flex()
							.px_2()
							.py_1()
							.gap_2()
							.border_b_1()
							.border_color(theme.border)
							.child(
								div()
									.flex_1()
									.min_w_0()
									.truncate()
									.text_xs()
									.text_color(theme.muted_foreground)
									.child(path.clone()),
							)
							.child(
								Button::new("preview-window")
									.icon(Icon::new(IconName::ExternalLink))
									.label(t!("packages.open_window"))
									.xsmall()
									.ghost()
									.on_click(move |_, _, cx| {
										TextWindow::open(path.clone(), text.clone(), cx);
									}),
							),
					)
					.child(
						div()
							.flex_1()
							.min_h_0()
							.child(text_window::editor_element(editor)),
					)
					.into_any_element()
			}
		}
	}
}

impl Render for PackagesView {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = cx.theme().clone();
		let prefs = self.model.read(cx).ui.packages.clone();
		let file = self.file.clone();
		let output = self.output.clone();
		let searching = self.scope == Scope::Game;
		let picked = self.picked(cx).len();

		let summary = match (&self.package, &self.open_error, self.opening) {
			(_, _, Some(_)) => t!("packages.opening").into_owned(),
			(_, Some(e), _) => e.clone(),
			(Some(package), _, _) => {
				let mut text = t!(
					"packages.summary",
					format = package.format,
					count = package.entries.len()
				)
				.into_owned();
				if let Some(title) = package.title.as_ref().filter(|t| !t.is_empty()) {
					text = format!("{title} · {text}");
				}
				text
			}
			(None, None, None) => t!("packages.none").into_owned(),
		};

		let list: AnyElement = if searching {
			if self.results.read(cx).delegate().matches.is_empty() {
				div()
					.flex_1()
					.flex()
					.items_center()
					.justify_center()
					.text_color(theme.muted_foreground)
					.child(t!("packages.search_hint").into_owned())
					.into_any_element()
			} else {
				DataTable::new(&self.results)
					.header_row_height(px(32.))
					.into_any_element()
			}
		} else {
			DataTable::new(&self.entries)
				.header_row_height(px(32.))
				.into_any_element()
		};

		let output_row = v_flex()
			.gap_2()
			.child(
				h_flex()
					.gap_1()
					.child(
						choice(
							"unpack-beside",
							t!("packages.out_beside"),
							prefs.output != OutputMode::Folder,
						)
						.on_click(cx.listener(|this, _, _, cx| {
							this.model
								.update(cx, |m, _| m.ui.packages.output = OutputMode::Beside);
							cx.notify();
						})),
					)
					.child(
						choice(
							"unpack-folder",
							t!("packages.out_folder"),
							prefs.output == OutputMode::Folder,
						)
						.on_click(cx.listener(|this, _, _, cx| {
							this.model
								.update(cx, |m, _| m.ui.packages.output = OutputMode::Folder);
							cx.notify();
						})),
					),
			)
			.when(prefs.output == OutputMode::Folder, |this| {
				this.child(path_row(
					&self.output,
					browse_button("unpack-browse").on_click(cx.listener(
						move |_, _, window, cx| {
							widgets::pick_folder_into(output.clone(), window, cx);
						},
					)),
				))
			});

		v_flex()
			.size_full()
			.p_5()
			.gap_3()
			.child(page_header(
				t!("page.packages").into_owned(),
				Some(summary),
				cx,
			))
			.child(
				h_flex()
					.gap_2()
					.child(div().flex_1().child(path_row(
						&self.file,
						browse_button("package-browse").on_click(cx.listener(
							move |_, _, window, cx| {
								let file = file.clone();
								widgets::pick_paths(
									widgets::PickOptions {
										folders: false,
										multiple: false,
									},
									window,
									cx,
									move |this: &mut Self, paths, window, cx| {
										widgets::set_text(
											&file,
											paths[0].display().to_string(),
											window,
											cx,
										);
										this.open(window, cx);
									},
								);
							},
						)),
					)))
					.child(
						Button::new("package-open")
							.label(t!("packages.open"))
							.small()
							.primary()
							.on_click(cx.listener(|this, _, window, cx| this.open(window, cx))),
					),
			)
			.child(
				h_flex()
					.gap_2()
					.child(
						div().flex_1().child(
							Input::new(&self.filter)
								.small()
								.prefix(Icon::new(IconName::Search).small()),
						),
					)
					.child(
						choice("scope-package", t!("packages.in_package"), !searching).on_click(
							cx.listener(|this, _, _, cx| this.set_scope(Scope::Package, cx)),
						),
					)
					.child(
						choice("scope-game", t!("packages.in_game"), searching).on_click(
							cx.listener(|this, _, _, cx| this.set_scope(Scope::Game, cx)),
						),
					)
					.when(searching, |this| {
						this.child(
							Button::new("search-go")
								.label(t!("packages.search"))
								.small()
								.outline()
								.on_click(
									cx.listener(|this, _, window, cx| this.search(window, cx)),
								),
						)
					}),
			)
			.child(
				h_flex()
					.flex_1()
					.min_h_0()
					.gap_3()
					.child(v_flex().flex_1().min_w_0().h_full().child(list))
					.child(
						v_flex()
							.w(px(460.))
							.flex_shrink_0()
							.h_full()
							.border_1()
							.border_color(theme.border)
							.rounded(theme.radius)
							.overflow_hidden()
							.child(self.preview(cx)),
					),
			)
			.child(field(t!("packages.output").into_owned(), output_row, cx))
			.child(
				h_flex()
					.gap_2()
					.child(
						Button::new("unpack-all")
							.label(t!("packages.unpack_all"))
							.small()
							.primary()
							.disabled(self.package.is_none())
							.on_click(
								cx.listener(|this, _, window, cx| this.unpack(false, window, cx)),
							),
					)
					.child(
						Button::new("unpack-selected")
							.label(t!("packages.unpack_selected", n = picked))
							.small()
							.outline()
							.disabled(self.package.is_none() || picked == 0)
							.on_click(
								cx.listener(|this, _, window, cx| this.unpack(true, window, cx)),
							),
					),
			)
	}
}

fn texture_caption(info: &prybar_core::vtf::Info) -> String {
	let mut size = format!("{}×{}", info.width, info.height);
	if info.depth > 1 {
		size += &format!("×{}", info.depth);
	}
	let (major, minor) = info.version;
	format!("{size} · {} · VTF {major}.{minor}", info.format.name())
}
