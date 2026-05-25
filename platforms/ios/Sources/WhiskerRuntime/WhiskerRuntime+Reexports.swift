// Phase J — `WhiskerValue` / `WhiskerLynxAliases` (WhiskerUI /
// WhiskerContext / WhiskerCustomEvent) moved to the smaller
// `WhiskerModule` SwiftPM target so third-party Whisker module
// packages can depend on just that. Host apps still `import
// WhiskerRuntime`, so re-export the WhiskerModule surface from here
// to preserve backward compatibility — anything that was reachable
// via `import WhiskerRuntime` before the split stays reachable.

@_exported import WhiskerModule
