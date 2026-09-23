//! Shared ordering rules for legacy and declarative project plugins.
use anyhow::{Result, anyhow, bail};
use std::collections::{BTreeMap, HashMap, VecDeque};

pub(crate) struct Order<'a> {
    pub name: &'a str,
    pub after: Vec<&'a str>,
    pub before: Vec<&'a str>,
}

/// Kahn's algorithm with deterministic ordering: ties between
/// candidates are broken alphabetically by plugin name so the same
/// `(plugins, Config)` pair always produces the same execution
/// order. The fingerprint path downstream depends on this.
pub(crate) fn sort(plugins: &[Order<'_>]) -> Result<Vec<usize>> {
    let mut name_to_idx: BTreeMap<&str, usize> = BTreeMap::new();
    for (i, p) in plugins.iter().enumerate() {
        if name_to_idx.insert(p.name, i).is_some() {
            bail!("two plugins registered with the same name `{}`", p.name);
        }
    }

    // `X.after(Y)` and `Y.before(X)` both produce the edge `Y → X`.
    let mut succ: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut in_degree: Vec<usize> = vec![0; plugins.len()];

    let resolve = |this_name: &str, target_name: &str, kind: &str| -> Result<usize> {
        name_to_idx.get(target_name).copied().ok_or_else(|| {
            anyhow!(
                "plugin `{this_name}` declares {kind}(`{target_name}`), \
                 but no plugin with that name is registered"
            )
        })
    };

    for (i, p) in plugins.iter().enumerate() {
        for after_name in p.after.iter().copied() {
            let j = resolve(p.name, after_name, "after")?;
            if j == i {
                bail!("plugin `{}` lists itself in after()", p.name);
            }
            succ.entry(j).or_default().push(i);
            in_degree[i] += 1;
        }
        for before_name in p.before.iter().copied() {
            let j = resolve(p.name, before_name, "before")?;
            if j == i {
                bail!("plugin `{}` lists itself in before()", p.name);
            }
            succ.entry(i).or_default().push(j);
            in_degree[j] += 1;
        }
    }

    let mut queue: VecDeque<usize> = VecDeque::new();
    let mut candidates: Vec<usize> = (0..plugins.len()).filter(|&i| in_degree[i] == 0).collect();
    candidates.sort_by_key(|&i| plugins[i].name);
    queue.extend(candidates);

    let mut order = Vec::with_capacity(plugins.len());
    while let Some(i) = queue.pop_front() {
        order.push(i);
        if let Some(succs) = succ.get(&i) {
            let mut newly_ready: Vec<usize> = Vec::new();
            for &j in succs {
                in_degree[j] -= 1;
                if in_degree[j] == 0 {
                    newly_ready.push(j);
                }
            }
            newly_ready.sort_by_key(|&j| plugins[j].name);
            queue.extend(newly_ready);
        }
    }

    if order.len() != plugins.len() {
        let unfinished: Vec<&str> = (0..plugins.len())
            .filter(|i| !order.contains(i))
            .map(|i| plugins[i].name)
            .collect();
        bail!("plugin ordering cycle involving: {}", unfinished.join(", "));
    }

    Ok(order)
}
