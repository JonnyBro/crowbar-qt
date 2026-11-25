//! The files of the open package and the matches of a search over the game's
//! packages, as sortable tables.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::table::{Column, ColumnSort, TableDelegate, TableState};
use gpui_kit::component::{ActiveTheme, h_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;
use prybar_core::package::Package;
use rust_i18n::t;

use crate::theme::Mono;
use crate::widgets::{file_name, format_size};

const CHECK: usize = 0;
const PATH: usize = 1;
const SIZE: usize = 2;

#[derive(Default)]
pub struct EntriesTable {
	pub package: Option<Arc<Package>>,
	/// Checked entries, as indexes into the package's entries.
	pub checked: BTreeSet<usize>,
	/// Entries the filter lets through, in display order.
	rows: Vec<usize>,
	sort: Option<(usize, ColumnSort)>,
}

impl EntriesTable {
	pub fn set_package(&mut self, package: Option<Arc<Package>>) {
		self.package = package;
		self.checked.clear();
		self.rows.clear();
	}

	/// Shows the entries whose path contains `needle` (lower case, forward slashes).
	pub fn set_filter(&mut self, needle: &str) {
		self.rows = match &self.package {
			Some(package) => package
				.entries
				.iter()
				.enumerate()
				.filter(|(_, e)| needle.is_empty() || e.path.to_lowercase().contains(needle))
				.map(|(i, _)| i)
				.collect(),
			None => Vec::new(),
		};
		self.apply_sort();
	}

	pub fn entry_index(&self, row_ix: usize) -> Option<usize> {
		self.rows.get(row_ix).copied()
	}

	pub fn row_of(&self, index: usize) -> Option<usize> {
		self.rows.iter().position(|&i| i == index)
	}

	fn apply_sort(&mut self) {
		let Some(package) = &self.package else {
			return;
		};
		let (col, sort) = self.sort.unwrap_or((PATH, ColumnSort::Default));
		let entries = &package.entries;
		self.rows.sort_by(|&a, &b| {
			let ordering = match (sort, col) {
				(ColumnSort::Default, _) => return a.cmp(&b),
				(_, SIZE) => entries[a].size.cmp(&entries[b].size),
				_ => entries[a]
					.path
					.to_lowercase()
					.cmp(&entries[b].path.to_lowercase()),
			};
			if sort == ColumnSort::Descending {
				ordering.reverse()
			} else {
				ordering
			}
		});
	}
}

impl TableDelegate for EntriesTable {
	fn columns_count(&self, _: &App) -> usize {
		3
	}

	fn rows_count(&self, _: &App) -> usize {
		self.rows.len()
	}

	fn column(&self, col_ix: usize, _: &App) -> Column {
		match col_ix {
			CHECK => Column::new("check", "").width(px(40.)).resizable(false),
			PATH => Column::new("path", t!("packages.col_path"))
				.width(px(620.))
				.min_width(px(160.))
				.sortable(),
			_ => Column::new("size", t!("packages.col_size"))
				.width(px(100.))
				.text_right()
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
		self.apply_sort();
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
		let all = !self.rows.is_empty() && self.rows.iter().all(|i| self.checked.contains(i));
		h_flex()
			.child(
				Checkbox::new("entry-check-all")
					.checked(all)
					.on_click(cx.listener(|table, checked: &bool, _, cx| {
						let delegate = table.delegate_mut();
						if *checked {
							delegate.checked.extend(delegate.rows.iter().copied());
						} else {
							for row in &delegate.rows {
								delegate.checked.remove(row);
							}
						}
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
		let Some(index) = self.entry_index(row_ix) else {
			return div().into_any_element();
		};
		let Some(entry) = self.package.as_ref().and_then(|p| p.entries.get(index)) else {
			return div().into_any_element();
		};
		match col_ix {
			CHECK => Checkbox::new(("entry-check", row_ix))
				.checked(self.checked.contains(&index))
				.on_click(cx.listener(move |table, checked: &bool, _, cx| {
					let checked_entries = &mut table.delegate_mut().checked;
					if *checked {
						checked_entries.insert(index);
					} else {
						checked_entries.remove(&index);
					}
					cx.stop_propagation();
					cx.notify();
				}))
				.into_any_element(),
			PATH => div()
				.truncate()
				.mono(cx)
				.text_sm()
				.child(entry.path.clone())
				.into_any_element(),
			_ => div()
				.w_full()
				.text_right()
				.text_sm()
				.text_color(cx.theme().muted_foreground)
				.whitespace_nowrap()
				.child(format_size(entry.size))
				.into_any_element(),
		}
	}

	fn cell_text(&self, row_ix: usize, col_ix: usize, _: &App) -> String {
		let entry = self
			.entry_index(row_ix)
			.and_then(|i| self.package.as_ref()?.entries.get(i));
		match (col_ix, entry) {
			(PATH, Some(entry)) => entry.path.clone(),
			(SIZE, Some(entry)) => format_size(entry.size),
			_ => String::new(),
		}
	}
}

const R_PACKAGE: usize = 0;
const R_PATH: usize = 1;
const R_SIZE: usize = 2;

/// A match of a search over the game's packages.
#[derive(Debug, Clone)]
pub struct Match {
	pub package: PathBuf,
	pub path: String,
	pub size: u64,
}

#[derive(Default)]
pub struct ResultsTable {
	pub matches: Vec<Match>,
	rows: Vec<usize>,
	sort: Option<(usize, ColumnSort)>,
}

impl ResultsTable {
	pub fn clear(&mut self) {
		self.matches.clear();
		self.rows.clear();
	}

	pub fn extend(&mut self, matches: impl IntoIterator<Item = Match>) {
		self.matches.extend(matches);
		self.rows = (0..self.matches.len()).collect();
		self.apply_sort();
	}

	pub fn get(&self, row_ix: usize) -> Option<&Match> {
		self.matches.get(*self.rows.get(row_ix)?)
	}

	fn apply_sort(&mut self) {
		let (col, sort) = self.sort.unwrap_or((R_PATH, ColumnSort::Default));
		let matches = &self.matches;
		self.rows.sort_by(|&a, &b| {
			let (ma, mb) = (&matches[a], &matches[b]);
			let ordering = match (sort, col) {
				(ColumnSort::Default, _) => return a.cmp(&b),
				(_, R_PACKAGE) => file_name(&ma.package)
					.to_lowercase()
					.cmp(&file_name(&mb.package).to_lowercase()),
				(_, R_SIZE) => ma.size.cmp(&mb.size),
				_ => ma.path.to_lowercase().cmp(&mb.path.to_lowercase()),
			};
			if sort == ColumnSort::Descending {
				ordering.reverse()
			} else {
				ordering
			}
		});
	}
}

impl TableDelegate for ResultsTable {
	fn columns_count(&self, _: &App) -> usize {
		3
	}

	fn rows_count(&self, _: &App) -> usize {
		self.rows.len()
	}

	fn column(&self, col_ix: usize, _: &App) -> Column {
		match col_ix {
			R_PACKAGE => Column::new("package", t!("packages.col_package"))
				.width(px(240.))
				.sortable(),
			R_PATH => Column::new("path", t!("packages.col_path"))
				.width(px(520.))
				.min_width(px(160.))
				.sortable(),
			_ => Column::new("size", t!("packages.col_size"))
				.width(px(100.))
				.text_right()
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
		self.apply_sort();
		cx.notify();
	}

	fn render_td(
		&mut self,
		row_ix: usize,
		col_ix: usize,
		_: &mut Window,
		cx: &mut Context<TableState<Self>>,
	) -> impl IntoElement {
		let Some(found) = self.get(row_ix) else {
			return div().into_any_element();
		};
		let muted = cx.theme().muted_foreground;
		match col_ix {
			R_PACKAGE => div()
				.truncate()
				.text_sm()
				.text_color(muted)
				.child(file_name(&found.package))
				.into_any_element(),
			R_PATH => div()
				.truncate()
				.mono(cx)
				.text_sm()
				.child(found.path.clone())
				.into_any_element(),
			_ => div()
				.w_full()
				.text_right()
				.text_sm()
				.text_color(muted)
				.whitespace_nowrap()
				.child(format_size(found.size))
				.into_any_element(),
		}
	}

	fn cell_text(&self, row_ix: usize, col_ix: usize, _: &App) -> String {
		let Some(found) = self.get(row_ix) else {
			return String::new();
		};
		match col_ix {
			R_PACKAGE => found.package.display().to_string(),
			R_PATH => found.path.clone(),
			_ => format_size(found.size),
		}
	}
}
