use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use whisker_protocol::ResourceId;

use crate::WebError;

/// Browser URLs for resources that have completed Host-side acquisition and
/// decoding. Frame projection only reads this store; acquisition lifecycle is
/// intentionally kept outside frame transactions.
#[derive(Clone, Default)]
pub struct WebResourceStore {
    urls: Rc<RefCell<HashMap<ResourceId, String>>>,
}

impl WebResourceStore {
    /// Creates an empty resource store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers or replaces the browser URL for a ready resource.
    pub fn register_url(
        &self,
        resource: ResourceId,
        url: impl Into<String>,
    ) -> Result<(), WebError> {
        let url = url.into();
        if url.is_empty() {
            return Err(WebError("Web resource URL must not be empty".into()));
        }
        self.urls.borrow_mut().insert(resource, url);
        Ok(())
    }

    /// Removes a resource URL after its external lifecycle has released it.
    pub fn unregister(&self, resource: ResourceId) -> Option<String> {
        self.urls.borrow_mut().remove(&resource)
    }

    pub(crate) fn contains(&self, resource: ResourceId) -> bool {
        self.urls.borrow().contains_key(&resource)
    }

    pub(crate) fn url(&self, resource: ResourceId) -> Option<String> {
        self.urls.borrow().get(&resource).cloned()
    }
}
