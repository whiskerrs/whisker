//! Emulator-only Firebase smoke test. The demo project has no production resources.
// Helpers shared by the service smoke tests are unused in builds without every service.
#![cfg_attr(
    not(all(feature = "firestore", feature = "auth", feature = "storage")),
    allow(dead_code)
)]
use std::cell::RefCell;
use std::future::poll_fn;
use std::rc::Rc;
use std::task::{Poll, Waker};
use whisker::prelude::*;
use whisker::runtime::view::Element;

type SmokeResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

const PLATFORM: &str = if cfg!(target_os = "android") {
    "android"
} else {
    "ios"
};
const HOST: &str = if cfg!(target_os = "android") {
    "10.0.2.2"
} else {
    "127.0.0.1"
};

#[whisker::main]
pub fn app() -> Element {
    let status = RwSignal::new("Initializing Firebase…".to_string());
    let live = live_view();
    let live_text = live.text;
    let push_text = push_view();
    let crash_text = crash_view();
    on_mount(move || {
        spawn_local(async move {
            let message = match smoke_test(status, live.clone()).await {
                Ok(message) => format!("FIREBASE_SMOKE_OK: {message}"),
                Err(error) => format!(
                    "FIREBASE_SMOKE_FAILED: {} ({error})",
                    status.get_untracked()
                ),
            };
            eprintln!("{message}");
            status.set(message);
        });
    });
    render! {
        View(style: Css::new().flex_direction(FlexDirection::Column).gap(px(12)).padding_top(px(80)).padding_left(px(20)).padding_right(px(20))) {
            Text(value: computed(move || status.get()))
            Text(value: computed(move || live_text.get()))
            Text(value: computed(move || push_text.get()))
            Text(value: computed(move || crash_text.get()))
            View(on_tap: move |_| crash_now(), style: Css::new().padding(px(12)).background_color(Color::hex(0xf0d0d0))) { Text(value: "CRASH") }
            View(on_tap: move |_| panic_now(), style: Css::new().padding(px(12)).background_color(Color::hex(0xf0e0c0))) { Text(value: "PANIC") }
        }
    }
}

/// A component-owned snapshot signal, exercised by the smoke test.
#[derive(Clone)]
struct Live {
    text: Signal<String>,
    changed: Notify,
}

#[cfg(feature = "firestore")]
fn live_view() -> Live {
    use whisker_firebase::firestore::Firestore;
    let changed = Notify::default();
    let Ok(db) = Firestore::instance() else {
        return Live {
            text: Signal::from(RwSignal::new("no Firestore".to_string())),
            changed,
        };
    };
    // The emulator must be configured before the listener below starts.
    let _ = db.use_emulator(HOST, 8080);
    let snapshot = db
        .doc(&format!("smoke/{PLATFORM}/items/live"))
        .snapshot_signal();
    let notify = changed.clone();
    effect(move || {
        snapshot.with(|_| ());
        notify.notify();
    });
    let text = computed(move || match snapshot.get() {
        None => "live: waiting".to_string(),
        Some(Err(error)) => format!("live: {error}"),
        Some(Ok(doc)) => format!("live: {:?}", doc.get::<String>("message").ok().flatten()),
    });
    Live {
        text: text.into(),
        changed,
    }
}

#[cfg(not(feature = "firestore"))]
fn live_view() -> Live {
    Live {
        text: Signal::from(RwSignal::new(String::new())),
        changed: Notify::default(),
    }
}

/// Permission prompts and received pushes need a person or `simctl push`, so they are
/// shown on screen rather than awaited by the smoke test.
#[cfg(feature = "messaging")]
fn push_view() -> Signal<String> {
    use whisker_firebase::messaging::{ForegroundPresentation, Messaging, PermissionOptions};
    let permission = RwSignal::new("permission: requesting".to_string());
    let push = RwSignal::new("push: none".to_string());
    if let Ok(messaging) = Messaging::instance() {
        let _ = messaging.set_foreground_presentation(ForegroundPresentation::all());
        let describe = |kind: &str, message: whisker_firebase::messaging::RemoteMessage| {
            let title = message
                .notification
                .and_then(|n| n.title)
                .unwrap_or_default();
            format!("{kind}: {title} {:?}", message.data)
        };
        if let Ok(registration) = messaging.on_message(move |m| push.set(describe("push", m))) {
            on_cleanup(move || drop(registration));
        }
        if let Ok(registration) =
            messaging.on_message_opened_app(move |m| push.set(describe("opened", m)))
        {
            on_cleanup(move || drop(registration));
        }
        spawn_local(async move {
            if let Ok(Some(message)) = messaging.initial_message().await {
                push.set(describe("launched by", message));
            }
            permission.set(
                match messaging
                    .request_permission(PermissionOptions::default())
                    .await
                {
                    Ok(settings) => format!("permission: {:?}", settings.authorization_status),
                    Err(error) => format!("permission: {error}"),
                },
            );
        });
    }
    computed(move || format!("{} / {}", permission.get(), push.get())).into()
}

#[cfg(not(feature = "messaging"))]
fn push_view() -> Signal<String> {
    Signal::from(RwSignal::new(String::new()))
}

/// Crashes end the app, so they are triggered by hand and checked on the next launch.
#[cfg(feature = "crashlytics")]
fn crash_view() -> Signal<String> {
    use whisker_firebase::crashlytics::Crashlytics;
    let Ok(crashlytics) = Crashlytics::instance() else {
        return Signal::from(RwSignal::new("crashlytics: unavailable".to_string()));
    };
    crashlytics.record_panics();
    let text = RwSignal::new(format!(
        "crashlytics: crashed last run = {:?}",
        crashlytics.did_crash_on_previous_execution()
    ));
    spawn_local(async move {
        let unsent = crashlytics.check_for_unsent_reports().await;
        text.update(|text| text.push_str(&format!(", unsent reports = {unsent:?}")));
    });
    text.into()
}

#[cfg(not(feature = "crashlytics"))]
fn crash_view() -> Signal<String> {
    Signal::from(RwSignal::new(String::new()))
}

fn crash_now() {
    #[cfg(feature = "crashlytics")]
    if let Ok(crashlytics) = whisker_firebase::crashlytics::Crashlytics::instance() {
        crashlytics.crash();
    }
}

fn panic_now() {
    #[cfg(feature = "crashlytics")]
    panic!("smoke test panic");
}

/// Wakes a waiting task whenever a listener fires.
#[derive(Clone, Default)]
struct Notify(Rc<RefCell<Option<Waker>>>);
impl Notify {
    fn notify(&self) {
        if let Some(waker) = self.0.borrow_mut().take() {
            waker.wake();
        }
    }
    async fn until(&self, mut done: impl FnMut() -> bool) {
        poll_fn(|cx| {
            if done() {
                Poll::Ready(())
            } else {
                *self.0.borrow_mut() = Some(cx.waker().clone());
                Poll::Pending
            }
        })
        .await
    }
}

fn ensure(condition: bool, message: &str) -> SmokeResult {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn failure<T>(
    result: whisker_firebase::Result<T>,
    message: &str,
) -> SmokeResult<whisker_firebase::FirebaseError> {
    result
        .err()
        .ok_or_else(|| format!("{message}: unexpectedly succeeded").into())
}

async fn smoke_test(status: RwSignal<String>, live: Live) -> SmokeResult<String> {
    let step = |name: &str| status.set(format!("running: {name}"));
    step("core");
    let app = whisker_firebase::FirebaseApp::initialize()?;
    let mut passed = vec!["core".to_string()];
    #[cfg(feature = "firestore")]
    {
        firestore_smoke(&step, &live).await?;
        passed.push("firestore".into());
    }
    #[cfg(feature = "auth")]
    {
        auth_smoke(&step).await?;
        passed.push("auth".into());
    }
    #[cfg(feature = "storage")]
    {
        storage_smoke(&step).await?;
        passed.push("storage".into());
    }
    #[cfg(feature = "analytics")]
    {
        analytics_smoke(&step).await?;
        passed.push("analytics".into());
    }
    #[cfg(feature = "crashlytics")]
    {
        crashlytics_smoke(&step)?;
        passed.push("crashlytics".into());
    }
    #[cfg(feature = "messaging")]
    {
        let token = messaging_smoke(&step).await?;
        passed.push(format!("messaging ({token})"));
    }
    let _ = (&live, &mut passed);
    Ok(format!("{}: {} passed", app.project_id, passed.join(", ")))
}

#[cfg(feature = "firestore")]
async fn firestore_smoke(step: &dyn Fn(&str), live: &Live) -> SmokeResult {
    use serde::{Deserialize, Serialize};
    use whisker_firebase::Timestamp;
    use whisker_firebase::firestore::*;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    enum Role {
        Admin,
        Member { since: i64 },
    }
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Profile {
        message: String,
        count: i64,
        nickname: Option<String>,
        created: Timestamp,
        home: GeoPoint,
        avatar: Bytes,
        friend: DocumentReference,
        roles: Vec<Role>,
    }

    let db = Firestore::instance()?;
    let root = db.doc(&format!("smoke/{PLATFORM}"));
    let items = root.collection("items");

    step("firestore cleanup");
    let existing = items.get().await?;
    let mut cleanup = db.batch();
    for doc in &existing {
        cleanup.delete(doc.reference());
    }
    cleanup.delete(&root);
    cleanup.commit().await?;
    ensure(!root.get().await?.exists(), "document should be missing")?;

    step("firestore typed round trip");
    let profile = Profile {
        message: "Hello Firebase 日本語".into(),
        count: i64::MAX - 1,
        nickname: None,
        created: Timestamp::new(123, 456_789_000)?,
        home: GeoPoint::new(35.0, 139.0),
        avatar: Bytes(vec![0, 1, 255]),
        friend: items.doc("friend"),
        roles: vec![Role::Admin, Role::Member { since: 2020 }],
    };
    root.set(&profile).await?;
    let snapshot = root.get().await?;
    ensure(
        snapshot.data::<Profile>()? == Some(profile.clone()),
        "typed round trip",
    )?;
    ensure(
        snapshot.get::<i64>("count")? == Some(i64::MAX - 1),
        "integer precision",
    )?;

    step("firestore update transforms");
    let missing = failure(
        items.doc("missing").update(&data! { "x" => 1 }).await,
        "update of a missing document",
    )?;
    ensure(
        missing.service == "firestore" && missing.code == "not-found",
        "update of a missing document",
    )?;
    root.update(&data! {
        "count" => FieldValue::increment(-5),
        "stats.visits" => FieldValue::increment(2),
        "updated" => FieldValue::server_timestamp(),
        "tags" => FieldValue::array_union(["a", "b"]),
        "nickname" => FieldValue::delete(),
    })
    .await?;
    root.set_with(
        &data! { "tags" => FieldValue::array_remove(["a"]) },
        SetOptions::merge(),
    )
    .await?;
    let updated = root.get().await?;
    ensure(
        updated.get::<i64>("count")? == Some(i64::MAX - 6),
        "increment",
    )?;
    ensure(
        updated.get::<i64>("stats.visits")? == Some(2),
        "dotted update",
    )?;
    ensure(
        updated.get::<Timestamp>("updated")?.is_some(),
        "server timestamp",
    )?;
    ensure(
        updated.get::<Vec<String>>("tags")? == Some(vec!["b".into()]),
        "array transforms",
    )?;
    ensure(
        updated
            .fields()
            .is_some_and(|f| !f.contains_key("nickname")),
        "field delete",
    )?;

    step("firestore listeners");
    let seen: Rc<RefCell<Vec<Vec<DocumentChangeType>>>> = Rc::default();
    let notify = Notify::default();
    let registration = items.order_by("score", Direction::Ascending).on_snapshot({
        let (seen, notify) = (seen.clone(), notify.clone());
        move |snapshot| {
            if let Ok(snapshot) = snapshot {
                seen.borrow_mut()
                    .push(snapshot.doc_changes().iter().map(|c| c.kind).collect());
            }
            notify.notify();
        }
    })?;
    notify.until(|| !seen.borrow().is_empty()).await;

    step("firestore batch and queries");
    let mut batch = db.batch();
    for score in 1..=3 {
        batch.set(
            &items.doc(&format!("item{score}")),
            &data! { "score" => score, "tag" => "x" },
        );
    }
    batch.commit().await?;
    let added = items.add(&data! { "score" => 10, "tag" => "y" }).await?;
    ensure(added.parent() == items && added.id().len() == 20, "auto ID")?;
    let top = items
        .where_field("tag", FilterOp::Equal, "x")
        .order_by("score", Direction::Descending)
        .limit(2)
        .get()
        .await?;
    let scores = top
        .iter()
        .map(|d| d.get::<i64>("score"))
        .collect::<Result<Vec<_>, _>>()?;
    let scores: Vec<i64> = scores.into_iter().flatten().collect();
    ensure(scores == [3, 2], "filtered, ordered, limited query")?;
    let after = items
        .order_by("score", Direction::Ascending)
        .start_after([2])
        .get()
        .await?;
    ensure(after.len() == 2, "cursor")?;
    let any = items
        .where_field("score", FilterOp::In, vec![1, 10])
        .get()
        .await?;
    ensure(any.len() == 2, "in filter")?;
    ensure(items.count().await? == 4, "count aggregation")?;
    let group = db
        .collection_group("items")
        .where_field("tag", FilterOp::Equal, "y")
        .get()
        .await?;
    ensure(
        group.iter().any(|d| d.reference() == &added),
        "collection group",
    )?;
    notify
        .until(|| {
            seen.borrow()
                .iter()
                .flatten()
                .filter(|kind| **kind == DocumentChangeType::Added)
                .count()
                >= 4
        })
        .await;
    registration.remove();

    step("firestore snapshot signal");
    items
        .doc("live")
        .set(&data! { "message" => "from the signal", "score" => 99 })
        .await?;
    live.changed
        .until(|| {
            live.text
                .try_get_untracked()
                .unwrap_or_default()
                .contains("from the signal")
        })
        .await;

    step("firestore security rules");
    let denied = failure(
        db.doc("denied/read").get_with(Source::Server).await,
        "security rules",
    )?;
    ensure(denied.code == "permission-denied", "security rules")?;
    Ok(())
}

#[cfg(feature = "auth")]
async fn auth_smoke(step: &dyn Fn(&str)) -> SmokeResult {
    use whisker_firebase::auth::{Auth, AuthCredential, ProfileUpdate};

    let auth = Auth::instance()?;
    auth.use_emulator(HOST, 9099)?;
    auth.sign_out()?;

    step("auth state listener");
    let states: Rc<RefCell<Vec<Option<String>>>> = Rc::default();
    let notify = Notify::default();
    let registration = auth.on_auth_state_changed({
        let (states, notify) = (states.clone(), notify.clone());
        move |user| {
            states.borrow_mut().push(user.map(|u| u.uid));
            notify.notify();
        }
    })?;
    notify.until(|| states.borrow().last() == Some(&None)).await;

    step("auth anonymous sign-in");
    let anonymous = auth.sign_in_anonymously().await?;
    let user = anonymous.user;
    ensure(user.is_anonymous, "anonymous user")?;
    ensure(
        auth.current_user()?.map(|u| u.uid) == Some(user.uid.clone()),
        "current user",
    )?;
    notify
        .until(|| states.borrow().last() == Some(&Some(user.uid.clone())))
        .await;
    ensure(!user.id_token(false).await?.is_empty(), "ID token")?;

    #[cfg(feature = "firestore")]
    {
        step("auth protects firestore");
        let db = whisker_firebase::firestore::Firestore::instance()?;
        let profile = db.collection("users").doc(&user.uid);
        profile
            .set(&whisker_firebase::firestore::data! { "platform" => PLATFORM })
            .await?;
    }

    step("auth link email credential");
    let millis = whisker_firebase::Timestamp::now().to_millis();
    let email = format!("smoke-{PLATFORM}-{millis}@example.com");
    let linked = user
        .link_with_credential(&AuthCredential::email(&email, "password123"))
        .await?;
    ensure(
        linked.user.uid == user.uid && !linked.user.is_anonymous,
        "linked account",
    )?;
    ensure(
        linked.user.email.as_deref() == Some(email.as_str()),
        "linked email",
    )?;
    let renamed = linked
        .user
        .update_profile(&ProfileUpdate::new().display_name(Some("Smoke")))
        .await?;
    ensure(
        renamed.display_name.as_deref() == Some("Smoke"),
        "profile update",
    )?;
    ensure(
        renamed
            .provider_data
            .iter()
            .any(|p| p.provider_id == "password"),
        "provider data",
    )?;

    step("auth sign-out and email sign-in");
    auth.sign_out()?;
    notify.until(|| states.borrow().last() == Some(&None)).await;
    #[cfg(feature = "firestore")]
    {
        let db = whisker_firebase::firestore::Firestore::instance()?;
        let denied = failure(
            db.collection("users")
                .doc(&user.uid)
                .get_with(whisker_firebase::firestore::Source::Server)
                .await,
            "signed-out read",
        )?;
        ensure(
            denied.code == "permission-denied",
            "signed-out read is denied",
        )?;
    }
    let wrong = failure(
        auth.sign_in_with_email_and_password(&email, "wrong-password")
            .await,
        "wrong password",
    )?;
    ensure(
        wrong.service == "auth" && !wrong.code.is_empty() && !wrong.code.starts_with("ERROR_"),
        "wrong password",
    )?;
    let again = auth
        .sign_in_with_email_and_password(&email, "password123")
        .await?;
    ensure(again.user.uid == user.uid, "email sign-in")?;
    let taken = failure(
        auth.create_user_with_email_and_password(&email, "password123")
            .await,
        "duplicate account",
    )?;
    ensure(taken.code == "email-already-in-use", "duplicate account")?;

    step("auth delete");
    again.user.delete().await?;
    notify.until(|| states.borrow().last() == Some(&None)).await;
    let stale = failure(again.user.reload().await, "stale user")?;
    ensure(stale.code == "no-current-user", "stale user")?;
    registration.remove();
    Ok(())
}

#[cfg(feature = "storage")]
async fn storage_smoke(step: &dyn Fn(&str)) -> SmokeResult {
    use whisker_firebase::storage::{SettableMetadata, Storage};

    let storage = Storage::instance()?;
    storage.use_emulator(HOST, 9199)?;
    let folder = storage.reference(&format!("smoke/{PLATFORM}"));
    let hello = folder.child("hello.txt");

    step("storage upload");
    let metadata = hello
        .put_bytes_with(
            "hello 日本語".as_bytes(),
            &SettableMetadata::new()
                .content_type("text/plain")
                .custom("purpose", "smoke"),
        )
        .await?;
    ensure(
        metadata.size == "hello 日本語".len() as u64,
        "uploaded size",
    )?;
    ensure(metadata.full_path == hello.full_path(), "uploaded path")?;
    ensure(
        metadata.settable.content_type.as_deref() == Some("text/plain"),
        "content type",
    )?;

    step("storage download");
    ensure(
        hello.get_bytes(1024).await? == "hello 日本語".as_bytes(),
        "downloaded bytes",
    )?;
    ensure(hello.get_bytes(2).await.is_err(), "download size limit")?;
    ensure(
        hello.download_url().await?.starts_with("http"),
        "download URL",
    )?;

    step("storage metadata and listing");
    let updated = hello
        .update_metadata(&SettableMetadata::new().cache_control("no-cache"))
        .await?;
    ensure(
        updated.settable.cache_control.as_deref() == Some("no-cache"),
        "metadata update",
    )?;
    ensure(
        hello
            .metadata()
            .await?
            .settable
            .custom_metadata
            .get("purpose")
            .map(String::as_str)
            == Some("smoke"),
        "custom metadata",
    )?;
    folder.child("nested/inner.txt").put_bytes(b"inner").await?;
    let listing = folder.list_all().await?;
    ensure(listing.items.contains(&hello), "listed item")?;
    ensure(
        listing.prefixes.iter().any(|p| p.name() == "nested"),
        "listed prefix",
    )?;

    #[cfg(target_os = "ios")]
    {
        step("storage files");
        let file = std::env::temp_dir().join("whisker-storage-smoke.txt");
        hello.write_to_file(&file).await?;
        ensure(
            std::fs::read(&file)? == "hello 日本語".as_bytes(),
            "downloaded file",
        )?;
        folder.child("copy.txt").put_file(&file).await?;
        folder.child("copy.txt").delete().await?;
    }

    step("storage delete and rules");
    hello.delete().await?;
    folder.child("nested/inner.txt").delete().await?;
    ensure(
        failure(hello.metadata().await, "deleted object")?.code == "object-not-found",
        "deleted object",
    )?;
    let denied = failure(
        storage.reference("denied/x.txt").put_bytes(b"x").await,
        "storage rules",
    )?;
    ensure(denied.code == "unauthorized", "storage rules")?;
    Ok(())
}

/// Token and topic calls need a real Firebase project and push credentials; with the
/// demo configuration they must fail with a Messaging error rather than crash.
#[cfg(feature = "messaging")]
async fn messaging_smoke(step: &dyn Fn(&str)) -> SmokeResult<String> {
    use whisker_firebase::messaging::Messaging;

    let messaging = Messaging::instance()?;
    step("messaging settings");
    messaging.notification_settings().await?;
    let auto_init = messaging.is_auto_init_enabled()?;
    messaging.set_auto_init_enabled(!auto_init)?;
    ensure(
        messaging.is_auto_init_enabled()? == !auto_init,
        "auto-init toggle",
    )?;
    messaging.set_auto_init_enabled(auto_init)?;

    messaging.apns_token()?;

    step("messaging validation");
    let invalid = failure(
        messaging.subscribe_to_topic("not a topic").await,
        "invalid topic",
    )?;
    ensure(invalid.code == "invalid-argument", "topic validation")?;

    step("messaging token");
    let token = match messaging.token().await {
        Ok(token) => format!("token received ({} chars)", token.len()),
        Err(error) => {
            ensure(
                error.service == "messaging" && error.code != "bridge-error",
                "token error",
            )?;
            format!("token unavailable: {error}")
        }
    };
    Ok(token)
}

#[cfg(feature = "analytics")]
async fn analytics_smoke(step: &dyn Fn(&str)) -> SmokeResult {
    use whisker_firebase::analytics::{Analytics, Consent, Param, params};

    let analytics = Analytics::instance()?;
    step("analytics events");
    analytics.set_consent(Consent::new().analytics_storage(true))?;
    analytics.set_user_property("smoke_platform", Some(PLATFORM))?;
    analytics.set_default_event_parameters(params! { "smoke" => true })?;
    analytics.log_event(
        "whisker_smoke",
        params! { "platform" => PLATFORM, "count" => 3, "ratio" => 0.5 },
    )?;
    let item = params! { "item_id" => "sku-1", "price" => 1200 };
    analytics.log_event(
        "purchase",
        vec![
            ("currency".to_string(), Param::from("JPY")),
            ("value".to_string(), Param::from(1200)),
            ("items".to_string(), Param::Items(vec![item])),
        ],
    )?;
    analytics.log_screen_view("Smoke", Some("SmokeView"))?;
    analytics.set_session_timeout(std::time::Duration::from_secs(1800))?;
    step("analytics validation");
    let invalid = failure(
        analytics.log_event("firebase_reserved", params! {}),
        "reserved name",
    )?;
    ensure(invalid.code == "invalid-argument", "event name validation")?;
    analytics.app_instance_id().await?;
    Ok(())
}

#[cfg(feature = "crashlytics")]
fn crashlytics_smoke(step: &dyn Fn(&str)) -> SmokeResult {
    use whisker_firebase::crashlytics::Crashlytics;

    let crashlytics = Crashlytics::instance()?;
    step("crashlytics reports");
    crashlytics.set_collection_enabled(true)?;
    ensure(crashlytics.is_collection_enabled()?, "collection toggle")?;
    crashlytics.set_user_id("smoke-user")?;
    crashlytics.set_custom_key("platform", PLATFORM)?;
    crashlytics.set_custom_key("attempt", 1)?;
    crashlytics.set_custom_key("healthy", true)?;
    crashlytics.log("smoke test breadcrumb")?;
    let error = std::io::Error::other("smoke non-fatal");
    crashlytics.record_error(&error)?;
    Ok(())
}
