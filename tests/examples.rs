// tests/examples.rs
//! The files in examples/ are what the playground, the samples page and the docs
//! show, so each one has to work: parse, survive source -> binary -> decode, and be
//! named in examples/index.json, which the playground reads (see
//! scripts/site/build_examples.py).

use std::fs;
use std::path::PathBuf;

use msx::{compile, decode, parse_scene, render};

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples")
}

fn example_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(examples_dir())
        .expect("examples/ is there")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "msx"))
        .collect();
    files.sort();
    files
}

fn normalise(svg: &str) -> String {
    svg.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn every_example_parses_and_survives_the_binary_roundtrip() {
    let files = example_files();
    assert!(files.len() >= 30, "found only {} examples", files.len());
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let source = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        let scene = parse_scene(&source).unwrap_or_else(|e| panic!("{name}: parse failed: {e}"));
        let before = normalise(&render(&scene));
        for compress in [false, true] {
            let data = compile(&scene, compress).unwrap_or_else(|e| panic!("{name}: compile failed: {e}"));
            let back = decode(&data).unwrap_or_else(|e| panic!("{name}: decode failed: {e}"));
            assert_eq!(before, normalise(&render(&back)), "{name}: SVG differs after a binary roundtrip (compress = {compress})");
        }
    }
}

#[test]
fn the_index_names_every_example() {
    let index = fs::read_to_string(examples_dir().join("index.json")).expect("examples/index.json");
    for path in example_files() {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        assert!(
            index.contains(&format!("\"{name}\"")),
            "{name} is not in examples/index.json: add it to \"examples\" (or to \"hidden\" for a duplicate)"
        );
    }
}

#[test]
fn every_svg_example_is_well_formed_enough_to_open() {
    // The importer has its own tests; this only keeps a stray file out of examples/svg/.
    let dir = examples_dir().join("svg");
    let mut seen = 0;
    for entry in fs::read_dir(&dir).expect("examples/svg/ is there").flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|x| x == "svg") {
            let text = fs::read_to_string(&path).unwrap();
            assert!(text.trim_start().starts_with("<svg"), "{}: does not start with <svg", path.display());
            assert!(text.trim_end().ends_with("</svg>"), "{}: does not end with </svg>", path.display());
            seen += 1;
        }
    }
    assert!(seen >= 4, "found only {seen} SVG examples");
}
