use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::Rc;

use whisker::runtime::module::{ModuleEventEmitter, ModulePromise, RustModuleDefinition};
use whisker::{ElementModuleDefinition, WhiskerModule};
use whisker_protocol::{CommandId, ElementRegistration, PropertyId};

use crate::{WebError, WhiskerMeasureRequest, WhiskerMeasuredSize, WhiskerTextStyle, WhiskerValue};

mod module_definition;

pub(crate) use module_definition::WebMeasurementHandler;
pub use module_definition::{
    ModuleDefinition, WebModuleDefinition, WebViewDefinition, WebViewImplementation,
};

/// Configuration for one browser surface.
#[derive(Clone, Debug)]
pub struct WebAppConfig {
    /// Document title.
    pub title: String,
    /// DOM element id used as the surface root.
    pub root_id: String,
    /// Modules selected for this target, paired with their portable schema.
    pub(crate) modules: Vec<WebModuleInstallation>,
}

#[derive(Clone, Debug)]
pub(crate) struct WebModuleInstallation {
    pub(crate) elements: ElementModuleDefinition,
    pub(crate) host: WebModuleDefinition,
}

impl WebAppConfig {
    /// Creates a browser configuration rooted at `#whisker-root`.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            root_id: "whisker-root".to_string(),
            modules: Vec::new(),
        }
    }

    /// Installs one module's portable element schema and Web implementation.
    ///
    /// Keeping the pair together makes it impossible for generated Hosts to
    /// accidentally install only one side of a module.
    pub fn with_module(
        mut self,
        elements: ElementModuleDefinition,
        host: WebModuleDefinition,
    ) -> Self {
        self.modules.push(WebModuleInstallation { elements, host });
        self
    }
}

/// DOM factory embedded for one element module.
#[derive(Clone)]
pub struct WebElementFactory {
    pub(crate) name: String,
    pub(crate) kind: WebElementFactoryKind,
    pub(crate) text_content: bool,
    pub(crate) scroll_content: bool,
    pub(crate) measurer: Option<WebMeasurementHandler>,
}

impl WebElementFactory {
    /// Creates a DOM factory joined to a Rust schema by element name.
    pub fn new(name: impl Into<String>, tag_name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: WebElementFactoryKind::Tag(tag_name.into()),
            text_content: false,
            scroll_content: false,
            measurer: None,
        }
    }

    /// Creates a DOM-native element whose implementation is owned by its
    /// external module beside the Rust schema.
    pub fn native<F>(name: impl Into<String>, create: F) -> Self
    where
        F: Fn(
                &web_sys::Document,
                WebEventEmitter,
            ) -> Result<Box<dyn WebNativeElement>, wasm_bindgen::JsValue>
            + 'static,
    {
        Self {
            name: name.into(),
            kind: WebElementFactoryKind::Native(Rc::new(create)),
            text_content: false,
            scroll_content: false,
            measurer: None,
        }
    }

    fn declared<T>(
        name: impl Into<String>,
        definition: WebViewDefinition<T>,
        text_content: bool,
        scroll_content: bool,
    ) -> Self
    where
        T: 'static,
    {
        let measurer = definition.measurement.clone();
        Self {
            name: name.into(),
            kind: WebElementFactoryKind::Declared(Rc::new(definition)),
            text_content,
            scroll_content,
            measurer,
        }
    }

    pub(crate) fn bind(&self, registration: &ElementRegistration) -> Result<Self, WebError> {
        if registration.child_policy.accepts_plain_text() != self.text_content {
            return Err(WebError(format!(
                "DOM factory {} plain-text policy differs: Host={}, Rust={:?}",
                registration.name, self.text_content, registration.child_policy
            )));
        }
        let needs_host_measurement = matches!(
            registration.measurement,
            whisker_protocol::ElementMeasurement::ReplacedContent
                | whisker_protocol::ElementMeasurement::Custom
        );
        if needs_host_measurement != self.measurer.is_some() {
            return Err(WebError(format!(
                "DOM factory {} measurement capability differs: Host={}, Rust={:?}",
                registration.name,
                self.measurer.is_some(),
                registration.measurement
            )));
        }
        let kind = match &self.kind {
            WebElementFactoryKind::Declared(definition) => {
                WebElementFactoryKind::Native(definition.bind(registration).map_err(|reason| {
                    WebError(format!(
                        "DOM factory {} contract mismatch: {reason}",
                        registration.name
                    ))
                })?)
            }
            other => other.clone(),
        };
        Ok(Self {
            name: self.name.clone(),
            kind,
            text_content: self.text_content,
            scroll_content: self.scroll_content,
            measurer: self.measurer.clone(),
        })
    }

    pub(crate) fn isolates_failures(&self) -> bool {
        !matches!(
            self.name.as_str(),
            "whisker.ui/View" | "whisker.ui/Text" | "whisker.ui/ScrollView"
        )
    }
}

impl fmt::Debug for WebElementFactory {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebElementFactory")
            .field("name", &self.name)
            .field("kind", &self.kind.name())
            .finish()
    }
}

pub(crate) type WebNativeConstructor = Rc<
    dyn Fn(
        &web_sys::Document,
        WebEventEmitter,
    ) -> Result<Box<dyn WebNativeElement>, wasm_bindgen::JsValue>,
>;

pub(crate) trait WebDeclaredFactory {
    fn bind(&self, registration: &ElementRegistration) -> Result<WebNativeConstructor, String>;
}

impl<T> WebDeclaredFactory for WebViewDefinition<T>
where
    T: 'static,
{
    fn bind(&self, registration: &ElementRegistration) -> Result<WebNativeConstructor, String> {
        WebViewDefinition::bind(self, registration)
    }
}

#[derive(Clone)]
pub(crate) enum WebElementFactoryKind {
    Tag(String),
    Native(WebNativeConstructor),
    Declared(Rc<dyn WebDeclaredFactory>),
}

impl WebElementFactoryKind {
    pub(crate) fn name(&self) -> &'static str {
        match self {
            Self::Tag(_) => "tag",
            Self::Native(_) => "native",
            Self::Declared(_) => "declared-native",
        }
    }
}

impl fmt::Debug for WebElementFactoryKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tag(tag) => formatter.debug_tuple("Tag").field(tag).finish(),
            Self::Native(_) => formatter.write_str("Native(..)"),
            Self::Declared(_) => formatter.write_str("Declared(..)"),
        }
    }
}

/// Event emitted by one module-owned DOM element.
#[derive(Clone, Debug, PartialEq)]
pub struct WebNativeEvent {
    /// Stable event name declared by the Host module.
    pub event: String,
    /// Typed detail routed to the Rust listener.
    pub detail: WhiskerValue,
}

/// Cloneable event channel handed to each DOM-native element instance.
#[derive(Clone)]
pub struct WebEventEmitter(pub(crate) Rc<dyn Fn(WebNativeEvent, bool)>);

impl WebEventEmitter {
    /// Emits an event after the browser callback returns, at the next runtime
    /// frame boundary.
    pub fn emit(&self, event: WebNativeEvent) {
        (self.0)(event, false);
    }

    /// Emits a latency-sensitive Host event before the browser's next paint.
    pub(crate) fn emit_urgent(&self, event: WebNativeEvent) {
        (self.0)(event, true);
    }
}

impl fmt::Debug for WebEventEmitter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("WebEventEmitter(..)")
    }
}

/// Per-node DOM implementation supplied by an external element module.
///
/// Implementations retain their browser listener closures and other resources;
/// dropping the instance during `DeleteNode` releases those resources.
pub trait WebNativeElement: fmt::Debug + 'static {
    /// Returns the DOM element mounted into the common Host projection.
    fn element(&self) -> web_sys::Element;

    /// Applies one schema-validated property.
    fn set_property(
        &mut self,
        property: PropertyId,
        value: &WhiskerValue,
    ) -> Result<(), wasm_bindgen::JsValue>;

    /// Restores one schema-validated property to its implementation default.
    fn clear_property(&mut self, property: PropertyId) -> Result<(), wasm_bindgen::JsValue>;

    /// Executes one schema-validated command.
    fn invoke_command(
        &mut self,
        command: CommandId,
        arguments: &WhiskerValue,
    ) -> Result<(), wasm_bindgen::JsValue>;

    /// Applies resolved inherited text style when declared by the schema.
    fn set_text_style(&mut self, _style: &WhiskerTextStyle) -> Result<(), wasm_bindgen::JsValue> {
        Ok(())
    }
}

/// Built-in element implementations contributed by the Web platform.
pub struct BuiltInElementModule;

#[WhiskerModule]
impl WhiskerModule for BuiltInElementModule {
    type Definition = WebModuleDefinition;

    fn definition() -> Self::Definition {
        fn div(
            document: &web_sys::Document,
            _events: WebEventEmitter,
        ) -> Result<web_sys::Element, wasm_bindgen::JsValue> {
            document.create_element("div")
        }

        fn set_scroll_orientation(
            element: &web_sys::Element,
            value: &WhiskerValue,
        ) -> Result<(), wasm_bindgen::JsValue> {
            let WhiskerValue::String(value) = value else {
                return Err(wasm_bindgen::JsValue::from_str(
                    "scroll-orientation must be a string",
                ));
            };
            let horizontal = value == "horizontal";
            let enabled = element
                .get_attribute("data-whisker-scroll-enabled")
                .as_deref()
                != Some("false");
            crate::set_style(
                element,
                "overflow-x",
                if horizontal && enabled {
                    "auto"
                } else {
                    "hidden"
                },
            )
            .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))?;
            crate::set_style(
                element,
                "overflow-y",
                if !horizontal && enabled {
                    "auto"
                } else {
                    "hidden"
                },
            )
            .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))?;
            element.set_attribute(
                "data-whisker-scroll-orientation",
                if horizontal { "horizontal" } else { "vertical" },
            )?;
            if element.has_attribute("data-whisker-snap-align") {
                crate::set_style(
                    element,
                    "scroll-snap-type",
                    if horizontal {
                        "x mandatory"
                    } else {
                        "y mandatory"
                    },
                )
                .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))?;
            }
            Ok(())
        }

        fn set_scroll_enabled(
            element: &web_sys::Element,
            value: &WhiskerValue,
        ) -> Result<(), wasm_bindgen::JsValue> {
            let WhiskerValue::Bool(enabled) = value else {
                return Err(wasm_bindgen::JsValue::from_str(
                    "enable-scroll must be a boolean",
                ));
            };
            element.set_attribute(
                "data-whisker-scroll-enabled",
                if *enabled { "true" } else { "false" },
            )?;
            let orientation = element
                .get_attribute("data-whisker-scroll-orientation")
                .unwrap_or_else(|| "vertical".to_owned());
            set_scroll_orientation(element, &WhiskerValue::String(orientation))
        }

        fn set_item_snap(
            element: &web_sys::Element,
            value: &WhiskerValue,
        ) -> Result<(), wasm_bindgen::JsValue> {
            let WhiskerValue::Map(value) = value else {
                return Err(wasm_bindgen::JsValue::from_str("item-snap must be a map"));
            };
            let factor = match value.get("factor") {
                Some(WhiskerValue::Float(value)) => *value,
                Some(WhiskerValue::Int(value)) => *value as f64,
                _ => 0.0,
            };
            let offset = match value.get("offset") {
                Some(WhiskerValue::Float(value)) => *value,
                Some(WhiskerValue::Int(value)) => *value as f64,
                _ => 0.0,
            };
            let alignment = if factor < 0.25 {
                "start"
            } else if factor < 0.75 {
                "center"
            } else {
                "end"
            };
            let axis = if element
                .get_attribute("data-whisker-scroll-orientation")
                .as_deref()
                == Some("horizontal")
            {
                "x"
            } else {
                "y"
            };
            crate::set_style(element, "scroll-snap-type", &format!("{axis} mandatory"))
                .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))?;
            element.set_attribute("data-whisker-snap-factor", &factor.to_string())?;
            element.set_attribute("data-whisker-snap-offset", &offset.to_string())?;
            crate::set_style(
                element,
                if axis == "x" {
                    "scroll-padding-left"
                } else {
                    "scroll-padding-top"
                },
                &format!("{}px", (-offset).max(0.0)),
            )
            .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))?;
            element.set_attribute("data-whisker-snap-align", alignment)?;
            let snap_stop = element
                .get_attribute("data-whisker-snap-stop")
                .unwrap_or_else(|| "normal".to_owned());
            let children = element.children();
            for index in 0..children.length() {
                if let Some(child) = children.item(index) {
                    crate::set_style(&child, "scroll-snap-align", alignment)
                        .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))?;
                    crate::set_style(&child, "scroll-snap-stop", &snap_stop)
                        .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))?;
                }
            }
            Ok(())
        }

        fn set_scroll_snap_stop(
            element: &web_sys::Element,
            value: &WhiskerValue,
        ) -> Result<(), wasm_bindgen::JsValue> {
            let WhiskerValue::String(value) = value else {
                return Err(wasm_bindgen::JsValue::from_str(
                    "scroll-snap-stop must be a string",
                ));
            };
            let value = if value == "always" {
                "always"
            } else {
                "normal"
            };
            element.set_attribute("data-whisker-snap-stop", value)?;
            let children = element.children();
            for index in 0..children.length() {
                if let Some(child) = children.item(index) {
                    crate::set_style(&child, "scroll-snap-stop", value)
                        .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))?;
                }
            }
            Ok(())
        }

        fn scroll(
            element: &web_sys::Element,
            value: &WhiskerValue,
            relative: bool,
        ) -> Result<(), wasm_bindgen::JsValue> {
            use wasm_bindgen::JsCast;

            let WhiskerValue::Map(value) = value else {
                return Err(wasm_bindgen::JsValue::from_str(
                    "scroll command arguments must be a map",
                ));
            };
            let offset = match value.get("offset") {
                Some(WhiskerValue::Float(value)) => *value,
                Some(WhiskerValue::Int(value)) => *value as f64,
                _ => 0.0,
            };
            let smooth = matches!(value.get("smooth"), Some(WhiskerValue::Bool(true)));
            let Some(element) = element.dyn_ref::<web_sys::HtmlElement>() else {
                return Err(wasm_bindgen::JsValue::from_str(
                    "ScrollView command target must be an HtmlElement",
                ));
            };
            let horizontal = element
                .get_attribute("data-whisker-scroll-orientation")
                .as_deref()
                == Some("horizontal");
            let current = if horizontal {
                element.scroll_left()
            } else {
                element.scroll_top()
            };
            let target = if relative {
                f64::from(current) + offset
            } else {
                offset
            };
            let options = web_sys::ScrollToOptions::new();
            options.set_behavior(if smooth {
                web_sys::ScrollBehavior::Smooth
            } else {
                web_sys::ScrollBehavior::Instant
            });
            if horizontal {
                options.set_left(target);
            } else {
                options.set_top(target);
            }
            element.scroll_to_with_scroll_to_options(&options);
            Ok(())
        }

        WebModuleDefinition::new()
            .name("whisker.ui")
            .view(WebViewDefinition::new("whisker.ui/View", div, Clone::clone))
            .view(WebViewDefinition::new("whisker.ui/Text", div, Clone::clone).plain_text())
            .view(
                WebViewDefinition::new("whisker.ui/ScrollView", div, Clone::clone)
                    .scroll_container()
                    .prop(
                        "scroll-orientation",
                        |element, value| set_scroll_orientation(element, value),
                        |element| {
                            set_scroll_orientation(
                                element,
                                &WhiskerValue::String("vertical".into()),
                            )
                        },
                    )
                    .prop(
                        "item-snap",
                        |element, value| set_item_snap(element, value),
                        |element| {
                            crate::set_style(element, "scroll-snap-type", "none").map_err(
                                |error| wasm_bindgen::JsValue::from_str(&error.to_string()),
                            )?;
                            element.remove_attribute("data-whisker-snap-align")?;
                            element.remove_attribute("data-whisker-snap-factor")?;
                            element.remove_attribute("data-whisker-snap-offset")?;
                            Ok(())
                        },
                    )
                    .prop(
                        "scroll-snap-stop",
                        |element, value| set_scroll_snap_stop(element, value),
                        |element| {
                            set_scroll_snap_stop(element, &WhiskerValue::String("normal".into()))
                        },
                    )
                    .prop(
                        "enable-scroll",
                        |element, value| set_scroll_enabled(element, value),
                        |element| set_scroll_enabled(element, &WhiskerValue::Bool(true)),
                    )
                    .command("scrollTo", |element, value| scroll(element, value, false))
                    .command("scrollBy", |element, value| scroll(element, value, true))
                    .event("scroll"),
            )
    }
}

#[cfg(test)]
pub(crate) fn built_in_element_factories() -> Vec<WebElementFactory> {
    BuiltInElementModule::definition().into_factories()
}

#[cfg(test)]
mod element_registry_tests {
    use super::*;
    use crate::scene::element_registry::DomElementRegistry;
    use whisker::ElementRegistry;
    use whisker_protocol::{
        AvailableSpace, CustomMeasurePayload, ElementMeasurement, ElementPropertySchema,
        ElementSchema, ElementTypeId, ElementValueKind, MeasureConstraints, MeasuredSize,
        MeasurementKey, MeasurementPayload, MeasurementRequest, NodeId,
    };

    #[test]
    fn built_in_and_package_modules_use_the_same_dom_binding_path() {
        let built_ins = BuiltInElementModule::definition();
        assert_eq!(built_ins.factories().len(), 3);
        let modules = built_ins.view(WebElementFactory::new("whisker.test/Badge", "span"));
        let elements = ElementRegistry::standard_builder()
            .register_provider(whisker::ElementProviderMetadata::named(ElementSchema {
                name: "whisker.test/Badge".into(),
                child_policy: whisker_protocol::ChildPolicy::Elements,
                measurement: ElementMeasurement::None,
                text_style: false,
                properties: Vec::new(),
                events: Vec::new(),
                commands: Vec::new(),
            }))
            .build()
            .unwrap();
        let factories = modules.factories().to_vec();

        let registry = DomElementRegistry::bind(elements.registrations(), &factories).unwrap();
        let badge = elements
            .registration_for_name("whisker.test/Badge")
            .unwrap();
        assert!(matches!(
            &registry.binding(badge.element_type).unwrap().factory,
            WebElementFactoryKind::Tag(tag_name) if tag_name == "span"
        ));
    }

    #[test]
    fn missing_or_unmatched_dom_factories_fail_bootstrap() {
        let registrations = ElementRegistry::standard().registrations().to_vec();
        let missing = DomElementRegistry::bind(&registrations, &[]).unwrap_err();
        assert!(missing.0.contains("missing DOM factory"));

        let mut factories = built_in_element_factories();
        factories.push(WebElementFactory::new("whisker.test/Unknown", "div"));
        let unknown = DomElementRegistry::bind(&registrations, &factories).unwrap_err();
        assert!(unknown.0.contains("has no Rust element schema"));
    }

    #[test]
    fn declared_host_members_must_match_the_rust_schema_at_bootstrap() {
        let registration = ElementRegistration {
            element_type: ElementTypeId::new(20).unwrap(),
            name: "whisker.test/Toggle".into(),
            child_policy: whisker_protocol::ChildPolicy::None,
            measurement: ElementMeasurement::None,
            text_style: false,
            properties: vec![ElementPropertySchema {
                property: PropertyId::new(1).unwrap(),
                name: "checked".into(),
                value: ElementValueKind::Bool,
            }],
            events: Vec::new(),
            commands: Vec::new(),
        };
        let definition = WebViewDefinition::new(
            "whisker.test/Toggle",
            |_, _| Ok(()),
            |_| unreachable!("the factory is not instantiated during bootstrap"),
        )
        .prop("misspelled", |_, _| Ok(()), |_| Ok(()));
        let factory = definition.into_web_factory();

        let error = DomElementRegistry::bind(&[registration], &[factory]).unwrap_err();
        assert!(error.0.contains("contract mismatch"));
        assert!(error.0.contains("misspelled"));
        assert!(error.0.contains("checked"));
    }

    #[test]
    fn declared_text_style_and_measurement_are_part_of_the_web_contract() {
        let registration = ElementRegistration {
            element_type: ElementTypeId::new(21).unwrap(),
            name: "whisker.test/NativeInput".into(),
            child_policy: whisker_protocol::ChildPolicy::None,
            measurement: ElementMeasurement::Custom,
            text_style: true,
            properties: Vec::new(),
            events: Vec::new(),
            commands: Vec::new(),
        };
        let definition = WebViewDefinition::new(
            "whisker.test/NativeInput",
            |_, _| Ok(()),
            |_| unreachable!("binding does not instantiate the DOM object"),
        )
        .text_style(|_, _| Ok(()))
        .measurement(|_| Some(MeasuredSize::new(90.0, 28.0)));
        let factory = definition.into_web_factory().bind(&registration).unwrap();
        let request = MeasurementRequest {
            key: MeasurementKey::new(1).unwrap(),
            node: NodeId::new(1).unwrap(),
            element_type: registration.element_type,
            environment_epoch: 5,
            constraints: MeasureConstraints {
                known_dimensions: [None, None],
                available_space: [AvailableSpace::MaxContent; 2],
            },
            payload: MeasurementPayload::Custom(CustomMeasurePayload {
                version: 1,
                data: Vec::new(),
            }),
        };
        let module_request = WhiskerMeasureRequest::from(&request);
        assert!(matches!(
            factory.measurer.unwrap()(&module_request),
            Some(size) if size == MeasuredSize::new(90.0, 28.0)
        ));
    }
}
