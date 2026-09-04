use super::*;

impl DynRenderer for SurfaceRuntime {
    fn create_element(&self, tag: ElementTag) -> Element {
        let mut state = self.state.borrow_mut();
        match state.allocate(tag) {
            Ok(element) => element,
            Err(error) => {
                state.record(Err(error));
                Element::from_raw(u32::MAX)
            }
        }
    }

    fn create_element_by_name(&self, tag_name: &str) -> Element {
        let mut state = self.state.borrow_mut();
        match state.allocate_named(tag_name) {
            Ok(element) => element,
            Err(error) => {
                state.record(Err(error));
                Element::from_raw(u32::MAX)
            }
        }
    }

    fn create_element_by_schema(&self, schema: &ElementSchema) -> Element {
        let mut state = self.state.borrow_mut();
        let registration = state.registry.registration_for_name(&schema.name);
        let result = match registration {
            None => Err(RuntimeBindingError::UnsupportedCustomElement {
                name: schema.name.clone(),
            }),
            Some(registration) if registration.schema() != *schema => Err(
                RuntimeBindingError::ElementRegistry(ElementRegistryError::ConflictingSchema {
                    name: schema.name.clone(),
                }),
            ),
            Some(_) => state.allocate_named(&schema.name),
        };
        match result {
            Ok(element) => element,
            Err(error) => {
                state.record(Err(error));
                Element::from_raw(u32::MAX)
            }
        }
    }

    fn release_element(&self, handle: Element) {
        let mut state = self.state.borrow_mut();
        let result = (|| {
            let Some(entry) = state.elements.remove(&handle) else {
                return Ok(());
            };
            if let Some(parent) = entry.parent
                && let Some(parent_entry) = state.elements.get_mut(&parent)
            {
                parent_entry.children.retain(|child| *child != handle);
            }
            if let Some(node) = entry.node {
                state.node_elements.remove(&node);
                if state.surface.node(node).is_some() {
                    let removed_nodes = state.surface_subtree(node);
                    let mut background_resources = state.background_resources.clone();
                    let resource_commands = background_resources.remove_nodes(&removed_nodes);
                    state.surface.delete_node(node)?;
                    state.background_resources = background_resources;
                    state.enqueue_automatic_commands(resource_commands);
                }
            }
            Ok(())
        })();
        state.record(result);
    }

    fn set_attribute(&self, handle: Element, key: &str, value: &str) {
        let mut state = self.state.borrow_mut();
        let result = state.set_attribute(handle, key, value);
        state.record(result);
    }

    fn set_element_id(&self, handle: Element, id: String) {
        let mut state = self.state.borrow_mut();
        let result = state.element_mut(handle).map(|entry| entry.id = id);
        state.record(result);
    }

    fn set_dataset(&self, handle: Element, dataset: Dataset) {
        let mut state = self.state.borrow_mut();
        let result = state
            .element_mut(handle)
            .map(|entry| entry.dataset = dataset.as_map().clone());
        state.record(result);
    }

    fn set_accessibility(&self, handle: Element, accessibility: Accessibility) {
        let mut state = self.state.borrow_mut();
        let result = (|| {
            let node = state
                .element(handle)?
                .node
                .ok_or(RuntimeBindingError::InvalidRoot { element: handle })?;
            state.element_mut(handle)?.accessibility = accessibility.clone();
            state.surface.set_accessibility(node, accessibility)?;
            Ok(())
        })();
        state.record(result);
    }

    fn set_text_max_lines(&self, handle: Element, max_lines: u32) {
        let mut state = self.state.borrow_mut();
        let result =
            (|| {
                let entry = state.element_mut(handle)?;
                let text = entry.text.as_mut().ok_or_else(|| {
                    RuntimeBindingError::UnsupportedAttribute {
                        element: handle,
                        name: "max-lines".to_owned(),
                    }
                })?;
                text.max_lines = (max_lines > 0).then_some(max_lines);
                state.apply_subtree(handle)
            })();
        state.record(result);
    }

    fn set_attribute_int(&self, handle: Element, key: &str, value: i64) {
        let mut state = self.state.borrow_mut();
        let result = state.set_property_value(handle, key, WhiskerValue::Int(value));
        state.record(result);
    }

    fn set_attribute_bool(&self, handle: Element, key: &str, value: bool) {
        let mut state = self.state.borrow_mut();
        let result = state.set_property_value(handle, key, WhiskerValue::Bool(value));
        state.record(result);
    }

    fn set_attribute_object(&self, handle: Element, key: &str, object: &[(String, f64)]) {
        let mut state = self.state.borrow_mut();
        let value = WhiskerValue::Map(
            object
                .iter()
                .map(|(name, value)| (name.clone(), WhiskerValue::Float(*value)))
                .collect(),
        );
        let result = state.set_property_value(handle, key, value);
        state.record(result);
    }

    fn set_attribute_double(&self, handle: Element, key: &str, value: f64) {
        let mut state = self.state.borrow_mut();
        let result = state.set_property_value(handle, key, WhiskerValue::Float(value));
        state.record(result);
    }

    fn set_specified_style(&self, handle: Element, style: &SpecifiedStyle) -> bool {
        let mut state = self.state.borrow_mut();
        let result = (|| {
            let entry = state.element(handle)?;
            if &entry.specified == style {
                if !entry.style_initialized {
                    state.element_mut(handle)?.style_initialized = true;
                }
                return Ok(());
            }
            let previous = entry.specified.clone();
            let batched = state.mutation_batch.is_some();
            let capture_change = batched
                && !state.mutation_batch.as_ref().is_some_and(|batch| {
                    batch
                        .style_changes
                        .iter()
                        .any(|(element, _)| *element == handle)
                });
            let snapshots = (!batched || capture_change)
                .then(|| state.motion_snapshots(handle))
                .transpose()?;
            state.element_mut(handle)?.specified = style.clone();
            if batched {
                if let Some(snapshots) = snapshots {
                    state
                        .mutation_batch
                        .as_mut()
                        .expect("a batched style change keeps its transaction")
                        .style_changes
                        .push((
                            handle,
                            PendingStyleChange {
                                previous,
                                snapshots,
                            },
                        ));
                }
                state.mark_subtree_dirty(handle);
                return Ok(());
            }
            if let Err(error) = state.apply_subtree(handle) {
                state.element_mut(handle)?.specified = previous;
                return Err(error);
            }
            state.configure_style_motion(
                snapshots.expect("an immediate style change captures motion state"),
            )
        })();
        let accepted = result.is_ok();
        state.record(result);
        accepted
    }

    fn specified_style(&self, handle: Element) -> Option<SpecifiedStyle> {
        self.state
            .borrow()
            .elements
            .get(&handle)
            .map(|entry| entry.specified.clone())
    }

    fn append_child(&self, parent: Element, child: Element) {
        let mut state = self.state.borrow_mut();
        let result = state.insert(parent, child, None);
        state.record(result);
    }

    fn remove_child(&self, parent: Element, child: Element) {
        let mut state = self.state.borrow_mut();
        let result = state.detach(parent, child);
        state.record(result);
    }

    fn supports_insert_before(&self) -> bool {
        true
    }

    fn insert_child_before(&self, parent: Element, child: Element, reference: Option<Element>) {
        let mut state = self.state.borrow_mut();
        let result = state.insert(parent, child, reference);
        state.record(result);
    }

    fn set_event_listener(
        &self,
        handle: Element,
        event_name: &str,
        bind_type: BindType,
        callback: Box<dyn Fn(WhiskerValue) + 'static>,
    ) {
        let mut state = self.state.borrow_mut();
        let result = (|| {
            let node = state
                .element(handle)?
                .node
                .ok_or(RuntimeBindingError::InvalidRoot { element: handle })?;
            state
                .element_mut(handle)?
                .listeners
                .entry(event_name.to_owned())
                .or_default()
                .push(RuntimeListener {
                    bind_type,
                    callback: Rc::from(callback),
                });
            let entry = state.element(handle)?;
            let mask = entry
                .listeners
                .keys()
                .fold(0, |mask, name| mask | event_mask(&entry.kind, name));
            state.surface.set_event_mask(node, mask)?;
            state.surface.set_hit_test(node, HitTestBehavior::Auto)?;
            Ok(())
        })();
        state.record(result);
    }

    fn observe_layout(&self, handle: Element, callback: Box<dyn Fn(LayoutObservation) + 'static>) {
        let mut state = self.state.borrow_mut();
        let result = state.element_mut(handle).map(|entry| {
            let observers = entry
                .layout_observers
                .get_or_insert_with(|| Box::new(super::LayoutObservers::default()));
            observers.callbacks.push(Rc::from(callback));
            // A newly registered observer must receive the current geometry
            // on the next completed layout even when it equals the geometry
            // seen by older observers.
            observers.last_notified = None;
        });
        state.record(result);
    }

    fn observe_layout_batch_end(&self, handle: Element, callback: Box<dyn Fn() + 'static>) {
        let mut state = self.state.borrow_mut();
        let result = state
            .element_mut(handle)
            .map(|entry| entry.layout_batch_end_observers.push(Rc::from(callback)));
        state.record(result);
    }

    fn invoke_element_command(
        &self,
        handle: Element,
        command: &str,
        parameters: WhiskerValue,
    ) -> Option<Result<(), String>> {
        let mut state = self.state.borrow_mut();
        Some(match state.invoke_command(handle, command, &parameters) {
            Ok(()) => {
                state.record(Ok(()));
                Ok(())
            }
            Err(error) => {
                let message = error.to_string();
                state.record(Err(error));
                Err(message)
            }
        })
    }

    fn set_root(&self, root: Element) {
        let mut state = self.state.borrow_mut();
        let result = match state.element(root) {
            Ok(entry) => match entry.node {
                Some(node) => {
                    state.root = Some(node);
                    Ok(())
                }
                None => Err(RuntimeBindingError::InvalidRoot { element: root }),
            },
            Err(error) => Err(error),
        };
        state.record(result);
    }

    fn flush(&self) {}
}
