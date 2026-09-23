mod project_support;
use whisker_plugin::project::ProjectUpdate;
use whisker_plugin::project::protocol::{self, Descriptor, Request, Response};

fn main() -> anyhow::Result<()> {
    let value: serde_json::Value = serde_json::from_reader(std::io::stdin())?;
    let bytes = serde_json::to_vec(&value)?;
    let request: Request = protocol::decode(&bytes)?;
    // Fault injection only for wire validation tests; normal calls use the
    // same author-side runner as a real project plugin binary.
    if let Request::Contribute { config, .. } = &request
        && let Some(fault) = config.get("__wire_fault").and_then(|v| v.as_str())
    {
        let descriptor = Descriptor::of(&project_support::Fixture);
        let response = Response::Contribute {
            descriptor: descriptor.clone(),
            update: ProjectUpdate::Keep,
        };
        let mut json: serde_json::Value = serde_json::from_slice(&protocol::encode(&response)?)?;
        match fault {
            "schema" => json["protocol"]["ir_schema"] = 99.into(),
            "descriptor" => json["payload"]["descriptor"]["name"] = "different".into(),
            "phase" => {
                json =
                    serde_json::from_slice(&protocol::encode(&Response::Describe { descriptor })?)?
            }
            "unversioned" => {
                json.as_object_mut().unwrap().remove("protocol");
            }
            _ => anyhow::bail!("unknown fixture fault"),
        }
        serde_json::to_writer(std::io::stdout(), &json)?;
        return Ok(());
    }
    protocol::serve(
        project_support::Fixture,
        bytes.as_slice(),
        std::io::stdout(),
    )
}
