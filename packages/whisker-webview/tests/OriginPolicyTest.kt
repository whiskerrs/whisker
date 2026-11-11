package rs.whisker.elements.webview

import java.io.File

fun main(args: Array<String>) {
    File(args[0]).forEachLine { row ->
        val fields = row.split('\t')
        check(fields.size == 3)
        check(OriginPolicy.matches(fields[1], fields[2]) == fields[0].toBooleanStrict()) { row }
    }
    println("origin policy regression cases passed")
}
