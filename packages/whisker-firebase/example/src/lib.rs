//! Emulator-only Firebase smoke test. The demo project has no production resources.
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
        View(style: Css::new().padding_top(px(80)).padding_left(px(20)).padding_right(px(20))) {
            Text(value: computed(move || status.get()))
            Text(value: computed(move || live_text.get()))
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
    let mut passed = vec!["core"];
    #[cfg(feature = "firestore")]
    {
        firestore_smoke(&step, &live).await?;
        passed.push("firestore");
    }
    #[cfg(feature = "auth")]
    {
        auth_smoke(&step).await?;
        passed.push("auth");
    }
    #[cfg(feature = "storage")]
    {
        storage_smoke(&step).await?;
        passed.push("storage");
    }
    let _ = live;
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
