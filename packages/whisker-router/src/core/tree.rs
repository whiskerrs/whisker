//! The static [`RouteTree`] and its compiled, addressable form
//! [`CompiledTree`].
//!
//! A [`RouteTree`] is the hand-written description of the app's screen
//! structure. Because there is no `routes!` macro yet, trees are built
//! with the constructor helpers ([`RouteTree::route`],
//! [`RouteTree::stack`], [`RouteTree::switch`]).
//!
//! [`CompiledTree`] wraps a `RouteTree` and pre-computes, in a
//! pre-order walk, a flat table of node metadata keyed by [`NodeId`],
//! plus parent links and full URLs — everything resolution and URL
//! derivation need without re-walking the tree each time.

use std::collections::BTreeMap;

/// A stable identity for a node, assigned in pre-order at
/// [`CompiledTree`] build time.
///
/// Two *different* nodes always get different `NodeId`s. Logical
/// **nav-target identity** (the "shared route deduped to one target"
/// case) is expressed separately by [`RouteDef::id`], not by `NodeId`:
/// the same `post/:id` route spread into several tabs is several
/// distinct `NodeId`s that share one [`RouteDef::id`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub usize);

/// A positional address: the chain of child indices from the root to a
/// node.
///
/// The root is `NodePath(vec![])`. A `Switch`'s third branch reached
/// through the root stack's first entry is `NodePath(vec![0, 2])`.
/// `NodePath` is what [`RouteState::current`](crate::core::RouteState::current)
/// returns and what resolution produces.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct NodePath(pub Vec<usize>);

impl NodePath {
    /// The root path (empty).
    pub fn root() -> Self {
        NodePath(Vec::new())
    }

    /// A child path: `self` extended by `index`.
    pub fn child(&self, index: usize) -> Self {
        let mut v = self.0.clone();
        v.push(index);
        NodePath(v)
    }

    /// Depth (number of edges from the root).
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether this is the root path.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether `self` is an ancestor of (or equal to) `other`.
    pub fn is_ancestor_of(&self, other: &NodePath) -> bool {
        other.0.len() >= self.0.len() && other.0[..self.0.len()] == self.0[..]
    }
}

/// The param spec + identity of a leaf screen.
///
/// For this phase, params are just the **names** of the dynamic
/// segments (e.g. `["id"]` for `post/:id`); typed params are the
/// macro's job in a later phase.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteDef {
    /// The path segment as written, e.g. `""`, `"search"`,
    /// `"post/:id"`. May contain multiple `/`-joined parts and any
    /// number of `:name` dynamic segments.
    pub segment: String,
    /// The nav-target identity. The same logical route spread into
    /// several stacks shares one `id` (dedupe), even though each
    /// placement is a distinct [`NodeId`].
    pub id: String,
    /// The names of the dynamic (`:name`) segments, in order.
    pub params: Vec<String>,
}

impl RouteDef {
    /// Build a [`RouteDef`] from a segment and an id, extracting the
    /// `:name` param names from the segment automatically.
    pub fn new(segment: impl Into<String>, id: impl Into<String>) -> Self {
        let segment = segment.into();
        let params = segment
            .split('/')
            .filter_map(|p| p.strip_prefix(':').map(str::to_string))
            .collect();
        RouteDef {
            segment,
            id: id.into(),
            params,
        }
    }
}

/// Optional configuration for a [`RouteTree::Switch`].
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct SwitchDef {
    /// An optional identifier (for debugging / `within(scope)` targeting).
    pub id: Option<String>,
    /// The branch selected on cold start (when `selected` is otherwise
    /// undefined). `None` falls back to branch `0` (declaration order).
    pub default: Option<usize>,
}

impl SwitchDef {
    /// A switch with a named id and an explicit default branch.
    pub fn new(id: impl Into<String>, default: usize) -> Self {
        SwitchDef {
            id: Some(id.into()),
            default: Some(default),
        }
    }

    /// The default branch index (the declared default, else `0`).
    pub fn default_branch(&self) -> usize {
        self.default.unwrap_or(0)
    }
}

/// The static route structure: a tree of three node kinds.
///
/// Built by hand here with [`RouteTree::route`], [`RouteTree::stack`]
/// and [`RouteTree::switch`]. Wrap in a [`CompiledTree`] to address and
/// query it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RouteTree {
    /// A leaf screen with a path segment + param spec.
    Route(RouteDef),
    /// An **ordered** container: push/pop, has history.
    Stack(Vec<RouteTree>),
    /// A **parallel** container: keeps all branches alive, selects one,
    /// no history.
    Switch(SwitchDef, Vec<RouteTree>),
}

impl RouteTree {
    /// A leaf [`RouteTree::Route`] from a segment + id.
    pub fn route(segment: impl Into<String>, id: impl Into<String>) -> Self {
        RouteTree::Route(RouteDef::new(segment, id))
    }

    /// A [`RouteTree::Stack`] from its ordered children.
    pub fn stack(children: Vec<RouteTree>) -> Self {
        RouteTree::Stack(children)
    }

    /// A [`RouteTree::Switch`] from a [`SwitchDef`] and its branches.
    pub fn switch(def: SwitchDef, branches: Vec<RouteTree>) -> Self {
        RouteTree::Switch(def, branches)
    }

    /// The children of a container node (empty for a `Route`).
    pub fn children(&self) -> &[RouteTree] {
        match self {
            RouteTree::Route(_) => &[],
            RouteTree::Stack(c) => c,
            RouteTree::Switch(_, c) => c,
        }
    }
}

/// Per-node metadata pre-computed by [`CompiledTree`].
#[derive(Clone, Debug)]
pub struct NodeInfo {
    /// Stable identity (pre-order index).
    pub id: NodeId,
    /// Positional address.
    pub path: NodePath,
    /// Path of the parent (`None` for the root).
    pub parent: Option<NodePath>,
    /// The [`RouteDef::id`] if this is a `Route`, else `None`.
    pub route_id: Option<String>,
    /// The full URL of this node if it is a `Route`, else `None`.
    pub url: Option<String>,
}

/// A [`RouteTree`] wrapped with a pre-computed, addressable metadata
/// table.
///
/// Build with [`CompiledTree::new`]. Lookups by [`NodePath`] are
/// `O(depth)`; lookups by [`NodeId`] are `O(log n)`.
#[derive(Clone, Debug)]
pub struct CompiledTree {
    root: RouteTree,
    // NodePath → info, keyed by the path vec for ordered iteration.
    by_path: BTreeMap<Vec<usize>, NodeInfo>,
    by_id: BTreeMap<usize, NodePath>,
}

impl CompiledTree {
    /// Compile a [`RouteTree`], assigning [`NodeId`]s in pre-order and
    /// pre-computing parents + URLs.
    pub fn new(root: RouteTree) -> Self {
        let mut by_path = BTreeMap::new();
        let mut by_id = BTreeMap::new();
        let mut counter = 0usize;
        Self::walk(
            &root,
            NodePath::root(),
            None,
            String::new(),
            &mut counter,
            &mut by_path,
            &mut by_id,
        );
        CompiledTree {
            root,
            by_path,
            by_id,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn walk(
        node: &RouteTree,
        path: NodePath,
        parent: Option<NodePath>,
        url_so_far: String,
        counter: &mut usize,
        by_path: &mut BTreeMap<Vec<usize>, NodeInfo>,
        by_id: &mut BTreeMap<usize, NodePath>,
    ) {
        let id = NodeId(*counter);
        *counter += 1;

        let (route_id, this_url, child_url_base) = match node {
            RouteTree::Route(def) => {
                let url = join_url(&url_so_far, &def.segment);
                (Some(def.id.clone()), Some(url.clone()), url)
            }
            // Containers are pathless: they contribute nothing to the URL.
            RouteTree::Stack(_) | RouteTree::Switch(_, _) => (None, None, url_so_far.clone()),
        };

        by_path.insert(
            path.0.clone(),
            NodeInfo {
                id,
                path: path.clone(),
                parent,
                route_id,
                url: this_url,
            },
        );
        by_id.insert(id.0, path.clone());

        for (i, child) in node.children().iter().enumerate() {
            Self::walk(
                child,
                path.child(i),
                Some(path.clone()),
                child_url_base.clone(),
                counter,
                by_path,
                by_id,
            );
        }
    }

    /// The root node.
    pub fn root(&self) -> &RouteTree {
        &self.root
    }

    /// The node at `path`, if any.
    pub fn node_at(&self, path: &NodePath) -> Option<&RouteTree> {
        let mut node = &self.root;
        for &idx in &path.0 {
            node = node.children().get(idx)?;
        }
        Some(node)
    }

    /// Metadata for the node at `path`.
    pub fn info_at(&self, path: &NodePath) -> Option<&NodeInfo> {
        self.by_path.get(&path.0)
    }

    /// The [`NodePath`] of a node addressed by [`NodeId`].
    pub fn path_of_id(&self, id: NodeId) -> Option<&NodePath> {
        self.by_id.get(&id.0)
    }

    /// The full URL of the route at `path` (pathless containers → `None`).
    ///
    /// This is the segment-concatenation derivation: walk from the root,
    /// joining each `Route`'s static segment parts; containers
    /// contribute nothing.
    pub fn url_of(&self, path: &NodePath) -> Option<String> {
        self.info_at(path).and_then(|i| i.url.clone())
    }

    /// All [`NodePath`]s whose `Route` has the given [`RouteDef::id`],
    /// in **declaration order** (pre-order).
    pub fn paths_with_route_id(&self, route_id: &str) -> Vec<NodePath> {
        self.by_path
            .values()
            .filter(|i| i.route_id.as_deref() == Some(route_id))
            .map(|i| i.path.clone())
            .collect()
    }

    /// All [`NodePath`]s whose `Route` derives the given full URL, in
    /// declaration order. (Two different ids may share a URL — the
    /// resolvable-ambiguity case.)
    pub fn paths_with_url(&self, url: &str) -> Vec<NodePath> {
        self.by_path
            .values()
            .filter(|i| i.url.as_deref() == Some(url))
            .map(|i| i.path.clone())
            .collect()
    }

    /// Iterate every node's metadata in declaration (pre-order) order.
    pub fn iter_infos(&self) -> impl Iterator<Item = &NodeInfo> {
        self.by_path.values()
    }
}

/// Join the static parts of a route segment onto an accumulated URL.
///
/// `:name` dynamic parts are kept as `:name` placeholders in the URL so
/// `post/:id` derives `/post/:id`. A pathless / empty segment leaves the
/// base unchanged (apart from ensuring a leading `/` at the root).
fn join_url(base: &str, segment: &str) -> String {
    // Build from the segment's own parts, preserving order and :params.
    let seg_parts: Vec<&str> = segment.split('/').filter(|p| !p.is_empty()).collect();

    if seg_parts.is_empty() {
        // Pathless route (e.g. the "" home route). Its URL is the base,
        // normalised to "/" at the root.
        if base.is_empty() {
            return "/".to_string();
        }
        return base.to_string();
    }

    let mut url = if base.is_empty() || base == "/" {
        String::new()
    } else {
        base.to_string()
    };
    for part in seg_parts {
        url.push('/');
        url.push_str(part);
    }
    url
}
