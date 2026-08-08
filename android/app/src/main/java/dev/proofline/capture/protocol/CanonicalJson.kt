package dev.proofline.capture.protocol

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject

object CanonicalJson {
    val json = Json { encodeDefaults = true; explicitNulls = false; ignoreUnknownKeys = true }

    fun encode(element: JsonElement): String = when (element) {
        is JsonObject -> element.entries.sortedBy { it.key }.joinToString(separator = ",", prefix = "{", postfix = "}") {
            "${Json.encodeToString(it.key)}:${encode(it.value)}"
        }
        is JsonArray -> element.joinToString(separator = ",", prefix = "[", postfix = "]") { encode(it) }
        else -> element.toString()
    }
}
