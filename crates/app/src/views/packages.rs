use std::collections::BTreeSet;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
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
use crate::theme::Mono;
use crate::ui_state::OutputMode;
use crate::widgets::{self, browse_button, choice, field, format_size, page_header, path_row};

const ROW_HEIGHT: f32 = 26.;
/// Bigger entries are not read for a preview.
const MAX_PREVIEW: u64 = 4 * 1024 * 1024;
const TEXT_PREVIEW_LINES: usize = 2000;
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
	Text(SharedString),
	Image(Arc<Image>),
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
	/// Entries the filter lets through, as indexes into the package's entries.
	shown: Vec<usize>,
	selected: BTreeSet<usize>,
	focused: Option<usize>,
	preview: Preview,
	scope: Scope,
	search: Option<RequestId>,
	/// The running search, for cancelling it when a new one starts.
	search_task: Option<TaskId>,
	/// Matches of the search over every package: package, entry path, size.
	results: Vec<(PathBuf, String, u64)>,
	/// Shown once the package being opened from a search result is there.
	pending_filter: Option<String>,
	scroll: UniformListScrollHandle,
	_subscriptions: Vec<Subscription>,
}

impl PackagesView {
	pub fn new(model: Entity<Model>, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let prefs = model.read(cx).ui.packages.clone();
		let file = widgets::new_input(
			&prefs.file,
			t!("packages.file_placeholder").into_owned(),
			window,
			cx,
		);
		let filter = widgets::new_input(
			"",
			t!("packages.filter_placeholder").into_owned(),
			window,
			cx,
		);
		let output = widgets::new_input(
			&prefs.folder,
			t!("packages.output_placeholder").into_owned(),
			window,
			cx,
		);
		let subscriptions = vec![
			cx.observe(&model, |_, _, cx| cx.notify()),
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
		let mut view = Self {
			model,
			file,
			filter,
			output,
			package: None,
			opening: None,
			open_error: None,
			shown: Vec::new(),
			selected: BTreeSet::new(),
			focused: None,
			preview: Preview::None,
			scope: Scope::Package,
			search: None,
			search_task: None,
			results: Vec::new(),
			pending_filter: None,
			scroll: UniformListScrollHandle::new(),
			_subscriptions: subscriptions,
		};
		if !prefs.file.is_empty() && Path::new(&prefs.file).is_file() {
			view.open(window, cx);
		}
		view
	}

	pub fn open_file(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
		widgets::set_text(&self.file, path.display().to_string(), window, cx);
		self.open(window, cx);
	}

	fn open(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
		let Some(path) = widgets::path(&self.file, cx) else {
			return;
		};
		let text = path.display().to_string();
		let request = self.model.update(cx, |m, _| {
			m.ui.packages.file = text;
			m.save_ui();
			m.request_id()
		});
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
				self.selected.clear();
				self.focused = None;
				self.preview = Preview::None;
				match result {
					Ok(package) => {
						self.package = Some(package.clone());
						self.open_error = None;
						if let Some(filter) = self.pending_filter.take() {
							self.scope = Scope::Package;
							widgets::set_text(&self.filter, filter, window, cx);
						}
					}
					Err(e) => {
						self.package = None;
						self.open_error = Some(e.clone());
					}
				}
				self.apply_filter(cx);
			}
			Event::EntryRead { request, result } => {
				if matches!(self.preview, Preview::Loading(r) if r == *request) {
					self.preview = match result {
						Ok(data) => self.make_preview(data),
						Err(e) => Preview::Error(e.clone()),
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
				self.results.extend(
					matches
						.iter()
						.map(|(path, size)| (package.clone(), path.clone(), *size)),
				);
				cx.notify();
			}
			_ => {}
		}
	}

	fn make_preview(&self, data: &[u8]) -> Preview {
		let Some(entry) = self.focused_entry() else {
			return Preview::None;
		};
		let extension = Path::new(&entry.path)
			.extension()
			.map(|e| e.to_string_lossy().to_lowercase())
			.unwrap_or_default();
		if let Some(format) = widgets::image_format(data) {
			return Preview::Image(Arc::new(Image::from_bytes(format, data.to_vec())));
		}
		if TEXT_EXTENSIONS.contains(&extension.as_str()) {
			let text = String::from_utf8_lossy(data);
			let text: String = text
				.lines()
				.take(TEXT_PREVIEW_LINES)
				.collect::<Vec<_>>()
				.join("\n");
			return Preview::Text(text.into());
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
		self.shown = match &self.package {
			Some(package) => package
				.entries
				.iter()
				.enumerate()
				.filter(|(_, e)| needle.is_empty() || e.path.to_lowercase().contains(&needle))
				.map(|(i, _)| i)
				.collect(),
			None => Vec::new(),
		};
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
		self.results.clear();
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
		if entry.size > MAX_PREVIEW {
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
			max_size: MAX_PREVIEW,
		});
		cx.notify();
	}

	fn output_dir(&mut self, cx: &mut Context<Self>) -> Option<PathBuf> {
		let package = self.package.as_ref()?;
		let folder = widgets::path(&self.output, cx);
		let text = widgets::text(&self.output, cx);
		let mode = self.model.update(cx, |m, _| {
			m.ui.packages.folder = text;
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
			self.selected.iter().copied().collect()
		} else {
			Vec::new()
		};
		self.model.read(cx).send(Command::Unpack {
			package,
			indexes,
			output_dir,
		});
	}

	fn render_entries(
		&mut self,
		range: Range<usize>,
		_: &mut Window,
		cx: &mut Context<Self>,
	) -> Vec<AnyElement> {
		let theme = cx.theme().clone();
		let Some(package) = self.package.clone() else {
			return Vec::new();
		};
		range
			.filter_map(|row| {
				let index = *self.shown.get(row)?;
				let entry = package.entries.get(index)?;
				let selected = self.selected.contains(&index);
				let focused = self.focused == Some(index);
				Some(
					h_flex()
						.id(("entry", row))
						.h(px(ROW_HEIGHT))
						.px_3()
						.gap_3()
						.border_b_1()
						.border_color(theme.border.opacity(0.4))
						.when(selected, |r| r.bg(theme.list_active))
						.when(focused && !selected, |r| r.bg(theme.list_hover))
						.hover(|r| r.bg(theme.list_hover))
						.on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
							if event.modifiers().control || event.modifiers().shift {
								if !this.selected.remove(&index) {
									this.selected.insert(index);
								}
							} else {
								this.selected = BTreeSet::from([index]);
							}
							this.focus(index, cx);
						}))
						.child(
							div()
								.flex_1()
								.min_w_0()
								.truncate()
								.mono(cx)
								.text_sm()
								.child(entry.path.clone()),
						)
						.child(
							div()
								.w(px(90.))
								.text_sm()
								.text_color(theme.muted_foreground)
								.child(format_size(entry.size)),
						)
						.into_any_element(),
				)
			})
			.collect()
	}

	fn render_results(
		&mut self,
		range: Range<usize>,
		_: &mut Window,
		cx: &mut Context<Self>,
	) -> Vec<AnyElement> {
		let theme = cx.theme().clone();
		range
			.filter_map(|row| {
				let (package, path, size) = self.results.get(row)?.clone();
				let open = package.clone();
				let filter = path.clone();
				Some(
					h_flex()
						.id(("result", row))
						.h(px(ROW_HEIGHT))
						.px_3()
						.gap_3()
						.border_b_1()
						.border_color(theme.border.opacity(0.4))
						.hover(|r| r.bg(theme.list_hover))
						.cursor_pointer()
						.on_click(cx.listener(move |this, _, window, cx| {
							this.pending_filter = Some(filter.clone());
							this.open_file(&open, window, cx);
						}))
						.child(
							div()
								.w(px(220.))
								.truncate()
								.text_sm()
								.text_color(theme.muted_foreground)
								.child(widgets::file_name(&package)),
						)
						.child(
							div()
								.flex_1()
								.min_w_0()
								.truncate()
								.mono(cx)
								.text_sm()
								.child(path),
						)
						.child(
							div()
								.w(px(90.))
								.text_sm()
								.text_color(theme.muted_foreground)
								.child(format_size(size)),
						)
						.into_any_element(),
				)
			})
			.collect()
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
			Preview::Image(image) => div()
				.size_full()
				.p_3()
				.flex()
				.items_center()
				.justify_center()
				.child(img(image.clone()).max_w_full().max_h_full())
				.into_any_element(),
			Preview::Text(text) => div()
				.id("text-preview")
				.size_full()
				.overflow_scroll()
				.p_3()
				.mono(cx)
				.text_xs()
				.whitespace_nowrap()
				.child(text.clone())
				.into_any_element(),
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
			if self.results.is_empty() {
				div()
					.flex_1()
					.flex()
					.items_center()
					.justify_center()
					.text_color(theme.muted_foreground)
					.child(t!("packages.search_hint").into_owned())
					.into_any_element()
			} else {
				uniform_list(
					"results",
					self.results.len(),
					cx.processor(Self::render_results),
				)
				.track_scroll(&self.scroll)
				.flex_1()
				.into_any_element()
			}
		} else {
			uniform_list(
				"entries",
				self.shown.len(),
				cx.processor(Self::render_entries),
			)
			.track_scroll(&self.scroll)
			.flex_1()
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
					.child(
						v_flex()
							.flex_1()
							.min_w_0()
							.h_full()
							.border_1()
							.border_color(theme.border)
							.rounded(theme.radius)
							.child(list),
					)
					.child(
						v_flex()
							.w(px(340.))
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
							.label(t!("packages.unpack_selected", n = self.selected.len()))
							.small()
							.outline()
							.disabled(self.package.is_none() || self.selected.is_empty())
							.on_click(
								cx.listener(|this, _, window, cx| this.unpack(true, window, cx)),
							),
					),
			)
	}
}
