use std::path::PathBuf;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::table::{DataTable, TableEvent, TableState};
use gpui_kit::component::{
	ActiveTheme, Disableable, Icon, IconName, Sizable, WindowExt, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::*;
use prybar_core::Command;
use prybar_core::engine::{Event, RequestId, TaskId};
use prybar_core::vfs::{Found, MountKind};
use rust_i18n::t;

use crate::model::{EngineEvent, Model};
use crate::theme::Mono;
use crate::widgets::{self, empty_state, page_header};

use super::packages_table::{ResultsTable, source_label};

/// Opens the package on the package page, filtered to `path`.
pub struct OpenInPackage {
	pub package: PathBuf,
	pub path: String,
}

enum State {
	Idle,
	Searching(RequestId),
	Done,
}

pub struct GameFilesView {
	model: Entity<Model>,
	query: Entity<InputState>,
	results: Entity<TableState<ResultsTable>>,
	state: State,
	/// Cancelled when a new search starts.
	task: Option<TaskId>,
	found: Option<Found>,
	_subscriptions: Vec<Subscription>,
}

pub struct InspectModel(pub String);

impl EventEmitter<OpenInPackage> for GameFilesView {}
impl EventEmitter<InspectModel> for GameFilesView {}

impl GameFilesView {
	pub fn new(model: Entity<Model>, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let query = widgets::new_input("", t!("files.placeholder").into_owned(), window, cx);
		let (results, widths) =
			super::columns::new_table(ResultsTable::default(), &model, window, cx);
		let subscriptions = vec![
			widths,
			cx.observe(&model, |_, _, cx| cx.notify()),
			cx.subscribe(&results, |this, table, event: &TableEvent, cx| {
				if let TableEvent::SelectRow(row) = event {
					this.found = table.read(cx).delegate().get(*row).cloned();
					cx.notify();
				}
			}),
			cx.subscribe_in(&model, window, |this, _, event: &EngineEvent, _, cx| {
				this.on_event(&event.0, cx)
			}),
			cx.subscribe_in(&query, window, |this, _, event: &InputEvent, window, cx| {
				if matches!(event, InputEvent::PressEnter { .. }) {
					this.search(window, cx);
				}
			}),
		];
		Self {
			model,
			query,
			results,
			state: State::Idle,
			task: None,
			found: None,
			_subscriptions: subscriptions,
		}
	}

	fn on_event(&mut self, event: &Event, cx: &mut Context<Self>) {
		if let Event::SearchResults {
			request,
			task,
			matches,
		} = event
			&& matches!(self.state, State::Searching(r) if r == *request)
		{
			self.state = State::Done;
			self.task = Some(*task);
			let matches = matches.clone();
			self.results.update(cx, |table, cx| {
				table.delegate_mut().extend(matches);
				cx.notify();
			});
			cx.notify();
		}
	}

	fn search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let needle = widgets::text(&self.query, cx);
		if needle.chars().count() < 2 {
			window.push_notification(
				Notification::warning(t!("files.too_short").into_owned()),
				cx,
			);
			return;
		}
		let model = self.model.read(cx);
		let Some(target) = model.target.clone() else {
			window.push_notification(Notification::warning(t!("game.none").into_owned()), cx);
			return;
		};
		if let Some(task) = self.task.take() {
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
		self.found = None;
		self.state = State::Searching(request);
		cx.notify();
	}

	fn list(&self, cx: &App) -> AnyElement {
		if !self.results.read(cx).delegate().matches.is_empty() {
			return DataTable::new(&self.results)
				.header_row_height(px(32.))
				.into_any_element();
		}
		let text = match self.state {
			State::Idle => t!("files.hint"),
			State::Searching(_) => t!("files.searching"),
			State::Done => t!("files.nothing"),
		};
		empty_state(text.into_owned(), cx)
	}

	fn sources(&self, cx: &mut Context<Self>) -> AnyElement {
		let theme = cx.theme().clone();
		let Some(found) = &self.found else {
			return empty_state(t!("files.sources_hint").into_owned(), cx);
		};
		let rows = found.sources.iter().enumerate().map(|(i, source)| {
			let (state, color) = if i == 0 {
				(t!("files.loaded"), theme.success)
			} else {
				(t!("files.shadowed"), theme.muted_foreground)
			};
			let action = match source.kind {
				MountKind::Package => {
					let package = source.mount.clone();
					let path = source.path.clone();
					Button::new(("open-source", i))
						.label(t!("files.open_package"))
						.xsmall()
						.ghost()
						.on_click(cx.listener(move |_, _, _, cx| {
							cx.emit(OpenInPackage {
								package: package.clone(),
								path: path.clone(),
							});
						}))
				}
				MountKind::Folder => {
					let file = source.file();
					Button::new(("open-source", i))
						.icon(Icon::new(IconName::FolderOpen))
						.label(t!("files.show_folder"))
						.xsmall()
						.ghost()
						.on_click(move |_, _, _| widgets::open_folder_of(&file))
				}
			};
			h_flex()
				.px_3()
				.py_2()
				.gap_3()
				.border_b_1()
				.border_color(theme.border)
				.child(
					v_flex()
						.flex_1()
						.min_w_0()
						.gap_0p5()
						.child(
							h_flex()
								.gap_2()
								.child(
									div()
										.truncate()
										.text_sm()
										.when(i == 0, |this| this.font_weight(FontWeight::SEMIBOLD))
										.child(source_label(source)),
								)
								.child(div().text_xs().text_color(color).child(state)),
						)
						.child(
							div()
								.truncate()
								.text_xs()
								.text_color(theme.muted_foreground)
								.child(source.file().display().to_string()),
						),
				)
				.child(
					div()
						.text_xs()
						.text_color(theme.muted_foreground)
						.whitespace_nowrap()
						.child(widgets::format_size(source.size)),
				)
				.child(action)
		});
		let model = found.path.to_ascii_lowercase().ends_with(".mdl").then(|| {
			let path = found.path.clone();
			Button::new("inspect-model")
				.label(t!("files.inspect"))
				.xsmall()
				.outline()
				.on_click(cx.listener(move |_, _, _, cx| {
					cx.emit(InspectModel(path.clone()));
				}))
		});
		v_flex()
			.size_full()
			.child(
				h_flex()
					.px_3()
					.py_2()
					.gap_2()
					.border_b_1()
					.border_color(theme.border)
					.child(
						div()
							.flex_1()
							.min_w_0()
							.truncate()
							.text_xs()
							.mono(cx)
							.child(found.path.clone()),
					)
					.children(model),
			)
			.child(
				v_flex()
					.id("sources")
					.flex_1()
					.min_h_0()
					.overflow_y_scroll()
					.children(rows),
			)
			.into_any_element()
	}
}

impl Render for GameFilesView {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = cx.theme().clone();
		let has_game = self.model.read(cx).target.is_some();
		let searching = matches!(self.state, State::Searching(_));
		v_flex()
			.size_full()
			.p_5()
			.gap_3()
			.child(page_header(
				t!("page.files").into_owned(),
				Some(t!("files.subtitle").into_owned()),
				cx,
			))
			.child(
				h_flex()
					.gap_2()
					.child(
						div().flex_1().child(
							Input::new(&self.query)
								.small()
								.prefix(Icon::new(IconName::Search).small()),
						),
					)
					.child(
						Button::new("files-search")
							.label(t!("files.search"))
							.small()
							.primary()
							.loading(searching)
							.disabled(!has_game)
							.on_click(cx.listener(|this, _, window, cx| this.search(window, cx))),
					),
			)
			.child(
				h_flex()
					.flex_1()
					.min_h_0()
					.gap_3()
					.child(v_flex().flex_1().min_w_0().h_full().child(self.list(cx)))
					.child(
						v_flex()
							.w(px(460.))
							.flex_shrink_0()
							.h_full()
							.border_1()
							.border_color(theme.border)
							.rounded(theme.radius)
							.overflow_hidden()
							.child(self.sources(cx)),
					),
			)
	}
}
