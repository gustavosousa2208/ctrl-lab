// Scratch trace dumper: prints a project's simulation as CSV to stdout.
// Usage: cargo run --example trace -- ../test-projects/04-2nd-order-system.json
use std::env;
use std::fs;

fn main() {
    let path = env::args().nth(1).expect("usage: trace <project.json>");
    let json = fs::read_to_string(&path).expect("project must be readable");
    let output = ctrl_backend::simulate_project_json(&json).expect("simulation must succeed");

    // Stable column order.
    let mut node_ids: Vec<&String> = output.values_by_node_id.keys().collect();
    node_ids.sort();

    print!("t");
    for id in &node_ids {
        print!(",{id}");
    }
    println!();

    for (i, t) in output.times.iter().enumerate() {
        print!("{t:.6}");
        for id in &node_ids {
            let v = output.values_by_node_id[*id][i];
            print!(",{v:.10}");
        }
        println!();
    }
}
