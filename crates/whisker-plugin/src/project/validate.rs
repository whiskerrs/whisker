//! Platform-independent structural validation without filesystem access

use super::*;
use anyhow::{Result, ensure};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub(super) fn project(project: &ProjectIr) -> Result<()> {
    match project {
        ProjectIr::Ios(ios) => super::apple::validate_apple(&ios.apple),
        ProjectIr::Macos(macos) => super::apple::validate_apple(&macos.apple),
        ProjectIr::Android(android) => {
            files(&android.files)?;
            super::android::validate_android(android)
        }
        ProjectIr::Windows(windows) => super::desktop::validate_windows(windows),
        ProjectIr::Linux(linux) => super::desktop::validate_linux(linux),
        ProjectIr::Web(web) => super::web::validate_web(web),
    }
}

pub(super) fn ids<'a>(ids: impl IntoIterator<Item = &'a String>) -> Result<()> {
    for id in ids {
        ensure!(
            !id.trim().is_empty() && !id.chars().any(char::is_control),
            "invalid empty/control-character project ID: {id:?}"
        );
    }
    Ok(())
}

pub(super) fn reference<T>(items: &BTreeMap<String, T>, id: &str, owner: &str) -> Result<()> {
    ensure!(
        items.contains_key(id),
        "{owner} references unknown ID {id:?}"
    );
    Ok(())
}

pub(super) fn files(files: &ProjectFiles) -> Result<()> {
    unique_paths(files.keys(), "staged project files")
}

pub(super) fn unique_paths<'a>(
    paths: impl IntoIterator<Item = &'a ProjectPath>,
    owner: &str,
) -> Result<()> {
    let mut all = BTreeSet::new();
    for path in paths {
        ensure!(
            all.insert(path.as_str()),
            "{owner}: duplicate destination {}",
            path.as_str()
        );
    }
    for path in &all {
        for (index, _) in path.match_indices('/') {
            let ancestor = &path[..index];
            ensure!(
                !all.contains(ancestor),
                "{owner}: overlapping destinations {ancestor} and {path}"
            );
        }
    }
    Ok(())
}

pub(super) fn acyclic(graph: BTreeMap<&str, BTreeSet<&str>>) -> Result<()> {
    let mut pending: BTreeMap<_, _> = graph.iter().map(|(id, edges)| (*id, edges.len())).collect();
    let mut dependents: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (id, edges) in &graph {
        for edge in edges {
            dependents.entry(edge).or_default().push(id);
        }
    }
    let mut ready: VecDeque<_> = pending
        .iter()
        .filter_map(|(id, count)| (*count == 0).then_some(*id))
        .collect();
    while let Some(id) = ready.pop_front() {
        for dependent in dependents.get(id).into_iter().flatten() {
            let count = pending
                .get_mut(dependent)
                .expect("validated graph reference");
            *count -= 1;
            if *count == 0 {
                ready.push_back(*dependent);
            }
        }
    }
    let blocked: Vec<_> = pending
        .into_iter()
        .filter_map(|(id, count)| (count != 0).then_some(id))
        .collect();
    ensure!(
        blocked.is_empty(),
        "cyclic project dependencies involving: {}",
        blocked.join(", ")
    );
    Ok(())
}
