use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::Rc;

use gtk4 as gtk;
use whisker::runtime::module::{ModuleEventEmitter, ModulePromise, RustModuleDefinition};
use whisker_protocol::{
    MeasuredSize, ModuleMeasureRequest, NodeId, TextStyleSnapshot, WhiskerValue,
};

use crate::scene::{ProviderEvent, SharedScrollFeedback};

/// Linux implementation of one module, as returned by its Host crate.
#[derive(Clone, Default)]
pub struct LinuxModuleDefinition {
    pub(crate) service: RustModuleDefinition,
    pub(crate) views: Vec<LinuxViewFactory>,
}

impl fmt::Debug for LinuxModuleDefinition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LinuxModuleDefinition")
            .field("service", &self.service)
            .field(
                "views",
                &self.views.iter().map(|view| &view.name).collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl From<RustModuleDefinition> for LinuxModuleDefinition {
    fn from(service: RustModuleDefinition) -> Self {
        Self {
            service,
            views: Vec::new(),
        }
    }
}

impl LinuxModuleDefinition {
    /// Starts an empty Linux module declaration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Declares the package-qualified service module name.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.service = self.service.name(name);
        self
    }

    /// Declares one synchronous service function.
    pub fn function(
        mut self,
        name: impl Into<String>,
        handler: impl Fn(&[WhiskerValue], &ModuleEventEmitter) -> WhiskerValue + 'static,
    ) -> Self {
        self.service = self.service.function(name, handler);
        self
    }

    /// Declares one asynchronous service function.
    pub fn async_function(
        mut self,
        name: impl Into<String>,
        handler: impl Fn(&[WhiskerValue], ModulePromise, &ModuleEventEmitter) + 'static,
    ) -> Self {
        self.service = self.service.async_function(name, handler);
        self
    }

    /// Declares one service event.
    pub fn event(mut self, name: impl Into<String>) -> Self {
        self.service = self.service.event(name);
        self
    }

    /// Declares one view backed by a GTK widget.
    pub fn view<W>(mut self, view: LinuxViewDefinition<W>) -> Self
    where
        W: LinuxView,
    {
        self.views.push(view.into_factory());
        self
    }
}

/// Content object of one mounted module view.
pub trait LinuxView: 'static {
    /// GTK widget placed in the node's content box.
    fn widget(&self) -> gtk::Widget;
}

impl LinuxView for gtk::Widget {
    fn widget(&self) -> gtk::Widget {
        self.clone()
    }
}

/// Delivers native widget events to the node that owns the widget.
#[derive(Clone)]
pub struct LinuxEventEmitter {
    node: NodeId,
    events: Rc<HashSet<String>>,
    feedback: SharedScrollFeedback,
    wake: Rc<dyn Fn()>,
}

impl LinuxEventEmitter {
    /// Queues one declared event for the next runtime turn; undeclared events are dropped.
    pub fn emit(&self, event: impl Into<String>, detail: WhiskerValue) -> bool {
        let name = event.into();
        if !self.events.contains(&name) {
            return false;
        }
        self.feedback.borrow_mut().push_event(ProviderEvent {
            target: self.node,
            name,
            detail,
        });
        (self.wake)();
        true
    }
}

type PropSetter<W> = Rc<dyn Fn(&W, &WhiskerValue)>;
type PropClearer<W> = Rc<dyn Fn(&W)>;
type CommandHandler<W> = Rc<dyn Fn(&W, &WhiskerValue)>;
type TextStyleHandler<W> = Rc<dyn Fn(&W, &TextStyleSnapshot)>;
pub(crate) type MeasureHandler = Rc<dyn Fn(&ModuleMeasureRequest) -> Option<MeasuredSize>>;

struct ViewBindings<W> {
    properties: HashMap<String, (PropSetter<W>, PropClearer<W>)>,
    commands: HashMap<String, CommandHandler<W>>,
    text_style: Option<TextStyleHandler<W>>,
}

/// The `View { Prop / Events / Command }` portion of a Linux declaration.
pub struct LinuxViewDefinition<W> {
    name: String,
    create: Rc<dyn Fn(LinuxEventEmitter) -> W>,
    events: HashSet<String>,
    bindings: ViewBindings<W>,
    measurement: Option<MeasureHandler>,
}

impl<W> LinuxViewDefinition<W>
where
    W: LinuxView,
{
    /// Declares how the widget is created for each mounted node.
    pub fn new(name: impl Into<String>, create: impl Fn(LinuxEventEmitter) -> W + 'static) -> Self {
        Self {
            name: name.into(),
            create: Rc::new(create),
            events: HashSet::new(),
            bindings: ViewBindings {
                properties: HashMap::new(),
                commands: HashMap::new(),
                text_style: None,
            },
            measurement: None,
        }
    }

    /// Declares one property by its stable Host name.
    pub fn prop(
        mut self,
        property: impl Into<String>,
        set: impl Fn(&W, &WhiskerValue) + 'static,
        clear: impl Fn(&W) + 'static,
    ) -> Self {
        let property = property.into();
        assert!(
            self.bindings
                .properties
                .insert(property.clone(), (Rc::new(set), Rc::new(clear)))
                .is_none(),
            "duplicate Linux property binding for {property}"
        );
        self
    }

    /// Declares one event by its stable Host name.
    pub fn event(mut self, event: impl Into<String>) -> Self {
        let event = event.into();
        assert!(
            self.events.insert(event.clone()),
            "duplicate Linux event binding for {event}"
        );
        self
    }

    /// Declares one command by its stable Host name.
    pub fn command(
        mut self,
        command: impl Into<String>,
        handler: impl Fn(&W, &WhiskerValue) + 'static,
    ) -> Self {
        let command = command.into();
        assert!(
            self.bindings
                .commands
                .insert(command.clone(), Rc::new(handler))
                .is_none(),
            "duplicate Linux command binding for {command}"
        );
        self
    }

    /// Receives the resolved inherited text style.
    pub fn text_style(mut self, update: impl Fn(&W, &TextStyleSnapshot) + 'static) -> Self {
        assert!(
            self.bindings.text_style.replace(Rc::new(update)).is_none(),
            "duplicate Linux TextStyle binding for {}",
            self.name
        );
        self
    }

    /// Returns the intrinsic size, or `None` when the request is unsupported.
    pub fn measure(
        mut self,
        measure: impl Fn(&ModuleMeasureRequest) -> Option<MeasuredSize> + 'static,
    ) -> Self {
        assert!(
            self.measurement.replace(Rc::new(measure)).is_none(),
            "duplicate Linux Measurement binding for {}",
            self.name
        );
        self
    }

    fn into_factory(self) -> LinuxViewFactory {
        let create = self.create;
        let bindings = Rc::new(self.bindings);
        LinuxViewFactory {
            name: self.name,
            events: Rc::new(self.events),
            measurement: self.measurement,
            create: Rc::new(move |emitter| {
                Box::new(BoundView {
                    widget: create(emitter),
                    bindings: Rc::clone(&bindings),
                })
            }),
        }
    }
}

/// Type-erased view declaration bound to an element by name.
#[derive(Clone)]
pub(crate) struct LinuxViewFactory {
    pub(crate) name: String,
    events: Rc<HashSet<String>>,
    pub(crate) measurement: Option<MeasureHandler>,
    create: Rc<dyn Fn(LinuxEventEmitter) -> Box<dyn NativeView>>,
}

impl LinuxViewFactory {
    pub(crate) fn create(
        &self,
        node: NodeId,
        feedback: SharedScrollFeedback,
        wake: Rc<dyn Fn()>,
    ) -> Box<dyn NativeView> {
        (self.create)(LinuxEventEmitter {
            node,
            events: Rc::clone(&self.events),
            feedback,
            wake,
        })
    }
}

/// One mounted module widget.
pub(crate) trait NativeView {
    fn widget(&self) -> gtk::Widget;
    fn set_property(&self, property: &str, value: Option<&WhiskerValue>);
    fn invoke(&self, command: &str, arguments: &WhiskerValue);
    fn set_text_style(&self, style: &TextStyleSnapshot);
}

struct BoundView<W> {
    widget: W,
    bindings: Rc<ViewBindings<W>>,
}

impl<W> NativeView for BoundView<W>
where
    W: LinuxView,
{
    fn widget(&self) -> gtk::Widget {
        self.widget.widget()
    }

    fn set_property(&self, property: &str, value: Option<&WhiskerValue>) {
        if let Some((set, clear)) = self.bindings.properties.get(property) {
            match value {
                Some(value) => set(&self.widget, value),
                None => clear(&self.widget),
            }
        }
    }

    fn invoke(&self, command: &str, arguments: &WhiskerValue) {
        if let Some(handler) = self.bindings.commands.get(command) {
            handler(&self.widget, arguments);
        }
    }

    fn set_text_style(&self, style: &TextStyleSnapshot) {
        if let Some(update) = &self.bindings.text_style {
            update(&self.widget, style);
        }
    }
}
