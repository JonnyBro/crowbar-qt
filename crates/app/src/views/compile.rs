use std::collections::BTreeSet;
use std::ops::Range;
use std::path::{Path, PathBuf};

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{
	ActiveTheme, Disableable, Icon, IconName, Sizable, WindowExt, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::*;
use prybar_core::engine::{CompileOutput, TaskKind, TaskState};
use prybar_core::{Command, launcher, studiomdl};
use rust_i18n::t;

use crate::model::Model;
use crate::theme::Mono;
use crate::ui_state::OutputMode;
use crate::widgets::{
	self, PickOptions, browse_button, choice, field, hint, page_header, path_row,
};

const ROW_HEIGHT: f32 = 28.;

pub struct CompileView {
	model: Entity<Model>,
	selected: BTreeSet<usize>,
	/// Files sent to the engine whose task hasn't started yet.
	queued: BTreeSet<PathBuf>,
	subfolder: Entity<InputState>,
	folder: Entity<InputState>,
	extra: Entity<InputState>,
	scroll: UniformListScrollHandle,
}

impl CompileView {
	pub fn new(model: Entity<Model>, window: &mut Window, cx: &mut Context<Self>) -> Self {
		cx.observe(&model, |this, model, cx| {
			let model = model.read(cx);
			this.queued.retain(|file| {
				model
					.task_for(file)
					.is_none_or(|t| t.snapshot.state.is_finished())
			});
			// A cancelled compile stops the rest of the queue.
			let cancelled = model.tasks.iter().any(|t| {
				t.snapshot.kind == TaskKind::Compile && t.snapshot.state == TaskState::Cancelled
			});
			if cancelled && model.running() == 0 {
				this.queued.clear();
			}
			cx.notify();
		})
		.detach();
		let prefs = model.read(cx).ui.compile.clone();
		Self {
			subfolder: widgets::new_input(&prefs.subfolder, "compiled".into(), window, cx),
			folder: widgets::new_input(
				&prefs.folder,
				t!("compile.folder_placeholder").into_owned(),
				window,
				cx,
			),
			extra: widgets::new_input(
				&prefs.extra,
				t!("compile.extra_placeholder").into_owned(),
				window,
				cx,
			),
			model,
			selected: BTreeSet::new(),
			queued: BTreeSet::new(),
			scroll: UniformListScrollHandle::new(),
		}
	}

	fn files<'a>(&self, cx: &'a App) -> &'a [PathBuf] {
		&self.model.read(cx).ui.compile.files
	}

	/// QC files, and every QC file under folders.
	pub fn add_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
		let mut found = Vec::new();
		for path in paths {
			if path.is_dir() {
				collect_qcs(&path, &mut found);
			} else {
				found.push(path);
			}
		}
		self.model.update(cx, |m, _| {
			for file in found {
				if !m.ui.compile.files.contains(&file) {
					m.ui.compile.files.push(file);
				}
			}
			m.save_ui();
		});
		cx.notify();
	}

	fn remove_selected(&mut self, cx: &mut Context<Self>) {
		let selected = std::mem::take(&mut self.selected);
		self.model.update(cx, |m, _| {
			let mut i = 0;
			m.ui.compile.files.retain(|_| {
				let keep = !selected.contains(&i);
				i += 1;
				keep
			});
			m.save_ui();
		});
		cx.notify();
	}

	fn clear(&mut self, cx: &mut Context<Self>) {
		self.selected.clear();
		self.model.update(cx, |m, _| {
			m.ui.compile.files.clear();
			m.save_ui();
		});
		cx.notify();
	}

	fn set_output(&mut self, mode: OutputMode, cx: &mut Context<Self>) {
		self.model.update(cx, |m, _| m.ui.compile.output = mode);
		cx.notify();
	}

	/// Writes the fields into the remembered state; `None` when the output folder is missing.
	fn store(&mut self, cx: &mut Context<Self>) -> Option<CompileOutput> {
		let subfolder = widgets::text(&self.subfolder, cx);
		let folder = widgets::text(&self.folder, cx);
		let extra = widgets::text(&self.extra, cx);
		let folder_path = widgets::path(&self.folder, cx);
		self.model.update(cx, |m, _| {
			let prefs = &mut m.ui.compile;
			prefs.subfolder = subfolder.clone();
			prefs.folder = folder;
			prefs.extra = extra;
			m.save_ui();
			match m.ui.compile.output {
				OutputMode::Beside => Some(CompileOutput::GameModels),
				OutputMode::Subfolder if !subfolder.is_empty() => {
					Some(CompileOutput::Subfolder(subfolder))
				}
				OutputMode::Subfolder => Some(CompileOutput::GameModels),
				OutputMode::Folder => folder_path.map(CompileOutput::Folder),
			}
		})
	}

	fn compile(&mut self, selected_only: bool, window: &mut Window, cx: &mut Context<Self>) {
		let Some(output) = self.store(cx) else {
			window.push_notification(
				Notification::warning(t!("compile.need_folder").into_owned()),
				cx,
			);
			return;
		};
		let model = self.model.read(cx);
		let Some(target) = model.target.clone() else {
			window.push_notification(Notification::warning(t!("game.none").into_owned()), cx);
			return;
		};
		let files: Vec<PathBuf> = model
			.ui
			.compile
			.files
			.iter()
			.enumerate()
			.filter(|(i, _)| !selected_only || self.selected.contains(i))
			.map(|(_, f)| f.clone())
			.collect();
		if files.is_empty() {
			return;
		}
		let prefs = &model.ui.compile;
		let options = studiomdl::Options {
			no_p4: prefs.no_p4,
			verbose: prefs.verbose,
			extra: widgets::split_words(&prefs.extra),
		};
		let view_after = prefs.view_after;
		self.queued.extend(files.iter().cloned());
		model.send(Command::Compile {
			target,
			files,
			options,
			output,
			view_after,
		});
		cx.notify();
	}

	fn cancel(&mut self, cx: &mut Context<Self>) {
		let model = self.model.read(cx);
		for task in &model.tasks {
			if task.snapshot.kind == TaskKind::Compile && task.snapshot.state == TaskState::Running
			{
				model.send(Command::Cancel(task.snapshot.id));
			}
		}
		self.queued.clear();
		cx.notify();
	}

	fn view_compiled(&mut self, qc: &Path, window: &mut Window, cx: &mut Context<Self>) {
		let output = self.store(cx);
		let model = self.model.read(cx);
		let Some(target) = model.target.clone() else {
			return;
		};
		let mdl = studiomdl::read_model_name(qc).and_then(|name| {
			let dir = output.as_ref().and_then(|o| o.dir_for(qc));
			studiomdl::compiled_model(&target.game_dir, dir.as_deref(), &name)
		});
		match mdl {
			Some(mdl) => model.send(Command::View {
				target,
				mdl: Some(mdl),
			}),
			None => {
				window.push_notification(
					Notification::warning(t!("task.no_compiled_model").into_owned()),
					cx,
				);
			}
		}
	}

	fn status(&self, file: &Path, cx: &App) -> (String, Hsla) {
		let theme = cx.theme();
		let task = self.model.read(cx).task_for(file);
		if task.is_some_and(|t| !t.snapshot.state.is_finished()) {
			return (t!("compile.compiling").into_owned(), theme.primary);
		}
		if self.queued.contains(file) {
			return (t!("compile.queued").into_owned(), theme.muted_foreground);
		}
		if !file.is_file() {
			return (t!("compile.missing").into_owned(), theme.danger);
		}
		match task.map(|t| t.snapshot.state) {
			Some(TaskState::Succeeded) => (t!("tasks.state_done").into_owned(), theme.success),
			Some(TaskState::Failed) => (t!("tasks.state_failed").into_owned(), theme.danger),
			Some(TaskState::Cancelled) => (
				t!("tasks.state_cancelled").into_owned(),
				theme.muted_foreground,
			),
			_ => (String::new(), theme.muted_foreground),
		}
	}

	fn render_rows(
		&mut self,
		range: Range<usize>,
		_: &mut Window,
		cx: &mut Context<Self>,
	) -> Vec<AnyElement> {
		let theme = cx.theme().clone();
		let files = self.files(cx).to_vec();
		range
			.filter_map(|ix| files.get(ix).map(|f| (ix, f.clone())))
			.map(|(ix, file)| {
				let (status, color) = self.status(&file, cx);
				let selected = self.selected.contains(&ix);
				let double = file.clone();
				h_flex()
					.id(("qc", ix))
					.h(px(ROW_HEIGHT))
					.px_3()
					.gap_3()
					.border_b_1()
					.border_color(theme.border.opacity(0.5))
					.when(selected, |r| r.bg(theme.list_active))
					.hover(|r| r.bg(theme.list_hover))
					.on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
						if event.click_count() == 2 {
							this.view_compiled(&double, window, cx);
							return;
						}
						if event.modifiers().control || event.modifiers().shift {
							if !this.selected.remove(&ix) {
								this.selected.insert(ix);
							}
						} else {
							this.selected = BTreeSet::from([ix]);
						}
						cx.notify();
					}))
					.child(
						div()
							.flex_1()
							.min_w_0()
							.truncate()
							.mono(cx)
							.text_sm()
							.child(file.display().to_string()),
					)
					.child(div().w(px(120.)).text_sm().text_color(color).child(status))
					.into_any_element()
			})
			.collect()
	}
}

fn collect_qcs(dir: &Path, found: &mut Vec<PathBuf>) {
	let Ok(read) = std::fs::read_dir(dir) else {
		return;
	};
	let mut entries: Vec<PathBuf> = read.flatten().map(|e| e.path()).collect();
	entries.sort();
	for path in entries {
		if path.is_dir() {
			collect_qcs(&path, found);
		} else if path
			.extension()
			.is_some_and(|e| e.eq_ignore_ascii_case("qc"))
		{
			found.push(path);
		}
	}
}

impl Render for CompileView {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let model = self.model.read(cx);
		let prefs = model.ui.compile.clone();
		let count = prefs.files.len();
		let no_studiomdl = model
			.target
			.as_ref()
			.is_some_and(|t| launcher::find_tool(&t.app, "studiomdl").is_none());
		let game_name = model
			.target
			.as_ref()
			.map(|t| t.app.name.clone())
			.unwrap_or_default();
		let compiling = model.tasks.iter().any(|t| {
			t.snapshot.kind == TaskKind::Compile && t.snapshot.state == TaskState::Running
		});
		let theme = cx.theme().clone();

		let list_actions = h_flex()
			.gap_2()
			.child(
				Button::new("qc-add")
					.icon(Icon::new(IconName::Plus))
					.label(t!("compile.add_qc"))
					.small()
					.outline()
					.on_click(cx.listener(|_, _, window, cx| {
						widgets::pick_paths(
							PickOptions {
								folders: false,
								multiple: true,
							},
							window,
							cx,
							|this: &mut Self, paths, _, cx| this.add_paths(paths, cx),
						);
					})),
			)
			.child(
				Button::new("qc-add-folder")
					.icon(Icon::new(IconName::FolderOpen))
					.label(t!("compile.add_folder"))
					.small()
					.outline()
					.on_click(cx.listener(|_, _, window, cx| {
						widgets::pick_paths(
							PickOptions {
								folders: true,
								multiple: true,
							},
							window,
							cx,
							|this: &mut Self, paths, _, cx| this.add_paths(paths, cx),
						);
					})),
			)
			.child(
				Button::new("qc-remove")
					.label(t!("compile.remove"))
					.small()
					.ghost()
					.disabled(self.selected.is_empty())
					.on_click(cx.listener(|this, _, _, cx| this.remove_selected(cx))),
			)
			.child(
				Button::new("qc-clear")
					.label(t!("compile.clear"))
					.small()
					.ghost()
					.disabled(count == 0)
					.on_click(cx.listener(|this, _, _, cx| this.clear(cx))),
			);

		let list: AnyElement = if count == 0 {
			div()
				.flex_1()
				.flex()
				.items_center()
				.justify_center()
				.text_color(theme.muted_foreground)
				.child(t!("compile.empty").into_owned())
				.into_any_element()
		} else {
			uniform_list("qc-files", count, cx.processor(Self::render_rows))
				.track_scroll(&self.scroll)
				.flex_1()
				.into_any_element()
		};

		let output = v_flex()
			.gap_2()
			.child(
				h_flex()
					.gap_1()
					.child(
						choice(
							"out-game",
							t!("compile.out_game"),
							prefs.output == OutputMode::Beside,
						)
						.on_click(
							cx.listener(|this, _, _, cx| this.set_output(OutputMode::Beside, cx)),
						),
					)
					.child(
						choice(
							"out-sub",
							t!("compile.out_subfolder"),
							prefs.output == OutputMode::Subfolder,
						)
						.on_click(
							cx.listener(|this, _, _, cx| {
								this.set_output(OutputMode::Subfolder, cx)
							}),
						),
					)
					.child(
						choice(
							"out-folder",
							t!("compile.out_folder"),
							prefs.output == OutputMode::Folder,
						)
						.on_click(
							cx.listener(|this, _, _, cx| this.set_output(OutputMode::Folder, cx)),
						),
					),
			)
			.when(prefs.output == OutputMode::Subfolder, |this| {
				this.child(Input::new(&self.subfolder).small())
			})
			.when(prefs.output == OutputMode::Folder, |this| {
				let folder = self.folder.clone();
				this.child(path_row(
					&self.folder,
					browse_button("compile-browse").on_click(cx.listener(
						move |_, _, window, cx| {
							widgets::pick_folder_into(folder.clone(), window, cx);
						},
					)),
				))
			});

		let options = h_flex()
			.gap_4()
			.child(
				Checkbox::new("nop4")
					.label(t!("compile.no_p4").into_owned())
					.checked(prefs.no_p4)
					.on_click(cx.listener(|this, checked: &bool, _, cx| {
						let checked = *checked;
						this.model.update(cx, |m, _| m.ui.compile.no_p4 = checked);
						cx.notify();
					})),
			)
			.child(
				Checkbox::new("verbose")
					.label(t!("compile.verbose").into_owned())
					.checked(prefs.verbose)
					.on_click(cx.listener(|this, checked: &bool, _, cx| {
						let checked = *checked;
						this.model.update(cx, |m, _| m.ui.compile.verbose = checked);
						cx.notify();
					})),
			)
			.child(div().flex_1().child(Input::new(&self.extra).small()));

		let selected_file = self
			.selected
			.iter()
			.next()
			.and_then(|i| prefs.files.get(*i).cloned());
		v_flex()
			.size_full()
			.p_5()
			.gap_4()
			.child(page_header(
				t!("page.compile").into_owned(),
				Some(t!("compile.subtitle").into_owned()),
				cx,
			))
			.when(no_studiomdl, |this| {
				this.child(
					div()
						.text_sm()
						.text_color(theme.warning)
						.child(t!("compile.no_studiomdl", name = game_name).into_owned()),
				)
			})
			.child(list_actions)
			.child(
				v_flex()
					.flex_1()
					.min_h_0()
					.border_1()
					.border_color(theme.border)
					.rounded(theme.radius)
					.child(list),
			)
			.child(field(t!("compile.output").into_owned(), output, cx))
			.child(field(t!("compile.options").into_owned(), options, cx))
			.child(field(
				String::new(),
				h_flex()
					.gap_2()
					.child(
						Checkbox::new("view-after")
							.label(t!("compile.view_after").into_owned())
							.checked(prefs.view_after)
							.on_click(cx.listener(|this, checked: &bool, _, cx| {
								let checked = *checked;
								this.model
									.update(cx, |m, _| m.ui.compile.view_after = checked);
								cx.notify();
							})),
					)
					.child(hint(t!("compile.view_after_hint"), cx)),
				cx,
			))
			.child(
				h_flex()
					.gap_2()
					.child(
						Button::new("compile-all")
							.icon(Icon::from(crate::icons::AppIcon::Compile))
							.label(t!("compile.all"))
							.small()
							.primary()
							.disabled(count == 0 || compiling)
							.on_click(
								cx.listener(|this, _, window, cx| this.compile(false, window, cx)),
							),
					)
					.child(
						Button::new("compile-selected")
							.label(t!("compile.selected"))
							.small()
							.outline()
							.disabled(self.selected.is_empty() || compiling)
							.on_click(
								cx.listener(|this, _, window, cx| this.compile(true, window, cx)),
							),
					)
					.child(
						Button::new("compile-view")
							.icon(Icon::from(crate::icons::AppIcon::View))
							.label(t!("compile.open_hlmv"))
							.small()
							.outline()
							.disabled(selected_file.is_none())
							.on_click(cx.listener(move |this, _, window, cx| {
								if let Some(file) = &selected_file {
									this.view_compiled(file, window, cx);
								}
							})),
					)
					.child(div().flex_1())
					.when(compiling || !self.queued.is_empty(), |this| {
						this.child(
							Button::new("compile-cancel")
								.label(t!("common.cancel"))
								.small()
								.danger()
								.on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
						)
					}),
			)
	}
}
