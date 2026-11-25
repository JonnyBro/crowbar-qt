//! The QC files to compile, with checkboxes for picking several and the state
//! of each file's last compile.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::table::{Column, ColumnSort, TableDelegate, TableState};
use gpui_kit::component::{ActiveTheme, h_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;
use prybar_core::engine::TaskState;
use rust_i18n::t;

use crate::model::Model;
use crate::theme::Mono;

const CHECK: usize = 0;
const FILE: usize = 1;
const STATUS: usize = 2;

pub struct CompileTable {
	model: Entity<Model>,
	pub files: Vec<PathBuf>,
	pub checked: BTreeSet<PathBuf>,
	/// Files sent to the engine whose task hasn't started yet.
	pub queued: BTreeSet<PathBuf>,
	/// Indexes into `files`, in display order.
	rows: Vec<usize>,
	sort: Option<(usize, ColumnSort)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Status {
	None,
	Queued,
	Compiling,
	Done,
	Failed,
	Cancelled,
	Missing,
}

impl CompileTable {
	pub fn new(model: Entity<Model>) -> Self {
		Self {
			model,
			files: Vec::new(),
			checked: BTreeSet::new(),
			queued: BTreeSet::new(),
			rows: Vec::new(),
			sort: None,
		}
	}

	pub fn add(&mut self, files: Vec<PathBuf>, cx: &App) {
		for file in files {
			if !self.files.contains(&file) {
				self.files.push(file);
			}
		}
		self.build_rows(cx);
	}

	pub fn remove(&mut self, gone: &BTreeSet<PathBuf>, cx: &App) {
		self.files.retain(|f| !gone.contains(f));
		self.checked.retain(|f| !gone.contains(f));
		self.build_rows(cx);
	}

	pub fn file(&self, row_ix: usize) -> Option<&PathBuf> {
		self.files.get(*self.rows.get(row_ix)?)
	}

	pub fn row_of(&self, file: &Path) -> Option<usize> {
		self.rows.iter().position(|&i| self.files[i] == file)
	}

	fn status(&self, file: &Path, cx: &App) -> Status {
		let task = self.model.read(cx).task_for(file);
		if task.is_some_and(|t| !t.snapshot.state.is_finished()) {
			return Status::Compiling;
		}
		if self.queued.contains(file) {
			return Status::Queued;
		}
		if !file.is_file() {
			return Status::Missing;
		}
		match task.map(|t| t.snapshot.state) {
			Some(TaskState::Succeeded) => Status::Done,
			Some(TaskState::Failed) => Status::Failed,
			Some(TaskState::Cancelled) => Status::Cancelled,
			_ => Status::None,
		}
	}

	fn build_rows(&mut self, cx: &App) {
		let mut rows: Vec<usize> = (0..self.files.len()).collect();
		if let Some((col, sort)) = self.sort.filter(|(_, sort)| *sort != ColumnSort::Default) {
			rows.sort_by(|&a, &b| {
				let (fa, fb) = (&self.files[a], &self.files[b]);
				let ordering = match col {
					STATUS => self.status(fa, cx).cmp(&self.status(fb, cx)),
					_ => fa.cmp(fb),
				};
				if sort == ColumnSort::Descending {
					ordering.reverse()
				} else {
					ordering
				}
			});
		}
		self.rows = rows;
	}
}

fn status_look(status: Status, cx: &App) -> (String, Hsla) {
	let theme = cx.theme();
	match status {
		Status::None => (String::new(), theme.muted_foreground),
		Status::Queued => (t!("compile.queued").into_owned(), theme.muted_foreground),
		Status::Compiling => (t!("compile.compiling").into_owned(), theme.primary),
		Status::Done => (t!("tasks.state_done").into_owned(), theme.success),
		Status::Failed => (t!("tasks.state_failed").into_owned(), theme.danger),
		Status::Cancelled => (
			t!("tasks.state_cancelled").into_owned(),
			theme.muted_foreground,
		),
		Status::Missing => (t!("compile.missing").into_owned(), theme.danger),
	}
}

impl TableDelegate for CompileTable {
	fn columns_count(&self, _: &App) -> usize {
		3
	}

	fn rows_count(&self, _: &App) -> usize {
		self.rows.len()
	}

	fn column(&self, col_ix: usize, _: &App) -> Column {
		match col_ix {
			CHECK => Column::new("check", "").width(px(40.)).resizable(false),
			FILE => Column::new("file", t!("compile.col_file"))
				.width(px(760.))
				.min_width(px(160.))
				.sortable(),
			_ => Column::new("status", t!("compile.col_status"))
				.width(px(140.))
				.sortable(),
		}
	}

	fn perform_sort(
		&mut self,
		col_ix: usize,
		sort: ColumnSort,
		_: &mut Window,
		cx: &mut Context<TableState<Self>>,
	) {
		self.sort = Some((col_ix, sort));
		self.build_rows(cx);
		cx.notify();
	}

	fn render_th(
		&mut self,
		col_ix: usize,
		_: &mut Window,
		cx: &mut Context<TableState<Self>>,
	) -> impl IntoElement {
		if col_ix != CHECK {
			return div()
				.w_full()
				.child(self.column(col_ix, cx).name)
				.into_any_element();
		}
		let all = !self.files.is_empty() && self.checked.len() == self.files.len();
		h_flex()
			.child(
				Checkbox::new("qc-check-all")
					.checked(all)
					.on_click(cx.listener(|table, checked: &bool, _, cx| {
						let delegate = table.delegate_mut();
						delegate.checked = if *checked {
							delegate.files.iter().cloned().collect()
						} else {
							BTreeSet::new()
						};
						cx.stop_propagation();
						cx.notify();
					})),
			)
			.into_any_element()
	}

	fn render_td(
		&mut self,
		row_ix: usize,
		col_ix: usize,
		_: &mut Window,
		cx: &mut Context<TableState<Self>>,
	) -> impl IntoElement {
		let Some(file) = self.file(row_ix).cloned() else {
			return div().into_any_element();
		};
		match col_ix {
			CHECK => {
				let checked = self.checked.contains(&file);
				Checkbox::new(("qc-check", row_ix))
					.checked(checked)
					.on_click(cx.listener(move |table, checked: &bool, _, cx| {
						let checked_files = &mut table.delegate_mut().checked;
						if *checked {
							checked_files.insert(file.clone());
						} else {
							checked_files.remove(&file);
						}
						cx.stop_propagation();
						cx.notify();
					}))
					.into_any_element()
			}
			FILE => div()
				.truncate()
				.mono(cx)
				.text_sm()
				.child(file.display().to_string())
				.into_any_element(),
			_ => {
				let (text, color) = status_look(self.status(&file, cx), cx);
				div()
					.text_sm()
					.text_color(color)
					.child(text)
					.into_any_element()
			}
		}
	}

	fn cell_text(&self, row_ix: usize, col_ix: usize, cx: &App) -> String {
		let Some(file) = self.file(row_ix) else {
			return String::new();
		};
		match col_ix {
			FILE => file.display().to_string(),
			STATUS => status_look(self.status(file, cx), cx).0,
			_ => String::new(),
		}
	}
}

impl CompileTable {
	/// The checked files, or the current one when none is checked.
	pub fn picked(&self, current: Option<&PathBuf>) -> Vec<PathBuf> {
		if self.checked.is_empty() {
			current.into_iter().cloned().collect()
		} else {
			self.files
				.iter()
				.filter(|f| self.checked.contains(*f))
				.cloned()
				.collect()
		}
	}
}
