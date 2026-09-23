use super::*;
use crate::project::merge::{
    Merge, equal, named, objects, optional, optional_object, public_merge, values,
};
use anyhow::Result;

public_merge!(WebProjectIr, WebAppManifest);
impl Merge for WebProjectIr {
    fn merge(&mut self, b: &Self, p: &str) -> Result<()> {
        optional(&mut self.wasm, &b.wasm, &format!("{p}.wasm"))?;
        optional(&mut self.document, &b.document, &format!("{p}.document"))?;
        if self.title.is_empty() {
            self.title = b.title.clone();
        } else if !b.title.is_empty() {
            equal(&self.title, &b.title, &format!("{p}.title"))?;
        }
        if self.lang.is_empty() {
            self.lang = b.lang.clone();
        } else if !b.lang.is_empty() {
            equal(&self.lang, &b.lang, &format!("{p}.lang"))?;
        }
        if self.base_path.is_empty() {
            self.base_path = b.base_path.clone();
        } else if !b.base_path.is_empty() {
            equal(&self.base_path, &b.base_path, &format!("{p}.base_path"))?;
        }
        values(
            &mut self.generated_artifacts,
            &b.generated_artifacts,
            &format!("{p}.generated_artifacts"),
        )?;
        values(
            &mut self.html_attributes,
            &b.html_attributes,
            &format!("{p}.html_attributes"),
        )?;
        named(
            &mut self.head,
            &b.head,
            |v| v.id.clone(),
            &format!("{p}.head"),
        )?;
        named(
            &mut self.body,
            &b.body,
            |v| v.id.clone(),
            &format!("{p}.body"),
        )?;
        named(
            &mut self.body_before,
            &b.body_before,
            |v| v.id.clone(),
            &format!("{p}.body_before"),
        )?;
        named(
            &mut self.body_after,
            &b.body_after,
            |v| v.id.clone(),
            &format!("{p}.body_after"),
        )?;
        named(
            &mut self.resources,
            &b.resources,
            |v| v.destination.clone(),
            &format!("{p}.resources"),
        )?;
        optional_object(&mut self.manifest, &b.manifest, &format!("{p}.manifest"))?;
        values(
            &mut self.service_workers,
            &b.service_workers,
            &format!("{p}.service_workers"),
        )?;
        named(
            &mut self.response_headers,
            &b.response_headers,
            |v| v.id.clone(),
            &format!("{p}.response_headers"),
        )?;
        values(&mut self.files, &b.files, &format!("{p}.files"))
    }
}
impl Merge for WebAppManifest {
    fn merge(&mut self, b: &Self, p: &str) -> Result<()> {
        equal(&self.path, &b.path, &format!("{p}.path"))?;
        optional(
            &mut self.crossorigin,
            &b.crossorigin,
            &format!("{p}.crossorigin"),
        )?;
        objects(
            &mut self.properties,
            &b.properties,
            &format!("{p}.properties"),
        )
    }
}
