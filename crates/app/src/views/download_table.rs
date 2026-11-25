//! The workshop items found for download, as a sortable table.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::table::{Column, ColumnSort, TableDelegate, TableState};
use gpui_kit::prelude::*;
use gpui_kit::*;
use prybar_core::engine::FoundItem;
use rust_i18n::t;

use crate::theme::Mono;
use crate::widgets::{format_date, format_size, missing_last};

pub const ROW_HEIGHT: f32 = 36.;
const PREVIEW_SIZE: f32 = 28.;

const PREVIEW: usize = 0;
const TITLE: usize = 1;
const SIZE: usize = 2;
const UPDATED: usize = 3;
const SOURCE: usize = 4;
const COLLECTION: usize = 5;
const ID: usize = 6;

/// Where a download would get the item, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
	Cache,
	Direct,
	SteamCmd,
	Unavailable,
}

pub struct Row {
	pub found: FoundItem,
	pub source: Source,
}

impl Row {
	fn size(&self) -> Option<u64> {
		Some(self.found.details.file_size).filter(|s| *s > 0)
	}

	fn updated(&self) -> Option<i64> {
		Some(self.found.details.updated).filter(|t| *t > 0)
	}
}

#[derive(Default)]
pub struct DownloadTable {
	pub rows: Vec<Row>,
	pub previews: HashMap<u64, Arc<Image>>,
	order: Vec<usize>,
	sort: Option<(usize, ColumnSort)>,
}

impl DownloadTable {
	pub fn set_rows(&mut self, rows: Vec<Row>) {
		self.rows = rows;
		self.order = (0..self.rows.len()).collect();
		self.apply_sort();
	}

	pub fn row(&self, row_ix: usize) -> Option<&Row> {
		self.rows.get(*self.order.get(row_ix)?)
	}

	fn apply_sort(&mut self) {
		let (col, sort) = self.sort.unwrap_or((TITLE, ColumnSort::Default));
		let rows = &self.rows;
		self.order.sort_by(|&a, &b| {
			let (ra, rb) = (&rows[a], &rows[b]);
			let ordering = match (sort, col) {
				(ColumnSort::Default, _) => return a.cmp(&b),
				(_, TITLE) => ra
					.found
					.details
					.title
					.to_lowercase()
					.cmp(&rb.found.details.title.to_lowercase()),
				(_, SIZE) => {
					return missing_last(ra.size(), rb.size(), sort == ColumnSort::Descending);
				}
				(_, UPDATED) => {
					return missing_last(ra.updated(), rb.updated(), sort == ColumnSort::Descending);
				}
				(_, SOURCE) => ra.source.cmp(&rb.source),
				(_, COLLECTION) => {
					return missing_last(
						ra.found.collection,
						rb.found.collection,
						sort == ColumnSort::Descending,
					);
				}
				(_, ID) => ra.found.details.id.cmp(&rb.found.details.id),
				_ => Ordering::Equal,
			};
			if sort == ColumnSort::Descending {
				ordering.reverse()
			} else {
				ordering
			}
		});
	}
}

fn source_look(source: Source, cx: &App) -> (String, Hsla) {
	let theme = cx.theme();
	match source {
		Source::Cache => (t!("dl.from_cache").into_owned(), theme.success),
		Source::Direct => (t!("dl.direct").into_owned(), theme.success),
		Source::SteamCmd => (t!("dl.steamcmd").into_owned(), theme.foreground),
		Source::Unavailable => (t!("dl.unavailable").into_owned(), theme.warning),
	}
}

impl TableDelegate for DownloadTable {
	fn columns_count(&self, _: &App) -> usize {
		7
	}

	fn rows_count(&self, _: &App) -> usize {
		self.order.len()
	}

	fn column(&self, col_ix: usize, _: &App) -> Column {
		match col_ix {
			PREVIEW => Column::new("preview", "").width(px(48.)).resizable(false),
			TITLE => Column::new("title", t!("dl.col_title"))
				.width(px(380.))
				.min_width(px(120.))
				.sortable(),
			SIZE => Column::new("size", t!("dl.col_size"))
				.width(px(100.))
				.text_right()
				.sortable(),
			UPDATED => Column::new("updated", t!("dl.col_updated"))
				.width(px(110.))
				.sortable(),
			SOURCE => Column::new("source", t!("dl.col_source"))
				.width(px(220.))
				.sortable(),
			COLLECTION => Column::new("collection", t!("dl.col_collection"))
				.width(px(130.))
				.sortable(),
			_ => Column::new("id", t!("dl.col_id"))
				.width(px(120.))
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
		let theme = cx.theme().clone();
		let Some(row) = self.row(row_ix) else {
			return div().into_any_element();
		};
		let details = &row.found.details;
		let muted = theme.muted_foreground;
		match col_ix {
			PREVIEW => match self.previews.get(&details.id) {
				Some(image) => img(image.clone())
					.size(px(PREVIEW_SIZE))
					.rounded(theme.radius)
					.into_any_element(),
				None => div()
					.size(px(PREVIEW_SIZE))
					.rounded(theme.radius)
					.bg(theme.muted)
					.into_any_element(),
			},
			TITLE => match &details.error {
				Some(error) => div()
					.truncate()
					.text_sm()
					.text_color(theme.danger)
					.child(error.clone())
					.into_any_element(),
				None => div()
					.truncate()
					.child(details.title.clone())
					.into_any_element(),
			},
			SIZE => div()
				.w_full()
				.text_right()
				.whitespace_nowrap()
				.child(row.size().map(format_size).unwrap_or_default())
				.into_any_element(),
			UPDATED => div()
				.text_sm()
				.text_color(muted)
				.whitespace_nowrap()
				.child(row.updated().map(format_date).unwrap_or_default())
				.into_any_element(),
			SOURCE if details.error.is_none() => {
				let (text, color) = source_look(row.source, cx);
				div()
					.truncate()
					.text_sm()
					.text_color(color)
					.child(text)
					.into_any_element()
			}
			COLLECTION => div()
				.mono(cx)
				.text_sm()
				.text_color(muted)
				.child(
					row.found
						.collection
						.map(|c| c.to_string())
						.unwrap_or_default(),
				)
				.into_any_element(),
			ID => div()
				.mono(cx)
				.text_sm()
				.text_color(muted)
				.child(details.id.to_string())
				.into_any_element(),
			_ => div().into_any_element(),
		}
	}

	fn cell_text(&self, row_ix: usize, col_ix: usize, cx: &App) -> String {
		let Some(row) = self.row(row_ix) else {
			return String::new();
		};
		let details = &row.found.details;
		match col_ix {
			TITLE => details
				.error
				.clone()
				.unwrap_or_else(|| details.title.clone()),
			SIZE => row.size().map(format_size).unwrap_or_default(),
			UPDATED => row.updated().map(format_date).unwrap_or_default(),
			SOURCE => source_look(row.source, cx).0,
			COLLECTION => row
				.found
				.collection
				.map(|c| c.to_string())
				.unwrap_or_default(),
			ID => details.id.to_string(),
			_ => String::new(),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use prybar_core::workshop::ItemDetails;
	// gpui_kit's prelude has a `test` attribute of its own.
	use ::core::prelude::v1::test;

	fn row(id: u64, title: &str, size: u64, source: Source) -> Row {
		Row {
			found: FoundItem {
				details: ItemDetails {
					id,
					title: title.into(),
					file_size: size,
					..ItemDetails::default()
				},
				collection: None,
			},
			source,
		}
	}

	fn ids(table: &DownloadTable) -> Vec<u64> {
		(0..table.order.len())
			.filter_map(|i| table.row(i).map(|r| r.found.details.id))
			.collect()
	}

	#[test]
	fn sorting() {
		let mut table = DownloadTable::default();
		table.set_rows(vec![
			row(1, "b", 0, Source::SteamCmd),
			row(2, "a", 10, Source::Cache),
			row(3, "c", 5, Source::Direct),
		]);
		assert_eq!(ids(&table), [1, 2, 3]);
		table.sort = Some((SIZE, ColumnSort::Descending));
		table.apply_sort();
		assert_eq!(ids(&table), [2, 3, 1]);
		table.sort = Some((SOURCE, ColumnSort::Ascending));
		table.apply_sort();
		assert_eq!(ids(&table), [2, 3, 1]);
		table.sort = Some((TITLE, ColumnSort::Ascending));
		table.apply_sort();
		assert_eq!(ids(&table), [2, 1, 3]);
	}
}
