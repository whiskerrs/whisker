package rs.whisker.modules.firebasestorage

import android.net.Uri
import com.google.android.gms.tasks.Task
import com.google.firebase.storage.FirebaseStorage
import com.google.firebase.storage.ListResult
import com.google.firebase.storage.StorageException
import com.google.firebase.storage.StorageMetadata
import com.google.firebase.storage.StorageReference
import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerModule
import rs.whisker.runtime.WhiskerPromise
import rs.whisker.runtime.WhiskerValue
import java.io.File

@WhiskerModule
class FirebaseStorageModule : Module() {
    private val storage get() = FirebaseStorage.getInstance()

    override fun definition() = ModuleDefinition {
        Name("FirebaseStorage")
        Function("useEmulator") { args ->
            try {
                val host = requireNotNull(args.getOrNull(0)?.asString())
                val port = requireNotNull(args.getOrNull(1)?.asInt()).toInt()
                require(port in 1..65535)
                storage.useEmulator(host, port)
                success(WhiskerValue.Null)
            } catch (error: Exception) { failure(error) }
        }
        AsyncFunction("putBytes") { args, promise ->
            resolve(promise, { it.metadata?.let(::encodeMetadata) ?: WhiskerValue.Null }) {
                val data = requireNotNull(args.getOrNull(1) as? WhiskerValue.Bytes).value
                reference(args).putBytes(data, settable(args.getOrNull(2)))
            }
        }
        AsyncFunction("putFile") { args, promise ->
            resolve(promise, { it.metadata?.let(::encodeMetadata) ?: WhiskerValue.Null }) {
                val file = File(requireNotNull(args.getOrNull(1)?.asString()))
                reference(args).putFile(Uri.fromFile(file), settable(args.getOrNull(2)))
            }
        }
        AsyncFunction("getBytes") { args, promise ->
            resolve(promise, { WhiskerValue.Bytes(it) }) {
                reference(args).getBytes(requireNotNull(args.getOrNull(1)?.asInt()))
            }
        }
        AsyncFunction("writeToFile") { args, promise ->
            resolve(promise, { WhiskerValue.Null }) {
                reference(args).getFile(File(requireNotNull(args.getOrNull(1)?.asString())))
            }
        }
        AsyncFunction("downloadUrl") { args, promise ->
            resolve(promise, { WhiskerValue.Str(it.toString()) }) { reference(args).downloadUrl }
        }
        AsyncFunction("getMetadata") { args, promise ->
            resolve(promise, ::encodeMetadata) { reference(args).metadata }
        }
        AsyncFunction("updateMetadata") { args, promise ->
            resolve(promise, ::encodeMetadata) { reference(args).updateMetadata(settable(args.getOrNull(1))) }
        }
        AsyncFunction("delete") { args, promise ->
            resolve(promise, { WhiskerValue.Null }) { reference(args).delete() }
        }
        AsyncFunction("list") { args, promise ->
            resolve(promise, ::encodeList) {
                val path = requireNotNull(args.getOrNull(0)?.asString())
                val reference = if (path.isEmpty()) storage.reference else storage.reference.child(path)
                val max = args.getOrNull(1)?.asInt()?.toInt()
                val token = args.getOrNull(2)?.asString()
                when {
                    max == null -> reference.listAll()
                    token != null -> reference.list(max, token)
                    else -> reference.list(max)
                }
            }
        }
    }

    private fun reference(args: List<WhiskerValue>): StorageReference {
        val path = requireNotNull(args.getOrNull(0)?.asString())
        require(path.isNotEmpty()) { "The bucket root is not an object" }
        return storage.reference.child(path)
    }

    private fun <T> resolve(promise: WhiskerPromise, encode: (T) -> WhiskerValue, start: () -> Task<T>) {
        try {
            start().addOnCompleteListener { task ->
                promise.resolve(if (task.isSuccessful) success(encode(task.result)) else failure(task.exception))
            }
        } catch (error: Exception) { promise.resolve(failure(error)) }
    }

    private fun settable(wire: WhiskerValue?): StorageMetadata {
        val fields = (wire as? WhiskerValue.Map)?.value ?: emptyMap()
        return StorageMetadata.Builder().apply {
            fields["content_type"]?.asString()?.let(::setContentType)
            fields["cache_control"]?.asString()?.let(::setCacheControl)
            fields["content_disposition"]?.asString()?.let(::setContentDisposition)
            fields["content_encoding"]?.asString()?.let(::setContentEncoding)
            fields["content_language"]?.asString()?.let(::setContentLanguage)
            (fields["custom_metadata"] as? WhiskerValue.Map)?.value?.forEach { (key, value) ->
                value.asString()?.let { setCustomMetadata(key, it) }
            }
        }.build()
    }

    private fun success(value: WhiskerValue) = WhiskerValue.Map(mapOf("value" to value))
    private fun failure(error: Exception?): WhiskerValue {
        val code = when (error) {
            is StorageException -> error.errorCode.toString()
            is IllegalArgumentException -> "invalid-argument"
            else -> "-13000"
        }
        return WhiskerValue.Map(mapOf("error" to WhiskerValue.Map(mapOf(
            "code" to WhiskerValue.Str(code),
            "message" to WhiskerValue.Str(error?.message ?: "Storage operation failed"),
        ))))
    }

    private fun optional(value: String?) = value?.let { WhiskerValue.Str(it) } ?: WhiskerValue.Null

    private fun encodeList(result: ListResult) = WhiskerValue.Map(mapOf(
        "items" to WhiskerValue.Array(result.items.map { WhiskerValue.Str(it.path.trimStart('/')) }),
        "prefixes" to WhiskerValue.Array(result.prefixes.map { WhiskerValue.Str(it.path.trimStart('/')) }),
        "next_page_token" to optional(result.pageToken),
    ))

    private fun encodeMetadata(metadata: StorageMetadata) = WhiskerValue.Map(mapOf(
        "bucket" to optional(metadata.bucket),
        "full_path" to optional(metadata.path),
        "name" to optional(metadata.name),
        "size" to WhiskerValue.Int(metadata.sizeBytes),
        "generation" to optional(metadata.generation),
        "md5_hash" to optional(metadata.md5Hash),
        "time_created" to WhiskerValue.Int(metadata.creationTimeMillis),
        "updated" to WhiskerValue.Int(metadata.updatedTimeMillis),
        "content_type" to optional(metadata.contentType),
        "cache_control" to optional(metadata.cacheControl),
        "content_disposition" to optional(metadata.contentDisposition),
        "content_encoding" to optional(metadata.contentEncoding),
        "content_language" to optional(metadata.contentLanguage),
        "custom_metadata" to WhiskerValue.Map(metadata.customMetadataKeys.associateWith { optional(metadata.getCustomMetadata(it)) }),
    ))
}
