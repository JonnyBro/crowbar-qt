use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::table::{DataTable, TableEvent, TableState};
use gpui_kit::component::{
	ActiveTheme, Disableable, Icon, IconName, Sizable, WindowExt, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::*;
use prybar_core::engine::{CompileOutput, TaskKind, TaskState};
use prybar_core::watch::Watcher;
use prybar_core::{Command, launcher, studiomdl};
use rust_i18n::t;

use crate::model::Model;
use crate::ui_state::OutputMode;
use crate::widgets::{
	self, PickOptions, browse_button, choice, field, hint, page_header, path_row,
};

use super::compile_table::CompileTable;

pub struct CompileView {
	model: Entity<Model>,
	table: Entity<TableState<CompileTable>>,
	/// The file clicked last; the one actions use when none is checked.
	current: Option<PathBuf>,
	subfolder: Entity<InputState>,
	folder: Entity<InputState>,
	extra: Entity<InputState>,
	watcher: Arc<Mutex<Watcher>>,
	/// Polls the watcher while watching is on.
	watching: Option<Task<()>>,
	_subscriptions: Vec<Subscription>,
}

/// How often the QCs' files are looked at; a change is built on the second look.
const WATCH_INTERVAL: Duration = Duration::from_millis(500);

/// Why a compile could not start.
enum Blocked {
	NoFolder,
	NoGame,
}

impl CompileView {
	pub fn new(model: Entity<Model>, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let (table, widths) =
			super::columns::new_table(CompileTable::new(model.clone()), &model, window, cx);
		let subscriptions = vec![
			widths,
			cx.observe(&model, |this, model, cx| {
				let model = model.read(cx);
				// A cancelled compile stops the rest of the queue.
				let cancelled = model.tasks.iter().any(|t| {
					t.snapshot.kind == TaskKind::Compile && t.snapshot.state == TaskState::Cancelled
				});
				let idle = model.running() == 0;
				let started: Vec<PathBuf> = this
					.table
					.read(cx)
					.delegate()
					.queued
					.iter()
					.filter(|f| {
						model
							.task_for(f)
							.is_some_and(|t| !t.snapshot.state.is_finished())
					})
					.cloned()
					.collect();
				this.table.update(cx, |table, cx| {
					let queued = &mut table.delegate_mut().queued;
					for file in started {
						queued.remove(&file);
					}
					if cancelled && idle {
						queued.clear();
					}
					cx.notify();
				});
				cx.notify();
			}),
			cx.observe(&table, |this, table, cx| {
				// Sorting moves rows; the highlight follows the current file, not its old row.
				if let Some(current) = &this.current {
					let index = table.read(cx).delegate().row_of(current);
					if let Some(index) = index
						&& table.read(cx).selected_row() != Some(index)
					{
						table.update(cx, |table, cx| table.set_selected_row(index, cx));
					}
				}
				cx.notify();
			}),
			cx.subscribe_in(
				&table,
				window,
				|this, table, event: &TableEvent, window, cx| {
					let file = |row: &usize| table.read(cx).delegate().file(*row).cloned();
					match event {
						TableEvent::SelectRow(row) => {
							this.current = file(row);
							cx.notify();
						}
						TableEvent::DoubleClickedRow(row) => {
							if let Some(file) = file(row) {
								this.view_compiled(&file, window, cx);
							}
						}
						_ => {}
					}
				},
			),
		];
		let prefs = model.read(cx).ui.compile.clone();
		let mut view = Self {
			subfolder: widgets::new_input(&prefs.subfolder, "compiled".into(), window, cx),
			folder: widgets::new_input(
				"",
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
			table,
			current: None,
			watcher: Arc::default(),
			watching: None,
			_subscriptions: subscriptions,
		};
		if prefs.watch {
			view.watching = Some(view.watch(window, cx));
		}
		view
	}

	fn set_watch(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
		self.model.update(cx, |m, _| {
			m.ui.compile.watch = on;
			m.save_ui();
		});
		self.watching = on.then(|| self.watch(window, cx));
		cx.notify();
	}

	fn watch(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Task<()> {
		// Starting fresh: what is on disk now counts as built.
		*self.watcher.lock().unwrap() = Watcher::default();
		let watcher = self.watcher.clone();
		cx.spawn_in(window, async move |this, cx| {
			loop {
				cx.background_executor().timer(WATCH_INTERVAL).await;
				let Ok(files) =
					this.update(cx, |this, cx| this.table.read(cx).delegate().files.clone())
				else {
					return;
				};
				let poll = watcher.clone();
				let changed = cx
					.background_executor()
					.spawn(async move { poll.lock().unwrap().poll(&files) })
					.await;
				if changed.is_empty() {
					continue;
				}
				if this
					.update(cx, |this, cx| this.rebuild(changed, cx))
					.is_err()
				{
					return;
				}
			}
		})
	}

	/// Changed QCs that are not compiling already; the rest are picked up on a later look.
	fn rebuild(&mut self, changed: Vec<PathBuf>, cx: &mut Context<Self>) {
		let model = self.model.read(cx);
		let idle: Vec<PathBuf> = changed
			.into_iter()
			.filter(|qc| {
				model
					.task_for(qc)
					.is_none_or(|t| t.snapshot.state.is_finished())
			})
			.collect();
		if idle.is_empty() {
			return;
		}
		// A missing game or folder would come up on every look; the next manual compile says why.
		if self.send(idle.clone(), false, cx).is_err() {
			let mut watcher = self.watcher.lock().unwrap();
			for qc in &idle {
				watcher.built(qc);
			}
		}
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
		self.table.update(cx, |table, cx| {
			table.delegate_mut().add(found, cx);
			cx.notify();
		});
		cx.notify();
	}

	/// The checked files, or the current one when none is checked.
	fn picked(&self, cx: &App) -> Vec<PathBuf> {
		self.table.read(cx).delegate().picked(self.current.as_ref())
	}

	fn remove_selected(&mut self, cx: &mut Context<Self>) {
		let gone: BTreeSet<PathBuf> = self.picked(cx).into_iter().collect();
		if self.current.as_ref().is_some_and(|c| gone.contains(c)) {
			self.current = None;
		}
		self.table.update(cx, |table, cx| {
			table.delegate_mut().remove(&gone, cx);
			table.clear_selection(cx);
			cx.notify();
		});
		cx.notify();
	}

	fn clear(&mut self, cx: &mut Context<Self>) {
		self.current = None;
		let all: BTreeSet<PathBuf> = self
			.table
			.read(cx)
			.delegate()
			.files
			.iter()
			.cloned()
			.collect();
		self.table.update(cx, |table, cx| {
			table.delegate_mut().remove(&all, cx);
			table.clear_selection(cx);
			cx.notify();
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
		let extra = widgets::text(&self.extra, cx);
		let folder_path = widgets::path(&self.folder, cx);
		self.model.update(cx, |m, _| {
			let prefs = &mut m.ui.compile;
			prefs.subfolder = subfolder.clone();
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
		let files = if selected_only {
			self.picked(cx)
		} else {
			self.table.read(cx).delegate().files.clone()
		};
		if files.is_empty() {
			return;
		}
		let view_after = self.model.read(cx).ui.compile.view_after;
		let warning = match self.send(files, view_after, cx) {
			Ok(()) => return,
			Err(Blocked::NoFolder) => t!("compile.need_folder"),
			Err(Blocked::NoGame) => t!("game.none"),
		};
		window.push_notification(Notification::warning(warning.into_owned()), cx);
	}

	fn send(
		&mut self,
		files: Vec<PathBuf>,
		view_after: bool,
		cx: &mut Context<Self>,
	) -> Result<(), Blocked> {
		let output = self.store(cx).ok_or(Blocked::NoFolder)?;
		let model = self.model.read(cx);
		let target = model.target.clone().ok_or(Blocked::NoGame)?;
		let prefs = &model.ui.compile;
		let options = studiomdl::Options {
			no_p4: prefs.no_p4,
			verbose: prefs.verbose,
			extra: widgets::split_words(&prefs.extra),
		};
		{
			let mut watcher = self.watcher.lock().unwrap();
			for qc in &files {
				watcher.built(qc);
			}
		}
		let queued = files.clone();
		model.send(Command::Compile {
			target,
			files,
			options,
			output,
			view_after,
		});
		self.table.update(cx, |table, cx| {
			table.delegate_mut().queued.extend(queued);
			cx.notify();
		});
		cx.notify();
		Ok(())
	}

	fn cancel(&mut self, cx: &mut Context<Self>) {
		let model = self.model.read(cx);
		for task in &model.tasks {
			if task.snapshot.kind == TaskKind::Compile && task.snapshot.state == TaskState::Running
			{
				model.send(Command::Cancel(task.snapshot.id));
			}
		}
		self.table.update(cx, |table, cx| {
			table.delegate_mut().queued.clear();
			cx.notify();
		});
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
					Notification::warning(t!("compile.no_compiled_model").into_owned()),
					cx,
				);
			}
		}
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
		let delegate = self.table.read(cx).delegate();
		let count = delegate.files.len();
		let queued = !delegate.queued.is_empty();
		let picked = self.picked(cx).len();
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
					.disabled(picked == 0)
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
			DataTable::new(&self.table)
				.header_row_height(px(32.))
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

		let selected_file = self.current.clone();
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
			.child(v_flex().flex_1().min_h_0().child(list))
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
			.child(field(
				String::new(),
				h_flex()
					.gap_2()
					.child(
						Switch::new("watch")
							.label(t!("compile.watch").into_owned())
							.checked(prefs.watch)
							.on_click(cx.listener(|this, checked: &bool, window, cx| {
								this.set_watch(*checked, window, cx);
							})),
					)
					.child(hint(t!("compile.watch_hint"), cx)),
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
							.disabled(picked == 0 || compiling)
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
					.when(compiling || queued, |this| {
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
