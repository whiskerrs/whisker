package rs.whisker.runtime

/**
 * Marks a concrete [Module] declaration for Whisker's generated registry.
 *
 * The annotation is the explicit registration signal shared with the Rust and
 * Swift module APIs. KSP validates that the annotated class extends [Module].
 */
@Target(AnnotationTarget.CLASS)
@Retention(AnnotationRetention.SOURCE)
public annotation class WhiskerModule
