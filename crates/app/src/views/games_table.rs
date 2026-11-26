//! Games and their folders; sorting keeps folders under their game.

use std::cmp::Ordering;
use std::path::PathBuf;

use gpui_kit::component::table::{Column, ColumnSort, TableDelegate, TableState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable, h_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;
use prybar_core::steam::{App as SteamApp, Platform};
use rust_i18n::t;

use super::columns::{Remembered, SavedWidths};
use crate::theme::Mono;
use crate::widgets::file_name;

const NAME: usize = 0;
const ID: usize = 1;
const BUILD: usize = 2;
const PROTON: usize = 3;
const TOOLS: usize = 4;
const LOCATION: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
	App(usize),
	Dir(usize, usize),
}

#[derive(Default)]
pub struct GamesTable {
	widths: SavedWidths,
	pub apps: Vec<SteamApp>,
	/// The game folder every page works with.
	pub current: Option<PathBuf>,
	rows: Vec<Row>,
	sort: Option<(usize, ColumnSort)>,
}

impl GamesTable {
	pub fn set_apps(&mut self, apps: Vec<SteamApp>) {
		self.apps = apps;
		self.build_rows();
	}

	pub fn row(&self, row_ix: usize) -> Option<Row> {
		self.rows.get(row_ix).copied()
	}

	pub fn row_index(&self, row: Row) -> Option<usize> {
		self.rows.iter().position(|r| *r == row)
	}

	fn build_rows(&mut self) {
		let mut order: Vec<usize> = (0..self.apps.len()).collect();
		if let Some((col, sort)) = self.sort.filter(|(_, sort)| *sort != ColumnSort::Default) {
			order.sort_by(|&a, &b| {
				let ordering = compare(&self.apps[a], &self.apps[b], col);
				if sort == ColumnSort::Descending {
					ordering.reverse()
				} else {
					ordering
				}
			});
		}
		self.rows = order
			.into_iter()
			.flat_map(|i| {
				std::iter::once(Row::App(i))
					.chain((0..self.apps[i].game_dirs.len()).map(move |d| Row::Dir(i, d)))
			})
			.collect();
	}
}

fn build(app: &SteamApp) -> &'static str {
	match app.platform {
		Platform::Windows => "Windows",
		Platform::Linux => "Linux",
	}
}

fn proton(app: &SteamApp) -> String {
	let windows = app.platform == Platform::Windows;
	match (&app.proton_path, &app.compat_tool_name) {
		(Some(_), Some(name)) if !windows => t!("games.proton_tools", name = name).into_owned(),
		(Some(_), Some(name)) => name.clone(),
		(None, _) if windows => t!("games.not_found").into_owned(),
		_ => String::new(),
	}
}

fn tools(app: &SteamApp) -> String {
	match (&app.tools_path, app.tools_overridden) {
		(None, _) => t!("games.tools_missing"),
		(Some(_), true) => t!("games.tools_custom"),
		(Some(_), false) => t!("games.tools_found"),
	}
	.into_owned()
}

fn name(app: &SteamApp) -> String {
	if app.is_mod {
		t!("games.mod", name = app.name).into_owned()
	} else {
		app.name.clone()
	}
}

fn compare(a: &SteamApp, b: &SteamApp, col: usize) -> Ordering {
	match col {
		NAME => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
		ID => a.id.cmp(&b.id),
		BUILD => build(a).cmp(build(b)),
		PROTON => proton(a).to_lowercase().cmp(&proton(b).to_lowercase()),
		TOOLS => tools(a).cmp(&tools(b)),
		_ => a.install_path.cmp(&b.install_path),
	}
}

impl Remembered for GamesTable {
	const KEY: &'static str = "games";

	fn widths_mut(&mut self) -> &mut SavedWidths {
		&mut self.widths
	}
}

impl TableDelegate for GamesTable {
	fn columns_count(&self, _: &App) -> usize {
		6
	}

	fn rows_count(&self, _: &App) -> usize {
		self.rows.len()
	}

	fn column(&self, col_ix: usize, _: &App) -> Column {
		let column = match col_ix {
			NAME => Column::new("name", t!("games.col_game"))
				.width(px(300.))
				.min_width(px(120.))
				.sortable(),
			ID => Column::new("id", t!("games.col_id"))
				.width(px(90.))
				.sortable(),
			BUILD => Column::new("build", t!("games.col_build"))
				.width(px(90.))
				.sortable(),
			PROTON => Column::new("proton", t!("games.col_proton"))
				.width(px(190.))
				.sortable(),
			TOOLS => Column::new("tools", t!("games.col_tools"))
				.width(px(90.))
				.sortable(),
			_ => Column::new("location", t!("games.col_location"))
				.width(px(520.))
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
		self.build_rows();
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
		let Some(row) = self.rows.get(row_ix).copied() else {
			return div().into_any_element();
		};
		let (i, dir) = match row {
			Row::App(i) => (i, None),
			Row::Dir(i, d) => (i, Some(d)),
		};
		let Some(app) = self.apps.get(i) else {
			return div().into_any_element();
		};
		// Games the tools can't work with are shown faded.
		let color = if app.tools_path.is_none() {
			theme.muted_foreground
		} else {
			theme.foreground
		};

		if let Some(d) = dir {
			let Some(game_dir) = app.game_dirs.get(d) else {
				return div().into_any_element();
			};
			return match col_ix {
				NAME => {
					let is_current = self.current.as_ref() == Some(game_dir);
					h_flex()
						.pl_4()
						.gap_2()
						.text_color(color)
						.child(if is_current {
							Icon::new(IconName::Check).small().text_color(theme.primary)
						} else {
							Icon::new(IconName::Folder)
								.small()
								.text_color(theme.muted_foreground)
						})
						.child(div().truncate().child(file_name(game_dir)))
						.into_any_element()
				}
				LOCATION => div()
					.truncate()
					.mono(cx)
					.text_sm()
					.text_color(color)
					.child(game_dir.display().to_string())
					.into_any_element(),
				_ => div().into_any_element(),
			};
		}

		match col_ix {
			NAME => {
				let tip = app.is_mod.then(|| match &app.base_name {
					Some(base) => t!("games.runs_on", name = base).into_owned(),
					None => t!("games.base_missing", id = app.id).into_owned(),
				});
				div()
					.id(("game-name", row_ix))
					.truncate()
					.font_weight(FontWeight::MEDIUM)
					.text_color(color)
					.child(name(app))
					.when_some(tip, |d, tip| {
						d.tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
					})
					.into_any_element()
			}
			ID => div()
				.mono(cx)
				.text_color(color)
				.child(app.id.to_string())
				.into_any_element(),
			BUILD => div().text_color(color).child(build(app)).into_any_element(),
			PROTON => div()
				.truncate()
				.text_color(color)
				.child(proton(app))
				.into_any_element(),
			TOOLS => {
				let tip = app.tools_path.as_ref().map_or_else(
					|| t!("games.tools_missing_tip").into_owned(),
					|p| p.display().to_string(),
				);
				div()
					.id(("game-tools", row_ix))
					.text_color(color)
					.child(tools(app))
					.tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
					.into_any_element()
			}
			_ => div()
				.truncate()
				.mono(cx)
				.text_sm()
				.text_color(color)
				.child(app.install_path.display().to_string())
				.into_any_element(),
		}
	}

	fn cell_text(&self, row_ix: usize, col_ix: usize, _: &App) -> String {
		let Some(row) = self.rows.get(row_ix) else {
			return String::new();
		};
		match *row {
			Row::App(i) => {
				let Some(app) = self.apps.get(i) else {
					return String::new();
				};
				match col_ix {
					NAME => name(app),
					ID => app.id.to_string(),
					BUILD => build(app).to_owned(),
					PROTON => proton(app),
					TOOLS => tools(app),
					_ => app.install_path.display().to_string(),
				}
			}
			Row::Dir(i, d) => {
				let dir = self.apps.get(i).and_then(|a| a.game_dirs.get(d));
				match (col_ix, dir) {
					(NAME, Some(dir)) => file_name(dir),
					(LOCATION, Some(dir)) => dir.display().to_string(),
					_ => String::new(),
				}
			}
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	// gpui_kit's prelude has a `test` attribute of its own.
	use ::core::prelude::v1::test;

	fn app(id: u32, name: &str, dirs: usize) -> SteamApp {
		SteamApp {
			id,
			name: name.into(),
			game_dirs: (0..dirs)
				.map(|d| PathBuf::from(format!("/g/{id}/{d}")))
				.collect(),
			..SteamApp::default()
		}
	}

	#[test]
	fn folders_stay_under_their_game() {
		let mut table = GamesTable::default();
		table.set_apps(vec![
			app(440, "Team Fortress 2", 1),
			app(4000, "Garry's Mod", 2),
		]);
		assert_eq!(
			table.rows,
			[
				Row::App(0),
				Row::Dir(0, 0),
				Row::App(1),
				Row::Dir(1, 0),
				Row::Dir(1, 1)
			]
		);
		table.sort = Some((NAME, ColumnSort::Ascending));
		table.build_rows();
		assert_eq!(
			table.rows,
			[
				Row::App(1),
				Row::Dir(1, 0),
				Row::Dir(1, 1),
				Row::App(0),
				Row::Dir(0, 0)
			]
		);
		table.sort = Some((ID, ColumnSort::Descending));
		table.build_rows();
		assert_eq!(table.rows[0], Row::App(1));
	}
}
