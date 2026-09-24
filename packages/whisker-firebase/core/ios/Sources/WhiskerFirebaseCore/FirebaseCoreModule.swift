import Foundation
import FirebaseCore
import WhiskerModule

@WhiskerModule
public final class FirebaseCoreModule: Module {
    public override func definition() -> ModuleDefinition {
        ModuleDefinition {
            Name("FirebaseCore")
            Function("initialize") { (_: [WhiskerValue]) -> WhiskerValue in
                if Thread.isMainThread { return Self.initializeApp() }
                return DispatchQueue.main.sync { Self.initializeApp() }
            }
        }
    }

    private static func initializeApp() -> WhiskerValue {
        if FirebaseApp.app() == nil {
            guard let path = Bundle.main.path(forResource: "GoogleService-Info", ofType: "plist"),
                  let options = FirebaseOptions(contentsOfFile: path) else {
                return failure("invalid-config", "Missing or invalid GoogleService-Info.plist in the application bundle")
            }
            FirebaseApp.configure(options: options)
        }
        guard let app = FirebaseApp.app(), let projectID = app.options.projectID else {
            return failure("invalid-config", "Firebase application has no project ID")
        }
        return .map(["value": .map([
            "name": .string(app.name),
            "app_id": .string(app.options.googleAppID),
            "project_id": .string(projectID),
            "storage_bucket": app.options.storageBucket.map { WhiskerValue.string($0) } ?? .null,
        ])])
    }

    private static func failure(_ code: String, _ message: String) -> WhiskerValue {
        .map(["error": .map(["code": .string(code), "message": .string(message)])])
    }
}
