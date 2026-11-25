use std::path::Path;

/// `*` matches any run of characters, slashes included.
const WHITELIST: &[&str] = &[
	"lua/*.lua",
	"scenes/*.vcd",
	"particles/*.pcf",
	"resource/fonts/*.ttf",
	"scripts/vehicles/*.txt",
	"resource/localization/*/*.properties",
	"maps/*.bsp",
	"maps/*.lmp",
	"maps/*.nav",
	"maps/*.ain",
	"maps/thumb/*.png",
	"sound/*.wav",
	"sound/*.mp3",
	"sound/*.ogg",
	"materials/*.vmt",
	"materials/*.vtf",
	"materials/*.png",
	"materials/*.jpg",
	"materials/*.jpeg",
	"materials/colorcorrection/*.raw",
	"models/*.mdl",
	"models/*.phy",
	"models/*.ani",
	"models/*.vvd",
	"models/*.vtx",
	"gamemodes/*/*.txt",
	"gamemodes/*/*.fgd",
	"gamemodes/*/logo.png",
	"gamemodes/*/icon24.png",
	"gamemodes/*/gamemode/*.lua",
	"gamemodes/*/entities/effects/*.lua",
	"gamemodes/*/entities/weapons/*.lua",
	"gamemodes/*/entities/entities/*.lua",
	"gamemodes/*/backgrounds/*.png",
	"gamemodes/*/backgrounds/*.jpg",
	"gamemodes/*/backgrounds/*.jpeg",
	"gamemodes/*/content/models/*.mdl",
	"gamemodes/*/content/models/*.phy",
	"gamemodes/*/content/models/*.ani",
	"gamemodes/*/content/models/*.vvd",
	"gamemodes/*/content/models/*.vtx",
	"gamemodes/*/content/materials/*.vmt",
	"gamemodes/*/content/materials/*.vtf",
	"gamemodes/*/content/materials/*.png",
	"gamemodes/*/content/materials/*.jpg",
	"gamemodes/*/content/materials/*.jpeg",
	"gamemodes/*/content/materials/colorcorrection/*.raw",
	"gamemodes/*/content/scenes/*.vcd",
	"gamemodes/*/content/particles/*.pcf",
	"gamemodes/*/content/resource/fonts/*.ttf",
	"gamemodes/*/content/scripts/vehicles/*.txt",
	"gamemodes/*/content/resource/localization/*/*.properties",
	"gamemodes/*/content/maps/*.bsp",
	"gamemodes/*/content/maps/*.nav",
	"gamemodes/*/content/maps/*.ain",
	"gamemodes/*/content/maps/thumb/*.png",
	"gamemodes/*/content/sound/*.wav",
	"gamemodes/*/content/sound/*.mp3",
	"gamemodes/*/content/sound/*.ogg",
	"data_static/*.txt",
	"data_static/*.dat",
	"data_static/*.json",
	"data_static/*.xml",
	"data_static/*.csv",
	"shaders/fxc/*.vcs",
];

/// gmad drops these on its own, on top of the addon's "ignore" list.
const DEFAULT_IGNORES: &[&str] = &["*thumbs.db", "*desktop.ini", ".git*"];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Check {
	/// Paths relative to the folder, with `/`, that gmad refuses.
	pub invalid: Vec<String>,
	/// Files that go into the addon.
	pub packed: usize,
	/// Files left out by the ignore lists.
	pub ignored: usize,
}

/// gmad compares without regard to case.
fn wildcard(pattern: &str, text: &str) -> bool {
	let pattern = pattern.to_ascii_lowercase();
	let text = text.to_ascii_lowercase();
	let (p, t) = (pattern.as_bytes(), text.as_bytes());
	let (mut pi, mut ti) = (0, 0);
	let mut star: Option<(usize, usize)> = None;
	while ti < t.len() {
		if pi < p.len() && (p[pi] == b'?' || p[pi] == t[ti]) {
			pi += 1;
			ti += 1;
		} else if pi < p.len() && p[pi] == b'*' {
			star = Some((pi, ti));
			pi += 1;
		} else if let Some((sp, st)) = star {
			pi = sp + 1;
			ti = st + 1;
			star = Some((sp, st + 1));
		} else {
			return false;
		}
	}
	p[pi..].iter().all(|&c| c == b'*')
}

pub fn is_allowed(path: &str) -> bool {
	WHITELIST.iter().any(|pattern| wildcard(pattern, path))
}

/// The "ignore" patterns of the folder's addon.json; none if it is missing or broken.
fn addon_ignores(folder: &Path) -> Vec<String> {
	let Ok(bytes) = std::fs::read(folder.join("addon.json")) else {
		return Vec::new();
	};
	let Ok(root) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
		return Vec::new();
	};
	root["ignore"]
		.as_array()
		.into_iter()
		.flatten()
		.filter_map(|v| v.as_str().map(str::to_owned))
		.collect()
}

pub fn check(folder: &Path) -> std::io::Result<Check> {
	let ignores: Vec<String> = DEFAULT_IGNORES
		.iter()
		.map(|s| (*s).to_owned())
		.chain(addon_ignores(folder))
		.collect();
	let mut files = Vec::new();
	collect(folder, "", &mut files)?;
	files.sort();

	let mut result = Check::default();
	for file in files {
		if file.eq_ignore_ascii_case("addon.json") {
			continue;
		}
		if ignores.iter().any(|pattern| wildcard(pattern, &file)) {
			result.ignored += 1;
		} else if is_allowed(&file) {
			result.packed += 1;
		} else {
			result.invalid.push(file);
		}
	}
	Ok(result)
}

fn collect(dir: &Path, prefix: &str, files: &mut Vec<String>) -> std::io::Result<()> {
	for entry in std::fs::read_dir(dir)? {
		let entry = entry?;
		let name = entry.file_name().to_string_lossy().into_owned();
		let relative = if prefix.is_empty() {
			name
		} else {
			format!("{prefix}/{name}")
		};
		// Follows links, as gmad does when it reads the folder.
		let path = entry.path();
		if path.is_dir() {
			collect(&path, &relative, files)?;
		} else if path.is_file() {
			files.push(relative);
		}
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn wildcards() {
		assert!(wildcard("lua/*.lua", "lua/autorun/client/x.lua"));
		assert!(wildcard("models/*.vtx", "models/a/b.dx90.vtx"));
		assert!(wildcard("*thumbs.db", "materials/Thumbs.db"));
		assert!(wildcard(".git*", ".gitignore"));
		assert!(!wildcard(".git*", "lua/.gitkeep"));
		assert!(!wildcard("lua/*.lua", "lua/x.luac"));
		assert!(wildcard(
			"gamemodes/*/logo.png",
			"gamemodes/sandbox/logo.png"
		));
	}

	#[test]
	fn whitelist() {
		assert!(is_allowed("Materials/Models/Car.VMT"));
		assert!(is_allowed("resource/localization/en/x.properties"));
		assert!(!is_allowed("materials/car.psd"));
		assert!(!is_allowed("readme.txt"));
		assert!(!is_allowed("models/car.qc"));
	}

	#[test]
	fn checks_a_folder() {
		let temp = tempfile::tempdir().unwrap();
		let root = temp.path();
		for file in [
			"addon.json",
			"lua/autorun/a.lua",
			"materials/a.vmt",
			"materials/a.psd",
			"materials/Thumbs.db",
			"src/car.qc",
			".gitignore",
		] {
			let path = root.join(file);
			std::fs::create_dir_all(path.parent().unwrap()).unwrap();
			std::fs::write(path, "").unwrap();
		}
		std::fs::write(root.join("addon.json"), r#"{"ignore": ["*.psd"]}"#).unwrap();
		let result = check(root).unwrap();
		assert_eq!(result.invalid, ["src/car.qc"]);
		assert_eq!(result.packed, 2);
		assert_eq!(result.ignored, 3);
	}
}
