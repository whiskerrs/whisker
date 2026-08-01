# Application Patterns

Use these patterns as boundaries, then confirm syntax in the target version's source and examples.

## Components and state

Keep components responsible for view composition and event wiring. Put state transitions, validation, serialization, and migration logic in plain Rust functions.

Component bodies run once at mount. Dynamic values must be read by an effect-backed attribute, a signal prop, `computed`, or another tracked closure.

```rust
let items = signal(Vec::<Item>::new());
let remaining = computed(move || {
    items.with(|items| items.iter().filter(|item| !item.done).count())
});
```

Passing `remaining` to a compatible prop stays reactive. Passing `remaining.get()` while mounting produces the current value only.

Use `update` for mutations that belong to one state transition:

```rust
items.update(|items| {
    if let Some(item) = items.iter_mut().find(|item| item.id == id) {
        item.done = !item.done;
    }
});
```

## Keyed collections

Use stable persisted identity rather than a list index.

```rust
ForEach(
    each: move || items.get(),
    key: |item: &Item| item.id,
    children: move |item: Item| render! {
        ItemRow(id: item.id, items: items)
    },
)
```

`ForEach` preserves the owner and element for a retained key. It does not call `children(item)` again, so `ItemRow` should derive mutable fields from the shared signal and stable ID. This preserves row-local state while allowing the row to repaint.

Use `list` when the collection is large enough to require native virtualization.

## Text input

Use `whisker-input` when it exists in the selected release. Prefer two-way binding for ordinary fields and controlled binding when input must be transformed.

```rust
let draft = signal(String::new());

render! {
    Input(
        text: draft,
        placeholder: "Add an item",
    )
}
```

Keep submission as one transition: normalize and validate the string, update state, persist it, then clear the field after success.

The input component accepts a raw style string in current releases. Confirm its props in `packages/whisker-input` instead of assuming every core `css!` value is accepted directly.

## Persistence

`whisker-local-store` stores strings. Serialize structured state with a versioned key and handle absent or malformed values explicitly.

```rust
const STATE_KEY: &str = "items.state.v1";

fn save_state(state: &StoredState) -> Result<(), String> {
    let json = serde_json::to_string(state).map_err(|error| error.to_string())?;
    WhiskerLocalStore::save(STATE_KEY.to_owned(), json)
        .map(|_| ())
        .map_err(|error| error.to_string())
}
```

Persist monotonic IDs with the data when identity must survive deletion, restart, and reordering. Treat schema migration separately from UI rendering.

Use secure storage for credentials and secrets. Local store is not a credential vault or cross-device database.

## Async work

Use `resource` for loadable async state and `spawn_local` for event-driven async work. Use `run_blocking` for synchronous work that would otherwise block the UI thread.

When the app enables Whisker's `tokio` feature, use the runtime already entered by Whisker. Current examples await Tokio-backed IO and `tokio::task::spawn_blocking` directly; verify that behavior in the target release before copying it.

Keep signal access on the UI thread. Return owned values from background work and apply them after awaiting on the Whisker task.

## Modules, plugins, and generated projects

- Use a module for a callable native capability.
- Use a plugin for native build or project configuration.
- Register plugins and app configuration in `whisker.rs`.
- Treat `gen/` as disposable output.
- Add only the platform permissions required by the feature.

## Validation

Test pure state transitions and serialization without a simulator. For UI behavior, verify input, keyboard handling, empty and long content, reordering, persistence after relaunch, accessibility, and a clean build in addition to hot reload.
