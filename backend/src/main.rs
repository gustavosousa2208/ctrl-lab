use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use ctrl_backend::plan::{build_control_plan, decode, encode};
use ctrl_backend::{exec, parse_project_json, simulate_validated_dag};

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

    let Some(first) = args.next() else {
        return Err(usage(&program));
    };

    // `--dump-plan` reads a compiled plan and takes no project file.
    if first == "--dump-plan" {
        let plan_path = args
            .next()
            .ok_or_else(|| format!("--dump-plan needs a plan path\n{}", usage(&program)))?;
        return dump_plan(Path::new(&plan_path));
    }

    let mut emit_plan: Option<String> = None;
    let mut emit_trace: Option<String> = None;
    let project_path = match first.as_str() {
        "--emit-plan" | "--emit-trace" => {
            let output = args
                .next()
                .ok_or_else(|| format!("{first} needs an output path\n{}", usage(&program)))?;
            if first == "--emit-plan" {
                emit_plan = Some(output);
            } else {
                emit_trace = Some(output);
            }
            args.next()
                .ok_or_else(|| format!("{first} needs a project path\n{}", usage(&program)))?
        }
        other if other.starts_with("--") => {
            return Err(format!("unknown option `{other}`\n{}", usage(&program)))
        }
        _ => first,
    };

    if args.next().is_some() {
        return Err(format!("unexpected extra argument\n{}", usage(&program)));
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

    if let Some(output) = emit_plan {
        let plan = build_control_plan(&validated)
            .map_err(|error| format!("plan generation failed: {error}"))?;
        let bytes = encode(&plan);
        fs::write(&output, &bytes)
            .map_err(|error| format!("failed to write `{output}`: {error}"))?;
        println!(
            "wrote {output} ({} bytes, {} blocks, {} signals, {} f32 state)",
            bytes.len(),
            plan.blocks.len(),
            plan.signal_count,
            plan.state_len
        );
        return Ok(());
    }

    if let Some(output) = emit_trace {
        let plan = build_control_plan(&validated)
            .map_err(|error| format!("plan generation failed: {error}"))?;
        let simulation = &validated.metadata.simulation;
        let steps = (simulation.end_time / simulation.step_size).floor() as usize + 1;
        let trace =
            exec::run(&plan, steps).map_err(|error| format!("plan execution failed: {error}"))?;

        let mut csv = String::from("t");
        for node_id in &validated.topological_order {
            csv.push(',');
            csv.push_str(node_id);
        }
        csv.push('\n');
        for (k, time) in trace.times.iter().enumerate() {
            csv.push_str(&format!("{time:.9}"));
            for series in &trace.signals {
                csv.push_str(&format!(",{:.9}", series[k]));
            }
            csv.push('\n');
        }
        fs::write(&output, csv).map_err(|error| format!("failed to write `{output}`: {error}"))?;
        println!(
            "wrote {output} ({} samples x {} signals, f32 reference trace)",
            trace.times.len(),
            trace.signals.len()
        );
        return Ok(());
    }

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

fn dump_plan(path: &Path) -> Result<(), String> {
    let bytes = fs::read(path)
        .map_err(|error| format!("failed to read plan `{}`: {error}", path.display()))?;
    let plan = decode(&bytes)
        .map_err(|error| format!("failed to decode `{}`: {error:?}", path.display()))?;

    println!("plan file: {}", path.display());
    println!(
        "format version: {}  kernel set version: {}",
        plan.format_version, plan.kernel_set_version
    );
    println!(
        "base ts: {} ns ({:.3} Hz)",
        plan.base_ts_ns,
        1.0e9 / plan.base_ts_ns as f64
    );
    println!(
        "signals: {}  state pool: {} f32  params: {} f32",
        plan.signal_count,
        plan.state_len,
        plan.params.len()
    );
    println!("wcet estimate: {} ns", plan.wcet_estimate_ns);
    println!(
        "model: {} (generated {}, backend {})",
        plan.meta.model_name, plan.meta.generated_at, plan.meta.backend_version
    );
    println!("blocks:");
    for (index, block) in plan.blocks.iter().enumerate() {
        println!(
            "{index:>3}. {:<18} in={:<12} out=s{:<3} state={}@{:<3} params={}@{}",
            format!("{:?}", block.kernel_id),
            format!("{:?}", block.inputs),
            block.output_signal,
            block.state_len,
            block.state_offset,
            block.param_len,
            block.param_offset
        );
    }
    Ok(())
}

fn usage(program: &str) -> String {
    format!(
        "usage:\n  \
         {program} <project.json>                          validate, simulate, report\n  \
         {program} --emit-plan <out.dcp> <project.json>    compile to a Deployable Control Plan\n  \
         {program} --emit-trace <out.csv> <project.json>   f32 reference trace (firmware contract)\n  \
         {program} --dump-plan <in.dcp>                    inspect a compiled plan"
    )
}
