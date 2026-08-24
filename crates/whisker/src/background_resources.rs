//! Runtime-owned lowering from semantic background URLs to Host resources.

use std::collections::{HashMap, HashSet};

use whisker_engine::whisker_protocol::{
    BackgroundAttachment, BackgroundLayer, BackgroundSize, BlendMode, ImageRepeat, NodeId,
    PaintBox, PaintImage, PaintPosition, ResourceCommand, ResourceEvent, ResourceId, ResourceKind,
    ResourceRequest, ResourceSource,
};
use whisker_engine::whisker_style::BackgroundImageValue;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ResourceKey {
    kind: ResourceKind,
    source: ResourceSourceKey,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ResourceSourceKey {
    Url(String),
}

impl ResourceKey {
    fn raster_url(url: String) -> Self {
        Self {
            kind: ResourceKind::RasterImage,
            source: ResourceSourceKey::Url(url),
        }
    }

    fn source(&self) -> ResourceSource {
        match &self.source {
            ResourceSourceKey::Url(url) => ResourceSource::Url(url.clone()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum DesiredImage {
    None,
    Resource(ResourceKey),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResourcePhase {
    Pending,
    Ready,
    Failed,
}

#[derive(Clone, Debug)]
struct ResourceEntry {
    key: ResourceKey,
    resource: ResourceId,
    generation: u64,
    phase: ResourcePhase,
    desired_users: HashSet<NodeId>,
    projected_users: HashSet<NodeId>,
    accepted_users: HashSet<NodeId>,
    retirement_pending: bool,
}

/// Failure while assigning an internal resource identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BackgroundResourceError {
    InvalidSource,
    ResourceIdExhausted,
}

/// Scene projection and resource commands produced by one style mutation.
pub(super) struct ReconcileResult {
    pub(super) layers: Vec<BackgroundLayer>,
    pub(super) commands: Vec<ResourceCommand>,
}

/// One deferred projection made safe at a runtime frame boundary.
pub(super) struct BackgroundProjection {
    pub(super) node: NodeId,
    pub(super) layers: Vec<BackgroundLayer>,
}

/// Per-surface resource cache shared by every node using the same exact source.
///
/// IDs are never reused. A source that is reacquired before its retirement
/// frame is accepted cancels retirement and keeps its ID. Once Release is
/// emitted, the same source receives a fresh ID with generation one.
#[derive(Clone, Debug)]
pub(super) struct BackgroundResourceManager {
    next_resource_id: u64,
    resources_by_key: HashMap<ResourceKey, ResourceId>,
    entries: HashMap<ResourceId, ResourceEntry>,
    owned_ids: HashSet<ResourceId>,
    node_images: HashMap<NodeId, Vec<DesiredImage>>,
    dirty_nodes: HashSet<NodeId>,
}

impl Default for BackgroundResourceManager {
    fn default() -> Self {
        Self {
            next_resource_id: 1,
            resources_by_key: HashMap::new(),
            entries: HashMap::new(),
            owned_ids: HashSet::new(),
            node_images: HashMap::new(),
            dirty_nodes: HashSet::new(),
        }
    }
}

impl BackgroundResourceManager {
    pub(super) fn owns(&self, resource: ResourceId) -> bool {
        self.owned_ids.contains(&resource)
    }

    pub(super) fn reconcile_node(
        &mut self,
        node: NodeId,
        images: &[BackgroundImageValue],
        externally_used: &HashSet<ResourceId>,
    ) -> Result<ReconcileResult, BackgroundResourceError> {
        let desired = images
            .iter()
            .map(|image| match image {
                BackgroundImageValue::None => Ok(DesiredImage::None),
                BackgroundImageValue::Url(url) if url.trim().is_empty() => {
                    Err(BackgroundResourceError::InvalidSource)
                }
                BackgroundImageValue::Url(url) => {
                    Ok(DesiredImage::Resource(ResourceKey::raster_url(url.clone())))
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let previous = self.node_images.get(&node).cloned().unwrap_or_default();
        let previous_keys = resource_keys(&previous);
        let desired_keys = resource_keys(&desired);
        let mut commands = Vec::new();

        // Acquire first so an unchanged source, or one whose retirement has not
        // crossed the Host boundary, never churns its identity.
        for key in desired_keys.difference(&previous_keys) {
            commands.extend(self.acquire(node, key.clone(), externally_used)?);
        }
        for key in desired_keys.intersection(&previous_keys) {
            let resource = self.resources_by_key[key];
            let entry = self
                .entries
                .get_mut(&resource)
                .expect("a shared key always names a live entry");
            entry.desired_users.insert(node);
            entry.retirement_pending = false;
        }

        self.node_images.insert(node, desired);
        self.refresh_projected_node(node);

        for key in previous_keys.difference(&desired_keys) {
            commands.extend(self.drop_user(node, key));
        }
        if images.is_empty() {
            self.node_images.remove(&node);
        }
        self.dirty_nodes.remove(&node);

        Ok(ReconcileResult {
            layers: self.projected_layers(node),
            commands,
        })
    }

    pub(super) fn remove_nodes(&mut self, nodes: &[NodeId]) -> Vec<ResourceCommand> {
        let mut commands = Vec::new();
        for node in nodes {
            let previous = self.node_images.remove(node).unwrap_or_default();
            self.dirty_nodes.remove(node);
            for key in resource_keys(&previous) {
                if let Some(resource) = self.resources_by_key.get(&key).copied()
                    && let Some(entry) = self.entries.get_mut(&resource)
                {
                    entry.projected_users.remove(node);
                }
                commands.extend(self.drop_user(*node, &key));
            }
        }
        commands
    }

    /// Records a Host completion without mutating the retained scene.
    ///
    /// The caller flushes `dirty_projections` at the next safe layout/present
    /// boundary, because a Host callback may arrive while a frame is prepared.
    pub(super) fn apply_event(&mut self, event: &ResourceEvent) -> bool {
        let (resource, generation, phase) = match event {
            ResourceEvent::Ready {
                resource,
                generation,
                ..
            } => (*resource, *generation, ResourcePhase::Ready),
            ResourceEvent::Failed {
                resource,
                generation,
                ..
            } => (*resource, *generation, ResourcePhase::Failed),
        };
        let Some(entry) = self.entries.get_mut(&resource) else {
            return false;
        };
        if entry.generation != generation {
            return true;
        }
        entry.phase = phase;
        self.dirty_nodes.extend(entry.desired_users.iter().copied());
        true
    }

    pub(super) fn dirty_projections(&self) -> Vec<BackgroundProjection> {
        let mut nodes = self.dirty_nodes.iter().copied().collect::<Vec<_>>();
        nodes.sort_by_key(|node| node.get());
        nodes
            .into_iter()
            .filter(|node| self.node_images.contains_key(node))
            .map(|node| BackgroundProjection {
                node,
                layers: self.projected_layers(node),
            })
            .collect()
    }

    pub(super) fn commit_dirty_projections(&mut self, projections: &[BackgroundProjection]) {
        for projection in projections {
            self.refresh_projected_node(projection.node);
            self.dirty_nodes.remove(&projection.node);
        }
    }

    /// Advances the accepted-reference ledger after Scene accepted the frame.
    pub(super) fn accept_frame(&mut self) -> Vec<ResourceCommand> {
        for entry in self.entries.values_mut() {
            entry.accepted_users.clone_from(&entry.projected_users);
        }
        let mut retire = self
            .entries
            .iter()
            .filter_map(|(resource, entry)| {
                (entry.retirement_pending
                    && entry.desired_users.is_empty()
                    && entry.accepted_users.is_empty())
                .then_some(*resource)
            })
            .collect::<Vec<_>>();
        retire.sort_by_key(|resource| resource.get());
        retire
            .into_iter()
            .map(|resource| self.finish_release(resource))
            .collect()
    }

    fn acquire(
        &mut self,
        node: NodeId,
        key: ResourceKey,
        externally_used: &HashSet<ResourceId>,
    ) -> Result<Vec<ResourceCommand>, BackgroundResourceError> {
        if let Some(resource) = self.resources_by_key.get(&key).copied() {
            let entry = self
                .entries
                .get_mut(&resource)
                .expect("a shared key always names a live entry");
            entry.desired_users.insert(node);
            entry.retirement_pending = false;
            return Ok(Vec::new());
        }

        let resource = self.allocate_id(externally_used)?;
        let generation = 1;
        self.owned_ids.insert(resource);
        self.resources_by_key.insert(key.clone(), resource);
        self.entries.insert(
            resource,
            ResourceEntry {
                key: key.clone(),
                resource,
                generation,
                phase: ResourcePhase::Pending,
                desired_users: HashSet::from([node]),
                projected_users: HashSet::new(),
                accepted_users: HashSet::new(),
                retirement_pending: false,
            },
        );
        Ok(vec![ResourceCommand::Load(ResourceRequest {
            resource,
            generation,
            kind: key.kind,
            source: key.source(),
        })])
    }

    fn allocate_id(
        &mut self,
        externally_used: &HashSet<ResourceId>,
    ) -> Result<ResourceId, BackgroundResourceError> {
        while self.next_resource_id != 0 {
            let raw = self.next_resource_id;
            self.next_resource_id = raw.checked_add(1).unwrap_or(0);
            let resource = ResourceId::new(raw).expect("the allocator skips reserved zero");
            if !self.owned_ids.contains(&resource) && !externally_used.contains(&resource) {
                return Ok(resource);
            }
        }
        Err(BackgroundResourceError::ResourceIdExhausted)
    }

    fn drop_user(&mut self, node: NodeId, key: &ResourceKey) -> Vec<ResourceCommand> {
        let Some(resource) = self.resources_by_key.get(key).copied() else {
            return Vec::new();
        };
        let entry = self
            .entries
            .get_mut(&resource)
            .expect("a shared key always names a live entry");
        entry.desired_users.remove(&node);
        entry.projected_users.remove(&node);
        if !entry.desired_users.is_empty() {
            return Vec::new();
        }
        if entry.accepted_users.is_empty() {
            vec![self.finish_release(resource)]
        } else {
            entry.retirement_pending = true;
            Vec::new()
        }
    }

    fn finish_release(&mut self, resource: ResourceId) -> ResourceCommand {
        let entry = self
            .entries
            .remove(&resource)
            .expect("only a live automatic resource can retire");
        if self.resources_by_key.get(&entry.key) == Some(&resource) {
            self.resources_by_key.remove(&entry.key);
        }
        ResourceCommand::Release {
            resource: entry.resource,
            generation: entry.generation,
        }
    }

    fn refresh_projected_node(&mut self, node: NodeId) {
        for entry in self.entries.values_mut() {
            entry.projected_users.remove(&node);
        }
        let keys = self
            .node_images
            .get(&node)
            .map(|images| resource_keys(images))
            .unwrap_or_default();
        for key in keys {
            let resource = self.resources_by_key[&key];
            let entry = self
                .entries
                .get_mut(&resource)
                .expect("a desired key always names a live entry");
            if entry.phase == ResourcePhase::Ready {
                entry.projected_users.insert(node);
            }
        }
    }

    fn projected_layers(&self, node: NodeId) -> Vec<BackgroundLayer> {
        self.node_images
            .get(&node)
            .into_iter()
            .flatten()
            .filter_map(|image| match image {
                DesiredImage::None => None,
                DesiredImage::Resource(key) => {
                    let resource = self.resources_by_key[key];
                    (self.entries[&resource].phase == ResourcePhase::Ready)
                        .then(|| initial_resource_layer(resource))
                }
            })
            .collect()
    }
}

fn resource_keys(images: &[DesiredImage]) -> HashSet<ResourceKey> {
    images
        .iter()
        .filter_map(|image| match image {
            DesiredImage::None => None,
            DesiredImage::Resource(key) => Some(key.clone()),
        })
        .collect()
}

fn initial_resource_layer(resource: ResourceId) -> BackgroundLayer {
    BackgroundLayer {
        image: PaintImage::Resource(resource),
        position: PaintPosition::default(),
        size: BackgroundSize::Auto,
        repeat_x: ImageRepeat::Repeat,
        repeat_y: ImageRepeat::Repeat,
        origin: PaintBox::Padding,
        clip: PaintBox::Border,
        attachment: BackgroundAttachment::Scroll,
        blend_mode: BlendMode::Normal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(value: u64) -> NodeId {
        NodeId::new(value).unwrap()
    }

    fn url(value: &str) -> BackgroundImageValue {
        BackgroundImageValue::Url(value.into())
    }

    fn load(command: &ResourceCommand) -> (ResourceId, u64) {
        let ResourceCommand::Load(request) = command else {
            panic!("expected load")
        };
        (request.resource, request.generation)
    }

    #[test]
    fn exact_url_is_shared_and_only_ready_resources_project() {
        let mut manager = BackgroundResourceManager::default();
        let first = manager
            .reconcile_node(
                node(1),
                &[url("https://example.test/a.png")],
                &HashSet::new(),
            )
            .unwrap();
        assert_eq!(first.commands.len(), 1);
        assert!(first.layers.is_empty());
        let (resource, generation) = load(&first.commands[0]);

        let second = manager
            .reconcile_node(
                node(2),
                &[url("https://example.test/a.png")],
                &HashSet::new(),
            )
            .unwrap();
        assert!(second.commands.is_empty());
        assert!(manager.apply_event(&ResourceEvent::Ready {
            resource,
            generation,
            dimensions: None,
        }));
        let projections = manager.dirty_projections();
        assert_eq!(projections.len(), 2);
        assert!(projections.iter().all(|projection| {
            projection.layers.len() == 1
                && projection.layers[0].image == PaintImage::Resource(resource)
        }));
    }

    #[test]
    fn accepted_reference_defers_release_and_reacquire_cancels_retirement() {
        let mut manager = BackgroundResourceManager::default();
        let acquired = manager
            .reconcile_node(node(1), &[url("same")], &HashSet::new())
            .unwrap();
        let (resource, generation) = load(&acquired.commands[0]);
        manager.apply_event(&ResourceEvent::Ready {
            resource,
            generation,
            dimensions: None,
        });
        let projections = manager.dirty_projections();
        manager.commit_dirty_projections(&projections);
        assert!(manager.accept_frame().is_empty());

        let removed = manager
            .reconcile_node(node(1), &[], &HashSet::new())
            .unwrap();
        assert!(removed.commands.is_empty());
        let reacquired = manager
            .reconcile_node(node(1), &[url("same")], &HashSet::new())
            .unwrap();
        assert!(reacquired.commands.is_empty());
        assert_eq!(reacquired.layers[0].image, PaintImage::Resource(resource));
        assert!(manager.accept_frame().is_empty());
    }

    #[test]
    fn source_reacquired_after_release_gets_fresh_id() {
        let mut manager = BackgroundResourceManager::default();
        let acquired = manager
            .reconcile_node(node(1), &[url("same")], &HashSet::new())
            .unwrap();
        let (first, _) = load(&acquired.commands[0]);
        let removed = manager
            .reconcile_node(node(1), &[], &HashSet::new())
            .unwrap();
        assert_eq!(
            removed.commands,
            vec![ResourceCommand::Release {
                resource: first,
                generation: 1,
            }]
        );
        let reacquired = manager
            .reconcile_node(node(1), &[url("same")], &HashSet::new())
            .unwrap();
        let (second, generation) = load(&reacquired.commands[0]);
        assert_ne!(first, second);
        assert_eq!(generation, 1);
    }
}
