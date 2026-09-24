package rs.whisker.modules.firebasecore

import com.google.firebase.FirebaseApp
import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerModule
import rs.whisker.runtime.WhiskerValue

@WhiskerModule
class FirebaseCoreModule : Module() {
    override fun definition() = ModuleDefinition {
        Name("FirebaseCore")
        Function("initialize") {
            try {
                // FirebaseInitProvider initializes from Google Services resources before the Activity.
                val app = FirebaseApp.getInstance()
                val projectId = app.options.projectId
                    ?: throw IllegalStateException("Firebase application has no project ID")
                WhiskerValue.Map(mapOf("value" to WhiskerValue.Map(mapOf(
                    "name" to WhiskerValue.Str(app.name),
                    "app_id" to WhiskerValue.Str(app.options.applicationId),
                    "project_id" to WhiskerValue.Str(projectId),
                    "storage_bucket" to (app.options.storageBucket?.let { WhiskerValue.Str(it) } ?: WhiskerValue.Null),
                ))))
            } catch (error: IllegalStateException) {
                WhiskerValue.Map(mapOf("error" to WhiskerValue.Map(mapOf(
                    "code" to WhiskerValue.Str("invalid-config"),
                    "message" to WhiskerValue.Str(error.message ?: "Firebase initialization failed"),
                ))))
            }
        }
    }
}
