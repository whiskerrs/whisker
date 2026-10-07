package rs.whisker.modules.firebasefirestore

import android.os.Handler
import android.os.Looper
import com.google.firebase.Timestamp
import com.google.firebase.firestore.AggregateSource
import com.google.firebase.firestore.Blob
import com.google.firebase.firestore.DocumentChange
import com.google.firebase.firestore.DocumentReference
import com.google.firebase.firestore.DocumentSnapshot
import com.google.firebase.firestore.FieldValue
import com.google.firebase.firestore.FirebaseFirestore
import com.google.firebase.firestore.FirebaseFirestoreException
import com.google.firebase.firestore.GeoPoint
import com.google.firebase.firestore.ListenerRegistration
import com.google.firebase.firestore.MetadataChanges
import com.google.firebase.firestore.Query
import com.google.firebase.firestore.QuerySnapshot
import com.google.firebase.firestore.SetOptions
import com.google.firebase.firestore.Source
import com.google.firebase.firestore.WriteBatch
import rs.whisker.runtime.Module
import rs.whisker.runtime.ModuleDefinition
import rs.whisker.runtime.WhiskerModule
import rs.whisker.runtime.WhiskerValue
import java.util.concurrent.ConcurrentHashMap

@WhiskerModule
class FirestoreModule : Module() {
    private val listeners = ConcurrentHashMap<Long, ListenerRegistration>()
    private val mainHandler = Handler(Looper.getMainLooper())

    override fun definition() = ModuleDefinition {
        Name("Firestore")
        Events("snapshot")
        Function("useEmulator") { args ->
            try {
                val host = requireNotNull(args.getOrNull(0)?.asString())
                val port = requireNotNull(args.getOrNull(1)?.asInt()).toInt()
                require(port in 1..65535)
                FirebaseFirestore.getInstance().useEmulator(host, port)
                success(WhiskerValue.Null)
            } catch (error: Exception) { failure(error) }
        }
        Function("documentId") { args ->
            try {
                val path = requireNotNull(args.firstOrNull()?.asString())
                success(WhiskerValue.Str(FirebaseFirestore.getInstance().collection(path).document().id))
            } catch (error: Exception) { failure(error) }
        }
        AsyncFunction("getDocument") { args, promise ->
            try {
                val db = FirebaseFirestore.getInstance()
                val path = requireNotNull(args.getOrNull(0)?.asString())
                db.document(path).get(source(args.getOrNull(1)?.asString())).addOnCompleteListener { task ->
                    promise.resolve(
                        if (task.isSuccessful) catching { encodeDocument(task.result, db) } else failure(task.exception),
                    )
                }
            } catch (error: Exception) { promise.resolve(failure(error)) }
        }
        AsyncFunction("getQuery") { args, promise ->
            try {
                val db = FirebaseFirestore.getInstance()
                query(requireNotNull(args.getOrNull(0)), db).get(source(args.getOrNull(1)?.asString()))
                    .addOnCompleteListener { task ->
                        promise.resolve(
                            if (task.isSuccessful) catching { encodeQuery(task.result, db) } else failure(task.exception),
                        )
                    }
            } catch (error: Exception) { promise.resolve(failure(error)) }
        }
        AsyncFunction("count") { args, promise ->
            try {
                val db = FirebaseFirestore.getInstance()
                query(requireNotNull(args.getOrNull(0)), db).count().get(AggregateSource.SERVER)
                    .addOnCompleteListener { task ->
                        promise.resolve(
                            if (task.isSuccessful) success(WhiskerValue.Int(task.result.count)) else failure(task.exception),
                        )
                    }
            } catch (error: Exception) { promise.resolve(failure(error)) }
        }
        AsyncFunction("commit") { args, promise ->
            try {
                val db = FirebaseFirestore.getInstance()
                val ops = requireNotNull(args.getOrNull(0) as? WhiskerValue.Array).value
                val batch = db.batch()
                ops.forEach { apply(it, batch, db) }
                batch.commit().addOnCompleteListener {
                    promise.resolve(if (it.isSuccessful) success(WhiskerValue.Null) else failure(it.exception))
                }
            } catch (error: Exception) { promise.resolve(failure(error)) }
        }
        Function("listen") { args ->
            try {
                val db = FirebaseFirestore.getInstance()
                val id = requireNotNull(args.getOrNull(0)?.asInt())
                val target = requireNotNull(args.getOrNull(1) as? WhiskerValue.Map).value
                val metadata = if (args.getOrNull(2)?.asBool() == true) MetadataChanges.INCLUDE else MetadataChanges.EXCLUDE
                val registration = when (target["kind"]?.asString()) {
                    "document" -> db.document(requireNotNull(target["path"]?.asString()))
                        .addSnapshotListener(metadata) { snapshot, error ->
                            emit(id, error) { encodeDocument(requireNotNull(snapshot), db) }
                        }
                    "query" -> query(requireNotNull(target["query"]), db)
                        .addSnapshotListener(metadata) { snapshot, error ->
                            emit(id, error) { encodeQuery(requireNotNull(snapshot), db) }
                        }
                    else -> throw IllegalArgumentException("Unknown listener target")
                }
                listeners[id] = registration
                success(WhiskerValue.Null)
            } catch (error: Exception) { failure(error) }
        }
        Function("unlisten") { args ->
            args.firstOrNull()?.asInt()?.let { listeners.remove(it)?.remove() }
            success(WhiskerValue.Null)
        }
        Function("unlistenAll") { _ ->
            listeners.keys.toList().forEach { listeners.remove(it)?.remove() }
            success(WhiskerValue.Null)
        }
    }

    private fun emit(id: Long, error: FirebaseFirestoreException?, encode: () -> WhiskerValue) {
        val send = Runnable {
            if (!listeners.containsKey(id)) return@Runnable
            val result = if (error != null) failure(error) else catching(encode)
            val payload = (result as WhiskerValue.Map).value.toMutableMap()
            payload["id"] = WhiskerValue.Int(id)
            sendEvent("snapshot", WhiskerValue.Map(payload))
        }
        if (Looper.myLooper() == Looper.getMainLooper()) send.run() else mainHandler.post(send)
    }

    private fun success(value: WhiskerValue) = WhiskerValue.Map(mapOf("value" to value))
    private fun failure(code: String, message: String) = WhiskerValue.Map(mapOf("error" to WhiskerValue.Map(mapOf(
        "code" to WhiskerValue.Str(code), "message" to WhiskerValue.Str(message),
    ))))
    private fun failure(error: Exception?, fallback: String = "invalid-argument"): WhiskerValue {
        val code = when (error) {
            is FirebaseFirestoreException -> error.code.name.lowercase(java.util.Locale.ROOT).replace('_', '-')
            is IllegalStateException -> "failed-precondition"
            else -> fallback
        }
        return failure(code, error?.message ?: "Firestore operation failed")
    }
    private fun catching(encode: () -> WhiskerValue): WhiskerValue =
        try { success(encode()) } catch (error: Exception) { failure(error, "unsupported-value") }
    private fun source(name: String?) = when (name) {
        "server" -> Source.SERVER
        "cache" -> Source.CACHE
        else -> Source.DEFAULT
    }

    private fun encodeDocument(snapshot: DocumentSnapshot, db: FirebaseFirestore) = WhiskerValue.Map(mapOf(
        "path" to WhiskerValue.Str(snapshot.reference.path),
        "data" to (snapshot.data?.let { data -> WhiskerValue.Map(data.mapValues { encode(it.value, db) }) } ?: WhiskerValue.Null),
        "from_cache" to WhiskerValue.Bool(snapshot.metadata.isFromCache),
        "has_pending_writes" to WhiskerValue.Bool(snapshot.metadata.hasPendingWrites()),
    ))

    private fun encodeQuery(snapshot: QuerySnapshot, db: FirebaseFirestore) = WhiskerValue.Map(mapOf(
        "docs" to WhiskerValue.Array(snapshot.documents.map { encodeDocument(it, db) }),
        "changes" to WhiskerValue.Array(snapshot.documentChanges.map { change ->
            WhiskerValue.Map(mapOf(
                "type" to WhiskerValue.Str(when (change.type) {
                    DocumentChange.Type.ADDED -> "added"
                    DocumentChange.Type.MODIFIED -> "modified"
                    DocumentChange.Type.REMOVED -> "removed"
                }),
                "old_index" to WhiskerValue.Int(change.oldIndex.toLong()),
                "new_index" to WhiskerValue.Int(change.newIndex.toLong()),
                "doc" to encodeDocument(change.document, db),
            ))
        }),
        "from_cache" to WhiskerValue.Bool(snapshot.metadata.isFromCache),
        "has_pending_writes" to WhiskerValue.Bool(snapshot.metadata.hasPendingWrites()),
    ))

    private fun query(wire: WhiskerValue, db: FirebaseFirestore): Query {
        val spec = requireNotNull(wire as? WhiskerValue.Map).value
        var query: Query = spec["path"]?.asString()?.let { db.collection(it) }
            ?: db.collectionGroup(requireNotNull(spec["group"]?.asString()))
        (spec["filters"] as? WhiskerValue.Array)?.value?.forEach { filter ->
            val f = requireNotNull(filter as? WhiskerValue.Map).value
            val field = requireNotNull(f["field"]?.asString())
            val value = decode(requireNotNull(f["value"]), db)
            val list = { requireNotNull(value as? List<*>) { "Filter needs an array" } }
            query = when (f["op"]?.asString()) {
                "==" -> query.whereEqualTo(field, value)
                "!=" -> query.whereNotEqualTo(field, value)
                "<" -> query.whereLessThan(field, requireNotNull(value))
                "<=" -> query.whereLessThanOrEqualTo(field, requireNotNull(value))
                ">" -> query.whereGreaterThan(field, requireNotNull(value))
                ">=" -> query.whereGreaterThanOrEqualTo(field, requireNotNull(value))
                "array-contains" -> query.whereArrayContains(field, requireNotNull(value))
                "array-contains-any" -> query.whereArrayContainsAny(field, list())
                "in" -> query.whereIn(field, list())
                "not-in" -> query.whereNotIn(field, list())
                else -> throw IllegalArgumentException("Unknown filter operator")
            }
        }
        (spec["order_by"] as? WhiskerValue.Array)?.value?.forEach { order ->
            val o = requireNotNull(order as? WhiskerValue.Map).value
            val direction = if (o["descending"]?.asBool() == true) Query.Direction.DESCENDING else Query.Direction.ASCENDING
            query = query.orderBy(requireNotNull(o["field"]?.asString()), direction)
        }
        spec["limit"]?.asInt()?.let { limit ->
            query = if (spec["limit_to_last"]?.asBool() == true) query.limitToLast(limit) else query.limit(limit)
        }
        fun cursor(key: String): Pair<Array<Any?>, Boolean>? {
            val c = (spec[key] as? WhiskerValue.Map)?.value ?: return null
            val values = requireNotNull(c["values"] as? WhiskerValue.Array).value.map { decode(it, db) }
            return values.toTypedArray() to (c["inclusive"]?.asBool() == true)
        }
        cursor("start")?.let { (values, inclusive) ->
            query = if (inclusive) query.startAt(*values) else query.startAfter(*values)
        }
        cursor("end")?.let { (values, inclusive) ->
            query = if (inclusive) query.endAt(*values) else query.endBefore(*values)
        }
        return query
    }

    @Suppress("UNCHECKED_CAST")
    private fun apply(wire: WhiskerValue, batch: WriteBatch, db: FirebaseFirestore) {
        val op = requireNotNull(wire as? WhiskerValue.Map).value
        val document = db.document(requireNotNull(op["path"]?.asString()))
        val data = { requireNotNull(op["data"] as? WhiskerValue.Map).value.mapValues { decode(it.value, db) } }
        when (op["op"]?.asString()) {
            "set" -> {
                val mergeFields = (op["merge_fields"] as? WhiskerValue.Array)?.value?.mapNotNull { it.asString() }
                when {
                    mergeFields != null -> batch.set(document, data(), SetOptions.mergeFields(mergeFields))
                    op["merge"]?.asBool() == true -> batch.set(document, data(), SetOptions.merge())
                    else -> batch.set(document, data())
                }
            }
            "update" -> batch.update(document, data() as Map<String, Any>)
            "delete" -> batch.delete(document)
            else -> throw IllegalArgumentException("Unknown write")
        }
    }

    private fun tag(type: String, value: WhiskerValue) = WhiskerValue.Map(mapOf("type" to WhiskerValue.Str(type), "value" to value))
    private fun encode(value: Any?, db: FirebaseFirestore): WhiskerValue = when (value) {
        null -> tag("null", WhiskerValue.Null)
        is Boolean -> tag("bool", WhiskerValue.Bool(value))
        is Long -> tag("integer", WhiskerValue.Int(value))
        is Int -> tag("integer", WhiskerValue.Int(value.toLong()))
        is Double -> tag("double", WhiskerValue.Float(value))
        is String -> tag("string", WhiskerValue.Str(value))
        is Blob -> tag("bytes", WhiskerValue.Bytes(value.toBytes()))
        is Timestamp -> tag("timestamp", WhiskerValue.Array(listOf(WhiskerValue.Int(value.seconds), WhiskerValue.Int(value.nanoseconds.toLong()))))
        is GeoPoint -> tag("geo_point", WhiskerValue.Array(listOf(WhiskerValue.Float(value.latitude), WhiskerValue.Float(value.longitude))))
        is DocumentReference -> {
            require(value.firestore === db) { "Cross-database references are unsupported" }
            tag("reference", WhiskerValue.Str(value.path))
        }
        is List<*> -> tag("array", WhiskerValue.Array(value.map { encode(it, db) }))
        is Map<*, *> -> tag("map", WhiskerValue.Map(value.entries.associate { requireNotNull(it.key as? String) to encode(it.value, db) }))
        else -> throw IllegalArgumentException("Unsupported Firestore value: ${value.javaClass.name}")
    }
    private fun decode(wire: WhiskerValue, db: FirebaseFirestore): Any? {
        val fields = requireNotNull(wire as? WhiskerValue.Map).value
        val type = requireNotNull(fields["type"]?.asString())
        val value = requireNotNull(fields["value"])
        val list = { requireNotNull(value as? WhiskerValue.Array).value.map { decode(it, db) } }
        return when (type) {
            "null" -> { require(value === WhiskerValue.Null); null }
            "bool" -> requireNotNull(value as? WhiskerValue.Bool).value
            "integer" -> requireNotNull(value as? WhiskerValue.Int).value
            "double" -> requireNotNull(value as? WhiskerValue.Float).value
            "string" -> requireNotNull(value as? WhiskerValue.Str).value
            "bytes" -> Blob.fromBytes(requireNotNull(value as? WhiskerValue.Bytes).value)
            "reference" -> db.document(requireNotNull(value.asString()))
            "array" -> list()
            "map" -> requireNotNull(value as? WhiskerValue.Map).value.mapValues { decode(it.value, db) }
            "timestamp" -> {
                val values = requireNotNull(value as? WhiskerValue.Array).value
                require(values.size == 2)
                val seconds = requireNotNull(values[0] as? WhiskerValue.Int).value
                val nanos = requireNotNull(values[1] as? WhiskerValue.Int).value
                require(nanos in 0..999999999)
                Timestamp(seconds, nanos.toInt())
            }
            "geo_point" -> {
                val values = requireNotNull(value as? WhiskerValue.Array).value
                require(values.size == 2)
                GeoPoint(requireNotNull(values[0].asDouble()), requireNotNull(values[1].asDouble()))
            }
            "server_timestamp" -> FieldValue.serverTimestamp()
            "delete" -> FieldValue.delete()
            "increment" -> when (val by = decode(value, db)) {
                is Long -> FieldValue.increment(by)
                is Double -> FieldValue.increment(by)
                else -> throw IllegalArgumentException("Invalid increment")
            }
            "array_union" -> FieldValue.arrayUnion(*list().toTypedArray())
            "array_remove" -> FieldValue.arrayRemove(*list().toTypedArray())
            else -> throw IllegalArgumentException("Unknown Firestore value type: $type")
        }
    }
}
