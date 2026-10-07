//! Shared staging for native project backends. All inputs are read before output mutation.
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeMap, path::Path};
use whisker_plugin::{FileEntry, project::*};
pub(crate) fn stage_declared(
    files: &mut BTreeMap<ProjectPath, FileEntry>,
    declared: &ProjectFiles,
    root: Option<&Path>,
) -> Result<()> {
    for (path, file) in declared {
        match file {
            ProjectFile::Generated { entry } => insert(files, path.clone(), entry.clone())?,
            ProjectFile::AppFile { source } | ProjectFile::AppDirectory { source } => {
                let root = root.context("app_crate_dir is required to stage app files")?;
                let source = root.join(source.as_str());
                ensure!(
                    !std::fs::symlink_metadata(&source)?.file_type().is_symlink(),
                    "symlink input is not supported: {}",
                    source.display()
                );
                ensure!(
                    source.canonicalize()?.starts_with(root.canonicalize()?),
                    "input escapes app crate"
                );
                ensure!(
                    source.is_dir() == matches!(file, ProjectFile::AppDirectory { .. }),
                    "input kind mismatch: {}",
                    source.display()
                );
                stage(files, path, &source)?;
            }
        }
    }
    Ok(())
}
pub(crate) fn insert(
    files: &mut BTreeMap<ProjectPath, FileEntry>,
    path: ProjectPath,
    entry: FileEntry,
) -> Result<()> {
    ensure!(
        !files.contains_key(&path),
        "multiple owners for generated output {}",
        path.as_str()
    );
    files.insert(path, entry);
    Ok(())
}
fn stage(
    files: &mut BTreeMap<ProjectPath, FileEntry>,
    dest: &ProjectPath,
    source: &Path,
) -> Result<()> {
    let metadata = std::fs::symlink_metadata(source)?;
    ensure!(
        !metadata.file_type().is_symlink(),
        "symlink input is not supported: {}",
        source.display()
    );
    if metadata.is_dir() {
        let mut children = std::fs::read_dir(source)?.collect::<std::io::Result<Vec<_>>>()?;
        children.sort_by_key(|e| e.file_name());
        for child in children {
            stage(
                files,
                &ProjectPath::new(format!(
                    "{}/{}",
                    dest.as_str(),
                    child
                        .file_name()
                        .to_str()
                        .context("non-UTF-8 input filename")?
                ))?,
                &child.path(),
            )?;
        }
    } else {
        ensure!(metadata.is_file(), "input is not a regular file");
        let entry = FileEntry::binary(&std::fs::read(source)?);
        #[cfg(unix)]
        let entry = {
            use std::os::unix::fs::PermissionsExt;
            let mut entry = entry;
            entry.mode = Some(metadata.permissions().mode() & 0o777);
            entry
        };
        insert(files, dest.clone(), entry)?;
    }
    Ok(())
}
pub(crate) fn check_destination(root: &Path, path: &Path) -> Result<()> {
    for ancestor in path.ancestors().take_while(|p| p.starts_with(root)) {
        if let Ok(meta) = std::fs::symlink_metadata(ancestor) {
            ensure!(
                !meta.file_type().is_symlink(),
                "refusing symlink output {}",
                ancestor.display()
            );
        }
    }
    Ok(())
}

pub(crate) fn validate(files: &BTreeMap<ProjectPath, FileEntry>) -> Result<()> {
    for (path, entry) in files {
        ensure!(
            path.as_str().split('/').next() != Some(".whisker-fingerprint"),
            "reserved generation fingerprint path"
        );
        for (i, _) in path.as_str().match_indices('/') {
            ensure!(
                !files.contains_key(&ProjectPath::new(&path.as_str()[..i])?),
                "overlapping output: {}",
                path.as_str()
            );
        }
        entry.to_bytes()?;
    }
    Ok(())
}
