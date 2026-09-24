# whisker-firebase

Firebase for Whisker on **Android and iOS**: Cloud Firestore, Authentication, and Cloud Storage for the default Firebase app.

```toml
[dependencies]
whisker-firebase = { version = "0.1", features = ["firestore", "auth", "storage"] }
serde = { version = "1", features = ["derive"] }
```

| Feature | Crate | Native SDK |
|---|---|---|
| (always) | `whisker-firebase-core` | FirebaseCore |
| `firestore` | `whisker-firebase-firestore` | Cloud Firestore |
| `auth` | `whisker-firebase-auth` | Firebase Authentication |
| `storage` | `whisker-firebase-storage` | Cloud Storage for Firebase |

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

Android initializes Firebase from generated resources. On iOS, `FirebaseApp::initialize()` configures the default app on the main thread; every service's `instance()` calls it for you. No AppDelegate replacement or method swizzling is installed by this package.

## Errors

Every operation returns `whisker_firebase::Result<T>`. `FirebaseError` has a `service` (`app`, `firestore`, `auth`, `storage`) and a `code` using the JavaScript SDK's names, and displays as `firestore/permission-denied: …`:

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

## Not yet supported

Transactions, OR/composite filters, snapshot cursors, aggregate `sum`/`average`, persistence settings, named apps/databases/buckets, phone and multi-factor auth, provider sign-in UI, upload progress, and other Firebase services (Messaging, Analytics, Remote Config, Functions, Realtime Database, Crashlytics). Desktop and Web calls return `unsupported-platform`.

## Local smoke test

The `example/` app uses a **demo project** and intentionally fake Firebase configuration. It talks only to the Emulator Suite; replace these files with your own downloaded configuration for a real app. Never deploy the example's permissive test rules to production.

```sh
# Repository root; leave running in another terminal.
firebase emulators:start --only auth,firestore,storage --project demo-whisker-firebase \
  --config packages/whisker-firebase/example/firebase.json

cargo run -p whisker-cli --bin whisker -- --no-tui run ios \
  --manifest-path packages/whisker-firebase/example/Cargo.toml --features firestore,auth,storage
# Or replace ios with android, using the Android Emulator.
```

The example reaches the emulators at `127.0.0.1` on iOS Simulator and `10.0.2.2` on Android Emulator. It exercises typed documents, transforms, queries, batches, listeners and snapshot signals, security rules, anonymous and email sign-in, account linking, uploads, downloads, metadata, and listing. Success appears as `FIREBASE_SMOKE_OK` with the list of services that passed; any subset of the features can be enabled.

When executing a configuration binary directly, pass app feature selection **after** `--`:

```sh
cargo run -p whisker-firebase-example --bin whisker-config \
  --features whisker-config -- ios android --features firestore,auth,storage
```

The Cargo flags before `--` compile the generator; the flags after it select the application dependency graph used by CNG and the native build.

## SDK references

- [Firebase Android setup](https://firebase.google.com/docs/android/setup)
- [Firebase Apple setup](https://firebase.google.com/docs/ios/setup)
- [Firebase Local Emulator Suite](https://firebase.google.com/docs/emulator-suite)

Native pins: Android BoM 34.19.0, Google Services Gradle plugin 4.5.0, Firebase Apple SDK 12.19.2. Core and service modules use the same Firebase SDK versions.
