# whisker-firebase

Firebase for Whisker on **Android and iOS**: Cloud Firestore, Authentication, Cloud Storage, Cloud Messaging, Analytics, and Crashlytics for the default Firebase app.

```toml
[dependencies]
whisker-firebase = { version = "0.1", features = ["firestore", "auth", "storage", "messaging", "analytics", "crashlytics"] }
serde = { version = "1", features = ["derive"] }
```

| Feature | Crate | Native SDK |
|---|---|---|
| (always) | `whisker-firebase-core` | FirebaseCore |
| `firestore` | `whisker-firebase-firestore` | Cloud Firestore |
| `auth` | `whisker-firebase-auth` | Firebase Authentication |
| `storage` | `whisker-firebase-storage` | Cloud Storage for Firebase |
| `messaging` | `whisker-firebase-messaging` | Firebase Cloud Messaging |
| `analytics` | `whisker-firebase-analytics` | Google Analytics for Firebase |
| `crashlytics` | `whisker-firebase-crashlytics` | Firebase Crashlytics |

No service is enabled by default. Each feature activates an optional crate that ships its own Kotlin/Swift sources and SDK dependency, and CNG links only the selected ones. These are ordinary Cargo packages; adding them to this repository does not publish them to crates.io.

The API follows the Firebase SDKs' names (`collection().doc()`, `where_field`, `on_snapshot`, `sign_in_with_email_and_password`, `put_bytes`, …) with Rust conventions: `async` operations, `Result<T, FirebaseError>`, serde for document data, and listeners that stop when their registration is dropped.

## App setup

Register an Android app and/or an iOS app in your Firebase project. Download the matching `google-services.json` and `GoogleService-Info.plist`, then put them in your app crate directory. The package/application ID and Bundle ID must match `whisker.rs`.

```rust,ignore
fn main() {
    whisker_cng::run(|app| {
        app.name("MyApp").bundle_id("com.example.myapp");
        app.project_plugin::<whisker_firebase::WhiskerFirebase>(|firebase| {
            // These are also the defaults. Paths are relative to the app crate.
            firebase.android_config = "google-services.json".into();
            firebase.ios_config = "GoogleService-Info.plist".into();
        });
    });
}
```

The CNG plugin validates the configuration and application identity, stages the Android JSON in the application module, applies the Google Services Gradle plugin, and copies the Apple plist into the main app bundle. Only the selected platform's file is required. Android's Firebase SDK minimum is enforced without lowering a higher application minimum; current Whisker apps require Android API 24 or later. The native Swift modules require iOS 15 or later. Firebase Apple SDK 12.19.2 requires Xcode 26.2 or later.

Android initializes Firebase from generated resources. On iOS, `FirebaseApp::initialize()` configures the default app on the main thread; every service's `instance()` calls it for you. No AppDelegate replacement is installed; only `messaging` relies on Firebase's AppDelegate proxy (see [Cloud Messaging](#cloud-messaging)).

## Errors

Every operation returns `whisker_firebase::Result<T>`. `FirebaseError` has a `service` (`app`, `firestore`, `auth`, `storage`, `messaging`, `analytics`, `crashlytics`) and a `code` using the JavaScript SDK's names, and displays as `firestore/permission-denied: …`:

```rust,ignore
match doc.get().await {
    Err(e) if e.code == "permission-denied" => show_sign_in(),
    Err(e) => return Err(e.into()),
    Ok(snapshot) => render(snapshot),
}
```

Invalid arguments (malformed paths, bad queries, `FieldValue::delete()` outside an update) fail with `invalid-argument` before reaching the SDK. `bridge-error` means the native module could not be reached; `unsupported-platform` is returned on desktop and Web.

## Firestore

```rust,ignore
use serde::{Deserialize, Serialize};
use whisker_firebase::firestore::{data, Direction, FieldValue, FilterOp, Firestore};
use whisker_firebase::Timestamp;

#[derive(Serialize, Deserialize)]
struct City {
    name: String,
    population: i64,
    founded: Timestamp,
}

let db = Firestore::instance()?;
let cities = db.collection("cities");

// Write and read your own types.
let tokyo = cities.doc("tokyo");
tokyo.set(&City { name: "Tokyo".into(), population: 14_000_000, founded: Timestamp::now() }).await?;
let city: Option<City> = tokyo.get().await?.data()?; // None if the document is missing
let id = cities.add(&City { /* … */ }).await?.id().to_owned();

// Partial updates: dotted field paths and write-time transforms.
tokyo.update(&data! {
    "population" => FieldValue::increment(1),
    "stats.updatedAt" => FieldValue::server_timestamp(),
}).await?;

// Queries. Invalid combinations are reported by `get`, `count`, or `on_snapshot`.
let largest = cities
    .where_field("population", FilterOp::GreaterThan, 1_000_000)
    .order_by("population", Direction::Descending)
    .limit(10)
    .get()
    .await?;
for doc in &largest {
    println!("{} {:?}", doc.id(), doc.get::<String>("name")?);
}
let total = cities.count().await?;

// Atomic batches.
let mut batch = db.batch();
batch.set(&cities.doc("osaka"), &osaka).delete(&cities.doc("old"));
batch.commit().await?;

// Realtime updates until `registration` is dropped.
let registration = cities.on_snapshot(|snapshot| {
    for change in snapshot.unwrap().doc_changes() { /* Added / Modified / Removed */ }
})?;

// Inside a component: a signal that follows the document.
let live = tokyo.snapshot_signal(); // ReadSignal<Option<Result<DocumentSnapshot>>>
```

- **Data.** Anything `Serialize`/`Deserialize` maps to Firestore values; structs and maps become maps, enums follow serde's default representation, and `Option::None` is `null`. Use `Timestamp`, `GeoPoint`, `Bytes`, and `DocumentReference` fields for the corresponding native types (a plain `Vec<u8>` is an array of integers). Integers are signed 64-bit; larger `u64` values are rejected. `Value` holds untyped data, and `to_value`/`from_value` convert explicitly.
- **Field names.** Keys in `set`/`add` data are literal field names. Keys passed to `update`, `where_field`, `order_by`, and `SetOptions::merge_fields` are dotted paths.
- **Writes** resolve after server acknowledgement and remain pending while offline. `set_with(…, SetOptions::merge())` merges; `update` fails with `not-found` for a missing document.
- **Reads** use the SDK's default server/cache policy; `get_with(Source::Cache | Source::Server)` overrides it. Snapshots expose `metadata()` (`from_cache`, `has_pending_writes`).
- **Queries** support `==`, `!=`, `<`, `<=`, `>`, `>=`, `array-contains`, `array-contains-any`, `in`, `not-in` (combined with AND), `order_by`, `limit`, `limit_to_last`, value cursors (`start_at`, `start_after`, `end_at`, `end_before`), collection groups, and `count()`.

## Authentication

```rust,ignore
use whisker_firebase::auth::{Auth, AuthCredential, ProfileUpdate};

let auth = Auth::instance()?;
let credential = auth.create_user_with_email_and_password("ada@example.com", "secret123").await?;
let user = credential.user;
user.update_profile(&ProfileUpdate::new().display_name(Some("Ada"))).await?;
let token = user.id_token(false).await?; // send to your backend

// Upgrade an anonymous account.
let guest = auth.sign_in_anonymously().await?.user;
guest.link_with_credential(&AuthCredential::email("ada@example.com", "secret123")).await?;

// Observe sign-in state (fires immediately with the current user).
let registration = auth.on_auth_state_changed(|user| { /* Option<User> */ })?;
// Or, inside a component:
let user = auth.user_signal(); // ReadSignal<Option<User>>
auth.sign_out()?;
```

Also available: `sign_in_with_custom_token`, `send_password_reset_email`, `on_id_token_changed`, and on `User`: `reload`, `update_password`, `verify_before_update_email`, `send_email_verification`, `delete`, `reauthenticate_with_credential`. A `User` value is a snapshot; its methods act on the SDK's current user and fail with `no-current-user` or `user-mismatch` if another user is signed in.

Google, Apple, and other OAuth providers work through `AuthCredential::google`, `::apple`, and `::oauth` with tokens from a native sign-in library; this package does not present provider sign-in UI. Error codes match the JavaScript SDK (`invalid-credential`, `email-already-in-use`, `requires-recent-login`, …). With email enumeration protection enabled, a wrong password reports `invalid-credential`.

## Cloud Storage

```rust,ignore
use whisker_firebase::storage::{SettableMetadata, Storage};

let storage = Storage::instance()?; // default bucket from the configuration file
let avatar = storage.reference("users/ada/avatar.png");
avatar.put_bytes_with(&png, &SettableMetadata::new().content_type("image/png")).await?;
let url = avatar.download_url().await?;
let bytes = avatar.get_bytes(5 * 1024 * 1024).await?; // fails above the limit
let listing = storage.reference("users/ada").list_all().await?; // items and prefixes
avatar.delete().await?;
```

`put_file`/`write_to_file` stream between the bucket and an absolute local path. `metadata`, `update_metadata`, and paged `list` are also available. Uploads and downloads resolve on completion; progress reporting, pausing, and cancellation are not exposed yet.

## Cloud Messaging

```rust,ignore
use whisker_firebase::messaging::{Messaging, PermissionOptions};

let messaging = Messaging::instance()?;
let settings = messaging.request_permission(PermissionOptions::default()).await?;
if settings.is_allowed() {
    let token = messaging.token().await?; // send to your server
}
let _refresh = messaging.on_token_refresh(|token| { /* re-upload */ })?;
messaging.subscribe_to_topic("news").await?;

// Foreground messages, and taps on notifications.
let _message = messaging.on_message(|message| { /* message.notification, message.data */ })?;
let _opened = messaging.on_message_opened_app(|message| { /* navigate */ })?;
if let Some(message) = messaging.initial_message().await? { /* the tap that launched the app */ }
```

Enabling `messaging` also runs its CNG plugin, which adds the `aps-environment` entitlement (`development` by default) and the `remote-notification` background mode on iOS. Android needs no project changes: the module's manifest declares the messaging service and `POST_NOTIFICATIONS`.

```rust,ignore
app.project_plugin::<whisker_firebase::messaging::WhiskerFirebaseMessaging>(|messaging| {
    messaging.aps_environment = "production".into();
});
```

To receive messages, upload an APNs authentication key in the Firebase console (iOS) and use a real Firebase project; the FCM backend is not part of the Emulator Suite.

- **Permission.** `request_permission` shows the system prompt on iOS and Android 13+. Android reports only `Authorized` or `Denied`.
- **Foreground.** On iOS, notifications are presented according to `set_foreground_presentation` (all by default) and delivered to `on_message`. Android shows nothing for foreground messages; handle them in `on_message`, which also receives data-only messages.
- **Taps.** `initial_message` returns the tap that launched the app; `on_message_opened_app` reports taps while the app keeps running. On Android, a tap recreates Whisker's activity (and the Rust app with it), so it is also reported through `initial_message`.
- **Background.** Notification messages are displayed by the system. Background data messages and iOS silent notifications are not delivered to Rust yet, because the Whisker runtime may not be running.
- **iOS integration.** The module becomes the `UNUserNotificationCenter` delegate at launch and relies on Firebase's default AppDelegate proxy for the APNs token, so keep `FirebaseAppDelegateProxyEnabled` enabled. Code that replaces the notification center delegate stops these callbacks.
- **Tokens.** `token`, `delete_token`, `on_token_refresh`, `set_auto_init_enabled`, and `apns_token` (iOS) are available. Firebase's installation-ID registration mode is not supported.

## Analytics

```rust,ignore
use whisker_firebase::analytics::{Analytics, Consent, params};

let analytics = Analytics::instance()?;
analytics.log_event("select_content", params! { "content_type" => "image", "item_id" => 42 })?;
analytics.log_screen_view("Settings", None)?; // call from your router
analytics.set_user_id(Some("user-42"))?;
analytics.set_user_property("favorite_food", Some("pizza"))?;
analytics.set_consent(Consent::new().analytics_storage(true).ad_storage(false))?;
let instance_id = analytics.app_instance_id().await?;
```

Parameter values are strings, integers, doubles (booleans become 0/1), or `Param::Items` for the ecommerce `items` list. Firebase's limits are checked before logging, because the SDKs drop invalid events silently: names use up to 40 letters, digits, and underscores and cannot start with `firebase_`, `google_`, or `ga_`; events carry up to 25 parameters; string values are limited to 100 characters; user properties are limited to 24-character names and 36-character values. Whisker has no native screens, so screen views are only logged when you call `log_screen_view`.

Enabling `analytics` runs its CNG plugin, which adds `-ObjC` to the iOS app's linker flags (required by the Analytics binary SDK). To collect nothing until the user consents, start with collection disabled and enable it at runtime with `set_collection_enabled(true)`:

```rust,ignore
app.project_plugin::<whisker_firebase::analytics::WhiskerFirebaseAnalytics>(|analytics| {
    analytics.collection_enabled = Some(false);
});
```

## Crashlytics

```rust,ignore
use whisker_firebase::crashlytics::Crashlytics;

let crashlytics = Crashlytics::instance()?;
crashlytics.record_panics(); // report Rust panics as crashes with their message
crashlytics.set_user_id("user-42")?;
crashlytics.set_custom_key("plan", "pro")?;
crashlytics.log("opened settings")?;
if let Err(error) = sync().await {
    crashlytics.record_error(&error)?; // non-fatal with the error's source chain
}
if crashlytics.did_crash_on_previous_execution()? { /* offer feedback */ }
```

Native crashes are reported on the next launch, including the abort that ends a Rust panic. Enabling `crashlytics` runs its CNG plugin, which applies the Crashlytics Gradle plugin (3.0.8) after Google Services and links `firebase-crashlytics-ndk` on Android, so native crashes are captured, and adds a Release-only build phase that uploads the iOS app's dSYMs (`upload_symbols = false` removes it). The plugin runs after the `whisker-firebase` plugin, so use the crate through the umbrella crate's `crashlytics` feature. `collection_enabled = Some(false)` keeps crash reports on the device until the app calls `set_collection_enabled(true)` or `send_unsent_reports()`; while collection is disabled, the iOS SDK does not record logs, custom keys, or non-fatal errors.

- **Panics.** Without `record_panics`, a panic appears as an anonymous native abort. With it, the crash is reported as `RustPanic` with the panic message and location (plus the Rust backtrace when the library keeps debug info), written synchronously through the SDK's crash handler (the regular non-fatal API is asynchronous and would not survive the abort).
- **Symbols.** Rust code lives in the WhiskerDriver framework on iOS and in the app's native library on Android. Upload their symbols (Crashlytics `upload-symbols` for the Rust dSYM on iOS, the Gradle plugin's native symbol upload on Android) to symbolicate Rust frames in native crashes.
- **Testing.** `crash()` crashes from native code. On iOS, Crashlytics does not capture crashes while the debugger is attached.

## Not yet supported

Transactions, OR/composite filters, snapshot cursors, aggregate `sum`/`average`, persistence settings, named apps/databases/buckets, phone and multi-factor auth, provider sign-in UI, upload progress, background message handlers, Analytics ecommerce helpers beyond `items`, and other Firebase services (Remote Config, Functions, Realtime Database, App Check, Performance Monitoring). Desktop and Web calls return `unsupported-platform`.

## Local smoke test

The `example/` app uses a **demo project** and intentionally fake Firebase configuration. It talks only to the Emulator Suite; replace these files with your own downloaded configuration for a real app. Never deploy the example's permissive test rules to production.

```sh
# Repository root; leave running in another terminal.
firebase emulators:start --only auth,firestore,storage --project demo-whisker-firebase \
  --config packages/whisker-firebase/example/firebase.json

cargo run -p whisker-cli --bin whisker -- --no-tui run ios \
  --manifest-path packages/whisker-firebase/example/Cargo.toml --features firestore,auth,storage,messaging,analytics,crashlytics
# Or replace ios with android, using the Android Emulator.
```

The example reaches the emulators at `127.0.0.1` on iOS Simulator and `10.0.2.2` on Android Emulator. It exercises typed documents, transforms, queries, batches, listeners and snapshot signals, security rules, anonymous and email sign-in, account linking, uploads, downloads, metadata, and listing. Success appears as `FIREBASE_SMOKE_OK` with the list of services that passed; any subset of the features can be enabled. With the demo configuration, FCM token requests fail with a Messaging error, which the smoke test reports. The app also requests notification permission and shows received messages; on iOS Simulator, send one with:

```sh
echo '{"aps": {"alert": {"title": "Hello", "body": "From simctl"}}, "gcm.message_id": "1", "kind": "smoke"}' \
  | xcrun simctl push booted rs.whisker.firebaseexample -
```

When executing a configuration binary directly, pass app feature selection **after** `--`:

```sh
cargo run -p whisker-firebase-example --bin whisker-config \
  --features whisker-config -- ios android --features firestore,auth,storage,messaging,analytics,crashlytics
```

The Cargo flags before `--` compile the generator; the flags after it select the application dependency graph used by CNG and the native build.

## SDK references

- [Firebase Android setup](https://firebase.google.com/docs/android/setup)
- [Firebase Apple setup](https://firebase.google.com/docs/ios/setup)
- [Firebase Local Emulator Suite](https://firebase.google.com/docs/emulator-suite)

Native pins: Android BoM 34.19.0, Google Services Gradle plugin 4.5.0, Firebase Apple SDK 12.19.2. Core and service modules use the same Firebase SDK versions.
