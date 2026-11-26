use super::*;

fn write(path: &Path, content: &str) {
	std::fs::create_dir_all(path.parent().unwrap()).unwrap();
	std::fs::write(path, content).unwrap();
}

fn names(files: &[PathBuf], root: &Path) -> Vec<String> {
	files
		.iter()
		.map(|f| f.strip_prefix(root).unwrap().to_string_lossy().into_owned())
		.collect()
}

#[test]
fn follows_includes_pushd_and_bare_names() {
	let temp = tempfile::tempdir().unwrap();
	let root = temp.path();
	write(
		&root.join("crate.qc"),
		r#"$modelname "props/crate.mdl"
$include "shared.qci"
// $body "old" "old.smd"
/* $body "older" "older.smd" */
$body "Body" "Crate_Ref"
$pushd "anims"
$sequence "idle" "idle.smd"
$popd
$collisionmodel "phys.smd" { $mass 10 }
$sequence "walk" "missing.smd"
"#,
	);
	write(
		&root.join("Shared.QCI"),
		"$cdmaterials \"x\"\n$include \"crate.qc\"\n",
	);
	write(&root.join("crate_ref.smd"), "");
	write(&root.join("old.smd"), "");
	write(&root.join("anims/idle.smd"), "");
	write(&root.join("phys.smd"), "");
	// Not a source: the sequence name matching a folder must not count.
	std::fs::create_dir_all(root.join("idle")).unwrap();

	let deps = dependencies(&root.join("crate.qc"));
	assert_eq!(
		names(&deps, root),
		[
			"crate.qc",
			"Shared.QCI",
			"crate_ref.smd",
			"anims/idle.smd",
			"phys.smd"
		]
	);
}

#[test]
fn watcher_waits_for_a_change_to_settle() {
	let temp = tempfile::tempdir().unwrap();
	let qc = temp.path().join("a.qc");
	let smd = temp.path().join("a.smd");
	write(&qc, "$body \"b\" \"a.smd\"\n");
	write(&smd, "one");
	let qcs = vec![qc.clone()];
	let mut watcher = Watcher::default();

	// The first look only remembers.
	assert!(watcher.poll(&qcs).is_empty());
	assert!(watcher.poll(&qcs).is_empty());

	write(&smd, "two, longer");
	assert!(watcher.poll(&qcs).is_empty());
	assert_eq!(watcher.poll(&qcs), qcs);
	// Still changed until it is built again.
	assert_eq!(watcher.poll(&qcs), qcs);
	watcher.built(&qc);
	assert!(watcher.poll(&qcs).is_empty());

	// A file that disappears counts as a change too.
	std::fs::remove_file(&smd).unwrap();
	assert!(watcher.poll(&qcs).is_empty());
	assert_eq!(watcher.poll(&qcs), qcs);

	// QCs taken off the list are forgotten.
	assert!(watcher.poll(&[]).is_empty());
	assert!(watcher.poll(&qcs).is_empty());
}
