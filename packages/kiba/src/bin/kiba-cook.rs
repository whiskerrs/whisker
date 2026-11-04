//! Cooks source assets into the files a shipped game loads.

use std::process::ExitCode;

const USAGE: &str = "usage:
  kiba-cook model <output.kiba> [--max-texture <pixels>] <model.glb>
  kiba-cook world <directory> <cell size> <probe spacing, or 0 for none> <model.glb>...
  kiba-cook synthetic-world <directory> <cells per side> <cell size> <probe spacing>
  kiba-cook probes <output.kiba> [--spacing <units>] <model.glb>...";
const DEFAULT_SPACING: f32 = 2.0;

fn run(arguments: &[String]) -> Result<String, String> {
    let [command, output, rest @ ..] = arguments else {
        return Err(USAGE.into());
    };
    if command == "model" {
        let (max_texture, input) = match rest {
            [flag, value, input] if flag == "--max-texture" => (
                value
                    .parse()
                    .map_err(|_| format!("{value} is not a whole number"))?,
                input,
            ),
            [input] => (u32::MAX, input),
            _ => return Err(USAGE.into()),
        };
        let bytes = kiba::cook::cook_model(&read_glb(input)?, max_texture)?;
        return write(output, &bytes);
    }
    if command == "world" {
        let [cell_size, probe_spacing, inputs @ ..] = rest else {
            return Err(USAGE.into());
        };
        let models = inputs
            .iter()
            .map(|path| read_glb(path))
            .collect::<Result<Vec<_>, _>>()?;
        let files = kiba::cook::cook_world(&models, number(cell_size)?, number(probe_spacing)?)?;
        return write_all(output, &files);
    }
    if command == "synthetic-world" {
        let [cells, cell_size, probe_spacing] = rest else {
            return Err(USAGE.into());
        };
        let cells: i32 = cells
            .parse()
            .map_err(|_| format!("{cells} is not a whole number"))?;
        let cell_size = number(cell_size)?;
        let model = kiba::cook::synthetic_world(cells, cell_size);
        let files = kiba::cook::cook_world(&[model], cell_size, number(probe_spacing)?)?;
        return write_all(output, &files);
    }
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
        .map(|path| read_glb(path))
        .collect::<Result<Vec<_>, _>>()?;
    let bytes = kiba::cook::bake_probes_to_bytes(&models, spacing)?;
    write(output, &bytes)
}

fn number(text: &str) -> Result<f32, String> {
    text.parse().map_err(|_| format!("{text} is not a number"))
}

fn write_all(directory: &str, files: &[(String, Vec<u8>)]) -> Result<String, String> {
    std::fs::create_dir_all(directory).map_err(|error| format!("{directory}: {error}"))?;
    let mut total = 0;
    for (name, bytes) in files {
        let path = format!("{directory}/{name}");
        std::fs::write(&path, bytes).map_err(|error| format!("{path}: {error}"))?;
        total += bytes.len();
    }
    Ok(format!(
        "wrote {} files to {directory}, {total} bytes",
        files.len()
    ))
}

fn read_glb(path: &str) -> Result<kiba::Model, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{path}: {error}"))?;
    kiba::Model::from_glb(&bytes).map_err(|error| format!("{path}: {error}"))
}

fn write(output: &str, bytes: &[u8]) -> Result<String, String> {
    std::fs::write(output, bytes).map_err(|error| format!("{output}: {error}"))?;
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
