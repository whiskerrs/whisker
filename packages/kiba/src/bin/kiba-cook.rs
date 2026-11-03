//! Cooks source assets into the files a shipped game loads.

use std::process::ExitCode;

const USAGE: &str = "usage: kiba-cook probes <output.kiba> [--spacing <units>] <model.glb>...";
const DEFAULT_SPACING: f32 = 2.0;

fn run(arguments: &[String]) -> Result<String, String> {
    let [command, output, rest @ ..] = arguments else {
        return Err(USAGE.into());
    };
    if command != "probes" {
        return Err(USAGE.into());
    }
    let (spacing, inputs) = match rest {
        [flag, value, inputs @ ..] if flag == "--spacing" => (
            value
                .parse()
                .map_err(|_| format!("{value} is not a number"))?,
            inputs,
        ),
        inputs => (DEFAULT_SPACING, inputs),
    };
    let models = inputs
        .iter()
        .map(|path| {
            let bytes = std::fs::read(path).map_err(|error| format!("{path}: {error}"))?;
            kiba::Model::from_glb(&bytes).map_err(|error| format!("{path}: {error}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let bytes = kiba::cook::bake_probes_to_bytes(&models, spacing)?;
    std::fs::write(output, &bytes).map_err(|error| format!("{output}: {error}"))?;
    Ok(format!("wrote {output}, {} bytes", bytes.len()))
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match run(&arguments) {
        Ok(report) => {
            println!("{report}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
