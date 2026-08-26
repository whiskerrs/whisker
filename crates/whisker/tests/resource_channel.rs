use whisker::{ResourceEventApply, RuntimeResourceError, SurfaceRuntime};
use whisker_engine::whisker_protocol::{
    ResourceCommand, ResourceDimensions, ResourceEvent, ResourceId, ResourceKind, ResourceRequest,
    ResourceSource, SurfaceId,
};
use whisker_engine::whisker_style::StyleEnvironment;

fn runtime() -> SurfaceRuntime {
    SurfaceRuntime::new(
        SurfaceId::new(1).unwrap(),
        StyleEnvironment::new(800.0, 600.0, 1.0, 16.0),
    )
}

fn load(resource: ResourceId, generation: u64, source: ResourceSource) -> ResourceCommand {
    ResourceCommand::Load(ResourceRequest {
        resource,
        generation,
        kind: ResourceKind::RasterImage,
        source,
    })
}

#[test]
fn resource_commands_are_ordered_and_generations_are_strictly_monotonic() {
    let runtime = runtime();
    let resource = ResourceId::new(7).unwrap();
    let first = load(
        resource,
        1,
        ResourceSource::Bytes {
            media_type: "image/png".into(),
            data: vec![1, 2, 3],
        },
    );
    let second = load(
        resource,
        2,
        ResourceSource::Url("https://example.com/image.png".into()),
    );
    let release = ResourceCommand::Release {
        resource,
        generation: 1,
    };

    runtime.enqueue_resource_command(first.clone()).unwrap();
    runtime.enqueue_resource_command(second.clone()).unwrap();
    runtime.enqueue_resource_command(release.clone()).unwrap();

    assert_eq!(
        runtime.enqueue_resource_command(second.clone()),
        Err(RuntimeResourceError::NonMonotonicGeneration {
            resource,
            current: 2,
            received: 2,
        })
    );
    assert_eq!(
        runtime.take_resource_commands(),
        vec![first, second, release]
    );
    assert!(runtime.take_resource_commands().is_empty());
}

#[test]
fn resource_events_ignore_replaced_and_released_generations() {
    let runtime = runtime();
    let resource = ResourceId::new(8).unwrap();
    runtime
        .enqueue_resource_command(load(
            resource,
            1,
            ResourceSource::BundledAsset("images/one.png".into()),
        ))
        .unwrap();
    runtime
        .enqueue_resource_command(load(
            resource,
            2,
            ResourceSource::BundledAsset("images/two.png".into()),
        ))
        .unwrap();

    let ready = |generation| ResourceEvent::Ready {
        resource,
        generation,
        dimensions: Some(ResourceDimensions {
            width: 24.0,
            height: 12.0,
            scale: 2.0,
        }),
    };
    assert_eq!(
        runtime.apply_resource_event(&ready(1)).unwrap(),
        ResourceEventApply::Stale
    );
    assert_eq!(
        runtime.apply_resource_event(&ready(2)).unwrap(),
        ResourceEventApply::Applied
    );

    runtime
        .enqueue_resource_command(ResourceCommand::Release {
            resource,
            generation: 2,
        })
        .unwrap();
    assert_eq!(
        runtime.apply_resource_event(&ready(2)).unwrap(),
        ResourceEventApply::Stale
    );
    assert_eq!(
        runtime.enqueue_resource_command(load(
            resource,
            1,
            ResourceSource::BundledAsset("images/old.png".into()),
        )),
        Err(RuntimeResourceError::NonMonotonicGeneration {
            resource,
            current: 2,
            received: 1,
        })
    );
    assert_eq!(
        runtime.apply_resource_event(&ready(3)),
        Err(RuntimeResourceError::UnknownGeneration {
            resource,
            generation: 3,
        })
    );
}
