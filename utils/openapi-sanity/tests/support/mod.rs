//! A fixture router tree and the findings the gate reports for it.
//!
//! Shared by `contract.rs`, `auth.rs` and `tags.rs`: they drive the same analyzer
//! over the same trees, and a second loader would be a second thing to keep in step
//! with the fixture layout. Each test binary uses a different part of it, so the
//! module is not entirely reachable from any of them.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use openapi_sanity::{
    AuthRule, GuardClass, SourceUnit, check_auth, check_contract, check_tags, spec_operations,
};

/// The prefix the public artifact omits on purpose, as the gate is run in the
/// repository. The clean fixture's probe lives under it.
pub const TEST_PREFIX: &str = "/get/test/";

/// The policy the `clean/` fixture tree is clean under.
///
/// Spelled out rather than derived from the tree: a derived policy would agree
/// with whatever the fixture declares and prove nothing, and the point of the
/// clean tree is that a known set of guards, a known set of 401 declarations and
/// a matching policy produce no finding.
pub const CLEAN_POLICY: &[AuthRule] = &[
    AuthRule::guarded("get_data", &[GuardClass::Timestamp]),
    AuthRule::guarded("get_rows", &[GuardClass::AdminCookie]),
    AuthRule::guarded("path_completion", &[GuardClass::AdminCookie]),
    AuthRule::guarded("get_metadata", &[GuardClass::AdminCookie]),
    AuthRule::public("login"),
];

/// One fixture tree: its router files, its document, and the findings the gate
/// reports for them.
///
/// The files are owned here because a [`SourceUnit`] borrows its label and
/// contents; the tree has to outlive the units built from it. They are read in
/// sorted order so the loaded unit set never depends on directory iteration, and
/// `reversed()` is what shows the report does not depend on it either.
pub struct Fixture {
    root: PathBuf,
    files: Vec<(String, String, String)>,
}

impl Fixture {
    /// Read the router files and the document of a named fixture tree.
    pub fn load(tree: &str) -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(tree);
        Self::in_directory(&root)
    }

    /// Read the router files of any tree on disk, in the layout a router root
    /// has. Used to run the gate over the repository itself.
    pub fn in_directory(root: &Path) -> Self {
        let mut files: Vec<(String, String, String)> = router_files(root)
            .into_iter()
            .map(|relative| {
                let source = read(&root.join(&relative));
                (root.join(&relative).display().to_string(), relative, source)
            })
            .collect();
        assert!(
            !files.is_empty(),
            "{} has no router sources",
            root.display()
        );
        files.sort();

        Self {
            root: root.to_path_buf(),
            files,
        }
    }

    /// The same tree with the router files in reverse order.
    pub fn reversed(mut self) -> Self {
        self.files.reverse();
        self
    }

    /// Every finding of the source/spec contract gate for this tree, rendered as
    /// it is printed.
    pub fn findings(&self, excluded: &[&str]) -> Vec<String> {
        let units = self.units();
        let document = self.document();
        let spec = spec_operations(&document);
        let label = self.label("openapi.json");

        check_contract(&units, &label, &spec, excluded)
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    /// Every finding of the auth policy gate for this tree, rendered as it is
    /// printed, against the tree's own `openapi.json`.
    ///
    /// `check_auth` also reports what the source scan found, so a dropped guard
    /// binding shows up here as well as in [`Self::findings`].
    pub fn auth_findings(&self, excluded: &[&str], policy: &[AuthRule]) -> Vec<String> {
        self.auth_findings_against(&self.root.join("openapi.json"), excluded, policy)
    }

    /// The same, against a document that does not sit inside the router root —
    /// which is where the repository keeps it, two directories up.
    pub fn auth_findings_against(
        &self,
        spec: &Path,
        excluded: &[&str],
        policy: &[AuthRule],
    ) -> Vec<String> {
        let units = self.units();
        let document: serde_json::Value =
            serde_json::from_str(&read(spec)).expect("the document is valid JSON");
        let spec_operations = spec_operations(&document);

        check_auth(
            &units,
            &spec.display().to_string(),
            &spec_operations,
            excluded,
            policy,
        )
        .iter()
        .map(ToString::to_string)
        .collect()
    }

    pub fn units(&self) -> Vec<SourceUnit<'_>> {
        self.files
            .iter()
            .map(|(label, relative, source)| SourceUnit::for_relative_path(label, relative, source))
            .collect()
    }

    /// Every finding of the tag taxonomy gate for this tree, against the tree's
    /// own `openapi.json`.
    ///
    /// Document-shaped like the auth gate's document half, so the router sources
    /// are not read: a tag lives in the generated document, and the taxonomy is
    /// about what the reference groups by.
    pub fn tag_findings(&self, excluded: &[&str]) -> Vec<String> {
        tag_findings_against(&self.root.join("openapi.json"), excluded)
    }

    /// The label the checks see for a fixture file.
    pub fn label(&self, relative: &str) -> String {
        self.root.join(relative).display().to_string()
    }

    pub fn read(&self, relative: &str) -> String {
        read(&self.root.join(relative))
    }

    pub fn document(&self) -> serde_json::Value {
        serde_json::from_str(&self.read("openapi.json"))
            .unwrap_or_else(|error| panic!("fixture document is not valid JSON: {error}"))
    }

    /// The tree's router files, written to a writable directory so a test can
    /// mutate one of them and re-run the gate over the result.
    ///
    /// The mutation tests cannot edit the checked-in fixture: a rule that stopped
    /// reporting would leave the fixture expectations passing over a fixture
    /// nobody perturbed.
    pub fn materialise(&self, directory: &Path) -> Materialised {
        for (_, relative, source) in &self.files {
            let path = directory.join(relative);
            fs::create_dir_all(path.parent().expect("a fixture file has a parent"))
                .expect("the parent directory is created");
            fs::write(&path, source).expect("a fixture file is written");
        }
        let document = self.read("openapi.json");
        fs::write(directory.join("openapi.json"), &document).expect("the document is written");

        Materialised {
            label: directory.display().to_string(),
            root: directory.to_path_buf(),
        }
    }
}

/// A fixture tree written to disk, for a test that mutates it.
pub struct Materialised {
    label: String,
    root: PathBuf,
}

impl Materialised {
    /// The gate's report for the tree, rendered as it is printed.
    pub fn auth_findings(&self, policy: &[AuthRule]) -> Vec<String> {
        let mut labels = Vec::new();
        let mut relatives = Vec::new();
        let mut sources = Vec::new();
        for relative in router_files(&self.root) {
            labels.push(format!("{}/{relative}", self.label));
            sources.push(read(&self.root.join(&relative)));
            relatives.push(relative);
        }
        let units: Vec<SourceUnit<'_>> = labels
            .iter()
            .zip(&relatives)
            .zip(&sources)
            .map(|((label, relative), source)| {
                SourceUnit::for_relative_path(label, relative, source)
            })
            .collect();

        let document: serde_json::Value =
            serde_json::from_str(&read(&self.root.join("openapi.json"))).expect("valid JSON");
        let spec = spec_operations(&document);
        let label = format!("{}/openapi.json", self.label);

        check_auth(&units, &label, &spec, &[], policy)
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    /// Replace one file of the tree, so a test can perturb a conforming tree and
    /// see what the gate says about the result.
    pub fn replace(&self, relative: &str, source: &str) {
        fs::write(self.root.join(relative), source).expect("the fixture file is written");
    }
}

/// The tag findings of a document that is not a fixture's own — the committed
/// artifact, or a tree a mutation rewrote — labelled as a caller would print it.
pub fn tag_findings_against(spec: &Path, excluded: &[&str]) -> Vec<String> {
    let document: serde_json::Value =
        serde_json::from_str(&read(spec)).expect("the document is valid JSON");
    let operations = spec_operations(&document);

    check_tags(&spec.display().to_string(), &operations, excluded)
        .iter()
        .map(ToString::to_string)
        .collect()
}

/// Every router file of a tree, as a path relative to it, sorted.
fn router_files(root: &Path) -> Vec<String> {
    let mut relative = Vec::new();
    walk(root, root, &mut relative);
    relative.sort();
    relative
}

/// The `.rs` files under a directory, recorded relative to the tree root.
fn walk(root: &Path, directory: &Path, files: &mut Vec<String>) {
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", directory.display()));

    for entry in entries {
        let path = entry.expect("a directory entry is readable").path();
        if path.is_dir() {
            walk(root, &path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let relative = path
                .strip_prefix(root)
                .expect("a walked file is under the tree root")
                .display()
                .to_string();
            files.push(relative);
        }
    }
}

pub fn read(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}
