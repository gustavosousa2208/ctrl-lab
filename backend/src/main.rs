use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use ctrl_backend::{parse_project_json, simulate_validated_dag};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args();
    let program = args.next().unwrap_or_else(|| "ctrl-backend".to_string());

    let Some(project_path) = args.next() else {
        return Err(usage(&program));
    };

    if args.next().is_some() {
        return Err(format!(
            "expected exactly one project path argument\n{}",
            usage(&program)
        ));
    }

    let project_file = Path::new(&project_path);
    let project_json = fs::read_to_string(project_file).map_err(|error| {
        format!(
            "failed to read project file `{}`: {error}",
            project_file.display()
        )
    })?;

    let validated = parse_project_json(&project_json).map_err(|error| {
        format!(
            "project validation failed for `{}`\nerror: {error}",
            project_file.display()
        )
    })?;

    println!("project file: {}", project_file.display());
    println!("status: valid simulation graph");
    println!("kind: {}", validated.metadata.kind);
    println!("version: {}", validated.metadata.version);
    println!("title: {}", validated.metadata.title);
    println!("nodes: {}", validated.nodes_by_id.len());
    println!("edges: {}", validated.edges.len());
    println!("execution order:");
    for (index, node_id) in validated.topological_order.iter().enumerate() {
        println!("{}. {}", index + 1, node_id);
    }

    let simulation = simulate_validated_dag(&validated).map_err(|error| {
        format!(
            "project simulation failed for `{}`\nerror: {error}",
            project_file.display()
        )
    })?;

    println!("simulation:");
    println!("samples: {}", simulation.times.len());
    println!(
        "time range: {} s -> {} s",
        simulation.times.first().copied().unwrap_or(0.0),
        simulation.times.last().copied().unwrap_or(0.0)
    );
    println!("final node values:");
    for (index, node_id) in validated.topological_order.iter().enumerate() {
        let final_value = simulation
            .values_by_node_id
            .get(node_id)
            .and_then(|values| values.last())
            .copied()
            .unwrap_or(0.0);
        println!("{}. {} = {:.6}", index + 1, node_id, final_value);
    }

    Ok(())
}

fn usage(program: &str) -> String {
    format!("usage: {program} <path-to-project.json>")
}
