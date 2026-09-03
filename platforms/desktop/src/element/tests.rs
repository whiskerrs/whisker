use whisker::{ElementRegistry, SurfaceRuntime, standard_element_registrations};
use whisker_protocol::{
    AvailableSpace, CustomMeasurePayload, ElementMeasurement, ElementPropertySchema, ElementSchema,
    ElementValueKind, MeasureConstraints, MeasuredSize, MeasurementKey, MeasurementPayload,
    SurfaceId,
};
use whisker_style::StyleEnvironment;

use super::*;

#[test]
fn desktop_raster_validates_dimensions_and_rgba_length() {
    assert_eq!(
        DesktopRaster::new(1, 0, 1, Vec::<u8>::new()).unwrap_err(),
        DesktopRasterError::EmptyDimensions,
    );
    assert_eq!(
        DesktopRaster::new(1, 2, 2, vec![0; 15]).unwrap_err(),
        DesktopRasterError::ByteLength {
            actual: 15,
            expected: 16,
        },
    );
    let raster = DesktopRaster::new(7, 2, 2, vec![255; 16]).unwrap();
    assert_eq!(raster.generation(), 7);
    assert_eq!((raster.width(), raster.height()), (2, 2));
    assert_eq!(raster.pixels(), &[255; 16]);
}

#[test]
fn built_in_module_binds_view_text_and_scroll_through_one_registry() {
    let definition = BuiltInElementModule::definition();
    assert_eq!(definition.factories().len(), 3);
    let registrations = standard_element_registrations();
    let factories = built_in_element_factories();
    let registry = DesktopElementRegistry::bind(&registrations, &factories).unwrap();
    for registration in &registrations {
        let content = registry
            .create(registration.element_type, DesktopEventEmitter::default())
            .unwrap();
        assert_eq!(
            content.is_scroll_container(),
            registration.name == whisker::SCROLL_VIEW_ELEMENT_NAME,
        );
        assert_eq!(
            matches!(
                content,
                DesktopElementContent::Text(_)
                    | DesktopElementContent::Native {
                        plain_text: true,
                        ..
                    }
            ),
            registration.name == whisker::TEXT_ELEMENT_NAME
        );
        assert_eq!(
            registry.child_policy(registration.element_type).unwrap(),
            registration.child_policy
        );
        assert_eq!(
            registry.measurement(registration.element_type).unwrap(),
            registration.measurement
        );
    }

    let surface = SurfaceRuntime::new(
        SurfaceId::new(1).unwrap(),
        StyleEnvironment::new(100.0, 100.0, 1.0, 14.0),
    );
    assert_eq!(surface.element_registrations(), registrations);
    assert!(registrations.iter().any(|registration| {
        registration.name == whisker::TEXT_ELEMENT_NAME
            && registration.measurement == ElementMeasurement::Text
    }));
}

#[test]
fn duplicate_and_missing_registrations_fail_before_a_frame() {
    let mut registrations = standard_element_registrations();
    registrations.push(registrations[0].clone());
    assert!(matches!(
        DesktopElementRegistry::bind(&registrations, &built_in_element_factories()),
        Err(DesktopElementError::DuplicateElementType { .. })
    ));

    assert!(matches!(
        DesktopElementRegistry::bind(&standard_element_registrations(), &[]),
        Err(DesktopElementError::MissingFactory { .. })
    ));
}

#[test]
fn module_binding_binds_by_versionless_name() {
    let schema = ElementSchema {
        name: "whisker.test/Badge".into(),
        child_policy: ChildPolicy::Elements,
        measurement: ElementMeasurement::None,
        text_style: false,
        properties: Vec::new(),
        events: Vec::new(),
        commands: Vec::new(),
    };
    let elements = ElementRegistry::standard_builder()
        .register_provider(whisker::ElementProviderMetadata::named(schema))
        .build()
        .unwrap();
    let badge = elements
        .registration_for_name("whisker.test/Badge")
        .unwrap();
    let mut factories = built_in_element_factories();
    factories.push(DesktopElementFactory::presentation("whisker.test/Badge"));
    let desktop = DesktopElementRegistry::bind(elements.registrations(), &factories).unwrap();

    assert!(matches!(
        desktop.create(badge.element_type, DesktopEventEmitter::default()),
        Ok(DesktopElementContent::Empty)
    ));
    assert_eq!(badge.name, "whisker.test/Badge");
}

#[test]
fn declared_host_members_must_match_the_rust_schema_at_bootstrap() {
    let registration = ElementRegistration {
        element_type: ElementTypeId::new(20).unwrap(),
        name: "whisker.test/Toggle".into(),
        child_policy: ChildPolicy::None,
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
    let definition = DesktopViewDefinition::new("whisker.test/Toggle", |_| ()).prop(
        "misspelled",
        |_, _| {},
        |_| {},
    );
    let factory = definition.into_desktop_factory();

    assert!(matches!(
        DesktopElementRegistry::bind(&[registration], &[factory]),
        Err(DesktopElementError::FactoryContractMismatch { name, .. })
            if name == "whisker.test/Toggle"
    ));
}

#[test]
fn declared_text_style_and_measurement_reach_the_module_handlers() {
    let registration = ElementRegistration {
        element_type: ElementTypeId::new(21).unwrap(),
        name: "whisker.test/NativeInput".into(),
        child_policy: ChildPolicy::None,
        measurement: ElementMeasurement::Custom,
        text_style: true,
        properties: Vec::new(),
        events: Vec::new(),
        commands: Vec::new(),
    };
    let font_size = Arc::new(std::sync::Mutex::new(None));
    let observed_font_size = Arc::clone(&font_size);
    let definition = DesktopViewDefinition::new("whisker.test/NativeInput", |_| ())
        .text_style(move |_, style| {
            *observed_font_size.lock().unwrap() = Some(style.style.font_size);
        })
        .measurement(|_| Some(MeasuredSize::new(80.0, 24.0)));
    let registry = DesktopElementRegistry::bind(
        std::slice::from_ref(&registration),
        &[definition.into_desktop_factory()],
    )
    .unwrap();
    let request = MeasurementRequest {
        key: MeasurementKey::new(1).unwrap(),
        node: NodeId::new(1).unwrap(),
        element_type: registration.element_type,
        environment_epoch: 9,
        constraints: MeasureConstraints {
            known_dimensions: [None, None],
            available_space: [AvailableSpace::MaxContent; 2],
        },
        payload: MeasurementPayload::Custom(CustomMeasurePayload {
            version: 1,
            data: Vec::new(),
        }),
    };
    assert!(matches!(
        registry.measure(&request).unwrap(),
        Some(MeasurementResponse::Ready {
            environment_epoch: 9,
            metrics,
            ..
        }) if metrics.size == MeasuredSize::new(80.0, 24.0)
    ));

    let mut content = registry
        .create(registration.element_type, DesktopEventEmitter::default())
        .unwrap();
    let mut style = WhiskerTextStyle {
        style: whisker_protocol::TextMeasureStyle::default(),
        locale: None,
        direction: whisker_protocol::MeasureTextDirection::Auto,
        alignment: whisker_protocol::MeasureTextAlignment::Start,
        paint: whisker_protocol::TextPaint::default(),
    };
    style.style.font_size = 18.0;
    content
        .set_text_style(NodeId::new(1).unwrap(), &style)
        .unwrap();
    assert_eq!(*font_size.lock().unwrap(), Some(18.0));
}
