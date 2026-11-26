//! The open package as a folder tree, and game file search results.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::table::{Column, ColumnSort, TableDelegate, TableState};
use gpui_kit::component::{ActiveTheme, Icon, IconName, Sizable, h_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;
use prybar_core::package::Package;
use prybar_core::vfs::{Found, MountKind, Source};
use rust_i18n::t;

use super::columns::{Remembered, SavedWidths};
use crate::theme::Mono;
use crate::widgets::{file_name, format_size};

const CHECK: usize = 0;
const PATH: usize = 1;
const SIZE: usize = 2;

/// Indent of one tree level.
const INDENT: f32 = 16.;

type NodeId = usize;

/// A chain of single folders is merged into one node named "a/b/c".
struct Node {
	name: String,
	/// Full path of the folder or file, for copying.
	path: String,
	children: Vec<NodeId>,
	/// The package entry of a file; `None` for a folder.
	entry: Option<usize>,
	size: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Row {
	node: NodeId,
	depth: usize,
}

#[derive(Default)]
pub struct EntriesTable {
	widths: SavedWidths,
	pub package: Option<Arc<Package>>,
	/// Checked entries, as indexes into the package's entries.
	pub checked: BTreeSet<usize>,
	/// Node 0 is the root and is never shown.
	nodes: Vec<Node>,
	expanded: HashSet<NodeId>,
	/// What the filter lets through, folders expanded; `None` without a filter.
	visible: Option<Vec<bool>>,
	rows: Vec<Row>,
	sort: Option<(usize, ColumnSort)>,
}

impl EntriesTable {
	pub fn set_package(&mut self, package: Option<Arc<Package>>) {
		self.checked.clear();
		self.expanded.clear();
		self.visible = None;
		self.nodes = package
			.as_deref()
			.map(|p| build_tree(p.entries.iter().map(|e| (e.path.as_str(), e.size))))
			.unwrap_or_default();
		// A package with a single top folder opens it right away.
		if let Some(root) = self.nodes.first()
			&& let [only] = root.children[..]
			&& self.nodes[only].entry.is_none()
		{
			self.expanded.insert(only);
		}
		self.package = package;
		self.rebuild_rows();
	}

	/// `needle` is lower case with forward slashes.
	pub fn set_filter(&mut self, needle: &str) {
		self.visible = (!needle.is_empty() && !self.nodes.is_empty()).then(|| {
			let mut visible = vec![false; self.nodes.len()];
			mark_visible(&self.nodes, 0, needle, &mut visible);
			visible
		});
		self.rebuild_rows();
	}

	/// The package entry shown in `row_ix`; `None` for a folder.
	pub fn entry_index(&self, row_ix: usize) -> Option<usize> {
		self.nodes[self.rows.get(row_ix)?.node].entry
	}

	/// The row of a package entry, if it is shown.
	pub fn row_of(&self, index: usize) -> Option<usize> {
		self.rows
			.iter()
			.position(|row| self.nodes[row.node].entry == Some(index))
	}

	/// Returns the folder's row, which stays put.
	pub fn toggle(&mut self, row_ix: usize) -> Option<usize> {
		let row = *self.rows.get(row_ix)?;
		if self.nodes[row.node].entry.is_some() || self.visible.is_some() {
			return None;
		}
		if !self.expanded.remove(&row.node) {
			self.expanded.insert(row.node);
		}
		self.rebuild_rows();
		Some(row_ix)
	}

	fn is_open(&self, node: NodeId) -> bool {
		self.visible.is_some() || self.expanded.contains(&node)
	}

	fn shown(&self, node: NodeId) -> bool {
		self.visible.as_ref().is_none_or(|v| v[node])
	}

	/// The entries of the files under `node` that the filter lets through.
	fn files_under(&self, node: NodeId) -> Vec<usize> {
		let mut files = Vec::new();
		let mut stack = vec![node];
		while let Some(id) = stack.pop() {
			if !self.shown(id) {
				continue;
			}
			match self.nodes[id].entry {
				Some(entry) => files.push(entry),
				None => stack.extend(&self.nodes[id].children),
			}
		}
		files
	}

	fn rebuild_rows(&mut self) {
		self.rows.clear();
		if self.nodes.is_empty() {
			return;
		}
		let mut stack: Vec<Row> = self
			.sorted_children(0)
			.into_iter()
			.rev()
			.map(|node| Row { node, depth: 0 })
			.collect();
		while let Some(row) = stack.pop() {
			self.rows.push(row);
			if self.nodes[row.node].entry.is_none() && self.is_open(row.node) {
				stack.extend(
					self.sorted_children(row.node)
						.into_iter()
						.rev()
						.map(|node| Row {
							node,
							depth: row.depth + 1,
						}),
				);
			}
		}
	}

	/// Folders first, then files; each group by the sorted column.
	fn sorted_children(&self, node: NodeId) -> Vec<NodeId> {
		let mut children: Vec<NodeId> = self.nodes[node]
			.children
			.iter()
			.copied()
			.filter(|&child| self.shown(child))
			.collect();
		let (col, sort) = match self.sort {
			Some((col, sort)) if sort != ColumnSort::Default => (col, sort),
			_ => (PATH, ColumnSort::Ascending),
		};
		children.sort_by(|&a, &b| {
			let (a, b) = (&self.nodes[a], &self.nodes[b]);
			let ordering = match col {
				SIZE => a.size.cmp(&b.size),
				_ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
			};
			let ordering = if sort == ColumnSort::Descending {
				ordering.reverse()
			} else {
				ordering
			};
			a.entry.is_some().cmp(&b.entry.is_some()).then(ordering)
		});
		children
	}
}

/// `entries` are package paths with forward slashes and sizes, in package order.
fn build_tree<'a>(entries: impl Iterator<Item = (&'a str, u64)>) -> Vec<Node> {
	let folder = |name: &str, path: String| Node {
		name: name.to_owned(),
		path,
		children: Vec::new(),
		entry: None,
		size: 0,
	};
	let mut nodes = vec![folder("", String::new())];
	let mut folders: HashMap<String, NodeId> = HashMap::new();
	for (index, (entry_path, entry_size)) in entries.enumerate() {
		let mut parent = 0;
		let mut path = String::new();
		let mut parts = entry_path.split('/').filter(|p| !p.is_empty()).peekable();
		while let Some(part) = parts.next() {
			if !path.is_empty() {
				path.push('/');
			}
			path.push_str(part);
			if parts.peek().is_none() {
				nodes.push(Node {
					name: part.to_owned(),
					path: entry_path.to_owned(),
					children: Vec::new(),
					entry: Some(index),
					size: entry_size,
				});
				let id = nodes.len() - 1;
				nodes[parent].children.push(id);
				break;
			}
			parent = match folders.get(&path) {
				Some(&id) => id,
				None => {
					nodes.push(folder(part, path.clone()));
					let id = nodes.len() - 1;
					nodes[parent].children.push(id);
					folders.insert(path.clone(), id);
					id
				}
			};
		}
	}
	for child in nodes[0].children.clone() {
		merge_single_folders(&mut nodes, child);
	}
	sum_sizes(&mut nodes, 0);
	nodes
}

/// "materials" holding only "models" holding only "props" becomes one node.
fn merge_single_folders(nodes: &mut [Node], id: NodeId) {
	if nodes[id].entry.is_some() {
		return;
	}
	while let [only] = nodes[id].children[..]
		&& nodes[only].entry.is_none()
	{
		let name = format!("{}/{}", nodes[id].name, nodes[only].name);
		nodes[id].name = name;
		nodes[id].path = std::mem::take(&mut nodes[only].path);
		nodes[id].children = std::mem::take(&mut nodes[only].children);
	}
	for child in nodes[id].children.clone() {
		merge_single_folders(nodes, child);
	}
}

fn sum_sizes(nodes: &mut [Node], id: NodeId) -> u64 {
	if nodes[id].entry.is_some() {
		return nodes[id].size;
	}
	let mut size = 0;
	for child in nodes[id].children.clone() {
		size += sum_sizes(nodes, child);
	}
	nodes[id].size = size;
	size
}

fn mark_visible(nodes: &[Node], id: NodeId, needle: &str, visible: &mut [bool]) -> bool {
	let shown = match nodes[id].entry {
		Some(_) => nodes[id].path.to_lowercase().contains(needle),
		None => {
			let mut any = false;
			for &child in &nodes[id].children {
				any |= mark_visible(nodes, child, needle, visible);
			}
			any
		}
	};
	visible[id] = shown;
	shown
}

impl Remembered for EntriesTable {
	const KEY: &'static str = "package_entries";

	fn widths_mut(&mut self) -> &mut SavedWidths {
		&mut self.widths
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
		let column = match col_ix {
			CHECK => Column::new("check", "").width(px(40.)).resizable(false),
			PATH => Column::new("path", t!("packages.col_path"))
				.width(px(620.))
				.min_width(px(160.))
				.sortable(),
			_ => Column::new("size", t!("packages.col_size"))
				.width(px(100.))
				.text_right()
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
		self.rebuild_rows();
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
		let files = if self.nodes.is_empty() {
			Vec::new()
		} else {
			self.files_under(0)
		};
		let all = !files.is_empty() && files.iter().all(|i| self.checked.contains(i));
		h_flex()
			.child(
				Checkbox::new("entry-check-all")
					.checked(all)
					.on_click(cx.listener(move |table, checked: &bool, _, cx| {
						let checked_entries = &mut table.delegate_mut().checked;
						if *checked {
							checked_entries.extend(files.iter().copied());
						} else {
							for file in &files {
								checked_entries.remove(file);
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
		let Some(row) = self.rows.get(row_ix).copied() else {
			return div().into_any_element();
		};
		let node = &self.nodes[row.node];
		let theme = cx.theme().clone();
		match col_ix {
			CHECK => {
				let files = match node.entry {
					Some(entry) => vec![entry],
					None => self.files_under(row.node),
				};
				let all = !files.is_empty() && files.iter().all(|i| self.checked.contains(i));
				Checkbox::new(("entry-check", row_ix))
					.checked(all)
					.on_click(cx.listener(move |table, checked: &bool, _, cx| {
						let checked_entries = &mut table.delegate_mut().checked;
						for file in &files {
							if *checked {
								checked_entries.insert(*file);
							} else {
								checked_entries.remove(file);
							}
						}
						cx.stop_propagation();
						cx.notify();
					}))
					.into_any_element()
			}
			PATH => {
				let folder = node.entry.is_none();
				let open = folder && self.is_open(row.node);
				let chevron = if open {
					IconName::ChevronDown
				} else {
					IconName::ChevronRight
				};
				let icon = match (folder, open) {
					(false, _) => IconName::File,
					(true, true) => IconName::FolderOpen,
					(true, false) => IconName::Folder,
				};
				h_flex()
					.id(("entry-name", row_ix))
					.w_full()
					.min_w_0()
					.gap_1()
					.pl(px(row.depth as f32 * INDENT))
					.child(div().w(px(14.)).flex_shrink_0().when(folder, |d| {
						d.child(
							Icon::new(chevron)
								.small()
								.text_color(theme.muted_foreground),
						)
					}))
					.child(
						Icon::new(icon)
							.small()
							.flex_shrink_0()
							.text_color(if folder {
								theme.primary
							} else {
								theme.muted_foreground
							}),
					)
					.child(div().truncate().mono(cx).text_sm().child(node.name.clone()))
					.when(folder, |d| {
						d.cursor_pointer()
							.on_click(cx.listener(move |table, _, _, cx| {
								if let Some(row) = table.delegate_mut().toggle(row_ix) {
									table.set_selected_row(row, cx);
								}
								cx.notify();
							}))
					})
					.into_any_element()
			}
			_ => div()
				.w_full()
				.text_right()
				.text_sm()
				.text_color(theme.muted_foreground)
				.whitespace_nowrap()
				.child(format_size(node.size))
				.into_any_element(),
		}
	}

	fn cell_text(&self, row_ix: usize, col_ix: usize, _: &App) -> String {
		let Some(node) = self.rows.get(row_ix).map(|row| &self.nodes[row.node]) else {
			return String::new();
		};
		match col_ix {
			PATH => node.path.clone(),
			SIZE => format_size(node.size),
			_ => String::new(),
		}
	}
}

const R_PATH: usize = 0;
const R_SOURCE: usize = 1;
const R_SHADOWED: usize = 2;
const R_SIZE: usize = 3;

#[derive(Default)]
pub struct ResultsTable {
	widths: SavedWidths,
	pub matches: Vec<Found>,
	rows: Vec<usize>,
	sort: Option<(usize, ColumnSort)>,
}

impl ResultsTable {
	pub fn clear(&mut self) {
		self.matches.clear();
		self.rows.clear();
	}

	pub fn extend(&mut self, matches: impl IntoIterator<Item = Found>) {
		self.matches.extend(matches);
		self.rows = (0..self.matches.len()).collect();
		self.apply_sort();
	}

	pub fn get(&self, row_ix: usize) -> Option<&Found> {
		self.matches.get(*self.rows.get(row_ix)?)
	}

	fn apply_sort(&mut self) {
		let (col, sort) = self.sort.unwrap_or((R_PATH, ColumnSort::Default));
		let matches = &self.matches;
		self.rows.sort_by(|&a, &b| {
			let (ma, mb) = (&matches[a], &matches[b]);
			let ordering = match (sort, col) {
				(ColumnSort::Default, _) => return a.cmp(&b),
				(_, R_SOURCE) => source_label(&ma.sources[0])
					.to_lowercase()
					.cmp(&source_label(&mb.sources[0]).to_lowercase()),
				(_, R_SHADOWED) => ma.sources.len().cmp(&mb.sources.len()),
				(_, R_SIZE) => ma.sources[0].size.cmp(&mb.sources[0].size),
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

/// Workshop items share file names, so they get their id.
pub fn source_label(source: &Source) -> String {
	let name = file_name(&source.mount);
	match source.kind {
		MountKind::Folder => format!("{name}/"),
		MountKind::Package => {
			let parent = source.mount.parent().map(file_name).unwrap_or_default();
			if !parent.is_empty() && parent.bytes().all(|b| b.is_ascii_digit()) {
				format!("{name} ({parent})")
			} else {
				name
			}
		}
	}
}

impl Remembered for ResultsTable {
	const KEY: &'static str = "game_files";

	fn widths_mut(&mut self) -> &mut SavedWidths {
		&mut self.widths
	}
}

impl TableDelegate for ResultsTable {
	fn columns_count(&self, _: &App) -> usize {
		4
	}

	fn rows_count(&self, _: &App) -> usize {
		self.rows.len()
	}

	fn column(&self, col_ix: usize, _: &App) -> Column {
		let column = match col_ix {
			R_PATH => Column::new("path", t!("packages.col_path"))
				.width(px(440.))
				.min_width(px(160.))
				.sortable(),
			R_SOURCE => Column::new("source", t!("packages.col_source"))
				.width(px(220.))
				.sortable(),
			R_SHADOWED => Column::new("shadowed", t!("packages.col_shadowed"))
				.width(px(90.))
				.text_right()
				.sortable(),
			_ => Column::new("size", t!("packages.col_size"))
				.width(px(100.))
				.text_right()
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
		let Some(found) = self.get(row_ix) else {
			return div().into_any_element();
		};
		let theme = cx.theme();
		let muted = theme.muted_foreground;
		let right = |text: String, color: Hsla| {
			div()
				.w_full()
				.text_right()
				.text_sm()
				.text_color(color)
				.whitespace_nowrap()
				.child(text)
				.into_any_element()
		};
		match col_ix {
			R_PATH => div()
				.truncate()
				.mono(cx)
				.text_sm()
				.child(found.path.clone())
				.into_any_element(),
			R_SOURCE => div()
				.truncate()
				.text_sm()
				.text_color(muted)
				.child(source_label(&found.sources[0]))
				.into_any_element(),
			R_SHADOWED => match found.sources.len() - 1 {
				0 => right(String::new(), muted),
				n => right(n.to_string(), theme.warning),
			},
			_ => right(format_size(found.sources[0].size), muted),
		}
	}

	fn cell_text(&self, row_ix: usize, col_ix: usize, _: &App) -> String {
		let Some(found) = self.get(row_ix) else {
			return String::new();
		};
		match col_ix {
			R_SOURCE => found.sources[0].file().display().to_string(),
			R_SHADOWED => (found.sources.len() - 1).to_string(),
			R_SIZE => format_size(found.sources[0].size),
			_ => found.path.clone(),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	// gpui_kit's prelude has a `test` attribute of its own.
	use ::core::prelude::v1::test;

	fn table(paths: &[(&str, u64)]) -> EntriesTable {
		let mut table = EntriesTable {
			nodes: build_tree(paths.iter().copied()),
			..EntriesTable::default()
		};
		table.rebuild_rows();
		table
	}

	fn shown(table: &EntriesTable) -> Vec<String> {
		table
			.rows
			.iter()
			.map(|row| format!("{}{}", "  ".repeat(row.depth), table.nodes[row.node].name))
			.collect()
	}

	#[test]
	fn builds_a_tree() {
		let mut t = table(&[
			("materials/models/props/a.vmt", 10),
			("materials/models/props/a.vtf", 20),
			("lua/autorun/x.lua", 1),
			("lua/y.lua", 2),
			("addon.txt", 5),
		]);
		assert_eq!(shown(&t), ["lua", "materials/models/props", "addon.txt"]);
		assert_eq!(t.nodes[t.rows[1].node].size, 30);

		let lua = t.toggle(0).unwrap();
		assert_eq!(lua, 0);
		assert_eq!(
			shown(&t),
			[
				"lua",
				"  autorun",
				"  y.lua",
				"materials/models/props",
				"addon.txt"
			]
		);
		assert_eq!(t.entry_index(2), Some(3));
		assert_eq!(t.row_of(3), Some(2));
		assert_eq!(t.entry_index(1), None);
		assert_eq!(t.files_under(t.rows[0].node).len(), 2);
	}

	#[test]
	fn filter_opens_the_way() {
		let mut t = table(&[
			("lua/autorun/x.lua", 1),
			("lua/y.lua", 2),
			("sound/z.wav", 3),
		]);
		t.set_filter("x.lua");
		assert_eq!(shown(&t), ["lua", "  autorun", "    x.lua"]);
		assert_eq!(t.files_under(0), [0]);
		t.set_filter("");
		assert_eq!(shown(&t), ["lua", "sound"]);
	}

	#[test]
	fn sorts_within_folders() {
		let mut t = table(&[("b.txt", 1), ("a.txt", 9), ("dir/c.txt", 5)]);
		t.sort = Some((SIZE, ColumnSort::Descending));
		t.rebuild_rows();
		assert_eq!(shown(&t), ["dir", "a.txt", "b.txt"]);
	}
}
