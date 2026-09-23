// An older/newer binary must fail preflight without receiving an app project.
fn main() -> anyhow::Result<()> {
    let request: serde_json::Value = serde_json::from_reader(std::io::stdin())?;
    assert_eq!(request["payload"]["request"], "describe");
    assert!(request["payload"].get("context").is_none());
    println!(
        r#"{{"protocol":{{"version":1,"ir_schema":999}},"payload":{{"response":"describe","descriptor":{{"name":"incompatible","after":[],"before":[]}}}}}}"#
    );
    Ok(())
}
