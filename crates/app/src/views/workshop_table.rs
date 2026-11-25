//! The table of your workshop items: sortable, with columns the user can resize.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::table::{Column, ColumnSort, TableDelegate, TableState};
use gpui_kit::prelude::*;
use gpui_kit::*;
use prybar_core::publisher::WorkshopItem;
use prybar_core::workshop::ItemDetails;
use rust_i18n::t;

use super::columns::{Remembered, SavedWidths};
use crate::theme::Mono;
use crate::widgets::{format_date, format_size, missing_last};

pub const ROW_HEIGHT: f32 = 36.;
const PREVIEW_SIZE: f32 = 28.;

const PREVIEW: usize = 0;
const TITLE: usize = 1;
const ID: usize = 2;
const SIZE: usize = 3;
const UPDATED: usize = 4;

pub struct Row {
	pub item: WorkshopItem,
	pub details: Option<ItemDetails>,
	/// Position in gmpublish's list, for going back to it.
	order: usize,
}

impl Row {
	/// Steam's size when it has one, otherwise gmpublish's.
	fn size(&self) -> Option<u64> {
		self.details
			.as_ref()
			.map(|d| d.file_size)
			.filter(|size| *size > 0)
			.or_else(|| self.item.size_bytes())
	}

	fn updated(&self) -> Option<i64> {
		self.details.as_ref().map(|d| d.updated).filter(|t| *t > 0)
	}
}

#[derive(Default)]
pub struct WorkshopTable {
	widths: SavedWidths,
	pub rows: Vec<Row>,
	pub previews: HashMap<u64, Arc<Image>>,
	sort: Option<(usize, ColumnSort)>,
}

impl WorkshopTable {
	pub fn set_items(&mut self, items: Vec<WorkshopItem>) {
		let mut details: HashMap<u64, ItemDetails> = self
			.rows
			.drain(..)
			.filter_map(|row| row.details.map(|d| (row.item.id, d)))
			.collect();
		self.rows = items
			.into_iter()
			.enumerate()
			.map(|(order, item)| Row {
				details: details.remove(&item.id),
				item,
				order,
			})
			.collect();
		self.apply_sort();
	}

	pub fn set_details(&mut self, details: &[ItemDetails]) {
		for item in details {
			if let Some(row) = self.rows.iter_mut().find(|r| r.item.id == item.id) {
				row.details = Some(item.clone());
			}
		}
		self.apply_sort();
	}

	pub fn row_id(&self, row_ix: usize) -> Option<u64> {
		self.rows.get(row_ix).map(|r| r.item.id)
	}

	pub fn row_of(&self, id: u64) -> Option<usize> {
		self.rows.iter().position(|r| r.item.id == id)
	}

	fn apply_sort(&mut self) {
		let (col, sort) = self.sort.unwrap_or((TITLE, ColumnSort::Default));
		self.rows.sort_by(|a, b| {
			let order = match (sort, col) {
				(ColumnSort::Default, _) => return a.order.cmp(&b.order),
				(_, TITLE) => a
					.item
					.title
					.to_lowercase()
					.cmp(&b.item.title.to_lowercase()),
				(_, ID) => a.item.id.cmp(&b.item.id),
				(_, SIZE) => {
					return missing_last(a.size(), b.size(), sort == ColumnSort::Descending);
				}
				(_, UPDATED) => {
					return missing_last(a.updated(), b.updated(), sort == ColumnSort::Descending);
				}
				_ => Ordering::Equal,
			};
			if sort == ColumnSort::Descending {
				order.reverse()
			} else {
				order
			}
		});
	}
}

impl Remembered for WorkshopTable {
	const KEY: &'static str = "workshop";

	fn widths_mut(&mut self) -> &mut SavedWidths {
		&mut self.widths
	}
}

impl TableDelegate for WorkshopTable {
	fn columns_count(&self, _: &App) -> usize {
		5
	}

	fn rows_count(&self, _: &App) -> usize {
		self.rows.len()
	}

	fn column(&self, col_ix: usize, _: &App) -> Column {
		let column = match col_ix {
			PREVIEW => Column::new("preview", "").width(px(48.)).resizable(false),
			TITLE => Column::new("title", t!("ws.col_title"))
				.width(px(420.))
				.min_width(px(120.))
				.sortable(),
			ID => Column::new("id", t!("ws.col_id"))
				.width(px(130.))
				.sortable(),
			SIZE => Column::new("size", t!("ws.col_size"))
				.width(px(110.))
				.text_right()
				.sortable(),
			_ => Column::new("updated", t!("ws.col_updated"))
				.width(px(120.))
				.sortable(),
		};
		self.widths.apply(col_ix, column)
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
		let theme = cx.theme();
		let Some(row) = self.rows.get(row_ix) else {
			return div().into_any_element();
		};
		let muted = theme.muted_foreground;
		match col_ix {
			PREVIEW => match self.previews.get(&row.item.id) {
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
			TITLE => div()
				.truncate()
				.child(row.item.title.clone())
				.into_any_element(),
			ID => div()
				.mono(cx)
				.text_sm()
				.text_color(muted)
				.child(row.item.id.to_string())
				.into_any_element(),
			SIZE => div()
				.w_full()
				.text_right()
				.whitespace_nowrap()
				.child(row.size().map_or_else(|| "—".to_owned(), format_size))
				.into_any_element(),
			_ => div()
				.text_sm()
				.text_color(muted)
				.whitespace_nowrap()
				.child(row.updated().map(format_date).unwrap_or_default())
				.into_any_element(),
		}
	}

	fn cell_text(&self, row_ix: usize, col_ix: usize, _: &App) -> String {
		let Some(row) = self.rows.get(row_ix) else {
			return String::new();
		};
		match col_ix {
			TITLE => row.item.title.clone(),
			ID => row.item.id.to_string(),
			SIZE => row.size().map(format_size).unwrap_or_default(),
			UPDATED => row.updated().map(format_date).unwrap_or_default(),
			_ => String::new(),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	// gpui_kit's prelude has a `test` attribute of its own.
	use ::core::prelude::v1::test;

	fn item(id: u64, title: &str, size: &str) -> WorkshopItem {
		WorkshopItem {
			id,
			size: size.into(),
			title: title.into(),
		}
	}

	fn titles(table: &WorkshopTable) -> Vec<&str> {
		table.rows.iter().map(|r| r.item.title.as_str()).collect()
	}

	#[test]
	fn sorting_keeps_unknown_sizes_last() {
		let mut table = WorkshopTable::default();
		table.set_items(vec![
			item(3, "beta", "2 KB"),
			item(1, "Alpha", "17179869184.0 GB"),
			item(2, "gamma", "1 MB"),
		]);
		assert_eq!(titles(&table), ["beta", "Alpha", "gamma"]);

		table.sort = Some((SIZE, ColumnSort::Ascending));
		table.apply_sort();
		assert_eq!(titles(&table), ["beta", "gamma", "Alpha"]);
		table.sort = Some((SIZE, ColumnSort::Descending));
		table.apply_sort();
		assert_eq!(titles(&table), ["gamma", "beta", "Alpha"]);

		table.sort = Some((TITLE, ColumnSort::Ascending));
		table.apply_sort();
		assert_eq!(titles(&table), ["Alpha", "beta", "gamma"]);

		table.sort = Some((TITLE, ColumnSort::Default));
		table.apply_sort();
		assert_eq!(titles(&table), ["beta", "Alpha", "gamma"]);
	}

	#[test]
	fn steam_size_wins_and_details_survive_a_refresh() {
		let mut table = WorkshopTable::default();
		table.set_items(vec![item(1, "a", "17179869184.0 GB")]);
		table.set_details(&[ItemDetails {
			id: 1,
			file_size: 4096,
			updated: 1_785_371_010,
			..ItemDetails::default()
		}]);
		assert_eq!(table.rows[0].size(), Some(4096));
		table.set_items(vec![item(1, "a", "17179869184.0 GB")]);
		assert_eq!(table.rows[0].updated(), Some(1_785_371_010));
	}
}
