//! The panel under the pages: every task, and the log of the one selected.

use std::ops::Range;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{
	ActiveTheme, Disableable, Icon, IconName, Selectable, Sizable, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::*;
use prybar_core::Command;
use prybar_core::engine::{TaskId, TaskState};
use rust_i18n::t;

use crate::icons::AppIcon;
use crate::model::{LogLine, Model, Severity};
use crate::theme::Mono;

const PANEL_HEIGHT: f32 = 280.;
const LINE_HEIGHT: f32 = 20.;

pub struct TasksPanel {
	model: Entity<Model>,
	selected: Option<TaskId>,
	/// The newest task is selected when it starts.
	follow_new: bool,
	/// The log keeps scrolling to its end while the task writes to it.
	follow_log: bool,
	known: usize,
	shown_lines: usize,
	scroll: UniformListScrollHandle,
}

impl TasksPanel {
	pub fn new(model: Entity<Model>, cx: &mut Context<Self>) -> Self {
		cx.observe(&model, |this, model, cx| {
			let tasks = &model.read(cx).tasks;
			if tasks.len() > this.known && this.follow_new {
				this.selected = tasks.last().map(|t| t.snapshot.id);
				this.follow_log = true;
			}
			this.known = tasks.len();
			if this
				.selected
				.is_some_and(|id| !tasks.iter().any(|t| t.snapshot.id == id))
			{
				this.selected = tasks.last().map(|t| t.snapshot.id);
			}
			cx.notify();
		})
		.detach();
		Self {
			model,
			selected: None,
			follow_new: true,
			follow_log: true,
			known: 0,
			shown_lines: 0,
			scroll: UniformListScrollHandle::new(),
		}
	}

	fn open(&self, cx: &App) -> bool {
		self.model.read(cx).ui.tasks_open
	}

	fn toggle(&mut self, cx: &mut Context<Self>) {
		self.model.update(cx, |model, _| {
			model.ui.tasks_open = !model.ui.tasks_open;
			model.save_ui();
		});
		cx.notify();
	}

	fn bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let model = self.model.read(cx);
		let theme = cx.theme();
		let running = model.running();
		let status = if running > 0 {
			t!("tasks.running", n = running).into_owned()
		} else if model.tasks.is_empty() {
			t!("tasks.none").into_owned()
		} else {
			t!("tasks.count", n = model.tasks.len()).into_owned()
		};
		let open = self.open(cx);
		h_flex()
			.id("tasks-bar")
			.h(px(30.))
			.px_3()
			.gap_2()
			.flex_shrink_0()
			.border_t_1()
			.border_color(theme.border)
			.bg(theme.sidebar)
			.text_sm()
			.text_color(theme.muted_foreground)
			.cursor_pointer()
			.on_click(cx.listener(|this, _, _, cx| this.toggle(cx)))
			.child(Icon::from(AppIcon::Tasks).small())
			.child(t!("tasks.title").into_owned())
			.child(div().child(status))
			.child(div().flex_1())
			.when(open && !model.tasks.is_empty(), |this| {
				this.child(
					Button::new("tasks-clear")
						.label(t!("tasks.clear"))
						.ghost()
						.xsmall()
						.disabled(model.tasks.iter().all(|t| !t.snapshot.state.is_finished()))
						.on_click(cx.listener(|this, _, _, cx| {
							cx.stop_propagation();
							this.model.update(cx, |m, cx| m.clear_finished(cx));
						})),
				)
			})
			.child(
				Icon::new(if open {
					IconName::ChevronDown
				} else {
					IconName::ChevronUp
				})
				.small(),
			)
	}

	fn list(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = cx.theme().clone();
		let rows: Vec<_> = self
			.model
			.read(cx)
			.tasks
			.iter()
			.rev()
			.map(|task| {
				let snapshot = &task.snapshot;
				let id = snapshot.id;
				let (color, label) = state_look(snapshot.state, cx);
				let selected = self.selected == Some(id);
				v_flex()
					.id(("task", id as usize))
					.px_3()
					.py_1p5()
					.gap_1()
					.cursor_pointer()
					.border_b_1()
					.border_color(theme.border.opacity(0.5))
					.when(selected, |row| row.bg(theme.list_active))
					.hover(|row| row.bg(theme.list_hover))
					.on_click(cx.listener(move |this, _, _, cx| {
						this.selected = Some(id);
						this.follow_new = false;
						this.follow_log = true;
						cx.notify();
					}))
					.child(
						h_flex()
							.gap_2()
							.child(div().size_2().flex_shrink_0().rounded_full().bg(color))
							.child(
								div()
									.flex_1()
									.min_w_0()
									.truncate()
									.child(snapshot.title.clone()),
							)
							.child(div().text_xs().text_color(color).child(label)),
					)
					.when(snapshot.state == TaskState::Running, |row| {
						row.child(match snapshot.progress {
							Some(p) => {
								Progress::new(("task-progress", id as usize)).value(p * 100.)
							}
							None => Progress::new(("task-progress", id as usize)).loading(true),
						})
					})
			})
			.collect();
		v_flex()
			.id("task-list")
			.w(px(320.))
			.flex_shrink_0()
			.h_full()
			.overflow_y_scroll()
			.border_r_1()
			.border_color(theme.border)
			.children(rows)
	}

	fn log(&mut self, cx: &mut Context<Self>) -> AnyElement {
		let model = self.model.read(cx);
		let Some(task) = self.selected.and_then(|id| model.task(id)) else {
			return div()
				.flex_1()
				.flex()
				.items_center()
				.justify_center()
				.text_color(cx.theme().muted_foreground)
				.child(t!("tasks.select").into_owned())
				.into_any_element();
		};
		let id = task.snapshot.id;
		let running = task.snapshot.state == TaskState::Running;
		let lines = task.log.len();
		let outputs = task.snapshot.outputs.clone();
		let log_text: String = task
			.log
			.iter()
			.map(|line| line.text.as_ref())
			.collect::<Vec<_>>()
			.join("\n");
		if self.follow_log && lines != self.shown_lines && lines > 0 {
			self.scroll
				.scroll_to_item(lines - 1, ScrollStrategy::Bottom);
		}
		self.shown_lines = lines;

		let toolbar = h_flex()
			.px_3()
			.py_1()
			.gap_2()
			.border_b_1()
			.border_color(cx.theme().border)
			.child(
				Button::new("log-follow")
					.icon(Icon::new(IconName::ArrowDown))
					.ghost()
					.xsmall()
					.selected(self.follow_log)
					.tooltip(t!("tasks.follow"))
					.on_click(cx.listener(|this, _, _, cx| {
						this.follow_log = !this.follow_log;
						cx.notify();
					})),
			)
			.child(
				Button::new("log-copy")
					.icon(Icon::new(IconName::Copy))
					.label(t!("tasks.copy"))
					.ghost()
					.xsmall()
					.on_click(move |_, _, cx| {
						cx.write_to_clipboard(ClipboardItem::new_string(log_text.clone()));
					}),
			)
			.when_some(outputs.first().cloned(), |bar, output| {
				bar.child(
					Button::new("log-output")
						.icon(Icon::new(IconName::FolderOpen))
						.label(t!("tasks.show_output"))
						.ghost()
						.xsmall()
						.on_click(move |_, _, _| crate::widgets::open_folder_of(&output)),
				)
			})
			.child(div().flex_1())
			.when(running, |bar| {
				bar.child(
					Button::new("log-cancel")
						.icon(Icon::new(IconName::Close))
						.label(t!("common.cancel"))
						.danger()
						.xsmall()
						.on_click(cx.listener(move |this, _, _, cx| {
							this.model.read(cx).send(Command::Cancel(id));
						})),
				)
			});

		let list = uniform_list("task-log", lines, cx.processor(Self::render_lines))
			.track_scroll(&self.scroll)
			.flex_1()
			.px_3()
			.py_1();
		v_flex()
			.flex_1()
			.min_w_0()
			.h_full()
			.child(toolbar)
			.child(list)
			.into_any_element()
	}

	fn render_lines(
		&mut self,
		range: Range<usize>,
		_: &mut Window,
		cx: &mut Context<Self>,
	) -> Vec<AnyElement> {
		let theme = cx.theme().clone();
		let model = self.model.read(cx);
		let editor = model.settings.editor.clone();
		let Some(task) = self.selected.and_then(|id| model.task(id)) else {
			return Vec::new();
		};
		range
			.filter_map(|ix| task.log.get(ix).map(|line| (ix, line)))
			.map(|(ix, line)| render_line(ix, line, &editor, &theme, cx))
			.collect()
	}
}

fn render_line(
	ix: usize,
	line: &LogLine,
	editor: &str,
	theme: &gpui_kit::component::Theme,
	cx: &App,
) -> AnyElement {
	let color = match line.severity {
		Severity::Error => theme.danger,
		Severity::Warning => theme.warning,
		Severity::Normal => theme.foreground,
	};
	let row = h_flex()
		.id(("log-line", ix))
		.h(px(LINE_HEIGHT))
		.mono(cx)
		.text_sm()
		.text_color(color)
		.whitespace_nowrap();
	let Some(link) = line.link.clone() else {
		return row.child(line.text.clone()).into_any_element();
	};
	let text = line.text.as_ref();
	let before = text[..link.range.start].to_owned();
	let linked = text[link.range.clone()].to_owned();
	let after = text[link.range.end..].to_owned();
	let editor = editor.to_owned();
	row.child(before)
		.child(
			div()
				.id(("log-link", ix))
				.text_color(if line.severity == Severity::Normal {
					theme.link
				} else {
					color
				})
				.underline()
				.cursor_pointer()
				.on_click(move |_, _, _| crate::widgets::open_link(&link, &editor))
				.child(linked),
		)
		.child(after)
		.into_any_element()
}

fn state_look(state: TaskState, cx: &App) -> (Hsla, String) {
	let theme = cx.theme();
	match state {
		TaskState::Running => (theme.primary, t!("tasks.state_running").into_owned()),
		TaskState::Succeeded => (theme.success, t!("tasks.state_done").into_owned()),
		TaskState::Failed => (theme.danger, t!("tasks.state_failed").into_owned()),
		TaskState::Cancelled => (
			theme.muted_foreground,
			t!("tasks.state_cancelled").into_owned(),
		),
	}
}

impl Render for TasksPanel {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let open = self.open(cx);
		let mut panel = v_flex().flex_shrink_0().child(self.bar(cx));
		if open {
			let body = h_flex()
				.h(px(PANEL_HEIGHT))
				.border_t_1()
				.border_color(cx.theme().border)
				.child(self.list(cx))
				.child(self.log(cx));
			panel = panel.child(body);
		}
		panel
	}
}
