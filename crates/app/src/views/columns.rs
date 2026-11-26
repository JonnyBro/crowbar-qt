//! Column widths, saved per table in the UI state.

use gpui_kit::component::table::{Column, TableDelegate, TableEvent, TableState};
use gpui_kit::*;

use crate::model::Model;

/// Widths from the last run, in column order. Empty until the user resizes.
#[derive(Debug, Clone, Default)]
pub struct SavedWidths(Vec<f32>);

impl SavedWidths {
	pub fn apply(&self, col_ix: usize, column: Column) -> Column {
		match self.0.get(col_ix) {
			Some(&width) if width.is_finite() && width >= 16. => column.width(px(width)),
			_ => column,
		}
	}
}

pub trait Remembered: TableDelegate {
	const KEY: &'static str;
	fn widths_mut(&mut self) -> &mut SavedWidths;
}

/// Moving columns is off: cells are drawn by column index.
pub fn new_table<D: Remembered, V: 'static>(
	mut delegate: D,
	model: &Entity<Model>,
	window: &mut Window,
	cx: &mut Context<V>,
) -> (Entity<TableState<D>>, Subscription) {
	if let Some(saved) = model.read(cx).ui.columns.get(D::KEY) {
		*delegate.widths_mut() = SavedWidths(saved.clone());
	}
	let table = cx.new(|cx| TableState::new(delegate, window, cx).col_movable(false));
	let model = model.downgrade();
	let subscription = cx.subscribe(&table, move |_, _, event: &TableEvent, cx| {
		if let TableEvent::ColumnWidthsChanged(widths) = event {
			let widths: Vec<f32> = widths.iter().map(|w| w.as_f32()).collect();
			let _ = model.update(cx, |model, _| {
				model.ui.columns.insert(D::KEY.into(), widths);
				model.save_ui();
			});
		}
	});
	(table, subscription)
}
