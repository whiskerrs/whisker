# RFC 0001: Runtime Modules and Build-time Plugins

- Status: Draft
- Authors: Whisker maintainers
- Created: 2026-08-18
- Discussion: TBD
- Tracking issue: TBD
- Depends on: None
- Supersedes: None

## Summary

Whisker has two first-class composition units:

- a **Whisker module** is a runtime component that provides and consumes
  versioned interfaces and whose instances and lifecycle are managed by a
  registry;
- a **Whisker plugin** is a build-time component that contributes to a
  declarative project model from which Whisker generates and builds the host
  project.

An application is assembled twice:

1. at build time, plugins compose the platform project and the set of module
   providers embedded in it;
2. at runtime, the module registry composes those providers into the running
   application.

This makes the two concepts counterparts without conflating them. A package
may distribute modules, plugins, both, or neither. A Rust crate, Swift package,
Gradle artifact, JavaScript package, or generated Xcode/Gradle project is a
distribution or build unit, not inherently a module or plugin.

## Motivation

The current terms describe particular implementations:

- `PlatformModule` is a name-based Rust-to-Kotlin/Swift invocation bridge;
- native `ModuleDefinition` values register functions, events, constants,
  views, and props;
- `whisker-plugin` exposes the CNG mutation API and an in-process/subprocess
  execution protocol;
- CNG produces generated iOS and Android projects from templates plus plugin
  mutations.

Those mechanisms are useful, but they are too narrow for the architecture
after removing Lynx. Rendering, layout, motion, storage, routing, and the
application entry point should be composable through the same runtime model.
Likewise, Android, iOS, Web, and Desktop projects should be reproducibly
assembled by build-time components instead of accumulating platform-specific
special cases in the CLI.

The model must also avoid two misleading equivalences:

- not every Rust crate is a Whisker module;
- not every package containing a module needs a package-specific plugin.

## Goals

- Define module and plugin independently of language, transport, and package
  manager.
- Make their identity, versioning, dependency resolution, and lifecycle
  explicit.
- Allow core facilities such as a renderer to be ordinary typed modules.
- Allow a generated Xcode, Gradle, Web, or Desktop project to be the
  deterministic result of plugin composition.
- Make the generated project contain all runtime providers, host code,
  registrations, resources, and build dependencies required by the selected
  application.
- Preserve an efficient direct-call path for Rust-only modules and packed
  protocols for hot paths.
- Keep most of the module and plugin layers testable without a device, JVM,
  Apple runtime, browser, or generated project.

## Non-goals

- Making every helper, algorithm, trait, or crate a module.
- Requiring one plugin for every module or one module for every plugin.
- Defining the renderer frame protocol or the style/layout/paint model. Later
  RFCs build those contracts on this module model.
- Making generated projects the source of truth. They remain disposable build
  products.
- Preserving the current raw module invocation API or CNG wire format as the
  final public protocol. They are migration inputs, not compatibility
  constraints for this RFC.
- Supporting Dioxus as an application or rendering backend.

## Terminology

### Interface

A named, versioned behavioral contract. An interface specifies methods,
events, value types, error behavior, scheduling rules, and any performance
contract visible to consumers. Examples include `whisker.renderer@1` and
`whisker.haptics@1`.

Interface versions belong to the contract, not its implementation. A consumer
requires an interface version range; it does not normally require a concrete
module by name.

### Module

A named runtime provider and/or consumer of interfaces. A module definition
declares identity, implementation version, provided and required interfaces,
supported targets, and lifecycle scope. The registry creates module instances,
resolves their requirements, and tears them down.

### Plugin

A named build-time transformation over a versioned Project IR. A plugin
declares identity, implementation version, supported Build API/IR versions,
targets, configuration schema, requirements, and ordering constraints. It does
not participate in the runtime registry.

### Package

A distribution unit such as a Cargo crate, Swift package, Maven artifact, npm
package, or repository checkout. One package may carry zero or more module
providers and zero or more plugins.

### Host

The platform environment to which the Whisker runtime is connected:
Kotlin/Java on Android, Swift/Objective-C on iOS, and JavaScript on Web and the
initial Desktop WebView backend. A Host implementation can provide module
interfaces, but `Host` is not a second module type.

### Kernel

The minimal runtime mechanism that makes modules possible: registry,
interface resolution, instance handles, lifecycle dispatch, callback/event
dispatch, memory ownership across transports, and frame transaction
coordination. The kernel contains no renderer or platform policy.

### Project IR

The declarative, platform-aware intermediate representation transformed by
plugins and materialized by target renderers into Xcode, Gradle, Web, or
Desktop project files.

## Two-phase composition

The complete application assembly is:

```text
source packages + application declaration
                    |
                    v
        discover and select build plugins
                    |
                    v
     resolve plugin requirements and ordering
                    |
                    v
       seed and transform versioned Project IR
                    |
                    v
 validate -> normalize -> render generated project
                    |
                    v
      Xcode / Gradle / Web / Desktop build
                    |
                    v
        installable Whisker application
                    |
                    v
        bootstrap runtime module registry
                    |
                    v
 resolve interfaces -> start module instances -> run app
```

Conceptually:

```text
ProjectIR = Compose(AppDeclaration, SelectedPlugins, ModuleProviders, Target)

AppArtifact = PlatformBuild(Render(ProjectIR))

RunningApp = RuntimeCompose(EmbeddedModuleProviders, AppModule)
```

Build-time composition determines what code and resources exist in the
artifact. Runtime composition determines which instances satisfy the running
application's interface requirements. Runtime module resolution must never
silently fetch or install missing build artifacts.

## Whisker module model

### Definition

A Whisker module is a named provider/consumer of versioned interfaces whose
instances and lifecycle are managed by a registry. Its implementation
language, call transport, and distribution package are independent properties.

A conceptual module descriptor is:

```rust
struct ModuleDescriptor {
    id: ModuleId,
    version: Version,
    provides: Vec<ProvidedInterface>,
    requires: Vec<RequiredInterface>,
    targets: TargetSet,
    lifecycle: LifecycleScope,
}

struct ProvidedInterface {
    id: InterfaceId,
    version: Version,
}

struct RequiredInterface {
    id: InterfaceId,
    compatible_with: VersionRequirement,
    cardinality: Cardinality,
}
```

The descriptor is explanatory, not a commitment to a particular serialization
or Rust API.

### Modules are not crates

The following are all valid:

- a Rust crate containing no modules, such as a CSS tokenizer or spring
  integrator;
- a Rust crate providing one module;
- a package providing several modules;
- a Host-language package providing a module with only a generated Rust
  consumer binding;
- a module whose implementation is split across Rust and the Host;
- two platform-specific providers implementing the same interface.

Code becomes a module when it needs runtime discovery, replacement,
capability resolution, managed lifecycle, or a stable boundary. Pure functions
and internal implementation details remain ordinary library code.

### Consumers resolve interfaces

Application code and framework modules depend on interface contracts:

```rust
let renderer = registry.require::<RendererV1>()?;
let surface = renderer.attach_surface(host_surface, config, events)?;
```

They do not perform a method-name lookup on every call. The registry resolves
the provider once and returns a typed handle or vtable. If several providers
satisfy a singular requirement, resolution fails unless application policy
selects one. Collection interfaces may explicitly accept multiple providers.

Concrete module IDs remain available for diagnostics, configuration, explicit
provider selection, and tests.

### Dependency cardinality and resolution

Runtime dependencies are edges between a consumer and a versioned interface,
not between Cargo crates and not normally between concrete module IDs. A
requirement declares its cardinality:

```rust,ignore
enum Cardinality {
    ExactlyOne,
    ZeroOrOne,
    Many,
}
```

`ExactlyOne` rejects both a missing provider and an ambiguous set of providers
unless application policy explicitly selects one. `ZeroOrOne` injects an
optional typed handle. `Many` injects every compatible provider in a stable,
deterministic order and is used for extension collections such as element or
style-extension providers.

For example, a scene runtime can require one renderer and collect multiple UI
element providers without any UI module naming or depending on that renderer:

```text
Application --requires exactly one--> SceneV1

Scene Runtime --requires exactly one--> RendererV1
Scene Runtime --requires many---------> ElementProviderV1

View Module ----provides--------------> ElementProviderV1
Text Module ----provides--------------> ElementProviderV1
DOM Renderer ---provides--------------> RendererV1
```

The registry performs these steps before starting application code:

1. collect descriptors embedded by build-time composition;
2. discard providers incompatible with the target or requested scope;
3. match every requirement by interface ID and compatible version;
4. apply explicit provider-selection policy;
5. reject missing, ambiguous, duplicate singular, or cyclic dependencies;
6. construct typed dependency handles or collections;
7. start providers before their consumers.

Two providers in a `Many` collection may still conflict at the contract level.
For example, two element providers claiming the same canonical element key are
an error unless that interface defines an explicit replacement policy.

Callbacks do not create reverse registry dependencies. If module A requires
module B and B must later notify A, A passes B a typed callback or event sink
while binding. B must not resolve A from the registry, because doing so would
turn a directed dependency into a cycle:

```text
A --requires--> B
A --passes callback/event sink--> B
B --emits through sink----------> A
```

This rule is particularly important for a renderer returning frame and input
events to the scene runtime.

### Runtime dependencies are not package or plugin dependencies

Three graphs coexist and must remain explicit:

| Graph | Edge means |
|---|---|
| Package/build graph | Code or artifacts are needed to compile and distribute another package |
| Plugin graph | One build-time Project IR transformation must run with or after another |
| Runtime module graph | A running module instance requires a versioned interface provider |

A Rust UI crate may depend on an `element-api` crate to compile while its
runtime module only *provides* `ElementProviderV1`. Selecting that runtime
provider may activate a companion build plugin that embeds a Host factory, but
neither relationship implies that the UI module requires a concrete renderer
at runtime.

### Identity and versioning

Four versions must not be collapsed into one:

| Version | Meaning |
|---|---|
| Package version | Version of the distributed Cargo/Maven/Swift/npm package |
| Module version | Version of one provider implementation |
| Interface version | Compatibility boundary seen by runtime consumers |
| Transport/protocol version | Encoding used across FFI, JNI, WASM, or another boundary |

For example, module `whisker.renderer.dom` version `2.4.0` may provide
`whisker.renderer@1`. Updating its internal DOM implementation does not require
an interface major version change.

Interface compatibility is checked during project composition when possible
and again when the runtime registry is created. An incompatible or missing
required interface is a startup error, not a delayed string-dispatch failure.

### Lifecycle

The initial lifecycle scopes are:

| Scope | Created | Destroyed | Example |
|---|---|---|---|
| Process | Once per process | Process shutdown | diagnostics sink |
| Application | Once per Whisker application | application shutdown | router root |
| Window | Once per native/WebView window | window close | window commands |
| Surface | Once per rendered surface | surface close | renderer |
| Owner | Bound to a reactive owner | owner disposal | scoped resource |

The registry starts providers after their required providers and stops them in
reverse dependency order. Failed startup tears down already-started instances
in reverse order. Module lifecycle is unrelated to Cargo object lifetime or
plugin execution lifetime.

Lifecycle callbacks must be idempotent from the registry's perspective: a
failed or repeated shutdown may not leak a Host registration. Exact callback
signatures are deferred until the runtime API is implemented.

### Calls and events

The interface definition declares, per operation:

- synchronous request/response, asynchronous request/response, or event
  stream;
- allowed caller and executor/thread;
- ownership of arguments, results, and callbacks;
- cancellation and backpressure behavior;
- whether the operation may cross a transport boundary;
- latency or batching requirements where performance is part of correctness.

Transport is selected when binding a provider:

```text
Rust consumer -> Rust provider       direct typed call
Rust consumer -> Kotlin/Swift Host   generated FFI/JNI binding
WASM consumer -> JavaScript Host     generated WASM/JS binding
```

The semantic interface remains the same, but the interface must not pretend an
inherently asynchronous transport is synchronous. Conversely, an implementation
must not force asynchronous serialization onto a direct Rust call.

Renderer-class hot paths use a typed, resolved handle and packed batches. The
generic name/value invocation API may remain as an escape hatch for debugging
and experimental integrations, but it is not the foundation of official
interfaces.

### Rust-only modules

A Rust-only provider is a normal module when it satisfies the module criteria.
It uses the same descriptor, interface version, registry, and lifecycle model,
but resolves to direct calls with no serialization. This allows a recording
renderer, fake clock, in-memory storage provider, style engine, or protocol
test double to run entirely in Rust.

Being Rust-only does not itself make a crate a module. A stateless color
conversion function remains an ordinary library function.

### The application is a module

The application entry point provides a versioned Application interface. After
constructing the registry, the bootstrap resolves and starts this module. This
removes a framework-only runtime path for application code; application startup
uses the same dependency and lifecycle rules as other runtime components.

The bootstrap and kernel remain irreducible mechanisms. Saying that a Whisker
application is composed of modules does not imply that registry construction
is itself dynamically resolved from that registry.

## Whisker plugin model

### Definition

A Whisker plugin is a build-time component that transforms Project IR through
a versioned Build API. Plugins are resolved, configured, ordered, executed,
and discarded before the generated platform project is built.

A conceptual descriptor is:

```rust
struct PluginDescriptor {
    id: PluginId,
    version: Version,
    build_api: VersionRequirement,
    project_ir: VersionRequirement,
    targets: TargetSet,
    requires: Vec<PluginRequirement>,
    before: Vec<PluginId>,
    after: Vec<PluginId>,
    activation: Activation,
}
```

Plugin identity and version are separate from any runtime module distributed
beside it.

### Project IR, not files, is the composition boundary

Plugins contribute intent to structured Project IR. The target renderer owns
filesystem writes and platform syntax. The IR must grow to represent at least:

- application identity, versions, targets, and deployment constraints;
- runtime module providers and generated registry bootstrap;
- Rust, Kotlin/Java, Swift/Objective-C, JavaScript, and WASM inputs;
- Maven, SwiftPM, Cargo, npm, system framework, and native library
  dependencies;
- manifests, entitlements, permissions, capabilities, URL schemes, and
  application lifecycle hooks;
- generated bindings and code-generation steps;
- resources, assets, fonts, and localization inputs;
- target build settings, packaging, signing requirements, and output metadata.

Escape hatches such as raw Gradle lines, pbxproj operations, and arbitrary
extra files are necessary while the typed IR is incomplete. They must be
explicitly marked as target-specific, included in conflict reporting, and
kept out of the portable contract. Repeated need for the same escape hatch is
a signal to add a typed IR field.

Plugins do not directly edit an existing Xcode or Gradle project. This keeps
generation deterministic, conflict detection possible, and generated trees
disposable.

### Build pipeline

For each target, the build engine performs these stages:

1. Resolve the application declaration and package dependency graph.
2. Discover available module providers and plugin descriptors.
3. Select plugins using application configuration and declared module
   requirements.
4. Validate plugin, Build API, Project IR, target, and interface compatibility.
5. Topologically order plugins and reject missing dependencies or cycles.
6. Seed Project IR from the application declaration and target defaults.
7. Run each plugin as a deterministic transformation, recording attributed
   contributions in a mutation journal.
8. Detect conflicts, normalize unordered collections, and validate the final
   IR.
9. Render the complete generated project into `gen/<target>/`.
10. Invoke the target build system and record the composition in build
    metadata suitable for diagnostics and caching.

Given the same application declaration, package lockfiles, plugin binaries,
plugin configuration, toolchain inputs, and target, composition must produce
equivalent Project IR and generated files. A plugin that reads undeclared
environment state, network state, current time, or arbitrary filesystem paths
is non-hermetic and invalidates reproducible caching unless those inputs are
explicitly declared and fingerprinted.

### Activation

Merely appearing transitively in a package graph must not grant a plugin
unbounded authority over the generated project. A plugin runs only when one of
the following is true:

- the application explicitly enables and configures it;
- a selected module provider declares it as a required build companion;
- it is a mandatory framework plugin for the selected target.

The resolved set and the reason each plugin was activated are written to build
diagnostics. Package metadata may advertise plugins, but discovery alone does
not imply activation.

This is stricter than parts of the current discovery pipeline and is an
intentional target for migration.

### Ordering and conflicts

Ordering constraints express semantic dependencies, not conflict resolution by
accident. The engine rejects cycles and references to required plugins that are
not selected.

Every contribution is attributed to a plugin. Two incompatible contributions
to the same singular field are errors unless the IR operation explicitly
expresses an override and ordering makes the winner deterministic. Set-like
collections are normalized and deduplicated. Source snippets and arbitrary
files cannot silently overwrite each other.

### Execution and trust

In-process and subprocess execution are implementation choices for the same
Build API. A subprocess JSON protocol is useful for version isolation and
diagnostics, but is not a security boundary by itself. Native plugin binaries
can access the developer's machine with the authority of the build command.

The engine should eventually support declared inputs/outputs and a restricted
execution mode. Until then, plugin installation and activation must be treated
as executing build code, shown in diagnostics, and suitable for repository
review and lockfile pinning.

## Relationship between modules and plugins

Modules and plugins are orthogonal but often shipped together:

| Distribution contents | Valid use |
|---|---|
| Module only | Pure Rust provider or provider covered by generic target integration |
| Plugin only | App icon, signing, generated resource, or platform project customization |
| Module and plugin | Native capability that needs runtime code plus project declarations |
| Neither | Ordinary algorithm, macro, or data-model library |

A module descriptor declares what must exist at runtime. A plugin declares how
the selected target project acquires and packages what is needed at build time.
The build engine checks that the final Project IR contains a provider for each
required runtime interface.

Not every module needs a custom plugin. Mandatory framework plugins can consume
declarative module-provider metadata and perform generic work such as:

- adding a module's Maven or SwiftPM artifact;
- including Rust features or object code;
- including a JavaScript Host provider;
- generating typed bindings and registration tables;
- preserving provider metadata for runtime bootstrap.

A package-specific plugin is needed only when typed metadata is insufficient or
the application needs configurable build-time behavior.

### Example: haptics

A future `whisker-haptics` package could contain:

```text
package whisker-haptics
|- Rust typed consumer API
|- Android provider: whisker.haptics.android
|- iOS provider:     whisker.haptics.ios
|- interface:        whisker.haptics@1
`- build plugin:     whisker.haptics.build
```

The runtime providers implement the same interface. The plugin contributes the
Android vibration permission and any required platform artifacts. Building for
iOS ignores the Android-only contribution; building for Android produces a
project containing the Android provider and its generated registry entry.

The module and plugin IDs are intentionally different. They may share a source
package and release version for convenience without becoming the same entity.

### Example: renderer

A DOM renderer package can provide a `whisker.renderer@1` module implemented by
JavaScript and a build plugin that adds the Host bootstrap, WASM loader, DOM
entry point, and generated module registry to Web and Desktop projects. A Rust
recording renderer used in unit tests can provide the same interface without a
plugin or Host code.

This is how renderer selection becomes ordinary module resolution while the
target-specific project assembly remains a plugin responsibility.

## Application declaration

The user-facing declaration selects targets, module-provider policy, and
build-time plugins. The exact API remains open, but it should preserve typed
configuration:

```rust,ignore
fn configure(app: &mut App) {
    app.target(Target::Android);
    app.module::<WhiskerHaptics>();
    app.plugin::<WhiskerHapticsBuild>(|config| {
        config.vibration_permission(true);
    });
}
```

This example does not require users to spell both calls in all cases. Selecting
the module may activate a declared required companion plugin with default
configuration. An explicit plugin declaration can configure it or activate a
plugin that has no runtime module.

The resulting declaration must be serializable without loading the runtime,
so configuration probing stays small and build tools other than the CLI can
produce the same input.

## Platform mapping

| Target | Runtime body | Host | Generated project |
|---|---|---|---|
| Android | Native Rust | Kotlin/Java | Gradle project |
| iOS | Native Rust | Swift/Objective-C | Xcode project |
| Web | Rust/WASM | JavaScript | Web project/bundle |
| Desktop v1 | Rust/WASM in system WebView | WebView JavaScript | Desktop launcher plus Web assets |

Desktop v1 has one Whisker-visible Host: JavaScript. Native launcher code may
be generated to create the WebView and load assets, but it is not a second
runtime Host or module registry. If native Desktop capabilities are introduced
later, their internal bridge remains behind the JavaScript Host unless a later
RFC deliberately changes this model.

## Invariants

The implementation must preserve these rules:

1. Runtime code cannot invoke a build plugin.
2. A plugin cannot resolve or call a live runtime module.
3. Package identity is not module, plugin, or interface identity.
4. Consumers require interfaces; provider selection is explicit policy.
5. Module implementation versions and interface versions are independent.
6. Plugin versions and Build API/Project IR versions are independent.
7. Transport affects bindings and performance, not interface identity.
8. Generated projects are complete build inputs but remain reproducible,
   disposable outputs of composition.
9. Missing runtime providers and incompatible interfaces fail no later than
   application bootstrap and should normally fail at build composition.
10. The module registry and plugin engine remain independently testable with
    Rust-only providers, plugins, and Project IR fixtures.

## Mapping from the current implementation

| Current concept | Direction under this RFC |
|---|---|
| `PlatformModule` and `module!` | Raw dynamic transport; official APIs move to generated typed interface handles |
| Kotlin/Swift `ModuleDefinition` | One source of Host provider descriptors and bindings |
| `whisker-plugin::Plugin` | Seed for the versioned Build Plugin API |
| `GenerateContext` with iOS/Android IR | Seed for a broader versioned Project IR including Web/Desktop and runtime composition |
| `PluginRequest`/`PluginResponse` JSON | One possible transport for the Build Plugin API |
| mutation journal | Retained and expanded to cover every contribution |
| Cargo metadata discovery | Retained for availability discovery, separated from plugin activation |
| `whisker-cng` target renderers | Retained as the sole materializers of generated project files |
| `#[whisker::main]` | Provider of the Application interface, resolved by bootstrap |

## Migration outline

1. Introduce stable IDs and separate version fields for interfaces, modules,
   plugins, Build API, and Project IR.
2. Split plugin discovery from activation and emit a resolved-composition
   diagnostic report.
3. Extend Project IR with module-provider metadata, generated registry entries,
   source/dependency declarations, and Web/Desktop targets.
4. Generate typed runtime interfaces over the existing native bridge while
   retaining raw invocation as an escape hatch.
5. Add a registry capable of direct Rust providers and Host providers, then
   model the current platform modules through it.
6. Move renderer selection and application bootstrap onto typed interfaces.
7. Remove legacy special cases only after their generated projects and runtime
   behavior are represented by plugins and modules respectively.

Each step can land without requiring the renderer and styling RFCs to be fully
implemented.

## Alternatives considered

### Treat every Rust crate as a module

Rejected. Crates are compile and distribution units. Applying registry,
versioned-interface, and lifecycle semantics to pure implementation libraries
would add ceremony without enabling composition.

### Make a module and plugin one object

Rejected. They execute in different processes and phases, have different
version boundaries, and may exist independently. A package may offer a
convenient facade configuring both, but their identities remain separate.

### Let plugins edit generated projects directly

Rejected as the primary contract. It makes ordering implicit, conflict
detection weak, and regeneration dependent on previous filesystem state.
Target-specific escape hatches remain available while the typed IR matures.

### Put all project generation logic in the CLI

Rejected. It centralizes every integration in the framework release and keeps
third-party packages from declaring their own build requirements. The CLI
should orchestrate plugin composition and target builds, not own package
policy.

## Open questions

The following must be resolved before this RFC becomes `Accepted`:

- canonical namespace rules for module, interface, and plugin IDs;
- the serialized package descriptor and whether a separate Whisker composition
  lockfile is needed in addition to package-manager lockfiles;
- exact compatible-version rules for interfaces and Project IR evolution;
- whether multiple major versions of one interface may coexist in a registry;
- the declarative boundary between module-provider metadata and a required
  companion plugin;
- the first restricted/sandboxed plugin execution contract and its declared
  input model;
- how application configuration selects one provider when several satisfy the
  same interface.
