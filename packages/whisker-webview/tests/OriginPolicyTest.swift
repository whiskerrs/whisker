import Foundation

@main
struct OriginPolicyTest {
    static func main() throws {
        let input = try String(contentsOfFile: CommandLine.arguments[1], encoding: .utf8)
        for row in input.split(separator: "\n") {
            let fields = row.split(separator: "\t", omittingEmptySubsequences: false).map(String.init)
            precondition(fields.count == 3)
            precondition(OriginPolicy.matches(fields[1], fields[2]) == (fields[0] == "true"), String(row))
        }
        print("origin policy regression cases passed")
    }
}
