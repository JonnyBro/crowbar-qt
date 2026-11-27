//! Checks a model the way the game loads it: companion files, materials, textures.

use std::path::{Path, PathBuf};

use crate::mdl::{self, Model};
use crate::package::Package;
use crate::vdf::{self, Node, ParseOptions};
use crate::vfs::{self, Found, MountKind, Source, Vfs, normalize};

const MAX_FILE: u64 = 256 * 1024 * 1024;
/// Longer Patch include chains are a loop.
const MAX_INCLUDES: usize = 8;
/// Brush shaders; models drawn with them look wrong.
const WORLD_SHADERS: [&str; 6] = [
	"lightmappedgeneric",
	"worldvertextransition",
	"lightmappedreflective",
	"lightmapped_4wayblend",
	"worldtwotextureblend",
	"water",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelSource {
	File(PathBuf),
	/// Such as `models/props_c17/oildrum001.mdl`.
	Game(String),
	/// A file inside a VPK or GMA, by its path there.
	Package {
		package: PathBuf,
		path: String,
	},
}

#[derive(Debug, Clone)]
pub struct Inspection {
	pub model: Model,
	pub file: Source,
	pub game_path: Option<String>,
	/// The copy the game loads instead of a file from disk.
	pub game_copy: Option<Source>,
	pub companions: Vec<Companion>,
	pub materials: Vec<MaterialCheck>,
	pub includes: Vec<Include>,
}

#[derive(Debug, Clone)]
pub struct Companion {
	pub extension: &'static str,
	pub required: bool,
	pub state: CompanionState,
}

#[derive(Debug, Clone)]
pub enum CompanionState {
	Missing,
	Found { source: Source, matches: bool },
	Unreadable { source: Source, error: String },
}

#[derive(Debug, Clone)]
pub struct MaterialCheck {
	pub name: String,
	/// Problems of unused materials don't count.
	pub used: bool,
	pub tried: Vec<String>,
	pub vmt: Option<Found>,
	pub shader: Option<String>,
	pub textures: Vec<TextureCheck>,
	pub problems: Vec<MaterialProblem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaterialProblem {
	Missing,
	Unreadable(String),
	Broken(String),
	WorldShader(String),
	IncludeMissing(String),
}

#[derive(Debug, Clone)]
pub struct TextureCheck {
	pub param: String,
	pub name: String,
	pub path: String,
	pub found: Option<Found>,
}

#[derive(Debug, Clone)]
pub struct Include {
	pub path: String,
	pub found: Option<Found>,
}

impl Inspection {
	pub fn problem_count(&self) -> usize {
		let companions = self
			.companions
			.iter()
			.filter(|c| match &c.state {
				CompanionState::Missing => c.required,
				CompanionState::Found { matches, .. } => !matches,
				CompanionState::Unreadable { .. } => true,
			})
			.count();
		let materials: usize = self
			.materials
			.iter()
			.filter(|m| m.used)
			.map(|m| m.problems.len() + m.textures.iter().filter(|t| t.found.is_none()).count())
			.sum();
		let includes = self.includes.iter().filter(|i| i.found.is_none()).count();
		companions + materials + includes
	}
}

/// The `.mdl` itself; `vfs` is only needed for a game path.
pub(crate) fn read_model(
	vfs: Option<&Vfs>,
	source: &ModelSource,
) -> Result<(Source, Vec<u8>), String> {
	match source {
		ModelSource::File(path) => {
			let data = vfs::read_file(path, MAX_FILE)?;
			let file = Source {
				mount: path.parent().map(Path::to_path_buf).unwrap_or_default(),
				kind: MountKind::Folder,
				path: path
					.file_name()
					.map(|n| n.to_string_lossy().into_owned())
					.unwrap_or_default(),
				size: data.len() as u64,
			};
			Ok((file, data))
		}
		ModelSource::Game(path) => {
			let (file, data) = vfs
				.and_then(|vfs| vfs.read(path, MAX_FILE))
				.ok_or_else(|| format!("the game has no {path}"))?;
			Ok((file, data?))
		}
		ModelSource::Package { package, path } => {
			let (file, data) = package_file(package, path)?
				.ok_or_else(|| format!("{} has no {path}", package.display()))?;
			Ok((file, data?))
		}
	}
}

/// Where a file was found, and its data or why it could not be read.
type Read = (Source, Result<Vec<u8>, String>);

fn package_file(package: &Path, path: &str) -> Result<Option<Read>, String> {
	let opened = Package::open(package).map_err(|e| e.to_string())?;
	let wanted = normalize(path);
	let Some(entry) = opened.entries.iter().find(|e| normalize(&e.path) == wanted) else {
		return Ok(None);
	};
	let source = Source {
		mount: package.to_path_buf(),
		kind: MountKind::Package,
		path: entry.path.clone(),
		size: entry.size,
	};
	Ok(Some((
		source,
		opened.read(entry, MAX_FILE).map_err(|e| e.to_string()),
	)))
}

/// `extension` next to the model: beside the file, or under the same game path.
pub(crate) fn read_companion(
	vfs: Option<&Vfs>,
	source: &ModelSource,
	extension: &str,
) -> Option<Read> {
	match source {
		ModelSource::File(path) => {
			let stem = path.file_stem()?.to_string_lossy();
			let found = vfs::resolve_ci(path.parent()?, &format!("{stem}.{extension}"))?;
			let data = vfs::read_file(&found, MAX_FILE);
			let source = Source {
				mount: path.parent()?.to_path_buf(),
				kind: MountKind::Folder,
				path: found.file_name()?.to_string_lossy().into_owned(),
				size: data.as_ref().map_or(0, |d| d.len() as u64),
			};
			Some((source, data))
		}
		ModelSource::Game(path) => {
			let path = normalize(path);
			let stem = path.strip_suffix(".mdl")?;
			vfs?.read(&format!("{stem}.{extension}"), MAX_FILE)
		}
		ModelSource::Package { package, path } => {
			let path = normalize(path);
			let stem = path.strip_suffix(".mdl")?;
			package_file(package, &format!("{stem}.{extension}"))
				.ok()
				.flatten()
		}
	}
}

/// The engine reads .dx90.vtx; old tools wrote the others.
pub(crate) const VTX_EXTENSIONS: [&str; 4] = ["dx90.vtx", "vtx", "dx80.vtx", "sw.vtx"];

pub fn inspect(vfs: &Vfs, source: &ModelSource) -> Result<Inspection, String> {
	let (file, data) = read_model(Some(vfs), source)?;
	let game_path = match source {
		ModelSource::File(path) => game_path_of(path),
		ModelSource::Game(path) | ModelSource::Package { path, .. } => Some(normalize(path)),
	};
	let model = mdl::parse(&data).map_err(|e| e.to_string())?;

	let game_copy = match source {
		ModelSource::File(path) => game_path
			.as_deref()
			.and_then(|p| vfs.find(p))
			.map(|found| found.sources[0].clone())
			.filter(|winner| !same_file(&winner.file(), path)),
		ModelSource::Game(_) | ModelSource::Package { .. } => None,
	};

	let companions = companions(vfs, source, &model);
	let materials = model
		.materials
		.iter()
		.enumerate()
		.map(|(i, name)| {
			let mut check = check_material(vfs, name, &model.material_folders);
			check.used = model.material_used(i);
			check
		})
		.collect();
	let includes = model
		.include_models
		.iter()
		.map(|path| Include {
			path: path.clone(),
			found: vfs.find(path),
		})
		.collect();
	Ok(Inspection {
		model,
		file,
		game_path,
		game_copy,
		companions,
		materials,
		includes,
	})
}

fn game_path_of(path: &Path) -> Option<String> {
	let parts: Vec<String> = path
		.components()
		.map(|c| c.as_os_str().to_string_lossy().into_owned())
		.collect();
	let start = parts
		.iter()
		.rposition(|p| p.eq_ignore_ascii_case("models"))?;
	Some(normalize(&parts[start..].join("/")))
}

fn same_file(a: &Path, b: &Path) -> bool {
	match (a.canonicalize(), b.canonicalize()) {
		(Ok(a), Ok(b)) => a == b,
		_ => a == b,
	}
}

fn companions(vfs: &Vfs, source: &ModelSource, model: &Model) -> Vec<Companion> {
	let read = |extension: &str| read_companion(Some(vfs), source, extension);
	let check = |extension: &'static str, required: bool| {
		let state = match read(extension) {
			None => CompanionState::Missing,
			Some((source, Ok(data))) => CompanionState::Found {
				source,
				// Valve's .ani files have no checksum; the engine doesn't check it.
				matches: extension == "ani"
					|| mdl::companion_checksum(extension, &data) == Some(model.checksum),
			},
			Some((source, Err(error))) => CompanionState::Unreadable { source, error },
		};
		Companion {
			extension,
			required,
			state,
		}
	};

	let geometry = model.has_geometry();
	let mut out = vec![check("vvd", geometry)];
	let vtx = VTX_EXTENSIONS
		.into_iter()
		.map(|ext| check(ext, geometry))
		.find(|c| !matches!(c.state, CompanionState::Missing))
		.unwrap_or_else(|| check("dx90.vtx", geometry));
	out.push(vtx);
	out.push(check("phy", false));
	if model.anim_blocks > 0 {
		out.push(check("ani", true));
	}
	out
}

/// Folder and name are joined as is, like the engine does.
pub fn material_paths(name: &str, folders: &[String]) -> Vec<String> {
	let name = name.strip_suffix(".vmt").unwrap_or(name);
	let mut paths: Vec<String> = if folders.is_empty() {
		vec![material_path(name)]
	} else {
		folders
			.iter()
			.map(|folder| material_path(&format!("{folder}{name}")))
			.collect()
	};
	paths.dedup();
	paths
}

fn material_path(name: &str) -> String {
	let name = normalize(name);
	let name = name.strip_prefix("materials/").unwrap_or(&name);
	format!("materials/{name}.vmt")
}

fn texture_path(name: &str) -> String {
	let name = normalize(name);
	let name = name.strip_suffix(".vtf").unwrap_or(&name);
	let name = name.strip_prefix("materials/").unwrap_or(name);
	format!("materials/{name}.vtf")
}

fn check_material(vfs: &Vfs, name: &str, folders: &[String]) -> MaterialCheck {
	let tried = material_paths(name, folders);
	let mut check = MaterialCheck {
		name: name.to_owned(),
		used: true,
		vmt: tried.iter().find_map(|path| vfs.find(path)),
		tried,
		shader: None,
		textures: Vec::new(),
		problems: Vec::new(),
	};
	let Some(vmt) = &check.vmt else {
		check.problems.push(MaterialProblem::Missing);
		return check;
	};
	let path = vmt.path.clone();
	match read_material(vfs, &path, 0) {
		Ok(Material { shader, params }) => {
			if WORLD_SHADERS.contains(&shader.to_lowercase().as_str()) {
				check
					.problems
					.push(MaterialProblem::WorldShader(shader.clone()));
			}
			check.shader = Some(shader);
			check.textures = params
				.into_iter()
				.filter(|(param, value)| is_texture_param(param, value))
				.map(|(param, value)| {
					let path = texture_path(&value);
					TextureCheck {
						found: vfs.find(&path),
						param,
						name: value,
						path,
					}
				})
				.collect();
		}
		Err(problem) => check.problems.push(problem),
	}
	check
}

struct Material {
	shader: String,
	/// Lower-case keys.
	params: Vec<(String, String)>,
}

fn read_material(vfs: &Vfs, path: &str, depth: usize) -> Result<Material, MaterialProblem> {
	let data = match vfs.read(path, MAX_FILE) {
		Some((_, Ok(data))) => data,
		Some((_, Err(e))) => return Err(MaterialProblem::Unreadable(e)),
		None => return Err(MaterialProblem::IncludeMissing(path.to_owned())),
	};
	let text = String::from_utf8_lossy(&data);
	let root = vdf::parse(&first_block(&text), ParseOptions::RAW)
		.map_err(|e| MaterialProblem::Broken(e.to_string()))?;
	let shader = root
		.children()
		.iter()
		.find(|n| n.is_object())
		.ok_or_else(|| MaterialProblem::Broken("no shader block".into()))?;
	let mut params = string_params(shader);
	if !shader.key.eq_ignore_ascii_case("patch") {
		return Ok(Material {
			shader: shader.key.clone(),
			params,
		});
	}
	let include = shader
		.get("include")
		.ok_or_else(|| MaterialProblem::Broken("Patch without include".into()))?;
	if depth >= MAX_INCLUDES {
		return Err(MaterialProblem::Broken("Patch includes loop".into()));
	}
	let mut base = read_material(vfs, &normalize(include), depth + 1)?;
	params.retain(|(key, _)| key != "include");
	for block in ["insert", "replace"] {
		if let Some(node) = shader.find(block) {
			params.extend(string_params(node));
		}
	}
	for (key, value) in params {
		match base.params.iter_mut().find(|(k, _)| *k == key) {
			Some(slot) => slot.1 = value,
			None => base.params.push((key, value)),
		}
	}
	Ok(base)
}

/// Like the engine: only the first block counts; an unclosed one ends with the file.
fn first_block(text: &str) -> String {
	let bytes = text.as_bytes();
	let mut depth = 0usize;
	let mut i = 0;
	while i < bytes.len() {
		match bytes[i] {
			b'"' => {
				i += 1;
				while i < bytes.len() && !matches!(bytes[i], b'"' | b'\n') {
					i += 1;
				}
			}
			b'/' if bytes.get(i + 1) == Some(&b'/') => {
				while i < bytes.len() && bytes[i] != b'\n' {
					i += 1;
				}
			}
			b'{' => depth += 1,
			b'}' if depth > 0 => {
				depth -= 1;
				if depth == 0 {
					return text[..=i].to_owned();
				}
			}
			_ => {}
		}
		i += 1;
	}
	let mut out = text.to_owned();
	out.extend(std::iter::repeat_n("\n}", depth));
	out
}

fn string_params(node: &Node) -> Vec<(String, String)> {
	node.children()
		.iter()
		.filter_map(|n| Some((n.key.to_lowercase(), n.as_str()?.trim().to_owned())))
		.collect()
}

/// `$basetexture` and kin, minus numbers, vectors, render targets and env_cubemap.
fn is_texture_param(param: &str, value: &str) -> bool {
	let Some(name) = param.strip_prefix('$') else {
		return false;
	};
	let name = name.trim_end_matches(|c: char| c.is_ascii_digit());
	let texture_like = name.ends_with("texture")
		|| name.ends_with("map")
		|| name.ends_with("mask")
		|| matches!(name, "detail" | "iris");
	let value = value.to_lowercase();
	texture_like
		&& !value.is_empty()
		&& value != "env_cubemap"
		&& !value.starts_with("_rt_")
		&& !value.starts_with(['[', '{', '-', '.'])
		&& !value.starts_with(|c: char| c.is_ascii_digit())
}

#[cfg(test)]
mod tests;
