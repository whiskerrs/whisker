//! Minimal application used to verify Whisker Host bootstrapping.

use whisker::css::BorderStyle;
use whisker::prelude::*;
use whisker::runtime::view::Element;
#[cfg(any(target_os = "android", target_os = "ios", test))]
use whisker_toggle::MODULE_NAME as TOGGLE_MODULE_NAME;
use whisker_toggle::Toggle;

#[cfg(any(target_os = "android", target_os = "ios"))]
async fn verify_mobile_module_bridge(
    status: RwSignal<String>,
    subscription_slot: StoredValue<Option<whisker::runtime::module::ModuleSubscription>>,
) {
    use whisker::platform_module::WhiskerValue;

    let module = whisker::PlatformModule::named(TOGGLE_MODULE_NAME);
    let result = module.invoke("echo", vec![WhiskerValue::String("module-ready".into())]);
    let async_result = module
        .invoke_async("echoAsync", vec![WhiskerValue::String("async".into())])
        .await;
    let (value, async_value) = match (result, async_result) {
        (WhiskerValue::String(value), WhiskerValue::String(async_value)) => (value, async_value),
        error => {
            status.set(format!("module-error: {error:?}"));
            return;
        }
    };

    status.set(format!("{value} + {async_value} + waiting-event"));
    let subscription = module.on_event("ready", move |payload| {
        eprintln!("Whisker mobile module event: {payload:?}");
        if matches!(payload, WhiskerValue::String(ref event) if event.ends_with("-ready")) {
            status.set(format!("{value} + {async_value} + event"));
        } else {
            status.set(format!("module-error: invalid ready event {payload:?}"));
        }
    });
    if let Some(error) = subscription.error() {
        status.set(format!("module-error: {error}"));
    } else {
        subscription_slot.set(Some(subscription));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_module_uses_the_toggle_service_name() {
        assert_eq!(TOGGLE_MODULE_NAME, "whisker-toggle:WhiskerToggle");
    }
}

#[component]
fn external_toggle() -> Element {
    render! {
        Toggle(
            checked: false,
            disabled: false,
            style: css!(
                width: px(48),
                height: px(32),
                margin_top: px(16),
                border_radius: px(16),
                background_color: Color::hex(0x0EA5E9),
            ),
            on_change: |_event| {},
        )
    }
}

#[whisker::main]
pub fn app() -> Element {
    let module_status = RwSignal::new("module-ready".to_string());
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        let status = module_status;
        let subscription_slot = StoredValue::new(None);
        spawn_local(async move {
            verify_mobile_module_bridge(status, subscription_slot).await;
        });
    }
    render! {
        view(style: css!(
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            background_color: Color::hex(0x20242A),
            padding: px(24),
        )) {
            text(
                style: css!(
                    color: Color::hex(0xF5F7FA),
                    font_size: px(18),
                ),
                value: "Whisker Host is running",
            )
            text(
                style: css!(
                    color: Color::hex(0x94A3B8),
                    font_size: px(12),
                    margin_top: px(4),
                ),
                value: module_status,
            )
            ExternalToggle()
            view(style: css!(
                width: percent(100),
                height: px(88),
                margin_top: px(24),
                border_radius: px(24),
                background_color: Color::hex(0x2563EB),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
            )) {
                text(
                    style: css!(
                        color: Color::hex(0xFFFFFF),
                        font_size: px(16),
                    ),
                    value: "24px radius",
                )
            }
            view(style: css!(
                width: percent(100),
                height: px(88),
                margin_top: px(16),
                border_top_width: px(3),
                border_right_width: px(3),
                border_bottom_width: px(3),
                border_left_width: px(3),
                border_top_style: BorderStyle::Solid,
                border_right_style: BorderStyle::Solid,
                border_bottom_style: BorderStyle::Solid,
                border_left_style: BorderStyle::Solid,
                border_top_color: Color::hex(0xFDE68A),
                border_right_color: Color::hex(0xFDE68A),
                border_bottom_color: Color::hex(0xFDE68A),
                border_left_color: Color::hex(0xFDE68A),
                border_top_left_radius: px(40),
                border_top_right_radius: px(8),
                border_bottom_right_radius: px(40),
                border_bottom_left_radius: px(8),
                background_color: Color::hex(0x15803D),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
            )) {
                text(
                    style: css!(
                        color: Color::hex(0xFFFFFF),
                        font_size: px(16),
                    ),
                    value: "Asymmetric radius + border",
                )
            }
            view(style: css!(
                width: percent(100),
                height: px(88),
                margin_top: px(16),
                border_radius: percent(50),
                background_color: Color::hex(0x7C3AED),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
            )) {
                text(
                    style: css!(
                        color: Color::hex(0xFFFFFF),
                        font_size: px(16),
                    ),
                    value: "50% radius",
                )
            }
        }
    }
}
