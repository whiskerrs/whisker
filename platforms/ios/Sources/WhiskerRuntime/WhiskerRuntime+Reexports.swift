// `WhiskerValue` / `WhiskerLynxAliases` (WhiskerUI / WhiskerContext /
// WhiskerCustomEvent) live in the smaller `WhiskerModule` SwiftPM
// target so third-party module packages can depend on just that.
// Host apps `import WhiskerRuntime`, so the WhiskerModule surface is
// re-exported here and stays reachable from either import.

@_exported import WhiskerModule
